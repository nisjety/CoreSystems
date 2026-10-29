package rawstore

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

const (
	sqlRestrictViolation = "23001" // the append-only guard
	sqlCheckViolation    = "23514"
	sqlFeatureNotSupp    = "0A000" // TRUNCATE of a table another table references
)

// seedEvidence writes one row to each evidence table and returns the notice id.
func seedEvidence(t *testing.T, s *Store, run int64) string {
	t.Helper()
	ctx := testCtx(t)
	const id = "2026-100001"
	if err := s.PutSearchPage(ctx, run, doffinsync.SearchPage{
		QueryKey: "issueDate=2026-10-01;perPage=100;sort=PUBLICATION_DATE_ASC", Page: 1,
		ContentType: "application/json", Body: []byte(`{"hits":[]}`), FetchedAt: fixedNow,
	}); err != nil {
		t.Fatalf("seed search page: %v", err)
	}
	if _, err := s.StoreNotice(ctx, run, guardNotice(id, []byte("notice body one"))); err != nil {
		t.Fatalf("seed notice: %v", err)
	}
	if err := s.RecordAttempt(ctx, run, doffinsync.Attempt{
		DoffinID: "2026-100002", Outcome: doffinsync.OutcomeNotFound,
		HTTPStatus: 404, Message: "notice not found", At: fixedNow,
	}); err != nil {
		t.Fatalf("seed attempt: %v", err)
	}
	return id
}

// T49: on each evidence table, UPDATE, DELETE and TRUNCATE are refused and the
// row is unchanged. TRUNCATE of raw_documents is refused earlier, with 0A000,
// because download_attempts references it (migration 0002 header, D4). Both
// codes are refused; the row survives either way.
func TestEvidenceRefusesUpdateDeleteTruncate(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	seedEvidence(t, s, run)

	type refusal struct {
		name  string
		stmt  string
		codes []string
	}
	refusals := []refusal{
		{"raw_search_pages update", `UPDATE raw_search_pages SET content_type = 'changed'`, []string{sqlRestrictViolation}},
		{"raw_search_pages delete", `DELETE FROM raw_search_pages`, []string{sqlRestrictViolation}},
		{"raw_search_pages truncate", `TRUNCATE raw_search_pages`, []string{sqlRestrictViolation}},
		{"raw_documents update", `UPDATE raw_documents SET content_type = 'changed'`, []string{sqlRestrictViolation}},
		{"raw_documents delete", `DELETE FROM raw_documents`, []string{sqlRestrictViolation}},
		{"raw_documents truncate", `TRUNCATE raw_documents`, []string{sqlFeatureNotSupp, sqlRestrictViolation}},
		{"raw_documents truncate cascade", `TRUNCATE raw_documents CASCADE`, []string{sqlFeatureNotSupp, sqlRestrictViolation}},
		{"download_attempts update", `UPDATE download_attempts SET message = 'changed'`, []string{sqlRestrictViolation}},
		{"download_attempts delete", `DELETE FROM download_attempts`, []string{sqlRestrictViolation}},
		{"download_attempts truncate", `TRUNCATE download_attempts`, []string{sqlRestrictViolation}},
	}

	before := map[string]int{
		"raw_search_pages":  count(t, pool, `SELECT count(*) FROM raw_search_pages`),
		"raw_documents":     count(t, pool, `SELECT count(*) FROM raw_documents`),
		"download_attempts": count(t, pool, `SELECT count(*) FROM download_attempts`),
	}
	for _, r := range refusals {
		_, err := pool.Exec(ctx, r.stmt)
		if err == nil {
			t.Errorf("%s: statement succeeded, want it refused", r.name)
			continue
		}
		if !contains(r.codes, pgCode(err)) {
			t.Errorf("%s: SQLSTATE %q (%v), want one of %v", r.name, pgCode(err), err, r.codes)
		}
	}

	after := map[string]int{
		"raw_search_pages":  count(t, pool, `SELECT count(*) FROM raw_search_pages`),
		"raw_documents":     count(t, pool, `SELECT count(*) FROM raw_documents`),
		"download_attempts": count(t, pool, `SELECT count(*) FROM download_attempts`),
	}
	for table, n := range before {
		if after[table] != n {
			t.Errorf("%s rows %d -> %d: a refused statement changed the table", table, n, after[table])
		}
	}
	// The stored body and its hash are the evidence, so check them too.
	if got := count(t, pool, `SELECT count(*) FROM raw_documents WHERE content_type = 'application/json'`); got != 1 {
		t.Errorf("raw_documents content type changed: %d rows still match, want 1", got)
	}
}

// T50: the database refuses a row that breaks its rules, even one written by
// hand. The valid row first shows that the statement shape itself works.
func TestSQLRefusesWrongHashAndBadIDs(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	body := []byte("a notice body")
	good := guardNotice("2026-100001", body)
	insertRaw := func(source, id string, body []byte, sha string) error {
		_, err := pool.Exec(ctx, `
			INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id)
			VALUES ($1, $2, '', $3, $4, now(), $5)`,
			source, id, body, sha, run)
		return err
	}

	if err := insertRaw("doffin", good.ID, body, good.SHA256); err != nil {
		t.Fatalf("control: a valid hand-written row was refused: %v", err)
	}

	sumOther := sha256.Sum256([]byte("some other bytes"))
	emptySum := sha256.Sum256(nil)
	refused := []struct {
		name string
		err  error
	}{
		{"wrong hash", insertRaw("doffin", "2026-100003", []byte("different"), good.SHA256)},
		{"hash of other bytes", insertRaw("doffin", "2026-100004", body, hex.EncodeToString(sumOther[:]))},
		{"uppercase hash", insertRaw("doffin", "2026-100005", body, strings.ToUpper(good.SHA256))},
		{"bad doffin_id", insertRaw("doffin", "2026-1", body, good.SHA256)},
		{"empty body", insertRaw("doffin", "2026-100006", []byte{}, hex.EncodeToString(emptySum[:]))},
		{"other source", insertRaw("tenderly", "2026-100007", body, good.SHA256)},
	}
	for _, r := range refused {
		if pgCode(r.err) != sqlCheckViolation {
			t.Errorf("%s: err = %v (SQLSTATE %q), want 23514", r.name, r.err, pgCode(r.err))
		}
	}

	// The same rule holds for search pages.
	pageSum := sha256.Sum256([]byte("page"))
	_, err := pool.Exec(ctx, `
		INSERT INTO raw_search_pages (source, query_key, page, content_type, body, body_sha256, fetched_at, run_id)
		VALUES ('doffin', 'k', 1, '', $1, $2, now(), $3)`,
		[]byte("other page bytes"), hex.EncodeToString(pageSum[:]), run)
	if pgCode(err) != sqlCheckViolation {
		t.Errorf("search page with a wrong hash: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 1 {
		t.Errorf("raw_documents rows = %d after refusals, want only the control row", got)
	}
}

func contains(xs []string, x string) bool {
	for _, v := range xs {
		if v == x {
			return true
		}
	}
	return false
}
