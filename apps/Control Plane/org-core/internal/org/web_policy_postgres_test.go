package org

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

// The Postgres tests for WS7.H run against the disposable lifecycle database the
// other org-core Postgres tests use and are skipped without
// CONTROL_LIFECYCLE_TEST_DATABASE_URL (the CI shape is org-service.yml:95-103).

// webPolicyPostgres opens the database, applies the migrations and seeds the
// organizations the tests use: two on the enterprise plan and one on free. Rows a
// previous test left behind are removed as the owner (the runtime role cannot).
func webPolicyPostgres(t *testing.T) (context.Context, *database.DB, *Repository, *Service, *recordingAuditPublisher) {
	t.Helper()
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	t.Cleanup(cancel)
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	t.Cleanup(db.Close)
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	service := NewService(repo, nil)
	publisher := &recordingAuditPublisher{}
	service.SetAuditPublisher(publisher)
	for orgID, plan := range map[string]string{"org-wp-a": "enterprise", "org-wp-b": "enterprise", "org-wp-free": "free"} {
		// A fixture an earlier run soft-deleted cannot be provisioned again.
		if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET deleted_at = NULL WHERE id = $1`, orgID); err != nil {
			t.Fatalf("restore %s: %v", orgID, err)
		}
		if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{ID: orgID, Name: orgID, Plan: plan, Status: "active"}, "owner-"+orgID); err != nil {
			t.Fatalf("seed %s: %v", orgID, err)
		}
		if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET plan = $2, deleted_at = NULL WHERE id = $1`, orgID, plan); err != nil {
			t.Fatalf("reset plan: %v", err)
		}
	}
	for _, statement := range []string{
		`DELETE FROM organization_web_policy_history WHERE org_id LIKE 'org-wp-%'`,
		`DELETE FROM organization_web_policy WHERE org_id LIKE 'org-wp-%'`,
		`DELETE FROM organization_web_policy_audit_outbox WHERE org_id LIKE 'org-wp-%'`,
	} {
		if _, err := db.Pool.Exec(ctx, statement); err != nil {
			t.Fatalf("reset web policy rows: %v", err)
		}
	}
	return ctx, db, repo, service, publisher
}

func countRows(t *testing.T, ctx context.Context, db *database.DB, query string, args ...any) int {
	t.Helper()
	var count int
	if err := db.Pool.QueryRow(ctx, query, args...).Scan(&count); err != nil {
		t.Fatalf("count rows: %v", err)
	}
	return count
}

func TestControlLifecyclePutCreatesWithIfMatchZero(t *testing.T) {
	ctx, db, _, service, publisher := webPolicyPostgres(t)

	absent, err := service.GetWebPolicyPosture(ctx, "org-wp-a")
	if err != nil {
		t.Fatalf("read absent policy: %v", err)
	}
	if absent.Exists || absent.Revision != 0 || absent.Rev != "0" || !absent.Policy.IsEmpty() {
		t.Fatalf("absent policy = %+v, want revision 0, rev 0, the empty document", absent)
	}

	doc := WebPolicyDoc{
		Caps:      map[string]string{"browser.act.upload": "disabled"},
		RobotsMax: "respect",
		Domains:   &WebPolicyDomains{Deny: []string{"Bad.example.com"}},
	}
	result, err := service.SetWebPolicy(ctx, "org-wp-a", "user-admin", "admin", doc, 0)
	if err != nil {
		t.Fatalf("first write with If-Match 0: %v", err)
	}
	if result.Revision != 1 || result.Rev != doc.Digest() || !result.Tightened {
		t.Fatalf("first write = %+v", result)
	}

	read, err := service.GetWebPolicyPosture(ctx, "org-wp-a")
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if !read.Exists || read.Revision != 1 || read.Rev != result.Rev || read.UpdatedBy == nil || *read.UpdatedBy != "user-admin" {
		t.Fatalf("read after write = %+v", read)
	}
	if got := strings.Join(read.Policy.Domains.Deny, ","); got != "bad.example.com" {
		t.Fatalf("stored deny list = %q, want the normalised host", got)
	}
	if len(read.Entitlements) != 3 || !read.Entitlements[WebProxyPoolEntitlementKey] {
		t.Fatalf("entitlements = %v", read.Entitlements)
	}

	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_history WHERE org_id = 'org-wp-a'`); rows != 1 {
		t.Fatalf("history rows = %d, want 1", rows)
	}

	// The audit intent was queued in the same transaction and flushed once after
	// the commit; it names keys and never a host.
	if len(publisher.events) != 1 || publisher.subjects[0] != WebPolicyAuditSubject {
		t.Fatalf("audit publish: subjects=%v events=%d", publisher.subjects, len(publisher.events))
	}
	body, _ := json.Marshal(publisher.events[0])
	if strings.Contains(string(body), "bad.example.com") {
		t.Fatalf("the audit event carries a host name: %s", body)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_audit_outbox WHERE org_id = 'org-wp-a' AND published_at IS NOT NULL`); rows != 1 {
		t.Fatalf("published audit rows = %d, want 1", rows)
	}
}

