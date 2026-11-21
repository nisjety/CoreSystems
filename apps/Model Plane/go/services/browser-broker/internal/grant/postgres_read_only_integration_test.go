//go:build integration

package grant

// WS21-S4 durable read level (migration 0009): browser_grants.read_only on the
// production store shape. The in-memory rules these match are in
// read_only_test.go.
//
//	TEST_DATABASE_URL=... go test -tags integration -run 'TestPg_ReadOnly|TestPg_Migration0009' ./internal/grant/...

import (
	"errors"
	"testing"
	"time"
)

// A read-only grant is stored with the level and reads back with it, from a new
// store instance too (a restart). A grant without the level stores false.
func TestPg_ReadOnlyPersistsAndReadsBack(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, clock := stores(pool)
	ro := session(t, store, CreateSessionParams{SessionKey: "ro", ReadOnly: true, TTL: 10 * time.Minute})
	if !ro.ReadOnly {
		t.Fatal("the create returned no read level")
	}
	if n := rowCount(t, pool, "WHERE id = $1 AND read_only", ro.ID); n != 1 {
		t.Fatalf("%d rows with read_only = true for the read-only grant, want 1", n)
	}
	got, err := store.GetScoped(ctx, ro.ID, ro.OrgID, ro.OwnerID)
	if err != nil {
		t.Fatalf("GetScoped: %v", err)
	}
	if !got.ReadOnly {
		t.Fatal("the read-only grant reads back with full access")
	}
	restarted := NewPgStore(pool)
	restarted.now = clock.Now
	if got, err := restarted.GetScoped(ctx, ro.ID, ro.OrgID, ro.OwnerID); err != nil || !got.ReadOnly {
		t.Fatalf("a new store instance reads the read-only grant as %v, %v", got != nil && got.ReadOnly, err)
	}

	full := session(t, store, CreateSessionParams{SessionKey: "full", TTL: 10 * time.Minute})
	if n := rowCount(t, pool, "WHERE id = $1 AND NOT read_only", full.ID); n != 1 {
		t.Fatalf("a grant without the level stores %d rows with read_only = false, want 1", n)
	}
	got, err = store.GetScoped(ctx, full.ID, full.OrgID, full.OwnerID)
	if err != nil || got.ReadOnly {
		t.Fatalf("a grant without the level reads back read-only=%v, err=%v", got != nil && got.ReadOnly, err)
	}
}

// A run derived from a read-only session, and an origin extension under that
// run, are stored read-only; an approval under either is refused.
func TestPg_ReadOnlyIsInheritedByRunAndExtension(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	root := session(t, store, CreateSessionParams{ReadOnly: true, TTL: 30 * time.Minute})

	run, err := store.Derive(ctx, deriveParams(root, "run-1"))
	if err != nil {
		t.Fatalf("derive: %v", err)
	}
	if !run.ReadOnly {
		t.Fatal("the derived run grant is not read-only")
	}
	if n := rowCount(t, pool, "WHERE id = $1 AND read_only", run.ID); n != 1 {
		t.Fatalf("%d rows with read_only = true for the run, want 1", n)
	}
	got, err := store.GetScoped(ctx, run.ID, run.OrgID, run.OwnerID)
	if err != nil || !got.ReadOnly {
		t.Fatalf("the run reads back read-only=%v, err=%v", got != nil && got.ReadOnly, err)
	}

	ext, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org"))
	if err != nil {
		t.Fatalf("extension under a read-only run: %v", err)
	}
	if !ext.ReadOnly {
		t.Fatal("the extension is not read-only")
	}
	if n := rowCount(t, pool, "WHERE id = $1 AND read_only", ext.ID); n != 1 {
		t.Fatalf("%d rows with read_only = true for the extension, want 1", n)
	}

	if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute)); !errors.Is(err, ErrApprovalParentInvalid) {
		t.Fatalf("approval under a read-only run: %v, want ErrApprovalParentInvalid", err)
	}
}

