package sessionreview

import (
	"context"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// reviewCounterDBEnv names the isolated PostgreSQL database the counter tests run against. Unset, the tests that need
// it are skipped, as the migration tests are. Each test works in a schema of its own and drops it afterwards.
const reviewCounterDBEnv = "CAPABILITY_CORE_REVIEW_COUNTER_TEST_DATABASE_URL"

// refusedReviewCounterKeys are keys the store must never count: every shape the review does not build.
var refusedReviewCounterKeys = []string{
	"",
	"learning-review",
	"learning-review:user:org-1:2026-11-20",
	"learning-review:org:org-1:user-1:2026-11-20",
	"learning-review:user:org-1:user-1:user-2:2026-11-20",
	"learning-review:user::user-1:2026-11-20",
	"learning-review:user:org-1::2026-11-20",
	"learning-review:org::2026-11-20",
	"learning-review:org:org-1:2026-13-01",
	"learning-review:org:org-1:20261120",
	"learning-review:team:org-1:2026-11-20",
	"review:org:org-1:2026-11-20",
}

func TestOrganizationOfReviewCounterKeyAcceptsOnlyTheTwoShapes(t *testing.T) {
	accepted := []struct {
		key, org string
	}{
		{"learning-review:user:org-1:user-1:2026-11-20", "org-1"},
		{"learning-review:org:org-1:2026-11-20", "org-1"},
	}
	for _, tc := range accepted {
		org, err := organizationOfReviewCounterKey(tc.key)
		if err != nil || org != tc.org {
			t.Errorf("organizationOfReviewCounterKey(%q) = %q, %v; want %q", tc.key, org, err, tc.org)
		}
	}
	for _, key := range refusedReviewCounterKeys {
		if org, err := organizationOfReviewCounterKey(key); err == nil {
			t.Errorf("organizationOfReviewCounterKey(%q) = %q; a key of an unknown shape must be refused", key, org)
		}
	}
}

func TestIncrRefusesAnyOtherKeyOrWindowBeforeTheDatabase(t *testing.T) {
	// No pool: a refused call must never reach the database, and a call that could not count must not pretend it did.
	budget := &PgReviewBudget{}
	ctx := context.Background()
	for _, key := range refusedReviewCounterKeys {
		if count, err := budget.Incr(ctx, key, reviewBudgetTTL); err == nil {
			t.Errorf("Incr(%q) = %d; a key of an unknown shape must be refused", key, count)
		}
	}
	if _, err := budget.Incr(ctx, "learning-review:org:org-1:2026-11-20", 0); err == nil {
		t.Error("Incr with a zero window must be refused: a counter that expires at once never reaches a cap")
	}
	if _, err := budget.Incr(ctx, "learning-review:org:org-1:2026-11-20", reviewBudgetTTL); err == nil {
		t.Error("Incr without a database must fail, not count")
	}
}

func TestNewPgReviewBudgetWithoutAPoolStartsNoSweeper(t *testing.T) {
	// A nil pool must not start a goroutine that would sweep nothing every hour; construction itself must not panic.
	budget := NewPgReviewBudget(context.Background(), nil)
	if budget == nil || budget.pool != nil {
		t.Fatal("NewPgReviewBudget(nil pool) must return a store with no pool")
	}
}

// openReviewCounterDB returns a pool whose connections search a schema of their own, with migration 0045 applied.
// The schema is dropped when the test ends.
func openReviewCounterDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	dsn := strings.TrimSpace(os.Getenv(reviewCounterDBEnv))
	if dsn == "" {
		t.Skipf("set %s to an isolated PostgreSQL test database", reviewCounterDBEnv)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	admin, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to the review counter test database: %v", err)
	}
	t.Cleanup(admin.Close)

	schema := "review_counters_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create isolated schema: %v", err)
	}
	t.Cleanup(func() {
		dropCtx, dropCancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer dropCancel()
		_, _ = admin.Exec(dropCtx, "DROP SCHEMA IF EXISTS "+schema+" CASCADE")
	})

	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatalf("parse the review counter test database URL: %v", err)
	}
	if cfg.ConnConfig.RuntimeParams == nil {
		cfg.ConnConfig.RuntimeParams = map[string]string{}
	}
	cfg.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		t.Fatalf("open the isolated schema: %v", err)
	}
	t.Cleanup(pool.Close)

	migration, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0045_learning_review_counters.up.sql"))
	if err != nil {
		t.Fatalf("read migration 0045: %v", err)
	}
	if _, err := pool.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("apply migration 0045: %v", err)
	}
	return pool
}