func TestControlLifecycleStalePutIs409(t *testing.T) {
	ctx, db, _, service, _ := webPolicyPostgres(t)
	if _, err := service.SetWebPolicy(ctx, "org-wp-a", "user-1", "owner", WebPolicyDoc{ProxyMax: "none"}, 0); err != nil {
		t.Fatalf("first write: %v", err)
	}
	_, err := service.SetWebPolicy(ctx, "org-wp-a", "user-2", "admin", WebPolicyDoc{ProxyMax: "pool"}, 0)
	var conflict *RevisionConflictError
	if !errors.As(err, &conflict) || conflict.Current != 1 || !errors.Is(err, ErrRevisionConflict) {
		t.Fatalf("stale write err=%v, want a revision conflict naming revision 1", err)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_history WHERE org_id = 'org-wp-a'`); rows != 1 {
		t.Fatalf("a stale write appended history: %d rows", rows)
	}
	if _, err := service.SetWebPolicy(ctx, "org-wp-a", "user-2", "admin", WebPolicyDoc{ProxyMax: "pool"}, 7); !errors.As(err, &conflict) || conflict.Current != 1 {
		t.Fatalf("a future revision must conflict too: %v", err)
	}
	// An unentitled unlock is refused after the revision check and stores nothing.
	if _, err := service.SetWebPolicy(ctx, "org-wp-free", "user-1", "owner", WebPolicyDoc{ProxyMax: "pool"}, 0); !errors.Is(err, ErrNotEntitled) {
		t.Fatalf("free plan proxy pool err=%v, want ErrNotEntitled", err)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy WHERE org_id = 'org-wp-free'`); rows != 0 {
		t.Fatalf("a refused write stored a policy")
	}
	if _, err := service.SetWebPolicy(ctx, "org-wp-missing", "user-1", "owner", WebPolicyDoc{}, 0); !errors.Is(err, ErrNotFound) {
		t.Fatalf("unknown organization err=%v, want ErrNotFound", err)
	}
}

