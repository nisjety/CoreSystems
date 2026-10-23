//go:build integration

package grant

// Integration tests for PgStore and Routed against a real PostgreSQL 16.
//
// Database: TEST_DATABASE_URL when set (any database on a server the test may
// CREATE DATABASE on), otherwise a postgres:16-alpine container started with
// testcontainers-go. Each test gets its own fresh database with migration
// 0001 applied.
//
//	GOWORK=off go test -tags integration ./internal/grant/...

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

var adminDSN string

func TestMain(m *testing.M) {
	adminDSN = os.Getenv("TEST_DATABASE_URL")
	terminate := func() {}
	if adminDSN == "" {
		ctx, cancel := context.WithTimeout(context.Background(), 180*time.Second)
		container, err := tcpostgres.Run(ctx,
			"postgres:16-alpine",
			tcpostgres.WithDatabase("browser_broker"),
			tcpostgres.WithUsername("test"),
			tcpostgres.WithPassword("test"),
			testcontainers.WithWaitStrategy(
				wait.ForLog("database system is ready to accept connections").
					WithOccurrence(2).WithStartupTimeout(90*time.Second),
			),
		)
		if err != nil {
			cancel()
			fmt.Fprintln(os.Stderr, "start postgres (set TEST_DATABASE_URL to use an existing server):", err)
			os.Exit(1)
		}
		adminDSN, err = container.ConnectionString(ctx, "sslmode=disable")
		cancel()
		if err != nil {
			fmt.Fprintln(os.Stderr, "postgres connection string:", err)
			os.Exit(1)
		}
		terminate = func() {
			c, cc := context.WithTimeout(context.Background(), 30*time.Second)
			defer cc()
			_ = container.Terminate(c)
		}
	}
	code := m.Run()
	terminate()
	os.Exit(code)
}

func migrationSQL(t *testing.T, name string) string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	return string(b)
}

func applyMigration(t *testing.T, pool *pgxpool.Pool, name string) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), migrationSQL(t, name)); err != nil {
		t.Fatalf("apply %s: %v", name, err)
	}
}

// newTestDB creates a fresh database with 0001 and 0003 applied and returns a pool on it.
func newTestDB(t *testing.T) (*pgxpool.Pool, *pgxpool.Config) {
	t.Helper()
	bg := context.Background()
	b := make([]byte, 6)
	_, _ = rand.Read(b)
	name := "bb_" + hex.EncodeToString(b)
	admin, err := pgx.Connect(bg, adminDSN)
	if err != nil {
		t.Fatalf("admin connect: %v", err)
	}
	if _, err := admin.Exec(bg, "CREATE DATABASE "+name); err != nil {
		t.Fatalf("create database: %v", err)
	}
	_ = admin.Close(bg)
	cfg, err := pgxpool.ParseConfig(adminDSN)
	if err != nil {
		t.Fatal(err)
	}
	cfg.ConnConfig.Database = name
	cfg.MaxConns = 16
	pool, err := pgxpool.NewWithConfig(bg, cfg.Copy())
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(func() {
		pool.Close()
		admin, err := pgx.Connect(bg, adminDSN)
		if err == nil {
			_, _ = admin.Exec(bg, "DROP DATABASE IF EXISTS "+name+" WITH (FORCE)")
			_ = admin.Close(bg)
		}
	})
	applyMigration(t, pool, "0001_browser_grants.up.sql")
	applyMigration(t, pool, "0003_browser_grants_budget.up.sql")
	return pool, cfg
}

// stores builds the production shape, Routed{PgStore, MemoryRepository},
// both reading one fake clock.
func stores(pool *pgxpool.Pool) (*PgStore, *MemoryRepository, *Routed, *fakeClock) {
	clock := &fakeClock{now: time.Date(2026, 10, 18, 12, 0, 0, 0, time.UTC)}
	pg := NewPgStore(pool)
	pg.now = clock.Now
	mem := NewMemoryRepository()
	mem.now = clock.Now
	return pg, mem, NewRouted(pg, mem), clock
}

func rowCount(t *testing.T, pool *pgxpool.Pool, where string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), "SELECT count(*) FROM browser_grants "+where, args...).Scan(&n); err != nil {
		t.Fatalf("count %q: %v", where, err)
	}
	return n
}

// The WS1.B Derive and approval contract, on the production store shape.
func TestPg_DeriveContract(t *testing.T) {
	for _, c := range deriveContract {
		t.Run(c.name, func(t *testing.T) {
			pool, _ := newTestDB(t)
			c.run(t, func(*testing.T) (Repository, *fakeClock) {
				_, _, routed, clock := stores(pool)
				return routed, clock
			})
		})
	}
}

// The WS6.E budget-claim contract, on the production store shape. Run grants
// and sessions land in browser_grants, ZDR ones in memory.
func TestPg_BudgetContract(t *testing.T) {
	for _, c := range budgetContract {
		t.Run(c.name, func(t *testing.T) {
			pool, _ := newTestDB(t)
			c.run(t, func(*testing.T) (Repository, *fakeClock) {
				_, _, routed, clock := stores(pool)
				return routed, clock
			})
		})
	}
}

