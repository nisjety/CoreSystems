package pg

import (
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// W3-E3.7.1 (migration 028). The text pin runs without a database in every lane.
// The database test skips without QUARRY_TEST_DSN and fails under
// QUARRY_REQUIRE_DB=1, as the other Postgres tests do.

// TestMigration028Text pins migration 028: the org-scoped partial unique index is
// built before the global 007 index is dropped, and nothing else is touched.
func TestMigration028Text(t *testing.T) {
	b, err := migrationFS.ReadFile("migrations/028_jobs_idempotency_org.sql")
	if err != nil {
		t.Fatalf("read 028: %v", err)
	}
	var stmt strings.Builder
	for _, line := range strings.Split(string(b), "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		stmt.WriteString(line + " ")
	}
	got := strings.Join(strings.Fields(strings.ToUpper(stmt.String())), " ")
	create := "CREATE UNIQUE INDEX IF NOT EXISTS IDX_JOBS_ORG_IDEMPOTENCY_KEY ON JOBS (ORG_ID, IDEMPOTENCY_KEY) WHERE IDEMPOTENCY_KEY IS NOT NULL;"
	drop := "DROP INDEX IF EXISTS IDX_JOBS_IDEMPOTENCY_KEY;"
	ci, di := strings.Index(got, create), strings.Index(got, drop)
	if ci < 0 {
		t.Fatalf("028 must contain %q", create)
	}
	if di < 0 {
		t.Fatalf("028 must contain %q", drop)
	}
	if di < ci {
		t.Fatal("028 must build the org-scoped index before it drops the global one")
	}
	for _, banned := range []string{"DROP TABLE", "DROP COLUMN", "DELETE ", "UPDATE ", "TRUNCATE ", "ALTER COLUMN"} {
		if strings.Contains(got, banned) {
			t.Fatalf("028 changes only the index, but it contains %q", banned)
		}
	}
}

// TestPostgres_IdempotencyKeyIsUniquePerOrg: two orgs may use the same key (each
// finds only its own job), and one org cannot use a key twice.
func TestPostgres_IdempotencyKeyIsUniquePerOrg(t *testing.T) {
	db := openIsolatedOrSkip(t)
	key := "idem-" + string(quarrycontracts.NewID(quarrycontracts.KindJob))
	newJob := func(org string) store.Job {
		k := key
		return store.Job{
			ID:             quarrycontracts.NewID(quarrycontracts.KindJob),
			OrgID:          org,
			Kind:           "batch",
			Status:         "accepted",
			CreatedAt:      time.Now().UnixMilli(),
			IdempotencyKey: &k,
		}
	}
	a, b := newJob("org_e371_a"), newJob("org_e371_b")
	if err := db.Jobs().Create(a); err != nil {
		t.Fatalf("org A create: %v", err)
	}
	t.Cleanup(func() { _ = db.Jobs().Delete(a.ID) })
	if err := db.Jobs().Create(b); err != nil {
		t.Fatalf("org B with the same key must not collide with org A: %v", err)
	}
	t.Cleanup(func() { _ = db.Jobs().Delete(b.ID) })

	if got, ok := db.Jobs().FindByIdempotencyKey("org_e371_a", key); !ok || got.ID != a.ID {
		t.Fatalf("org A must find its own job, got %v %v", got.ID, ok)
	}
	if got, ok := db.Jobs().FindByIdempotencyKey("org_e371_b", key); !ok || got.ID != b.ID {
		t.Fatalf("org B must find its own job, got %v %v", got.ID, ok)
	}
	if _, ok := db.Jobs().FindByIdempotencyKey("org_e371_c", key); ok {
		t.Fatal("an org that never used the key must not find a job")
	}

	again := newJob("org_e371_a")
	err := db.Jobs().Create(again)
	if err == nil {
		_ = db.Jobs().Delete(again.ID)
		t.Fatal("the same org must not create a second job with the same key")
	}
	if !errors.Is(err, store.ErrConflict) {
		t.Fatalf("a repeated key in one org must be ErrConflict, got %v", err)
	}
}