func TestControlLifecycleCrossOrgReadIsNotFound(t *testing.T) {
	ctx, db, repo, service, _ := webPolicyPostgres(t)
	if _, err := service.SetWebPolicy(ctx, "org-wp-a", "user-1", "owner", WebPolicyDoc{ProxyMax: "none"}, 0); err != nil {
		t.Fatalf("write: %v", err)
	}
	// Scoped to org B, org A's row is invisible: RLS, not the application, says so.
	err := db.WithOrgScope(ctx, "org-wp-b", func(tx pgx.Tx) error {
		var revision int64
		return tx.QueryRow(ctx, `SELECT revision FROM organization_web_policy WHERE org_id = 'org-wp-a'`).Scan(&revision)
	})
	if !errors.Is(err, pgx.ErrNoRows) {
		t.Fatalf("cross-org policy read err=%v, want no rows", err)
	}
	err = db.WithOrgScope(ctx, "org-wp-b", func(tx pgx.Tx) error {
		var revision int64
		return tx.QueryRow(ctx, `SELECT revision FROM organization_web_policy_history WHERE org_id = 'org-wp-a'`).Scan(&revision)
	})
	if !errors.Is(err, pgx.ErrNoRows) {
		t.Fatalf("cross-org history read err=%v, want no rows", err)
	}
	// And a write for another org is refused by the policy's WITH CHECK.
	err = db.WithOrgScope(ctx, "org-wp-b", func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
INSERT INTO organization_web_policy (org_id, policy, revision, rev_digest, updated_by)
VALUES ('org-wp-a', '{}'::jsonb, 9, '000000000000', 'attacker')`)
		return err
	})
	if err == nil {
		t.Fatal("a transaction scoped to org B wrote org A's policy")
	}
	if _, err := service.GetWebPolicyPosture(ctx, "org-wp-missing"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("unknown organization read err=%v, want ErrNotFound", err)
	}
	if _, _, err := service.ListWebPolicyHistory(ctx, "org-wp-missing", 10, 0); !errors.Is(err, ErrNotFound) {
		t.Fatalf("unknown organization history err=%v, want ErrNotFound", err)
	}
	// A soft-deleted organization reads as not found. The test uses an organization
	// of its own, because the fixture cannot be provisioned again once deleted.
	softID := "org-wp-soft-" + strconv.FormatInt(time.Now().UnixNano(), 36)
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{ID: softID, Name: softID, Plan: "enterprise", Status: "active"}, "owner-"+softID); err != nil {
		t.Fatalf("seed %s: %v", softID, err)
	}
	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET deleted_at = NOW() WHERE id = $1`, softID); err != nil {
		t.Fatalf("soft delete: %v", err)
	}
	if _, err := service.GetWebPolicyPosture(ctx, softID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("soft-deleted organization read err=%v, want ErrNotFound", err)
	}
	if _, err := service.SetWebPolicy(ctx, softID, "user-1", "owner", WebPolicyDoc{ProxyMax: "none"}, 0); !errors.Is(err, ErrNotFound) {
		t.Fatalf("soft-deleted organization write err=%v, want ErrNotFound", err)
	}
}