func TestPg_BudgetClaimIsDurableNullableAndConstrained(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, clock := stores(pool)

	parent := session(t, store, CreateSessionParams{
		TTL: 15 * time.Minute, Budget: claim(5_000_000, 0, 600, BudgetSourceOrgDefault),
	})
	bare := session(t, store, CreateSessionParams{SessionKey: "bare", TTL: 15 * time.Minute})
	run, err := store.Derive(ctx, withBudget(parent, "run", claim(0, 12, 0, BudgetSourceRequest)))
	if err != nil {
		t.Fatal(err)
	}

	// A zero bound is stored as NULL, a bound as its value, never as 0.
	type row struct {
		micro, steps, runtime *int64
		source                *string
		minted                *int64
	}
	read := func(id string) row {
		var r row
		if err := pool.QueryRow(ctx, `SELECT budget_micro_usd, budget_max_steps::bigint, budget_max_runtime_s::bigint,
			budget_source, budget_minted_at_unix FROM browser_grants WHERE id = $1`, id).
			Scan(&r.micro, &r.steps, &r.runtime, &r.source, &r.minted); err != nil {
			t.Fatal(err)
		}
		return r
	}
	if r := read(bare.ID); r.micro != nil || r.steps != nil || r.runtime != nil || r.source != nil || r.minted != nil {
		t.Fatalf("an unclaimed grant stores %#v, want all NULL", r)
	}
	r := read(parent.ID)
	if r.micro == nil || *r.micro != 5_000_000 || r.steps != nil || r.runtime == nil || *r.runtime != 600 ||
		r.source == nil || *r.source != BudgetSourceOrgDefault || r.minted == nil || *r.minted != clock.Now().Unix() {
		t.Fatalf("parent row = %#v", r)
	}
	r = read(run.ID)
	if r.micro == nil || *r.micro != 5_000_000 || r.steps == nil || *r.steps != 12 || r.runtime == nil || *r.runtime != 600 ||
		r.source == nil || *r.source != BudgetSourceRequest {
		t.Fatalf("run row = %#v", r)
	}

	// A new broker instance on the same database serves the same claim.
	_, _, restarted, _ := stores(pool)
	got, err := restarted.GetScoped(ctx, run.ID, run.OrgID, run.OwnerID)
	if err != nil || got.Budget != run.Budget {
		t.Fatalf("after restart: %#v, %v (want %#v)", got, err, run.Budget)
	}

	// The schema refuses what the Go layer would never write. Each statement
	// runs against a row where only the CHECK under test can refuse it: on the
	// unclaimed row the table-level "a source labels a bound" CHECK would fire
	// first and the column CHECKs would never be the reason. The parent already
	// carries a source, a cost bound and a runtime bound (and no step bound), so
	// that table constraint still holds after each of these.
	for name, stmt := range map[string]string{
		"a zero bound":          `UPDATE browser_grants SET budget_max_steps = 0 WHERE id = $1`,
		"a negative bound":      `UPDATE browser_grants SET budget_micro_usd = -5 WHERE id = $1`,
		"a zero cost bound":     `UPDATE browser_grants SET budget_micro_usd = 0 WHERE id = $1`,
		"a negative step bound": `UPDATE browser_grants SET budget_max_steps = -1 WHERE id = $1`,
		"a negative runtime":    `UPDATE browser_grants SET budget_max_runtime_s = -60 WHERE id = $1`,
		"an unknown source":     `UPDATE browser_grants SET budget_source = 'client' WHERE id = $1`,
		"a negative stamp":      `UPDATE browser_grants SET budget_minted_at_unix = -1 WHERE id = $1`,
		"a zero stamp":          `UPDATE browser_grants SET budget_minted_at_unix = 0 WHERE id = $1`,
	} {
		if _, err := pool.Exec(ctx, stmt, parent.ID); err == nil {
			t.Fatalf("%s was accepted by the schema", name)
		}
	}
	// The table-level CHECK, on the row that has nothing: a source with no bound.
	if _, err := pool.Exec(ctx, `UPDATE browser_grants SET budget_source = 'request' WHERE id = $1`, bare.ID); err == nil {
		t.Fatal("a source with no bound was accepted by the schema")
	}
	if _, err := pool.Exec(ctx, `UPDATE browser_grants SET budget_micro_usd = NULL, budget_max_runtime_s = NULL WHERE id = $1`, parent.ID); err == nil {
		t.Fatal("clearing every bound but keeping the source was accepted by the schema")
	}

	// A ZDR grant's claim never reaches the table.
	zdr := session(t, store, CreateSessionParams{
		SessionKey: "zdr", TTL: 15 * time.Minute, Zdr: true, Budget: claim(1, 1, 1, BudgetSourceRequest),
	})
	if n := rowCount(t, pool, "WHERE id = $1", zdr.ID); n != 0 {
		t.Fatalf("a ZDR grant with a claim has %d rows in browser_grants", n)
	}
	if n := rowCount(t, pool, "WHERE budget_source IS NOT NULL AND org_id = $1", "org-a"); n != 2 {
		t.Fatalf("rows with a claim = %d, want 2 (the parent and the run)", n)
	}
}