func TestReviewCounterIncrementsOncePerCallAcrossConcurrentCallers(t *testing.T) {
	pool := openReviewCounterDB(t)
	budget := &PgReviewBudget{pool: pool}
	const callers = 20
	key := "learning-review:org:org-1:2026-11-20"

	counts := make([]int64, callers)
	errs := make([]error, callers)
	var wg sync.WaitGroup
	for i := range callers {
		wg.Add(1)
		go func() {
			defer wg.Done()
			counts[i], errs[i] = budget.Incr(context.Background(), key, reviewBudgetTTL)
		}()
	}
	wg.Wait()
	for i, err := range errs {
		if err != nil {
			t.Fatalf("caller %d: Incr: %v", i, err)
		}
	}
	sort.Slice(counts, func(a, b int) bool { return counts[a] < counts[b] })
	for i, count := range counts {
		if count != int64(i+1) {
			t.Fatalf("concurrent increments must yield exactly 1..%d once each, got %v", callers, counts)
		}
	}
}

func TestReviewCounterRestartsAtOneOnceItsWindowHasPassed(t *testing.T) {
	pool := openReviewCounterDB(t)
	budget := &PgReviewBudget{pool: pool}
	ctx := context.Background()
	key := "learning-review:user:org-1:user-1:2026-11-20"
	if _, err := pool.Exec(ctx,
		`INSERT INTO learning_review_counters (key, org_id, count, expires_at) VALUES ($1, 'org-1', 7, now() - interval '1 minute')`,
		key); err != nil {
		t.Fatalf("seed an expired counter: %v", err)
	}

	count, err := budget.Incr(ctx, key, reviewBudgetTTL)
	if err != nil || count != 1 {
		t.Fatalf("an expired counter must restart at 1, got %d, %v", count, err)
	}
	var live bool
	if err := pool.QueryRow(ctx,
		`SELECT expires_at > now() FROM learning_review_counters WHERE key = $1`, key).Scan(&live); err != nil || !live {
		t.Fatalf("the restarted counter must carry a new window: live=%v, %v", live, err)
	}
	count, err = budget.Incr(ctx, key, reviewBudgetTTL)
	if err != nil || count != 2 {
		t.Fatalf("a live counter must keep counting, got %d, %v", count, err)
	}
}

func TestReviewCounterRefusesAKeyOfAnyOtherShapeWithoutWritingARow(t *testing.T) {
	pool := openReviewCounterDB(t)
	budget := &PgReviewBudget{pool: pool}
	ctx := context.Background()
	for _, key := range refusedReviewCounterKeys {
		if _, err := budget.Incr(ctx, key, reviewBudgetTTL); err == nil {
			t.Fatalf("Incr(%q) must be refused", key)
		}
	}
	var rows int64
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM learning_review_counters`).Scan(&rows); err != nil {
		t.Fatalf("count rows: %v", err)
	}
	if rows != 0 {
		t.Fatalf("refused keys must write no row, found %d", rows)
	}
}

func TestReviewCounterSweepDeletesOnlyRowsExpiredADayAgo(t *testing.T) {
	pool := openReviewCounterDB(t)
	budget := &PgReviewBudget{pool: pool}
	ctx := context.Background()
	seeds := []struct {
		key, org, expiry string
	}{
		{"learning-review:org:org-old:2026-11-01", "org-old", "now() - interval '2 days'"},
		{"learning-review:org:org-recent:2026-11-19", "org-recent", "now() - interval '1 hour'"},
		{"learning-review:org:org-live:2026-11-20", "org-live", "now() + interval '1 day'"},
	}
	for _, seed := range seeds {
		if _, err := pool.Exec(ctx,
			"INSERT INTO learning_review_counters (key, org_id, count, expires_at) VALUES ($1, $2, 1, "+seed.expiry+")",
			seed.key, seed.org); err != nil {
			t.Fatalf("seed %s: %v", seed.key, err)
		}
	}

	deleted, err := budget.sweep(ctx)
	if err != nil || deleted != 1 {
		t.Fatalf("the sweep must delete only the counter that expired two days ago: deleted %d, %v", deleted, err)
	}
	rows, err := pool.Query(ctx, `SELECT key FROM learning_review_counters ORDER BY key`)
	if err != nil {
		t.Fatalf("list remaining counters: %v", err)
	}
	defer rows.Close()
	var remaining []string
	for rows.Next() {
		var key string
		if err := rows.Scan(&key); err != nil {
			t.Fatalf("scan remaining counter: %v", err)
		}
		remaining = append(remaining, key)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("read remaining counters: %v", err)
	}
	want := []string{"learning-review:org:org-live:2026-11-20", "learning-review:org:org-recent:2026-11-19"}
	if strings.Join(remaining, ",") != strings.Join(want, ",") {
		t.Fatalf("remaining counters = %v, want %v", remaining, want)
	}
}