func TestControlLifecycleHistoryIsAppendOnly(t *testing.T) {
	ctx, db, _, service, _ := webPolicyPostgres(t)
	bodies := []WebPolicyDoc{
		{Caps: map[string]string{"browser.act.upload": "disabled"}},
		{Caps: map[string]string{"browser.act.upload": "disabled", "browser.act.input": "disabled"}},
		{},
	}
	for index, doc := range bodies {
		if _, err := service.SetWebPolicy(ctx, "org-wp-a", "user-1", "owner", doc, int64(index)); err != nil {
			t.Fatalf("write %d: %v", index, err)
		}
	}
	history, next, err := service.ListWebPolicyHistory(ctx, "org-wp-a", 2, 0)
	if err != nil || len(history) != 2 || history[0].Revision != 3 || history[1].Revision != 2 || next != 2 {
		t.Fatalf("history page = %+v next=%d err=%v", history, next, err)
	}
	if history[0].Tightened || !history[1].Tightened {
		t.Fatalf("tightened flags = %v, %v; revision 3 loosened and revision 2 tightened", history[0].Tightened, history[1].Tightened)
	}
	rest, next, err := service.ListWebPolicyHistory(ctx, "org-wp-a", 2, next)
	if err != nil || len(rest) != 1 || rest[0].Revision != 1 || next != 0 {
		t.Fatalf("second page = %+v next=%d err=%v", rest, next, err)
	}

	// The runtime role can read and insert history and nothing else.
	denied := func(label, statement string) {
		t.Helper()
		tx, err := db.Pool.Begin(ctx)
		if err != nil {
			t.Fatalf("begin %s check: %v", label, err)
		}
		defer func() { _ = tx.Rollback(ctx) }()
		if _, err := tx.Exec(ctx, `SELECT set_config('app.current_org', 'org-wp-a', true)`); err != nil {
			t.Fatalf("scope %s check: %v", label, err)
		}
		if _, err := tx.Exec(ctx, `SET LOCAL ROLE org_core_app`); err != nil {
			t.Fatalf("role %s check: %v", label, err)
		}
		if _, err := tx.Exec(ctx, statement); err == nil {
			t.Fatalf("the runtime role performed a forbidden web policy %s", label)
		}
	}
	denied("history update", `UPDATE organization_web_policy_history SET changed_by = 'forged' WHERE org_id = 'org-wp-a'`)
	denied("history delete", `DELETE FROM organization_web_policy_history WHERE org_id = 'org-wp-a'`)
	denied("history truncate", `TRUNCATE organization_web_policy_history`)
	denied("policy delete", `DELETE FROM organization_web_policy WHERE org_id = 'org-wp-a'`)
	denied("outbox update", `UPDATE organization_web_policy_audit_outbox SET published_at = NOW() WHERE org_id = 'org-wp-a'`)
	denied("outbox delete", `DELETE FROM organization_web_policy_audit_outbox WHERE org_id = 'org-wp-a'`)
	denied("duplicate revision", `INSERT INTO organization_web_policy_history (org_id, revision, policy, changed_by, tightened) VALUES ('org-wp-a', 1, '{}'::jsonb, 'x', false)`)

	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_history WHERE org_id = 'org-wp-a'`); rows != 3 {
		t.Fatalf("history rows = %d, want 3", rows)
	}
}

func TestControlLifecycleOrgCoreAppHoldsOnlyTheGrantsItNeedsOnWebPolicyTables(t *testing.T) {
	ctx, db, _, _, _ := webPolicyPostgres(t)
	want := []struct{ table, need string }{
		{"organization_web_policy", "SELECT,INSERT,UPDATE"},
		{"organization_web_policy_history", "SELECT,INSERT"},
		{"organization_web_policy_audit_outbox", "SELECT,INSERT"},
	}
	names := []string{"SELECT", "INSERT", "UPDATE", "DELETE"}
	for _, entry := range want {
		var granted [4]bool
		if err := db.Pool.QueryRow(ctx, `
SELECT has_table_privilege('org_core_app', $1::text, 'SELECT'),
       has_table_privilege('org_core_app', $1::text, 'INSERT'),
       has_table_privilege('org_core_app', $1::text, 'UPDATE'),
       has_table_privilege('org_core_app', $1::text, 'DELETE')`, entry.table,
		).Scan(&granted[0], &granted[1], &granted[2], &granted[3]); err != nil {
			t.Fatalf("inspect privileges on %s: %v", entry.table, err)
		}
		var have []string
		for i, name := range names {
			if granted[i] {
				have = append(have, name)
			}
		}
		if got := strings.Join(have, ","); got != entry.need {
			t.Errorf("org_core_app holds %q on %s; the code needs %q", got, entry.table, entry.need)
		}
	}
}

func TestControlLifecycleHardDeleteRemovesPolicyAndHistory(t *testing.T) {
	ctx, db, repo, service, _ := webPolicyPostgres(t)
	// Erasure is permanent, so this test erases an organization of its own.
	orgID := "org-wp-erase-" + strconv.FormatInt(time.Now().UnixNano(), 36)
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{ID: orgID, Name: orgID, Plan: "enterprise", Status: "active"}, "owner-"+orgID); err != nil {
		t.Fatalf("seed %s: %v", orgID, err)
	}
	for index, doc := range []WebPolicyDoc{{ProxyMax: "none"}, {ProxyMax: "none", RobotsMax: "strict"}} {
		if _, err := service.SetWebPolicy(ctx, orgID, "user-1", "owner", doc, int64(index)); err != nil {
			t.Fatalf("write %d: %v", index, err)
		}
	}
	if countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy WHERE org_id = $1`, orgID) != 1 ||
		countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_history WHERE org_id = $1`, orgID) != 2 {
		t.Fatal("fixture rows were not written")
	}
	var receipt []byte
	if err := db.Pool.QueryRow(ctx, `SELECT gdpr_hard_delete_organization($1)`, orgID).Scan(&receipt); err != nil {
		t.Fatalf("hard delete: %v", err)
	}
	if err := validateDeletionReceipt(receipt); err != nil {
		t.Fatalf("erasure receipt: %v", err)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy WHERE org_id = $1`, orgID); rows != 0 {
		t.Fatalf("the policy survived erasure: %d rows", rows)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_history WHERE org_id = $1`, orgID); rows != 0 {
		t.Fatalf("the history survived erasure: %d rows", rows)
	}
}

func TestControlLifecycleConcurrentPutsExactlyOneWins(t *testing.T) {
	ctx, db, _, service, _ := webPolicyPostgres(t)
	const writers = 8
	var (
		wg        sync.WaitGroup
		mu        sync.Mutex
		winners   int
		conflicts int
		other     []error
	)
	start := make(chan struct{})
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			<-start
			doc := WebPolicyDoc{Domains: &WebPolicyDomains{Deny: []string{"writer" + string(rune('a'+i)) + ".example.com"}}}
			_, err := service.SetWebPolicy(ctx, "org-wp-a", "user-"+string(rune('a'+i)), "admin", doc, 0)
			mu.Lock()
			defer mu.Unlock()
			var conflict *RevisionConflictError
			switch {
			case err == nil:
				winners++
			case errors.As(err, &conflict):
				conflicts++
			default:
				other = append(other, err)
			}
		}(i)
	}
	close(start)
	wg.Wait()
	if winners != 1 || conflicts != writers-1 || len(other) != 0 {
		t.Fatalf("winners=%d conflicts=%d other=%v; exactly one concurrent first write may win", winners, conflicts, other)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_history WHERE org_id = 'org-wp-a'`); rows != 1 {
		t.Fatalf("history rows = %d, want 1", rows)
	}
	var revision int64
	if err := db.Pool.QueryRow(ctx, `SELECT revision FROM organization_web_policy WHERE org_id = 'org-wp-a'`).Scan(&revision); err != nil || revision != 1 {
		t.Fatalf("stored revision = %d err=%v", revision, err)
	}
}