func TestPg_CreateGetExpireRevoke(t *testing.T) {
	pool, _ := newTestDB(t)
	pg, _, _, clock := stores(pool)

	g, err := pg.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "s1", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"Example.com", "api.example.org"}, TTL: 5 * time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	got, err := pg.GetScoped(ctx, g.ID, "org-a", "user-a")
	if err != nil {
		t.Fatal(err)
	}
	if got.ID != g.ID || got.Kind != KindSession || got.SessionKey != "s1" || got.ScopeURL != "browser://cloud" ||
		!sameStrings(got.AllowedDomains, []string{"api.example.org", "example.com"}) || !got.ExpiresAt.Equal(g.ExpiresAt) ||
		got.ParentGrantID != "" || got.Revoked || got.Zdr || len(got.AllowedActions) != 0 {
		t.Fatalf("read back %#v, created %#v", got, g)
	}
	var absEqual bool
	if err := pool.QueryRow(ctx, "SELECT absolute_expires_at = expires_at FROM browser_grants WHERE id = $1", g.ID).Scan(&absEqual); err != nil || !absEqual {
		t.Fatalf("absolute_expires_at = expires_at: %v, %v", absEqual, err)
	}

	clock.Advance(5*time.Minute + time.Second)
	if _, err := pg.GetScoped(ctx, g.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("expired grant: %v", err)
	}

	h, err := pg.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "s2", AllowedDomains: []string{"example.com"}, TTL: time.Minute,
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := pg.RevokeScoped(ctx, h.ID, "org-a", "user-a", "  session_closed  "); err != nil {
		t.Fatal(err)
	}
	revoked, err := pg.fetch(ctx, pool, "test", h.ID, "")
	if err != nil || !revoked.Revoked || revoked.RevokedReason != "session_closed" {
		t.Fatalf("revoked row = %#v, %v", revoked, err)
	}
	if _, err := pg.GetScoped(ctx, h.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("revoked grant: %v", err)
	}
	// Revoking again is accepted (idempotent), as in the memory store.
	if err := pg.RevokeScoped(ctx, h.ID, "org-a", "user-a", "again"); err != nil {
		t.Fatalf("second revoke: %v", err)
	}
	if err := pg.RevokeScoped(ctx, "0123456789abcdef0123456789abcdef", "org-a", "user-a", "x"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("revoke of an unknown id: %v", err)
	}
}

func TestPg_RevokeCascadesWholeSubtree(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, clock := stores(pool)
	parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute, AllowedDomains: []string{"example.com"}})
	r1, err := store.Derive(ctx, deriveParams(parent, "r1"))
	if err != nil {
		t.Fatal(err)
	}
	r2, err := store.Derive(ctx, deriveParams(parent, "r2"))
	if err != nil {
		t.Fatal(err)
	}
	a1, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(r1, []string{"example.com"}, time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	a2, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(parent, []string{"example.com"}, time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	sibling := session(t, store, CreateSessionParams{SessionKey: "sibling", TTL: 10 * time.Minute})
	siblingRun, err := store.Derive(ctx, deriveParams(sibling, "r1"))
	if err != nil {
		t.Fatal(err)
	}

	// Revoking a leaf touches only the leaf.
	if err := store.RevokeScoped(ctx, r2.ID, parent.OrgID, parent.OwnerID, "run_finished"); err != nil {
		t.Fatal(err)
	}
	if n := rowCount(t, pool, "WHERE revoked_at IS NOT NULL"); n != 1 {
		t.Fatalf("after a leaf revoke %d rows are revoked, want 1", n)
	}
	revokedAt := func(id string) time.Time {
		t.Helper()
		var at time.Time
		if err := pool.QueryRow(ctx, `SELECT revoked_at FROM browser_grants WHERE id = $1`, id).Scan(&at); err != nil {
			t.Fatal(err)
		}
		return at
	}
	leafRevokedAt := revokedAt(r2.ID)
	clock.Advance(time.Minute)
	// Revoking the session takes the whole subtree, at every depth.
	if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	for _, g := range []*Grant{parent, r1, r2, a1, a2} {
		if _, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("%s %s after the session revoke: %v", g.Kind, g.ID, err)
		}
	}
	if n := rowCount(t, pool, "WHERE revoked_at IS NOT NULL"); n != 5 {
		t.Fatalf("%d rows revoked, want 5", n)
	}
	// The leaf revoked earlier keeps its first revocation time; the rest of
	// the subtree carries the later one.
	if got := revokedAt(r2.ID); !got.Equal(leafRevokedAt) {
		t.Fatalf("leaf revoked_at moved from %v to %v", leafRevokedAt, got)
	}
	if got := revokedAt(r1.ID); !got.After(leafRevokedAt) {
		t.Fatalf("run revoked_at %v is not after the leaf's %v", got, leafRevokedAt)
	}
	for _, g := range []*Grant{sibling, siblingRun} {
		if _, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); err != nil {
			t.Fatalf("unrelated %s was touched: %v", g.Kind, err)
		}
	}
}

