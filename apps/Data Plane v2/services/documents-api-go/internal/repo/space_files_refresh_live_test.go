//go:build integration

package repo_test

// WS14-S3: a same-key room-file refresh is admitted like a new room file, and an
// ordinary write can never rewrite a room file. Runs against a real Postgres
// (TEST_DATABASE_URL, a throwaway database) through the helpers in
// space_files_live_test.go; skipped when the variable is unset.

import (
	"context"
	"errors"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

// withKey returns the input with an idempotency key, so a second write with the
// same key reaches the same-key branch.
func withKey(in model.CreateDocumentInput, key string) model.CreateDocumentInput {
	in.IdempotencyKey = key
	return in
}

// roomRow reads what a room file row holds now: its content, its hash and its type.
func roomRow(t *testing.T, pool *pgxpool.Pool, documentID string) (content, hash, docType string) {
	t.Helper()
	err := pool.QueryRow(context.Background(),
		`SELECT content, metadata->>'sha256', type FROM documents WHERE document_id = $1`, documentID,
	).Scan(&content, &hash, &docType)
	if err != nil {
		t.Fatalf("read room file %s: %v", documentID, err)
	}
	return content, hash, docType
}

func TestRoomFileSameKeyRefreshIsAdmittedLikeANewFile(t *testing.T) {
	r, pool, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	ctx := context.Background()
	// The room holds at most two files and ten text bytes.
	limits := func(hash string) *model.SpaceFileConstraints {
		return &model.SpaceFileConstraints{SHA256: hash, MaxFiles: 2, MaxTextBytes: 10}
	}

	a, err := r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(1), "aaaa", limits(sha(1))), "ka"), lifecycleFactory)
	if err != nil {
		t.Fatalf("file A: %v", err)
	}
	b, err := r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(2), "bbbb", limits(sha(2))), "kb"), lifecycleFactory)
	if err != nil {
		t.Fatalf("file B: %v", err)
	}
	aID := a.Document.DocumentID

	// A refresh that takes B's hash is a duplicate, and A keeps what it held.
	_, err = r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(2), "cccc", limits(sha(2))), "ka"), lifecycleFactory)
	var dup *repo.SpaceFileDuplicateError
	if !errors.As(err, &dup) || dup.DocumentID != b.Document.DocumentID {
		t.Fatalf("refresh onto B's hash: err = %v, want a duplicate naming %s", err, b.Document.DocumentID)
	}
	if content, hash, _ := roomRow(t, pool, aID); content != "aaaa" || hash != sha(1) {
		t.Fatalf("A after a refused duplicate = content %q hash %s, want its original", content, hash)
	}

	// A refresh that would push the room past ten text bytes (B holds 4, 4+7 > 10)
	// is refused with the byte limit, and A keeps what it held.
	_, err = r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(3), "ccccccc", limits(sha(3))), "ka"), lifecycleFactory)
	var quota *repo.SpaceFileQuotaError
	if !errors.As(err, &quota) || quota.Limit != "max_text_bytes" {
		t.Fatalf("refresh past the byte limit: err = %v, want max_text_bytes", err)
	}
	if content, hash, _ := roomRow(t, pool, aID); content != "aaaa" || hash != sha(1) {
		t.Fatalf("A after a refused quota = content %q hash %s, want its original", content, hash)
	}

	// A refresh within budget is admitted in place. The row it rewrites is not
	// counted twice: the room holds two files, which is at MaxFiles, and B's 4 bytes
	// plus 6 new bytes is exactly the limit. Counting A's old 4 bytes too would refuse.
	got, err := r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(3), "cccccc", limits(sha(3))), "ka"), lifecycleFactory)
	if err != nil {
		t.Fatalf("refresh within budget: %v", err)
	}
	if !got.Updated || got.Document.DocumentID != aID {
		t.Fatalf("refresh within budget = updated %v document %s, want the same row %s updated", got.Updated, got.Document.DocumentID, aID)
	}
	if content, hash, _ := roomRow(t, pool, aID); content != "cccccc" || hash != sha(3) {
		t.Fatalf("A after the admitted refresh = content %q hash %s", content, hash)
	}

	// A refresh that keeps the row's own hash is not a duplicate of itself. A holds
	// 6 bytes and B 4; with A left out, this 4-byte refresh is judged at 4 + 4 of 10,
	// so it is admitted too (counting A's own 6 would refuse it).
	again, err := r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(3), "dddd", limits(sha(3))), "ka"), lifecycleFactory)
	if err != nil {
		t.Fatalf("refresh that keeps its own hash: %v", err)
	}
	if !again.Updated || again.Document.DocumentID != aID {
		t.Fatalf("own-hash refresh = updated %v document %s", again.Updated, again.Document.DocumentID)
	}
}

func TestRoomFileSameKeyRefreshNeverCrossesTheRoomOrTheKind(t *testing.T) {
	r, pool, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	ctx := context.Background()
	a, err := r.CreateWithOutbox(ctx, withKey(roomFile(org, roomSpace, "u1", sha(4), "aaaa", &model.SpaceFileConstraints{SHA256: sha(4)}), "kx"), lifecycleFactory)
	if err != nil {
		t.Fatalf("room file A: %v", err)
	}
	aID := a.Document.DocumentID

	// The same key written as a room file of another Space is an ownership conflict.
	otherRoom := roomFile(org, "space:org:room-2", "u1", sha(5), "bbbb", &model.SpaceFileConstraints{SHA256: sha(5)})
	if _, err := r.CreateWithOutbox(ctx, withKey(otherRoom, "kx"), lifecycleFactory); !errors.Is(err, repo.ErrIdempotencyOwnershipConflict) {
		t.Fatalf("refresh from another Space: err = %v, want ErrIdempotencyOwnershipConflict", err)
	}

	// An ordinary write with the same key never rewrites a room file.
	ordinary := model.CreateDocumentInput{
		OrgID: org, Source: "manual", Type: "text", Title: "f-ordinary", Content: "zzzz",
		OwnerID: "u1", CreatedBy: "u1", Visibility: "shared", IdempotencyKey: "kx",
	}
	if _, err := r.CreateWithOutbox(ctx, ordinary, lifecycleFactory); !errors.Is(err, repo.ErrIdempotencyOwnershipConflict) {
		t.Fatalf("ordinary write over a room file: err = %v, want ErrIdempotencyOwnershipConflict", err)
	}
	if content, hash, docType := roomRow(t, pool, aID); content != "aaaa" || hash != sha(4) || docType != repo.SpaceFileType {
		t.Fatalf("room file A after both refusals = content %q hash %s type %s, want it untouched", content, hash, docType)
	}
}
