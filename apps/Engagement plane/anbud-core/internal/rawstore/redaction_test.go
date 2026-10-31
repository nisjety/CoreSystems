package rawstore

import (
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// O1: what an erased notice means to the sync. The erasure itself is a DBA procedure
// (migration 0006 and docs/PERSONAL-DATA.md, never application code), so these tests
// call the redaction function the way a DBA does and then run the store the way the sync
// does. The role model is tested in internal/db; here the pool is the throwaway server's own
// role, which owns the tables and so may run the function.

const redactNoticeSQL = `SELECT anbud_redact_raw_document($1, 'erasure_request', 'TEST-1')`

// rawDocumentID is the id of the one raw row a notice id and a hash name.
func rawDocumentID(t *testing.T, pool *pgxpool.Pool, doffinID, sha string) int64 {
	t.Helper()
	var id int64
	if err := pool.QueryRow(testCtx(t), `SELECT id FROM raw_documents WHERE doffin_id = $1 AND body_sha256 = $2`, doffinID, sha).Scan(&id); err != nil {
		t.Fatalf("find the raw row of %s: %v", doffinID, err)
	}
	return id
}

// attemptRows are the outcome and the raw row of each attempt for a notice id, oldest first.
func attemptRows(t *testing.T, pool *pgxpool.Pool, doffinID string) (outcomes []string, rawIDs []int64) {
	t.Helper()
	rows, err := pool.Query(testCtx(t), `SELECT outcome, raw_document_id FROM download_attempts WHERE doffin_id = $1 ORDER BY id`, doffinID)
	if err != nil {
		t.Fatalf("read attempts: %v", err)
	}
	type attempt struct {
		outcome string
		raw     *int64
	}
	got, err := pgx.CollectRows(rows, func(row pgx.CollectableRow) (attempt, error) {
		var a attempt
		err := row.Scan(&a.outcome, &a.raw)
		return a, err
	})
	if err != nil {
		t.Fatalf("scan attempts: %v", err)
	}
	for _, a := range got {
		if a.raw == nil {
			t.Fatalf("attempt %q has no raw_document_id", a.outcome)
		}
		outcomes = append(outcomes, a.outcome)
		rawIDs = append(rawIDs, *a.raw)
	}
	return outcomes, rawIDs
}

// A redacted notice is still held: its row, its hash and its attempts stay, so the sync
// does not look for it again. If the same bytes arrive anyway, the identity (source, id,
// hash) finds the redacted row and the insert does nothing: the erased body is never
// stored a second time, and the attempt is a duplicate that points at the redacted row.
func TestRedactedNoticeStaysHeldAndTheSyncNeverStoresItAgain(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	n := guardNotice("2026-104837", []byte("<ContractNotice><Name>Kari Nordmann</Name><Email>kari.nordmann@example.no</Email></ContractNotice>"))
	if inserted, err := s.StoreNotice(ctx, run, n); err != nil || !inserted {
		t.Fatalf("first StoreNotice = %v, %v; want inserted", inserted, err)
	}
	docID := rawDocumentID(t, pool, n.ID, n.SHA256)
	var tombstone int64
	if err := pool.QueryRow(ctx, redactNoticeSQL, docID).Scan(&tombstone); err != nil {
		t.Fatalf("redact the notice: %v", err)
	}

	// Resolve still says held, for the redacted id, and says nothing of an id it never saw.
	got, err := s.Resolve(ctx, []string{n.ID, "2026-100002"})
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if len(got) != 1 || got[n.ID].Kind != doffinsync.ResolutionHeld {
		t.Fatalf("Resolve = %v, want only %s held", got, n.ID)
	}

	// The same bytes again are a duplicate, and the row stays as the redaction left it.
	inserted, err := s.StoreNotice(ctx, run, n)
	if err != nil || inserted {
		t.Fatalf("StoreNotice of the erased bytes = %v, %v; want not inserted and no error", inserted, err)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, n.ID); got != 1 {
		t.Fatalf("raw rows for %s = %d after the same bytes arrived again, want 1", n.ID, got)
	}
	if got := count(t, pool, `
		SELECT count(*) FROM raw_documents
		WHERE id = $1 AND redacted_at IS NOT NULL AND octet_length(body) = 0 AND body_sha256 = $2`, docID, n.SHA256); got != 1 {
		t.Fatalf("the row is not still redacted, empty and under its original hash (matches %d)", got)
	}
	outcomes, rawIDs := attemptRows(t, pool, n.ID)
	if len(outcomes) != 2 || outcomes[0] != doffinsync.OutcomeStored || outcomes[1] != doffinsync.OutcomeDuplicate {
		t.Fatalf("attempt outcomes = %v, want [stored duplicate]", outcomes)
	}
	if rawIDs[0] != docID || rawIDs[1] != docID {
		t.Fatalf("attempts point at raw rows %v, want both at the redacted row %d", rawIDs, docID)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_redactions WHERE raw_document_id = $1 AND id = $2`, docID, tombstone); got != 1 {
		t.Fatalf("the tombstone is not there once (matches %d)", got)
	}
	// And the parser's view never offers the erased row.
	if got := count(t, pool, `SELECT count(*) FROM raw_documents_live WHERE doffin_id = $1`, n.ID); got != 0 {
		t.Fatalf("raw_documents_live shows %d row(s) of the redacted notice, want none", got)
	}
}

// What a redaction is not: a block on the notice id. It erases one body, named by its row.
// A different body under the same id is a second row, as before (T52), and it is stored
// with whatever it holds. The sync does not look for a changed body under a held id (the
// id resolves as held, so nothing is downloaded), so this happens only if another path
// stores one. docs/PERSONAL-DATA.md lists it among the open questions, under standing
// requests: this test pins the behaviour so that nobody mistakes the tombstone for a
// suppression list.
func TestChangedBodyAfterARedactionIsStoredAsAnotherRow(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	first := guardNotice("2026-104837", []byte("version one, with a contact person"))
	second := guardNotice("2026-104837", []byte("version two, with the same contact person"))
	if _, err := s.StoreNotice(ctx, run, first); err != nil {
		t.Fatalf("store the first version: %v", err)
	}
	var tombstone int64
	if err := pool.QueryRow(ctx, redactNoticeSQL, rawDocumentID(t, pool, first.ID, first.SHA256)).Scan(&tombstone); err != nil {
		t.Fatalf("redact the first version: %v", err)
	}

	inserted, err := s.StoreNotice(ctx, run, second)
	if err != nil || !inserted {
		t.Fatalf("StoreNotice of a changed body = %v, %v; want inserted: a redaction names one body, not an id", inserted, err)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, first.ID); got != 2 {
		t.Fatalf("raw rows for the id = %d, want 2", got)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents_live WHERE doffin_id = $1 AND body_sha256 = $2`, first.ID, second.SHA256); got != 1 {
		t.Fatalf("raw_documents_live shows the second version %d times, want once", got)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Fatalf("tombstones = %d, want only the first version's", got)
	}
}