func TestPg_DeriveIsIdempotentUnderConcurrency(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	parent := session(t, store, CreateSessionParams{AllowedDomains: []string{"example.com", "example.org"}, TTL: 10 * time.Minute})

	const n = 24
	var wg sync.WaitGroup
	ids := make([]string, n)
	errs := make([]error, n)
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			hosts := []string{"example.com"}
			if i%2 == 1 {
				hosts = []string{"example.org"}
			}
			g, err := store.Derive(ctx, deriveParams(parent, "same-key", hosts...))
			errs[i] = err
			if err == nil {
				ids[i] = g.ID
			}
		}(i)
	}
	wg.Wait()
	winners := map[string]struct{}{}
	ok, conflicts := 0, 0
	for i := range errs {
		switch {
		case errs[i] == nil:
			ok++
			winners[ids[i]] = struct{}{}
		case errors.Is(errs[i], ErrDeriveConflict):
			conflicts++
		default:
			t.Fatalf("derive %d: unexpected error %v", i, errs[i])
		}
	}
	// One host list wins; every caller with it gets the same grant, every
	// caller with the other gets a conflict.
	if len(winners) != 1 || ok != n/2 || conflicts != n/2 {
		t.Fatalf("winners=%d ok=%d conflicts=%d", len(winners), ok, conflicts)
	}
	if c := rowCount(t, pool, "WHERE kind = 'run'"); c != 1 {
		t.Fatalf("%d run rows for one (parent, key), want 1", c)
	}
}

func TestPg_DeriveRejectsRevokedParentRace(t *testing.T) {
	t.Run("revoke arrives while derive holds the parent", func(t *testing.T) {
		pool, _ := newTestDB(t)
		pg, _, store, _ := stores(pool)
		parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
		revokeDone := make(chan error, 1)
		var once sync.Once
		pg.afterParentLock = func() {
			once.Do(func() {
				go func() {
					revokeDone <- store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed")
				}()
				select {
				case err := <-revokeDone:
					t.Errorf("revoke finished while a derive held the parent FOR SHARE: %v", err)
					revokeDone <- err // keep the later receive from blocking
				case <-time.After(300 * time.Millisecond):
				}
			})
		}
		run, err := store.Derive(ctx, deriveParams(parent, "k"))
		if err != nil {
			t.Fatalf("derive that locked first: %v", err)
		}
		if err := <-revokeDone; err != nil {
			t.Fatalf("revoke: %v", err)
		}
		// The revoke waited, then its recursive update saw the new child.
		if _, err := store.GetScoped(ctx, run.ID, parent.OrgID, parent.OwnerID); !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("run grant outlived its revoked parent: %v", err)
		}
	})
	t.Run("derive arrives while revoke holds the parent", func(t *testing.T) {
		pool, _ := newTestDB(t)
		pg, _, store, _ := stores(pool)
		parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
		deriveDone := make(chan error, 1)
		pg.afterRootLock = func() {
			go func() {
				_, err := store.Derive(ctx, deriveParams(parent, "k"))
				deriveDone <- err
			}()
			select {
			case err := <-deriveDone:
				t.Errorf("derive finished while a revoke held the parent FOR UPDATE: %v", err)
				deriveDone <- err // keep the later receive from blocking
			case <-time.After(300 * time.Millisecond):
			}
		}
		if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
			t.Fatal(err)
		}
		if err := <-deriveDone; !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("derive after the revoke committed: %v, want ErrGrantRevoked", err)
		}
		if c := rowCount(t, pool, "WHERE kind = 'run'"); c != 0 {
			t.Fatalf("%d run rows under a revoked parent", c)
		}
	})
	t.Run("approval on a run grant while the session is revoked", func(t *testing.T) {
		pool, _ := newTestDB(t)
		pg, _, store, _ := stores(pool)
		parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
		run, err := store.Derive(ctx, deriveParams(parent, "k"))
		if err != nil {
			t.Fatal(err)
		}
		done := make(chan error, 1)
		pg.afterRootLock = func() {
			go func() {
				_, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute))
				done <- err
			}()
			select {
			case err := <-done:
				t.Errorf("approval finished while its root session was being revoked: %v", err)
				done <- err // keep the later receive from blocking
			case <-time.After(300 * time.Millisecond):
			}
		}
		if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
			t.Fatal(err)
		}
		if err := <-done; !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("approval after the revoke: %v, want ErrGrantRevoked", err)
		}
		if c := rowCount(t, pool, "WHERE kind = 'sensitive_approval'"); c != 0 {
			t.Fatalf("%d approvals under a revoked session", c)
		}
	})
	// The approval's own parent is the session: only the FOR SHARE on that
	// parent row (not the root lock a run-parented approval also takes)
	// stands between it and a revoke.
	t.Run("approval directly under the session while the session is revoked", func(t *testing.T) {
		pool, _ := newTestDB(t)
		pg, _, store, _ := stores(pool)
		parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
		done := make(chan error, 1)
		pg.afterRootLock = func() {
			go func() {
				_, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(parent, []string{"example.com"}, time.Minute))
				done <- err
			}()
			select {
			case err := <-done:
				t.Errorf("approval finished while its session was being revoked: %v", err)
				done <- err // keep the later receive from blocking
			case <-time.After(300 * time.Millisecond):
			}
		}
		if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
			t.Fatal(err)
		}
		if err := <-done; !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("approval after the session revoke: %v, want ErrGrantRevoked", err)
		}
		if c := rowCount(t, pool, "WHERE kind = 'sensitive_approval'"); c != 0 {
			t.Fatalf("%d approvals under a revoked session", c)
		}
	})
}

