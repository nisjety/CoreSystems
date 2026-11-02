//go:build integration

package repo_test

// WS14-S3 / WS14-S4: room-file admission and the shared-with-me listing against
// a real Postgres (TEST_DATABASE_URL, a throwaway database). Skipped when the
// variable is unset. The fixture schema (integration_test.go) predates
// documents.space_ref and the room-file index, so this file adds both the way
// migrations 20260907120000 and 20261102062000 do.

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

const roomSpace = "space:org:room-1"

func setupSpaceFilesDB(t *testing.T) (*repo.DocumentRepo, *pgxpool.Pool, string, func()) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping room-file live test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	for _, stmt := range []string{
		schemaSQL,
		`ALTER TABLE documents ADD COLUMN IF NOT EXISTS space_ref TEXT`,
		`CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_space_file_sha256
		   ON documents (org_id, space_ref, (metadata->>'sha256'))
		   WHERE type = 'space_upload' AND deleted_at IS NULL AND space_ref IS NOT NULL`,
	} {
		if _, err := pool.Exec(ctx, stmt); err != nil {
			pool.Close()
			t.Fatalf("ensure schema: %v", err)
		}
	}
	grantScopedRuntimeRole(ctx, t, pool)
	org := fmt.Sprintf("sf-%d-%d", os.Getpid(), time.Now().UnixNano())
	cleanup := func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM documents WHERE org_id LIKE $1`, org+"%")
		pool.Close()
	}
	return repo.NewDocumentRepo(pool), pool, org, cleanup
}

func lifecycleFactory(*model.Document, bool) (string, []byte, error) {
	return "document.created", []byte(`{}`), nil
}

func roomFile(org, space, owner, sha, content string, c *model.SpaceFileConstraints) model.CreateDocumentInput {
	return model.CreateDocumentInput{
		OrgID: org, Source: "space-upload", Type: repo.SpaceFileType, Title: "f-" + sha, Content: content,
		Metadata: []byte(fmt.Sprintf(`{"sha256":%q}`, sha)), OwnerID: owner, CreatedBy: owner,
		Visibility: "shared", SpaceRef: space, SpaceFile: c,
	}
}

func sha(n int) string { return fmt.Sprintf("%064x", n) }

func TestRoomFileAdmissionCountQuotaUnderConcurrentUploads(t *testing.T) {
	r, _, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	const max, uploads = 3, 8

	var wg sync.WaitGroup
	errs := make([]error, uploads)
	for i := 0; i < uploads; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			_, errs[i] = r.CreateWithOutbox(context.Background(),
				roomFile(org, roomSpace, "u1", sha(i+1), "text", &model.SpaceFileConstraints{SHA256: sha(i + 1), MaxFiles: max}),
				lifecycleFactory)
		}(i)
	}
	wg.Wait()

	ok := 0
	for _, err := range errs {
		var quota *repo.SpaceFileQuotaError
		switch {
		case err == nil:
			ok++
		case errors.As(err, &quota):
			if quota.Limit != "max_files" || quota.MaxFiles != max {
				t.Fatalf("quota error = %+v", quota)
			}
		default:
			t.Fatalf("unexpected error: %v", err)
		}
	}
	if ok != max {
		t.Fatalf("%d uploads admitted, want exactly %d (the advisory lock must serialise the count)", ok, max)
	}
}

func TestRoomFileAdmissionTextByteQuota(t *testing.T) {
	r, _, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	c := func(n int) *model.SpaceFileConstraints {
		return &model.SpaceFileConstraints{SHA256: sha(n), MaxTextBytes: 10}
	}
	ctx := context.Background()
	if _, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(1), "123456", c(1)), lifecycleFactory); err != nil {
		t.Fatalf("first file: %v", err)
	}
	_, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(2), "123456", c(2)), lifecycleFactory)
	var quota *repo.SpaceFileQuotaError
	if !errors.As(err, &quota) || quota.Limit != "max_text_bytes" || quota.MaxTextBytes != 10 {
		t.Fatalf("second file err = %v, want max_text_bytes", err)
	}
	// Exactly at the limit is allowed (6 + 4 = 10).
	if _, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(3), "1234", c(3)), lifecycleFactory); err != nil {
		t.Fatalf("file that fills the quota exactly: %v", err)
	}
	// Another Space has its own budget.
	if _, err := r.CreateWithOutbox(ctx, roomFile(org, "space:org:room-2", "u1", sha(4), "123456", c(4)), lifecycleFactory); err != nil {
		t.Fatalf("other Space: %v", err)
	}
}

func TestRoomFileHashDedupeIsPerOrgAndSpaceAndReleasedByDelete(t *testing.T) {
	r, _, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	ctx := context.Background()
	c := &model.SpaceFileConstraints{SHA256: sha(7)}
	first, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(7), "a", c), lifecycleFactory)
	if err != nil {
		t.Fatal(err)
	}

	_, err = r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u2", sha(7), "b", c), lifecycleFactory)
	var dup *repo.SpaceFileDuplicateError
	if !errors.As(err, &dup) || dup.DocumentID != first.Document.DocumentID {
		t.Fatalf("duplicate err = %v, want one naming %s", err, first.Document.DocumentID)
	}
	if _, err := r.CreateWithOutbox(ctx, roomFile(org, "space:org:room-2", "u1", sha(7), "a", c), lifecycleFactory); err != nil {
		t.Fatalf("same bytes in another Space: %v", err)
	}
	if _, err := r.CreateWithOutbox(ctx, roomFile(org+"-b", roomSpace, "u1", sha(7), "a", c), lifecycleFactory); err != nil {
		t.Fatalf("same bytes in another org: %v", err)
	}

	if err := r.SoftDelete(ctx, org, first.Document.DocumentID, "u1"); err != nil {
		t.Fatal(err)
	}
	if _, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(7), "a", c), lifecycleFactory); err != nil {
		t.Fatalf("a deleted file must release its hash: %v", err)
	}
}

func TestRoomFileUniqueIndexBackstopMapsToDuplicateError(t *testing.T) {
	r, pool, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	ctx := context.Background()
	if _, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(9), "a", &model.SpaceFileConstraints{SHA256: sha(9)}), lifecycleFactory); err != nil {
		t.Fatal(err)
	}
	// Skipping the lookup (empty SHA256 constraint) leaves only the index to
	// stop the second row; it must still surface as the typed duplicate.
	_, err := r.CreateWithOutbox(ctx, roomFile(org, roomSpace, "u1", sha(9), "b", &model.SpaceFileConstraints{}), lifecycleFactory)
	var dup *repo.SpaceFileDuplicateError
	if !errors.As(err, &dup) {
		t.Fatalf("err = %v, want SpaceFileDuplicateError from the unique index", err)
	}
	var n int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM documents WHERE org_id=$1 AND type='space_upload'`, org).Scan(&n); err != nil || n != 1 {
		t.Fatalf("rows = %d err=%v, want the failed insert rolled back", n, err)
	}
}
