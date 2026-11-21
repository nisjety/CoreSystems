package browsersites

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

// TestControlLifecycleBrowserSiteBackfillErasesOnlyNonActiveMembers runs migration 035 over the
// rows a member removed before migration 034 leaves behind (P2-cp-04-rel1), on the disposable
// lifecycle Postgres. The choices and the user_pref history of every member who is not active are
// erased and redacted; the active member's choice and history and the policy history are
// untouched; and a second run changes nothing. The migration runs inside a transaction that is
// rolled back, because it is written for every organisation of the database and the lifecycle
// database is shared with the other tests of this package.
func TestControlLifecycleBrowserSiteBackfillErasesOnlyNonActiveMembers(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	suffix := time.Now().UnixNano()
	orgB := fmt.Sprintf("org-browser-backfill-%d", suffix)
	seedBrowserSiteOrganization(t, ctx, db, orgB)
	for _, member := range []struct{ userID, status string }{
		{"active-b", "active"},
		{"removed-b", "removed"},
		{"suspended-b", "suspended"},
	} {
		if _, err := db.Pool.Exec(ctx, `INSERT INTO organization_members (org_id, user_id, role, status) VALUES ($1, $2, 'member', $3)`, orgB, member.userID, member.status); err != nil {
			t.Fatalf("seed member %s: %v", member.userID, err)
		}
	}

	// The rows as they stood before migration 034: each member's choice and the member's own
	// history of it, at revisions 1 and 2. The policy history row is an administrator's.
	for _, userID := range []string{"active-b", "removed-b", "suspended-b"} {
		if _, err := db.Pool.Exec(ctx, `
INSERT INTO user_browser_site_prefs (org_id, user_id, host, level, expires_at)
VALUES ($1, $2, 'news.example.com', 'act', NOW() + INTERVAL '1 day')`, orgB, userID); err != nil {
			t.Fatalf("seed choice of %s: %v", userID, err)
		}
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO org_browser_site_history (org_id, change_kind, revision, actor_id, subject_user_id, before_set, after_set)
VALUES
  ($1, 'user_pref', 1, 'removed-b', 'removed-b', '{}'::jsonb, '{"host":"news.example.com","level":"act"}'::jsonb),
  ($1, 'user_pref', 2, 'removed-b', 'removed-b', '{"host":"news.example.com","level":"act"}'::jsonb, '{}'::jsonb),
  ($1, 'user_pref', 1, 'suspended-b', 'suspended-b', '{}'::jsonb, '{"host":"news.example.com","level":"act"}'::jsonb),
  ($1, 'user_pref', 1, 'active-b', 'active-b', '{}'::jsonb, '{"host":"news.example.com","level":"act"}'::jsonb),
  ($1, 'policy', 1, 'admin-b', NULL, '{"mode":"open","rules":[]}'::jsonb, '{"mode":"allowlist","rules":[]}'::jsonb)`, orgB); err != nil {
		t.Fatalf("seed legacy history: %v", err)
	}

	migration, err := os.ReadFile("../../migrations/035_browser_site_choices_backfill.up.sql")
	if err != nil {
		t.Fatalf("read migration 035: %v", err)
	}
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin backfill transaction: %v", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("apply migration 035 over the legacy rows: %v", err)
	}
	assertBackfilledLegacyRows(t, ctx, tx, orgB)

	// A second run finds nothing left to change: the redacted rows are excluded by the statement
	// itself, so the trigger is not asked to redact a row twice.
	if _, err := tx.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("a second run of migration 035 must change nothing and succeed: %v", err)
	}
	assertBackfilledLegacyRows(t, ctx, tx, orgB)
}

// assertBackfilledLegacyRows checks the state after migration 035 for one organisation, inside
// the backfill transaction.
func assertBackfilledLegacyRows(t *testing.T, ctx context.Context, tx pgx.Tx, orgID string) {
	t.Helper()
	count := func(query string, args ...any) int64 {
		t.Helper()
		var n int64
		if err := tx.QueryRow(ctx, query, args...).Scan(&n); err != nil {
			t.Fatalf("read backfill result: %v", err)
		}
		return n
	}

	if left := count(`SELECT count(*) FROM user_browser_site_prefs WHERE org_id = $1 AND user_id = 'active-b'`, orgID); left != 1 {
		t.Fatalf("the active member's choice must survive the backfill: %d rows", left)
	}
	if left := count(`SELECT count(*) FROM user_browser_site_prefs WHERE org_id = $1`, orgID); left != 1 {
		t.Fatalf("only the active member's choice may remain: %d rows survive for the organisation", left)
	}

	// Three history rows belong to members who are not active (two for removed-b at revisions 1
	// and 2, one for suspended-b at revision 1). They keep their revisions and lose their content.
	if erased := count(`
SELECT count(*) FROM org_browser_site_history
WHERE org_id = $1 AND change_kind = 'user_pref' AND subject_user_id = 'erased' AND actor_id = 'erased'
  AND before_set = '{}'::jsonb AND after_set = '{}'::jsonb`, orgID); erased != 3 {
		t.Fatalf("the non-active members' history rows must be redacted: %d rows redacted, want 3", erased)
	}
	if revisions := count(`
SELECT COALESCE(sum(revision), 0)::bigint FROM org_browser_site_history
WHERE org_id = $1 AND subject_user_id = 'erased' AND change_kind = 'user_pref'`, orgID); revisions != 4 {
		t.Fatalf("redaction must keep each row's revision: revision sum %d, want 4", revisions)
	}
	if named := count(`
SELECT count(*) FROM org_browser_site_history
WHERE org_id = $1 AND (subject_user_id IN ('removed-b', 'suspended-b') OR actor_id IN ('removed-b', 'suspended-b'))`, orgID); named != 0 {
		t.Fatalf("no history row may still name a non-active member: %d rows", named)
	}

	if kept := count(`
SELECT count(*) FROM org_browser_site_history
WHERE org_id = $1 AND subject_user_id = 'active-b' AND actor_id = 'active-b' AND after_set <> '{}'::jsonb`, orgID); kept != 1 {
		t.Fatalf("the active member's history must be untouched: %d rows", kept)
	}
	if policy := count(`
SELECT count(*) FROM org_browser_site_history
WHERE org_id = $1 AND change_kind = 'policy' AND actor_id = 'admin-b' AND subject_user_id IS NULL
  AND after_set <> '{}'::jsonb`, orgID); policy != 1 {
		t.Fatalf("the policy history must be untouched: %d rows", policy)
	}
}