// Concurrent revokes of a session and of its own run, repeated (a client
// retry beside a session close), used to deadlock: they locked the same rows
// in different orders and PostgreSQL aborted one, so a revoke failed with
// Unavailable and its parent stayed live. None may fail now, and the subtree
// must end revoked.
func TestPg_ConcurrentRevokesOfSessionAndRunNeverFail(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	const iterations = 150
	failures := 0
	deadlocksBefore := deadlockRetries.Load()
	for i := 0; i < iterations; i++ {
		parent := session(t, store, CreateSessionParams{SessionKey: fmt.Sprintf("s%d", i), TTL: 10 * time.Minute})
		run, err := store.Derive(ctx, deriveParams(parent, "run"))
		if err != nil {
			t.Fatal(err)
		}
		ids := []string{run.ID, parent.ID}
		for a := 0; a < 3; a++ {
			ap, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute))
			if err != nil {
				t.Fatal(err)
			}
			ids = append(ids, ap.ID)
		}
		start := make(chan struct{})
		var wg sync.WaitGroup
		var mu sync.Mutex
		for _, id := range []string{parent.ID, run.ID, parent.ID, run.ID} {
			wg.Add(1)
			go func(id string) {
				defer wg.Done()
				<-start
				if err := store.RevokeScoped(ctx, id, parent.OrgID, parent.OwnerID, "close"); err != nil {
					mu.Lock()
					failures++
					mu.Unlock()
					t.Errorf("iteration %d: revoke of %s: %v", i, id, err)
				}
			}(id)
		}
		close(start)
		wg.Wait()
		for _, id := range ids {
			if c := rowCount(t, pool, "WHERE id = $1 AND revoked_at IS NOT NULL", id); c != 1 {
				t.Fatalf("iteration %d: grant %s is still live after both revokes", i, id)
			}
		}
	}
	if failures > 0 {
		t.Fatalf("%d concurrent revokes failed over %d iterations", failures, iterations)
	}
	// The retry in inTx would mask a deadlock (the revoke succeeds on the
	// second try), so the advisory lock itself is proven by no deadlock
	// having happened at all.
	if n := deadlockRetries.Load() - deadlocksBefore; n != 0 {
		t.Fatalf("%d transactions were aborted as deadlock victims and retried; the revoke lock order is broken", n)
	}
}

// A caller string PostgreSQL cannot store never turns into a store outage and
// never leaves a grant live: a revoke with a NUL in its reason still revokes,
// and a NUL in a key or id is invalid input (or not found), not Unavailable.
func TestPg_UnstorableTextIsNotAStoreOutage(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
	if err := store.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "bad\x00reason\xff"); err != nil {
		t.Fatalf("revoke with a NUL reason: %v", err)
	}
	if _, err := store.GetScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("grant after a NUL-reason revoke: %v, want ErrGrantRevoked", err)
	}
	var reason string
	if err := pool.QueryRow(ctx, `SELECT revoked_reason FROM browser_grants WHERE id = $1`, parent.ID).Scan(&reason); err != nil || reason != "badreason" {
		t.Fatalf("stored reason %q, %v", reason, err)
	}
	if _, err := store.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "s\x00k", ScopeURL: "browser://cloud", TTL: time.Minute,
	}); !errors.Is(err, ErrInvalidGrantText) || errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("create with a NUL session key: %v, want ErrInvalidGrantText", err)
	}
	live := session(t, store, CreateSessionParams{SessionKey: "live", TTL: 10 * time.Minute})
	if _, err := store.Derive(ctx, deriveParams(live, "r\x00")); !errors.Is(err, ErrInvalidGrantText) {
		t.Fatalf("derive with a NUL key: %v, want ErrInvalidGrantText", err)
	}
	if _, err := store.GetScoped(ctx, "g_\x00", live.OrgID, live.OwnerID); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("get with a NUL id: %v, want ErrGrantNotFound", err)
	}
	if err := store.RevokeScoped(ctx, "g_\x00", live.OrgID, live.OwnerID, "x"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("revoke with a NUL id: %v, want ErrGrantNotFound", err)
	}
}

