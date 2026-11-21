//go:build integration

package server

import (
	"bytes"
	"context"
	"strings"
	"testing"
	"time"

	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"
	"google.golang.org/grpc/codes"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/objectstore"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// TestEraseThreadWorkspaceForRetentionRemovesRowsAndPrefix drives the F-f10-08
// service erase against a throwaway Postgres and MinIO. A service principal that
// holds the narrow scope erases a thread's rows and its blob prefix. Each refusal
// (no scope, a user principal, the switch off, another organization, a thread a
// second owner also holds) removes nothing.
func TestEraseThreadWorkspaceForRetentionRemovesRowsAndPrefix(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()

	pool := startThreadPostgres(t, ctx)
	endpoint, accessKey, secretKey := startMinIO(t, ctx)
	bucket := objectstore.DefaultBucket

	raw, err := minio.New(strings.TrimPrefix(endpoint, "http://"), &minio.Options{
		Creds:  credentials.NewStaticV4(accessKey, secretKey, ""),
		Region: objectstore.DefaultRegion,
	})
	if err != nil {
		t.Fatalf("minio client: %v", err)
	}
	if err := raw.MakeBucket(ctx, bucket, minio.MakeBucketOptions{Region: objectstore.DefaultRegion}); err != nil {
		t.Fatalf("make bucket: %v", err)
	}
	putBlob := func(key string) {
		t.Helper()
		content := []byte("content")
		if _, err := raw.PutObject(ctx, bucket, key, bytes.NewReader(content), int64(len(content)), minio.PutObjectOptions{}); err != nil {
			t.Fatalf("put %s: %v", key, err)
		}
	}
	hashA, hashB, hashC := strings.Repeat("a", 64), strings.Repeat("b", 64), strings.Repeat("c", 64)
	putBlob("cas/thread/org-1/thread-1/" + hashA)
	putBlob("cas/thread/org-1/thread-1/" + hashB)
	putBlob("cas/thread/org-1/thread-10/" + hashC)
	putBlob("cas/thread/org-1/thread-2/" + hashA)

	threadStore, err := threadworkspace.NewStore(pool)
	if err != nil {
		t.Fatalf("thread store: %v", err)
	}
	rowsEraser, err := threadworkspace.NewEraser(pool)
	if err != nil {
		t.Fatalf("eraser: %v", err)
	}
	blobs, err := objectstore.New(objectstore.Config{
		Endpoint: endpoint, AccessKey: accessKey, SecretKey: secretKey, Bucket: bucket, Region: objectstore.DefaultRegion,
	})
	if err != nil {
		t.Fatalf("object store client: %v", err)
	}
	limits := threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20}
	apply := func(owner, threadID string, files ...threadworkspace.Change) {
		t.Helper()
		if _, err := threadStore.Apply(ctx, "org-1", owner, threadID, files, nil, limits); err != nil {
			t.Fatalf("apply %s: %v", threadID, err)
		}
	}
	apply("user-1", "thread-1",
		threadworkspace.Change{Path: "a.txt", ContentHash: "sha256:" + hashA, SizeBytes: 7},
		threadworkspace.Change{Path: "b.txt", ContentHash: "sha256:" + hashB, SizeBytes: 7},
	)
	apply("user-1", "thread-10", threadworkspace.Change{Path: "c.txt", ContentHash: "sha256:" + hashC, SizeBytes: 7})
	// thread-2 is held by two owners: the retention erase must not touch it.
	apply("user-1", "thread-2", threadworkspace.Change{Path: "a.txt", ContentHash: "sha256:" + hashA, SizeBytes: 7})
	apply("user-2", "thread-2", threadworkspace.Change{Path: "a.txt", ContentHash: "sha256:" + hashA, SizeBytes: 7})

	s := NewServer(NewMemoryLeaseStore(), NewMemorySnapshotStore(), NewMemoryWorkspaceStore())
	who := retentionService()
	s.principal = func(context.Context) (authctx.Principal, error) { return who, nil }
	s.WithThreadWorkspace(true, threadStore, limits).WithThreadErasure(rowsEraser, blobs)

	untouched := func(step string) {
		t.Helper()
		if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-1"); n != 2 {
			t.Fatalf("%s changed thread-1: %d files left, want 2", step, n)
		}
		if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-1/"); n != 2 {
			t.Fatalf("%s removed thread-1 blobs: %d left, want 2", step, n)
		}
	}

	// Switch off: refused, nothing removed.
	_, err = s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-1", "user-1")
	assertStatusCode(t, err, codes.FailedPrecondition)
	untouched("the switch off")

	s.WithServiceThreadErase(true)

	// A service without the scope, and a user, are refused.
	who = authctx.Principal{OrganizationID: "org-1", ActorID: "service:execution-core", PrincipalType: "service", Scopes: []string{authz.ScopeWrite}}
	_, err = s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-1", "user-1")
	assertStatusCode(t, err, codes.PermissionDenied)
	untouched("a service without the scope")
	who = authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}
	_, err = s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-1", "user-1")
	assertStatusCode(t, err, codes.PermissionDenied)
	untouched("a user principal")

	// A service cannot name another organization.
	who = retentionService()
	_, err = s.eraseThreadWorkspaceForRetention(ctx, "org-2", "thread-1", "user-1")
	assertStatusCode(t, err, codes.PermissionDenied)
	untouched("a cross-organization request")

	// A wrong owner_id finds another owner's rows on the thread and removes nothing.
	_, err = s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-1", "user-2")
	assertStatusCode(t, err, codes.PermissionDenied)
	untouched("a wrong owner")

	// A thread two owners hold is not erased, and its blobs stay.
	_, err = s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-2", "user-1")
	assertStatusCode(t, err, codes.PermissionDenied)
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-2"); n != 2 {
		t.Fatalf("a shared thread lost rows: %d left, want 2", n)
	}
	if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-2/"); n != 1 {
		t.Fatalf("a shared thread lost blobs: %d left, want 1", n)
	}

	// The scoped service erases the thread's rows and its prefix.
	files, err := s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-1", "user-1")
	if err != nil {
		t.Fatalf("retention erase: %v", err)
	}
	if files != 2 {
		t.Fatalf("files deleted = %d, want 2", files)
	}
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-1"); n != 0 {
		t.Fatalf("thread-1 still holds %d file rows", n)
	}
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspaces WHERE thread_id = $1`, "thread-1"); n != 0 {
		t.Fatal("thread-1 still holds its header row")
	}
	if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-1/"); n != 0 {
		t.Fatalf("thread-1 prefix still holds %d blobs", n)
	}

	// The sibling whose id extends it is untouched.
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-10"); n != 1 {
		t.Fatalf("thread-10 lost its file rows: %d left", n)
	}
	if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-10/"); n != 1 {
		t.Fatalf("thread-10 lost its blobs: %d left", n)
	}

	// A retry finds nothing left and completes without error.
	files, err = s.eraseThreadWorkspaceForRetention(ctx, "org-1", "thread-1", "user-1")
	if err != nil || files != 0 {
		t.Fatalf("retry = (%d files, %v); want a clean no-op", files, err)
	}
}
