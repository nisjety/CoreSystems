//go:build integration

package server

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"

	"google.golang.org/grpc/codes"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/objectstore"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// minioImage is the image docker-compose.yml runs for the object store.
const minioImage = "minio/minio:RELEASE.2025-09-07T16-13-09Z"

// TestDeleteThreadWorkspaceRemovesRowsAndPrefix erases one thread through the
// server against a throwaway Postgres and MinIO. The thread's rows and its blob
// prefix go. A sibling thread whose id extends it keeps its rows and blobs. Another
// user's request for the same thread is refused with nothing removed.
func TestDeleteThreadWorkspaceRemovesRowsAndPrefix(t *testing.T) {
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
	apply := func(threadID string, files ...threadworkspace.Change) {
		t.Helper()
		if _, err := threadStore.Apply(ctx, "org-1", "user-1", threadID, files, nil, limits); err != nil {
			t.Fatalf("apply %s: %v", threadID, err)
		}
	}
	apply("thread-1",
		threadworkspace.Change{Path: "a.txt", ContentHash: "sha256:" + hashA, SizeBytes: 7},
		threadworkspace.Change{Path: "b.txt", ContentHash: "sha256:" + hashB, SizeBytes: 7},
	)
	apply("thread-10", threadworkspace.Change{Path: "c.txt", ContentHash: "sha256:" + hashC, SizeBytes: 7})

	s := NewServer(NewMemoryLeaseStore(), NewMemorySnapshotStore(), NewMemoryWorkspaceStore())
	who := authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}
	s.principal = func(context.Context) (authctx.Principal, error) { return who, nil }
	s.WithThreadWorkspace(true, threadStore, limits).WithThreadErasure(rowsEraser, blobs)

	// Another user cannot erase a thread another owner holds, and removes nothing.
	who = authctx.Principal{OrganizationID: "org-1", ActorID: "user-2", PrincipalType: "user"}
	_, err = s.DeleteThreadWorkspace(ctx, &DeleteThreadWorkspaceRequest{OrgId: "org-1", ThreadId: "thread-1"})
	assertStatusCode(t, err, codes.PermissionDenied)
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-1"); n != 2 {
		t.Fatalf("another user's erase changed thread-1: %d files left, want 2", n)
	}
	if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-1/"); n != 2 {
		t.Fatalf("another user's erase removed blobs: %d left, want 2", n)
	}

	// The owner's erase removes the thread's rows and its prefix.
	who = authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}
	resp, err := s.DeleteThreadWorkspace(ctx, &DeleteThreadWorkspaceRequest{OrgId: "org-1", ThreadId: "thread-1"})
	if err != nil {
		t.Fatalf("DeleteThreadWorkspace: %v", err)
	}
	if resp.GetFilesDeleted() != 2 || resp.GetObjectPrefix() != "cas/thread/org-1/thread-1/" {
		t.Fatalf("response = %d files at %q", resp.GetFilesDeleted(), resp.GetObjectPrefix())
	}
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-1"); n != 0 {
		t.Fatalf("thread-1 still holds %d file rows", n)
	}
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspaces WHERE thread_id = $1`, "thread-1"); n != 0 {
		t.Fatalf("thread-1 still holds its header row")
	}
	if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-1/"); n != 0 {
		t.Fatalf("thread-1 prefix still holds %d blobs", n)
	}

	// The sibling thread is untouched.
	if n := countRows(t, ctx, pool, `SELECT count(*) FROM thread_workspace_files WHERE thread_id = $1`, "thread-10"); n != 1 {
		t.Fatalf("thread-10 lost its file rows: %d left", n)
	}
	if n := objectsUnder(t, ctx, raw, bucket, "cas/thread/org-1/thread-10/"); n != 1 {
		t.Fatalf("thread-10 lost its blobs: %d left", n)
	}

	// A retry finds nothing left and completes without error.
	resp, err = s.DeleteThreadWorkspace(ctx, &DeleteThreadWorkspaceRequest{OrgId: "org-1", ThreadId: "thread-1"})
	if err != nil || resp.GetFilesDeleted() != 0 {
		t.Fatalf("retry = (%d files, %v); want a clean no-op", resp.GetFilesDeleted(), err)
	}
}

func startThreadPostgres(t *testing.T, ctx context.Context) *pgxpool.Pool {
	t.Helper()
	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("sandbox_manager"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})
	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)
	migration, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0005_thread_workspace.up.sql"))
	if err != nil {
		t.Fatalf("read migration 0005: %v", err)
	}
	if _, err := pool.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("apply migration 0005: %v", err)
	}
	return pool
}

// startMinIO runs a throwaway object store with root credentials generated for
// this test run, so no credential literal lives in the repository.
func startMinIO(t *testing.T, ctx context.Context) (endpoint, accessKey, secretKey string) {
	t.Helper()
	accessKey, secretKey = randomCredential(t), randomCredential(t)
	container, err := testcontainers.Run(ctx, minioImage,
		testcontainers.WithCmd("server", "/data"),
		testcontainers.WithExposedPorts("9000/tcp"),
		testcontainers.WithEnv(map[string]string{"MINIO_ROOT_USER": accessKey, "MINIO_ROOT_PASSWORD": secretKey}),
		testcontainers.WithWaitStrategy(
			wait.ForHTTP("/minio/health/live").WithPort("9000/tcp").WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start minio: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})
	host, err := container.Host(ctx)
	if err != nil {
		t.Fatalf("minio host: %v", err)
	}
	port, err := container.MappedPort(ctx, "9000/tcp")
	if err != nil {
		t.Fatalf("minio port: %v", err)
	}
	return "http://" + net.JoinHostPort(host, port.Port()), accessKey, secretKey
}

func randomCredential(t *testing.T) string {
	t.Helper()
	buf := make([]byte, 16)
	if _, err := rand.Read(buf); err != nil {
		t.Fatalf("random credential: %v", err)
	}
	return hex.EncodeToString(buf)
}

func countRows(t *testing.T, ctx context.Context, pool *pgxpool.Pool, query, threadID string) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(ctx, query, threadID).Scan(&n); err != nil {
		t.Fatalf("count rows: %v", err)
	}
	return n
}

func objectsUnder(t *testing.T, ctx context.Context, client *minio.Client, bucket, prefix string) int {
	t.Helper()
	n := 0
	for obj := range client.ListObjects(ctx, bucket, minio.ListObjectsOptions{Prefix: prefix, Recursive: true}) {
		if obj.Err != nil {
			t.Fatalf("list %s: %v", prefix, obj.Err)
		}
		n++
	}
	return n
}