// The plan names this test for a per-owner cap. Under the WS1.B review
// (73ad8767), run grants are bounded per parent session (8 live) and do not
// count toward the per-owner cap, which bounds session grants (20 by
// default). Both caps are checked here, and both are exact under concurrency.
func TestPg_PerOwnerCapIsExactUnderConcurrency(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)

	const attempts = 24
	run := func(f func(i int) error) (ok, refused int) {
		var wg sync.WaitGroup
		errs := make([]error, attempts)
		for i := 0; i < attempts; i++ {
			wg.Add(1)
			go func(i int) { defer wg.Done(); errs[i] = f(i) }(i)
		}
		wg.Wait()
		for i, err := range errs {
			switch {
			case err == nil:
				ok++
			case errors.Is(err, ErrTooManyGrants):
				refused++
			default:
				t.Fatalf("attempt %d: unexpected error %v", i, err)
			}
		}
		return ok, refused
	}

	const ownerCap = 5
	ok, refused := run(func(i int) error {
		_, err := store.CreateSession(ctx, CreateSessionParams{
			OrgID: "org-a", OwnerID: "user-cap", SessionKey: fmt.Sprintf("s%d", i),
			AllowedDomains: []string{"example.com"}, TTL: 10 * time.Minute, MaxActivePerOwner: ownerCap,
		})
		return err
	})
	if ok != ownerCap || refused != attempts-ownerCap {
		t.Fatalf("sessions: ok=%d refused=%d, want %d/%d", ok, refused, ownerCap, attempts-ownerCap)
	}
	if c := rowCount(t, pool, "WHERE owner_id = 'user-cap' AND kind = 'session'"); c != ownerCap {
		t.Fatalf("%d session rows, want %d", c, ownerCap)
	}

	parent := session(t, store, CreateSessionParams{OwnerID: "user-runs", TTL: 10 * time.Minute})
	ok, refused = run(func(i int) error {
		_, err := store.Derive(ctx, deriveParams(parent, fmt.Sprintf("run-%d", i)))
		return err
	})
	if ok != DefaultMaxActiveRunsPerParent || refused != attempts-DefaultMaxActiveRunsPerParent {
		t.Fatalf("runs: ok=%d refused=%d, want %d/%d", ok, refused, DefaultMaxActiveRunsPerParent, attempts-DefaultMaxActiveRunsPerParent)
	}
	if c := rowCount(t, pool, "WHERE parent_grant_id = $1 AND kind = 'run'", parent.ID); c != DefaultMaxActiveRunsPerParent {
		t.Fatalf("%d run rows, want %d", c, DefaultMaxActiveRunsPerParent)
	}
	// Live runs did not consume the owner's session cap.
	if _, err := store.CreateSession(ctx, CreateSessionParams{
		OrgID: parent.OrgID, OwnerID: parent.OwnerID, SessionKey: "after-runs",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, MaxActivePerOwner: 2,
	}); err != nil {
		t.Fatalf("a second session after %d runs: %v", DefaultMaxActiveRunsPerParent, err)
	}
}

func TestPg_OwnerAndOrgScopingReturnNotFound(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	parent := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
	run, err := store.Derive(ctx, deriveParams(parent, "k"))
	if err != nil {
		t.Fatal(err)
	}
	for _, scope := range []struct{ name, org, owner string }{
		{"other org", "org-b", "user-a"},
		{"other org as a service", "org-b", ""},
		{"other user", "org-a", "user-b"},
	} {
		for _, g := range []*Grant{parent, run} {
			if _, err := store.GetScoped(ctx, g.ID, scope.org, scope.owner); !errors.Is(err, ErrGrantNotFound) {
				t.Fatalf("%s get %s: %v", scope.name, g.Kind, err)
			}
			if err := store.RevokeScoped(ctx, g.ID, scope.org, scope.owner, "x"); !errors.Is(err, ErrGrantNotFound) {
				t.Fatalf("%s revoke %s: %v", scope.name, g.Kind, err)
			}
		}
		p := deriveParams(parent, "foreign")
		p.OrgID, p.OwnerID = scope.org, scope.owner
		if _, err := store.Derive(ctx, p); !errors.Is(err, ErrGrantNotFound) {
			t.Fatalf("%s derive: %v", scope.name, err)
		}
		a := approvalOn(parent, []string{"example.com"}, time.Minute)
		a.OrgID, a.OwnerID = scope.org, scope.owner
		if _, err := store.CreateScopedSensitiveApproval(ctx, a); !errors.Is(err, ErrGrantNotFound) {
			t.Fatalf("%s approval: %v", scope.name, err)
		}
	}
	// The refused revokes changed nothing.
	if n := rowCount(t, pool, "WHERE revoked_at IS NOT NULL"); n != 0 {
		t.Fatalf("%d rows revoked by foreign callers", n)
	}
	// A scoped service inside the tenant sees the grant.
	if _, err := store.GetScoped(ctx, run.ID, "org-a", ""); err != nil {
		t.Fatalf("in-tenant service get: %v", err)
	}
}

