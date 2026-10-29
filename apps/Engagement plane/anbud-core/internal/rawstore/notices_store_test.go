package rawstore

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// T51: the same body twice is one raw row. The second download is a duplicate
// attempt that points at the same row.
func TestStoreNoticeIsIdempotent(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	n := guardNotice("2026-104837", []byte("notice body"))

	first, err := s.StoreNotice(ctx, run, n)
	if err != nil || !first {
		t.Fatalf("first StoreNotice = %v, %v; want inserted", first, err)
	}
	second, err := s.StoreNotice(ctx, run, n)
	if err != nil || second {
		t.Fatalf("second StoreNotice = %v, %v; want not inserted", second, err)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, n.ID); got != 1 {
		t.Fatalf("raw rows for %s = %d, want 1", n.ID, got)
	}

	rows, err := pool.Query(ctx, `SELECT outcome, raw_document_id FROM download_attempts WHERE doffin_id = $1 ORDER BY id`, n.ID)
	if err != nil {
		t.Fatalf("read attempts: %v", err)
	}
	defer rows.Close()
	var outcomes []string
	var ids []int64
	for rows.Next() {
		var outcome string
		var raw *int64
		if err := rows.Scan(&outcome, &raw); err != nil {
			t.Fatalf("scan attempt: %v", err)
		}
		if raw == nil {
			t.Fatalf("attempt %q has no raw_document_id", outcome)
		}
		outcomes = append(outcomes, outcome)
		ids = append(ids, *raw)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("iterate attempts: %v", err)
	}
	if len(outcomes) != 2 || outcomes[0] != doffinsync.OutcomeStored || outcomes[1] != doffinsync.OutcomeDuplicate {
		t.Fatalf("attempt outcomes = %v, want [stored duplicate]", outcomes)
	}
	if ids[0] != ids[1] {
		t.Fatalf("attempts point at raw rows %d and %d, want the same row", ids[0], ids[1])
	}
}

// T52: a changed body under a known id is a second row. Nothing is overwritten.
func TestChangedBodyIsSecondRow(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	for _, body := range []string{"version one of the notice", "version two of the notice"} {
		inserted, err := s.StoreNotice(ctx, run, guardNotice("2026-104837", []byte(body)))
		if err != nil || !inserted {
			t.Fatalf("StoreNotice(%q) = %v, %v; want inserted", body, inserted, err)
		}
	}
	if got := count(t, pool, `SELECT count(DISTINCT body_sha256) FROM raw_documents WHERE doffin_id = $1`, "2026-104837"); got != 2 {
		t.Fatalf("distinct bodies for the id = %d, want 2", got)
	}
}

// T53: a notice and its attempt are one unit. An attempt that cannot be written
// leaves no raw row behind. An unknown run id fails on the raw row first, which
// is the same outcome. A failure injected on the attempt insert is the case that
// tests the transaction itself.
func TestStoreNoticeIsAtomic(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	n := guardNotice("2026-104837", []byte("atomic body"))

	if _, err := s.StoreNotice(ctx, 999999, n); err == nil {
		t.Fatal("StoreNotice with an unknown run id succeeded")
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 0 {
		t.Fatalf("raw rows after an unknown run id = %d, want 0", got)
	}

	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	if _, err := pool.Exec(ctx, `
		CREATE FUNCTION rawstore_test_refuse_attempts() RETURNS trigger LANGUAGE plpgsql AS $$
		BEGIN RAISE EXCEPTION 'injected attempt failure'; END;
		$$`); err != nil {
		t.Fatalf("create injected failure: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		CREATE TRIGGER rawstore_test_refuse BEFORE INSERT ON download_attempts
		FOR EACH ROW EXECUTE FUNCTION rawstore_test_refuse_attempts()`); err != nil {
		t.Fatalf("create injected trigger: %v", err)
	}
	if _, err := s.StoreNotice(ctx, run, n); err == nil {
		t.Fatal("StoreNotice succeeded although its attempt insert fails")
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 0 {
		t.Fatalf("raw rows after a failed attempt insert = %d, want 0 (the transaction must roll back)", got)
	}
}

// T54: bytes come back exactly as stored: NUL, invalid UTF-8 and CRLF included,
// and at a size well above a typical notice.
func TestBytesRoundTripExactly(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	pattern := []byte{0x00, 0xff, 0xfe, 'a', '\r', '\n', 0xc3, 0x28, 0x80}
	body := bytes.Repeat(pattern, (2<<20)/len(pattern)+1)
	n := guardNotice("2026-104837", body)
	if _, err := s.StoreNotice(ctx, run, n); err != nil {
		t.Fatalf("StoreNotice: %v", err)
	}

	var got []byte
	var sha string
	if err := pool.QueryRow(ctx, `SELECT body, body_sha256 FROM raw_documents WHERE doffin_id = $1`, n.ID).Scan(&got, &sha); err != nil {
		t.Fatalf("read back: %v", err)
	}
	if !bytes.Equal(got, body) {
		t.Fatalf("read-back body differs: %d bytes stored, %d sent", len(got), len(body))
	}
	sum := sha256.Sum256(got)
	if sha != hex.EncodeToString(sum[:]) || sha != n.SHA256 {
		t.Fatalf("read-back digest %s does not match the notice digest %s", sha, n.SHA256)
	}
}

// Content types from upstream are cleaned before insert, so a hostile header
// cannot fail the insert. The stored value is printable and at most 255 characters.
func TestHostileContentTypeCannotFailTheInsert(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	hostile := "text/plain\x00\xff" + strings.Repeat("x", 400) + "\r\n"

	n := guardNotice("2026-104837", []byte("a notice body"))
	n.ContentType = hostile
	if _, err := s.StoreNotice(ctx, run, n); err != nil {
		t.Fatalf("StoreNotice with a hostile content type: %v", err)
	}
	var stored string
	if err := pool.QueryRow(ctx, `SELECT content_type FROM raw_documents WHERE doffin_id = $1`, n.ID).Scan(&stored); err != nil {
		t.Fatalf("read content type: %v", err)
	}
	if strings.ContainsAny(stored, "\x00\r\n") || len([]rune(stored)) > 255 || !strings.HasPrefix(stored, "text/plain") {
		t.Fatalf("stored content type %q is not clean and clipped", stored)
	}
}

// Resolve answers only for ids that can be notices. A malformed id, NUL included,
// is never held and never 404, so it is left out of the query: it must not fail
// the lookup for the whole day.
func TestResolveIgnoresIDsThatCannotBeNotices(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	if _, err := s.StoreNotice(ctx, run, guardNotice("2026-100001", []byte("held body"))); err != nil {
		t.Fatalf("seed held notice: %v", err)
	}

	got, err := s.Resolve(ctx, []string{"2026-100001", "bad\x00id", "../x", "2026-1"})
	if err != nil {
		t.Fatalf("Resolve with malformed ids: %v", err)
	}
	if len(got) != 1 || got["2026-100001"].Kind != doffinsync.ResolutionHeld {
		t.Fatalf("Resolve = %v, want only the held notice", got)
	}
}