// A row written before migration 0009 has no read_only value. The migration adds
// the column with a false default, so the row reads back as the full-access
// grant it always was.
func TestPg_ReadOnlyRowWrittenBeforeTheMigrationReadsFalse(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	applyMigration(t, pool, "0009_browser_grants_read_only.down.sql")
	pool.Reset()
	const id = "pre-0009-row"
	// The insert the broker made before the column existed: no read_only.
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url,
		allowed_domains, expires_at, absolute_expires_at)
		VALUES ($1, 'session', 'org-a', 'user-a', 'session-a', 'browser://cloud', ARRAY['example.com'],
			TIMESTAMPTZ '2026-10-18 13:00:00+00', TIMESTAMPTZ '2026-10-18 13:00:00+00')`, id); err != nil {
		t.Fatalf("pre-0009 insert: %v", err)
	}
	applyMigration(t, pool, "0009_browser_grants_read_only.up.sql")
	pool.Reset()
	got, err := store.GetScoped(ctx, id, "org-a", "user-a")
	if err != nil {
		t.Fatalf("read the pre-0009 row: %v", err)
	}
	if got.ReadOnly {
		t.Fatal("a row written before the migration reads read-only; it must read as the full access it always had")
	}
}

// The startup check follows the column: present after the migration, absent
// after its down migration, present again after the up migration is re-run.
func TestPg_ReadOnlyColumnPresentFollowsTheMigration(t *testing.T) {
	pool, _ := newTestDB(t)
	pg, _, _, _ := stores(pool)
	present := func() bool {
		t.Helper()
		ok, err := pg.ReadOnlyColumnPresent(ctx)
		if err != nil {
			t.Fatalf("ReadOnlyColumnPresent: %v", err)
		}
		return ok
	}
	if !present() {
		t.Fatal("the column is reported absent after migration 0009")
	}
	applyMigration(t, pool, "0009_browser_grants_read_only.down.sql")
	pool.Reset()
	if present() {
		t.Fatal("the column is reported present after its down migration")
	}
	applyMigration(t, pool, "0009_browser_grants_read_only.up.sql")
	pool.Reset()
	if !present() {
		t.Fatal("the column is reported absent after the up migration is re-applied")
	}
}

// 0009 is re-applied on every broker start: the up file is idempotent, the down
// file is too, and a row survives a down then up.
func TestPg_Migration0009_IsIdempotentAndReversible(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	ro := session(t, store, CreateSessionParams{ReadOnly: true, TTL: 10 * time.Minute})

	applyMigration(t, pool, "0009_browser_grants_read_only.up.sql")
	applyMigration(t, pool, "0009_browser_grants_read_only.up.sql")
	if n := rowCount(t, pool, "WHERE id = $1 AND read_only", ro.ID); n != 1 {
		t.Fatalf("re-applying 0009 changed the read level: %d rows", n)
	}

	applyMigration(t, pool, "0009_browser_grants_read_only.down.sql")
	applyMigration(t, pool, "0009_browser_grants_read_only.down.sql")
	pool.Reset()
	if _, err := store.GetScoped(ctx, ro.ID, ro.OrgID, ro.OwnerID); err == nil {
		t.Fatal("0009 down must drop read_only, which the store selects: the read should fail")
	}

	applyMigration(t, pool, "0009_browser_grants_read_only.up.sql")
	pool.Reset()
	// The down file dropped the level, so the row comes back as full access. That
	// is why the down file must run only with the switch off and the read-only
	// grants gone.
	got, err := store.GetScoped(ctx, ro.ID, ro.OrgID, ro.OwnerID)
	if err != nil {
		t.Fatalf("0009 down then up lost the row: %v", err)
	}
	if got.ReadOnly {
		t.Fatal("0009 down then up re-added the column as true; a re-added column must read false")
	}
}
