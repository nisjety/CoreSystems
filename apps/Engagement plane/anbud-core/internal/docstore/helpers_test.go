package docstore

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

func sha(s string) string {
	sum := sha256.Sum256([]byte(s))
	return hex.EncodeToString(sum[:])
}

// sample is an upload of three files: one read, one scan with no text, one workbook that is not read.
func sample(org, notice, archive string) NewUpload {
	return NewUpload{
		OrgID: org, NoticeID: notice, UploadedBy: "user-1", ArchiveName: "konkurransegrunnlag.zip",
		ArchiveSHA256: sha(archive), ArchiveBytes: 12345,
		Documents: []Document{
			{Path: "konkurransegrunnlag.pdf", Kind: "pdf", Size: 5000, SHA256: sha("kg"), Status: StatusRead, Pages: 12, Chars: 3000, Extractor: "pdf"},
			{Path: "vedlegg/skann.pdf", Kind: "pdf", Size: 9000, SHA256: sha("skann"), Status: StatusEmpty, Pages: 3, Extractor: "pdf"},
			{Path: "prisskjema.xlsx", Kind: "xlsx", Size: 800, SHA256: sha("pris"), Status: StatusUnsupported},
		},
		Findings: []Finding{
			{Document: "konkurransegrunnlag.pdf", Capability: "iso9001", Class: "requirement", Page: 3, Snippet: "Leverandøren skal ha ISO 9001."},
		},
		Retention: 90 * 24 * time.Hour,
	}
}

func testCtx(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	t.Cleanup(cancel)
	return ctx
}

// openStore returns a store over a fresh, migrated throwaway database, as the server's own role.
// That role owns the tables and so is not held to row level security: tests of the store's own logic
// use it, and the tests of the role model use the roles.
func openStore(t *testing.T) (*Store, *pgxpool.Pool) {
	t.Helper()
	pool := dbtest.Open(t)
	return New(pool), pool
}

func count(t *testing.T, pool *pgxpool.Pool, query string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(testCtx(t), query, args...).Scan(&n); err != nil {
		t.Fatalf("count %q: %s", query, dbtest.Describe(err))
	}
	return n
}

// insertExpired writes a row that expired a day ago, which Create refuses to: the store does not make
// a past that did not happen.
func insertExpired(t *testing.T, pool *pgxpool.Pool, org, notice, archive string) {
	t.Helper()
	_, err := pool.Exec(testCtx(t), `
INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, created_at, expires_at)
VALUES ($1, $2, $3, 1, '[]', '[]', 0, 0, now() - interval '3 days', now() - interval '1 day')`, org, notice, sha(archive))
	if err != nil {
		t.Fatalf("insert an expired row: %s", dbtest.Describe(err))
	}
}