func TestControlLifecycleWebPolicyAuditOutboxRetriesAFailedPublish(t *testing.T) {
	ctx, db, _, service, publisher := webPolicyPostgres(t)
	publisher.err = errors.New("nats down")
	if _, err := service.SetWebPolicy(ctx, "org-wp-a", "user-1", "owner", WebPolicyDoc{ProxyMax: "none"}, 0); err != nil {
		t.Fatalf("a failed audit publish must not fail the write: %v", err)
	}
	if rows := countRows(t, ctx, db, `SELECT count(*) FROM organization_web_policy_audit_outbox WHERE org_id = 'org-wp-a' AND published_at IS NULL`); rows != 1 {
		t.Fatalf("pending audit rows = %d, want 1", rows)
	}
	// The row is backed off after the failed publish; make it due and flush again.
	if _, err := db.Pool.Exec(ctx, `UPDATE organization_web_policy_audit_outbox SET next_attempt_at = NOW() WHERE org_id = 'org-wp-a'`); err != nil {
		t.Fatalf("make the retry due: %v", err)
	}
	publisher.err = nil
	result, err := service.FlushWebPolicyAuditOutbox(ctx, 10)
	if err != nil || result.Published != 1 {
		t.Fatalf("retry flush = %+v err=%v", result, err)
	}
	if len(publisher.events) != 1 || publisher.subjects[0] != WebPolicyAuditSubject {
		t.Fatalf("retried event: subjects=%v", publisher.subjects)
	}
}