func TestPg_ZdrCreateWritesZeroRows(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	zdr := session(t, store, CreateSessionParams{Zdr: true, TTL: 10 * time.Minute})
	run, err := store.Derive(ctx, deriveParams(zdr, "k"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute)); err != nil {
		t.Fatal(err)
	}
	if err := store.RevokeScoped(ctx, zdr.ID, zdr.OrgID, zdr.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	if n := rowCount(t, pool, ""); n != 0 {
		t.Fatalf("ZDR grants wrote %d rows", n)
	}
	// A ZDR approval cannot hang off a persistent parent either.
	plain := session(t, store, CreateSessionParams{SessionKey: "plain"})
	raised := approvalOn(plain, []string{"example.com"}, time.Minute)
	raised.Zdr = true
	if _, err := store.CreateScopedSensitiveApproval(ctx, raised); !errors.Is(err, ErrZdrPostureChanged) {
		t.Fatalf("ZDR approval on a persistent parent: %v", err)
	}
	if n := rowCount(t, pool, ""); n != 1 {
		t.Fatalf("%d rows, want only the plain session", n)
	}

	// L1 assertion 7: no zdr column, and the z_ CHECK stays.
	var zdrColumns int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM information_schema.columns
		WHERE table_name = 'browser_grants' AND column_name ILIKE '%zdr%'`).Scan(&zdrColumns); err != nil || zdrColumns != 0 {
		t.Fatalf("zdr columns = %d, %v", zdrColumns, err)
	}
	_, err = pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url,
		allowed_domains, expires_at, absolute_expires_at)
		VALUES ('z_0123', 'session', 'org-a', 'user-a', 's', '', '{example.com}', now(), now())`)
	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) || pgErr.Code != "23514" || pgErr.ConstraintName != "browser_grants_no_zdr_id_check" {
		t.Fatalf("insert of a z_ id: %v, want check violation browser_grants_no_zdr_id_check", err)
	}
	// "z" followed by any other character is an ordinary id: the underscore
	// is escaped in the LIKE pattern.
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url,
		allowed_domains, expires_at, absolute_expires_at)
		VALUES ('zx0123', 'session', 'org-a', 'user-a', 's', '', '{example.com}', now(), now())`); err != nil {
		t.Fatalf("insert of a zx id: %v", err)
	}
}

func TestPg_MigrationIsRerunnable(t *testing.T) {
	pool, _ := newTestDB(t) // applied once already
	_, _, store, _ := stores(pool)
	before := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
	applyMigration(t, pool, "0001_browser_grants.up.sql")
	applyMigration(t, pool, "0001_browser_grants.up.sql")
	applyMigration(t, pool, "0003_browser_grants_budget.up.sql")
	applyMigration(t, pool, "0003_browser_grants_budget.up.sql")
	if _, err := store.GetScoped(ctx, before.ID, before.OrgID, before.OwnerID); err != nil {
		t.Fatalf("a re-applied migration lost a row: %v", err)
	}
	var indexes []string
	rows, err := pool.Query(ctx, `SELECT indexname FROM pg_indexes WHERE tablename = 'browser_grants' ORDER BY indexname`)
	if err != nil {
		t.Fatal(err)
	}
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			t.Fatal(err)
		}
		indexes = append(indexes, name)
	}
	rows.Close()
	want := []string{"browser_grants_expiry_idx", "browser_grants_owner_active_idx", "browser_grants_parent_idx",
		"browser_grants_pkey", "browser_grants_run_idempotency_idx"}
	sort.Strings(want)
	if !sameStrings(indexes, want) {
		t.Fatalf("indexes = %v, want %v", indexes, want)
	}
	applyMigration(t, pool, "0003_browser_grants_budget.down.sql")
	applyMigration(t, pool, "0003_browser_grants_budget.down.sql")
	// 0003 down leaves the 0001 table and its rows intact.
	pool.Reset()
	if _, err := store.GetScoped(ctx, before.ID, before.OrgID, before.OwnerID); err == nil {
		t.Fatal("0003 down must drop the budget columns, which the store selects: the read should fail")
	}
	applyMigration(t, pool, "0003_browser_grants_budget.up.sql")
	pool.Reset()
	if _, err := store.GetScoped(ctx, before.ID, before.OrgID, before.OwnerID); err != nil {
		t.Fatalf("0003 down then up lost the 0001 row: %v", err)
	}
	applyMigration(t, pool, "0003_browser_grants_budget.down.sql")
	applyMigration(t, pool, "0001_browser_grants.down.sql")
	applyMigration(t, pool, "0001_browser_grants.down.sql")
	applyMigration(t, pool, "0001_browser_grants.up.sql")
	applyMigration(t, pool, "0003_browser_grants_budget.up.sql")
	// The table was re-created: drop connections holding cached statements.
	pool.Reset()
	after := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
	if _, err := store.GetScoped(ctx, after.ID, after.OrgID, after.OwnerID); err != nil {
		t.Fatalf("store after down/up: %v", err)
	}
}

// Restart semantics: non-ZDR grants survive a new broker instance on the same
// database, ZDR grants do not.
func TestPg_SurvivesNewRepositoryInstanceOnSameDatabase(t *testing.T) {
	pool1, cfg := newTestDB(t)
	_, _, first, _ := stores(pool1)
	parent := session(t, first, CreateSessionParams{AllowedDomains: []string{"example.com", "example.org"}, TTL: 10 * time.Minute})
	run, err := first.Derive(ctx, deriveParams(parent, "run-1", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	approval, err := first.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	zdr := session(t, first, CreateSessionParams{SessionKey: "zdr", Zdr: true, TTL: 10 * time.Minute})
	pool1.Close()

	pool2, err := pgxpool.NewWithConfig(ctx, cfg.Copy())
	if err != nil {
		t.Fatal(err)
	}
	defer pool2.Close()
	_, _, second, _ := stores(pool2)
	for _, g := range []*Grant{parent, run, approval} {
		got, err := second.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
		if err != nil || got.Kind != g.Kind || !got.ExpiresAt.Equal(g.ExpiresAt) || !sameStrings(got.AllowedDomains, g.AllowedDomains) {
			t.Fatalf("%s after restart: %#v, %v", g.Kind, got, err)
		}
	}
	again, err := second.Derive(ctx, deriveParams(parent, "run-1", "example.com"))
	if err != nil || again.ID != run.ID {
		t.Fatalf("replay after restart: %#v, %v; want %s", again, err, run.ID)
	}
	if _, err := second.GetScoped(ctx, zdr.ID, zdr.OrgID, zdr.OwnerID); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("a ZDR grant survived the restart: %v", err)
	}
	if err := second.RevokeScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID, "session_closed"); err != nil {
		t.Fatal(err)
	}
	if _, err := second.GetScoped(ctx, approval.ID, approval.OrgID, approval.OwnerID); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("approval after a revoke on the new instance: %v", err)
	}
}

func TestPg_PurgeRemovesExpiredAndCascadesChildren(t *testing.T) {
	pool, _ := newTestDB(t)
	pg, _, store, clock := stores(pool)
	old := session(t, store, CreateSessionParams{TTL: 5 * time.Minute})
	child, err := store.Derive(ctx, deriveParams(old, "k"))
	if err != nil {
		t.Fatal(err)
	}
	// The API never lets a child outlive its parent; force it, so the
	// cascade (not the child's own expiry) is what the purge relies on.
	if _, err := pool.Exec(ctx, `UPDATE browser_grants SET expires_at = $2, absolute_expires_at = $2 WHERE id = $1`,
		child.ID, clock.Now().Add(1000*time.Hour)); err != nil {
		t.Fatal(err)
	}
	clock.Advance(25 * time.Hour)
	recent := session(t, store, CreateSessionParams{SessionKey: "recent", TTL: time.Minute})
	clock.Advance(2 * time.Minute) // recent has expired, but not 24 h ago
	live := session(t, store, CreateSessionParams{SessionKey: "live", TTL: 10 * time.Minute})

	n, err := pg.PurgeExpired(ctx, clock.Now().Add(-24*time.Hour))
	if err != nil || n != 1 {
		t.Fatalf("purged %d, %v; want 1", n, err)
	}
	for id, want := range map[string]int{old.ID: 0, child.ID: 0, recent.ID: 1, live.ID: 1} {
		if c := rowCount(t, pool, "WHERE id = $1", id); c != want {
			t.Fatalf("row %s: %d, want %d", id, c, want)
		}
	}
}

// A failing database is ErrStoreUnavailable (gRPC Unavailable), never a grant
// that looks valid and never the database's own error text.
func TestPg_ClosedPoolIsStoreUnavailable(t *testing.T) {
	pool, _ := newTestDB(t)
	pg, _, store, _ := stores(pool)
	g := session(t, store, CreateSessionParams{TTL: 10 * time.Minute})
	pool.Close()
	_, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("get on a closed pool: %v", err)
	}
	if err.Error() != ErrStoreUnavailable.Error() {
		t.Fatalf("store error text %q carries more than the fixed message", err.Error())
	}
	if _, err := store.Derive(ctx, deriveParams(g, "k")); !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("derive on a closed pool: %v", err)
	}
	if err := pg.Ping(ctx); !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("ping on a closed pool: %v", err)
	}
}
