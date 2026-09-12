//go:build integration

package lease

import (
	"context"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// setupLeaseDB spins a throwaway Postgres and applies the migrations the
// lease/snapshot stores need, mirroring capability-core's
// scope_store_integration_test.go's own setupRegistryDB.
func setupLeaseDB(t *testing.T) string {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

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
		t.Fatalf("conn string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	for _, migration := range []string{
		"0001_workspace_manifest.up.sql",
		"0002_lease_and_snapshot_store.up.sql",
	} {
		sqlBytes, readErr := os.ReadFile(filepath.Join("..", "..", "migrations", migration))
		if readErr != nil {
			t.Fatalf("read migration %s: %v", migration, readErr)
		}
		if _, applyErr := pool.Exec(ctx, string(sqlBytes)); applyErr != nil {
			t.Fatalf("apply migration %s: %v", migration, applyErr)
		}
	}
	pool.Close()
	return dsn
}

func newPoolStore(t *testing.T, dsn string) *Store {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	return store
}

func TestLeaseStore_FullLifecycle(t *testing.T) {
	dsn := setupLeaseDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()

	l, err := store.Create(ctx, "scope-1", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	if l.State != mpv1.SandboxLifecycleState_SCRATCH {
		t.Fatalf("State = %v, want SCRATCH", l.State)
	}

	// SCRATCH blocks snapshot for a Space-scoped lease.
	if _, err := store.BeginSnapshot(ctx, l.ID, "org-a", "user-a", "backend-a"); err != ErrLeaseNotActivated {
		t.Fatalf("BeginSnapshot on SCRATCH: error = %v, want ErrLeaseNotActivated", err)
	}

	activated, err := store.Activate(ctx, l.ID, "org-a", "user-a", "backend-a")
	if err != nil || activated.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("Activate: lease = %+v, err = %v", activated, err)
	}

	snapshotting, err := store.BeginSnapshot(ctx, l.ID, "org-a", "user-a", "backend-a")
	if err != nil || snapshotting.State != mpv1.SandboxLifecycleState_SNAPSHOTTING {
		t.Fatalf("BeginSnapshot: lease = %+v, err = %v", snapshotting, err)
	}
	store.EndSnapshot(ctx, l.ID)
	backToActive, err := store.GetScoped(ctx, l.ID, "org-a", "user-a", "backend-a")
	if err != nil || backToActive.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("after EndSnapshot: lease = %+v, err = %v", backToActive, err)
	}

	if ok, err := store.ReleaseScoped(ctx, l.ID, "org-a", "user-a", "backend-a"); err != nil || !ok {
		t.Fatalf("ReleaseScoped: ok = %v, err = %v", ok, err)
	}
	if _, err := store.GetScoped(ctx, l.ID, "org-a", "user-a", "backend-a"); err != ErrLeaseNotFound {
		t.Fatalf("GetScoped after release: error = %v, want ErrLeaseNotFound", err)
	}
	// Idempotent release.
	if ok, err := store.ReleaseScoped(ctx, l.ID, "org-a", "user-a", "backend-a"); err != nil || !ok {
		t.Fatalf("second ReleaseScoped: ok = %v, err = %v", ok, err)
	}
	if _, err := store.BeginSnapshot(ctx, l.ID, "org-a", "user-a", "backend-a"); err != ErrLeaseNotFound {
		t.Fatalf("BeginSnapshot after destroy: error = %v, want ErrLeaseNotFound", err)
	}
}

// TestLeaseStore_GetAnyResolvesALeaseAfterItIsReleased is PromoteWorkspace's
// own real-Postgres proof: ReleaseLease only ever marks the leases row
// DESTROYED (it never touches workspace_files), and GetAny must still
// resolve that lease's SpaceID afterward — unlike GetScoped, which the
// FullLifecycle test above already confirmed returns ErrLeaseNotFound for
// the exact same destroyed lease.
func TestLeaseStore_GetAnyResolvesALeaseAfterItIsReleased(t *testing.T) {
	dsn := setupLeaseDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()

	l, err := store.Create(ctx, "scope-1", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	if ok, err := store.ReleaseScoped(ctx, l.ID, "org-a", "user-a", "backend-a"); err != nil || !ok {
		t.Fatalf("ReleaseScoped: ok = %v, err = %v", ok, err)
	}

	if _, err := store.GetScoped(ctx, l.ID, "org-a", "user-a", "backend-a"); err != ErrLeaseNotFound {
		t.Fatalf("GetScoped after release: error = %v, want ErrLeaseNotFound (the gap GetAny exists to work around)", err)
	}

	resolved, err := store.GetAny(ctx, l.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatalf("GetAny after release: unexpected error: %v", err)
	}
	if resolved.SpaceID != "space-a" {
		t.Fatalf("SpaceID = %q, want space-a", resolved.SpaceID)
	}
	if resolved.State != mpv1.SandboxLifecycleState_DESTROYED {
		t.Fatalf("State = %v, want DESTROYED (GetAny does not hide it, just doesn't exclude it)", resolved.State)
	}
}

// TestLeaseStore_SurvivesAFreshPoolAgainstTheSameDatabase is the S3.2 close-out
// design's own named "restart" gap, now actually closed: a lease created
// through one pool/Store instance must still be visible through a
// completely separate pool/Store instance connected to the SAME database —
// simulating a process restart. This is the direct, opposite-outcome
// counterpart to the old in-memory store's TestFreshStoreRejectsPreRestartLeaseID
// (correct for a store with no persistence at all; the whole point of this
// port is that this assertion now flips).
func TestLeaseStore_SurvivesAFreshPoolAgainstTheSameDatabase(t *testing.T) {
	dsn := setupLeaseDB(t)
	before := newPoolStore(t, dsn)
	created, err := before.Create(context.Background(), "scope-1", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatalf("Create: %v", err)
	}

	after := newPoolStore(t, dsn)
	found, err := after.GetScoped(context.Background(), created.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatalf("GetScoped from a fresh pool: %v", err)
	}
	if found.ID != created.ID || found.State != mpv1.SandboxLifecycleState_SCRATCH {
		t.Fatalf("lease did not survive: %+v", found)
	}
}

// TestLeaseStore_ConcurrentCreateIsRaceFree is the S3.2 close-out design's
// named "concurrent provision" scenario, now against the real durable store.
func TestLeaseStore_ConcurrentCreateIsRaceFree(t *testing.T) {
	dsn := setupLeaseDB(t)
	store := newPoolStore(t, dsn)
	const concurrency = 20
	var wg sync.WaitGroup
	ids := make([]string, concurrency)
	errs := make([]error, concurrency)
	for i := 0; i < concurrency; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			l, err := store.Create(context.Background(), "scope-a", "agent", "org-a", "user-a", "", "", time.Minute)
			errs[i] = err
			if l != nil {
				ids[i] = l.ID
			}
		}(i)
	}
	wg.Wait()

	seen := make(map[string]bool, concurrency)
	for i, err := range errs {
		if err != nil {
			t.Fatalf("Create[%d]: unexpected error: %v", i, err)
		}
		if ids[i] == "" || seen[ids[i]] {
			t.Fatalf("Create[%d]: empty or duplicate id %q", i, ids[i])
		}
		seen[ids[i]] = true
	}
}
