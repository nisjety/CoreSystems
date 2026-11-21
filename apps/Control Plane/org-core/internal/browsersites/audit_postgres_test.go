package browsersites

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

// auditStoredIntent is one stored intent, read back for the assertions.
type auditStoredIntent struct {
	eventID   string
	subject   string
	published bool
	payload   map[string]any
}

// readAuditIntents returns every intent of one organisation, oldest first.
func readAuditIntents(t *testing.T, ctx context.Context, db *database.DB, orgID string) []auditStoredIntent {
	t.Helper()
	rows, err := db.Pool.Query(ctx, `
SELECT event_id, subject, published_at IS NOT NULL, payload
FROM browser_site_audit_outbox
WHERE org_id = $1
ORDER BY created_at, event_id`, orgID)
	if err != nil {
		t.Fatalf("read browser site audit outbox: %v", err)
	}
	defer rows.Close()
	var out []auditStoredIntent
	for rows.Next() {
		var intent auditStoredIntent
		var body []byte
		if err := rows.Scan(&intent.eventID, &intent.subject, &intent.published, &body); err != nil {
			t.Fatalf("scan browser site audit outbox: %v", err)
		}
		if err := json.Unmarshal(body, &intent.payload); err != nil {
			t.Fatalf("decode browser site audit payload: %v", err)
		}
		out = append(out, intent)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("iterate browser site audit outbox: %v", err)
	}
	return out
}

// TestControlLifecycleBrowserSiteAuditOutbox proves F-f06-11 on the disposable lifecycle Postgres.
// Each policy write and each choice write queues one content-free intent in its own transaction. A
// refused write queues none, and so does a write with the switch off. The flush marks what it
// published. The runtime role can insert and read the outbox but not delete from it, and the
// organisation purge removes what is still pending.
func TestControlLifecycleBrowserSiteAuditOutbox(t *testing.T) {
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

	orgID := fmt.Sprintf("org-browser-audit-%d", time.Now().UnixNano())
	owner := "owner-" + orgID
	member := "member-" + orgID
	seedBrowserSiteOrganization(t, ctx, db, orgID)
	service := NewService(NewRepository(db), time.Now)
	service.SetAuditEnabled(true)
	// No publisher yet, so each write queues its intent and nothing publishes it.

	// 1. A policy write queues one policy intent: its mode and rule count, never a rule host or note.
	zero := int64(0)
	if _, err := service.Put(ctx, orgID, owner, "owner", PutRequest{
		ExpectedRevision: &zero, Mode: ModeAllowlist,
		Rules: []Rule{{Host: "shop.example.com", Rule: RuleAct, Note: "checkout note"}},
	}); err != nil {
		t.Fatalf("policy write: %v", err)
	}
	intents := readAuditIntents(t, ctx, db, orgID)
	if len(intents) != 1 {
		t.Fatalf("after the policy write there are %d intents, want 1", len(intents))
	}
	if intents[0].payload["event"] != BrowserSitePolicyChanged || intents[0].published {
		t.Fatalf("policy intent = %v, published=%v", intents[0].payload, intents[0].published)
	}
	policyDetails := intents[0].payload["details"].(map[string]any)
	if policyDetails["mode"] != ModeAllowlist || policyDetails["rule_count"] != float64(1) || policyDetails["revision"] != float64(1) {
		t.Fatalf("policy details = %v", policyDetails)
	}
	if body, _ := json.Marshal(intents[0].payload); strings.Contains(string(body), "shop.example.com") || strings.Contains(string(body), "checkout note") {
		t.Fatalf("the policy intent carries a rule host or a note: %s", body)
	}

	// 2. A choice write queues one choice intent with its host and level.
	one := int64(1)
	if _, err := service.SetPref(ctx, orgID, member, "member", "news.example.com", PrefRequest{ExpectedRevision: &one, Level: LevelRead}); err != nil {
		t.Fatalf("choice write: %v", err)
	}
	// 3. A choice delete queues one choice intent whose level is none.
	if err := service.DeletePref(ctx, orgID, member, "member", "news.example.com"); err != nil {
		t.Fatalf("choice delete: %v", err)
	}
	intents = readAuditIntents(t, ctx, db, orgID)
	if len(intents) != 3 {
		t.Fatalf("after the choice write and delete there are %d intents, want 3", len(intents))
	}
	setDetails := intents[1].payload["details"].(map[string]any)
	if intents[1].payload["event"] != BrowserSitePrefChanged || intents[1].payload["user_id"] != member ||
		intents[1].payload["actor_role"] != "member" || setDetails["host"] != "news.example.com" ||
		setDetails["level"] != LevelRead || setDetails["revision"] != float64(1) {
		t.Fatalf("choice intent = %v", intents[1].payload)
	}
	deleteDetails := intents[2].payload["details"].(map[string]any)
	if deleteDetails["level"] != LevelNone || deleteDetails["host"] != "news.example.com" {
		t.Fatalf("choice delete details = %v", deleteDetails)
	}

	// 4. A refused write, a choice against a superseded revision, queues nothing.
	stale := int64(0)
	_, err = service.SetPref(ctx, orgID, member, "member", "other.example.com", PrefRequest{ExpectedRevision: &stale, Level: LevelAct})
	var conflict *RevisionConflictError
	if !errors.As(err, &conflict) {
		t.Fatalf("a stale choice must be a revision conflict, got %v", err)
	}
	if n := len(readAuditIntents(t, ctx, db, orgID)); n != 3 {
		t.Fatalf("a refused write queued an intent: %d intents, want 3", n)
	}

	// 5. With the switch off a write is unchanged and queues nothing.
	service.SetAuditEnabled(false)
	if _, err := service.SetPref(ctx, orgID, member, "member", "quiet.example.com", PrefRequest{ExpectedRevision: &one, Level: LevelRead}); err != nil {
		t.Fatalf("choice write with the switch off: %v", err)
	}
	service.SetAuditEnabled(true)
	if n := len(readAuditIntents(t, ctx, db, orgID)); n != 3 {
		t.Fatalf("a write with the switch off queued an intent: %d intents, want 3", n)
	}

	// 6. A rolled-back transaction keeps no intent: the intent is written in the transaction of
	// the change, so when the change is rolled back so is the intent.
	rollback := withWriter(ctx, writer{id: member, role: "member", audit: true})
	err = db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := enqueueAudit(rollback, tx, auditChange{
			orgID: orgID, event: BrowserSitePrefChanged, revision: 1, host: "rolled.example.com", historyID: 0,
			details: map[string]any{"host": "rolled.example.com", "level": LevelRead, "revision": int64(1)},
		}); err != nil {
			return err
		}
		return errors.New("roll back on purpose")
	})
	if err == nil {
		t.Fatal("the rollback probe must fail")
	}
	if n := len(readAuditIntents(t, ctx, db, orgID)); n != 3 {
		t.Fatalf("a rolled-back transaction kept an intent: %d intents, want 3", n)
	}

	// 7. The flush publishes every pending intent and marks each one published.
	recording := &auditRecordingPublisher{}
	service.SetAuditPublisher(recording)
	result, err := service.FlushBrowserSiteAuditOutbox(ctx, 100)
	if err != nil {
		t.Fatalf("flush: %v", err)
	}
	if result.Published < 3 || result.DeadLettered != 0 {
		t.Fatalf("flush result = %+v, want at least the three intents published", result)
	}
	for _, intent := range readAuditIntents(t, ctx, db, orgID) {
		if !intent.published {
			t.Fatalf("intent %s is still pending after the flush", intent.eventID)
		}
		auditAssertKeys(t, intent.payload, "event_id", "occurred_at", "org_id", "user_id", "actor_role", "plane",
			"producer", "event", "subject", "resource_id", "outcome", "details")
	}

	// 8. The runtime role inserts and reads the outbox, and never updates or deletes a row.
	var insert, read, update, remove bool
	if err := db.Pool.QueryRow(ctx, `
SELECT has_table_privilege('org_core_app', 'browser_site_audit_outbox', 'INSERT'),
       has_table_privilege('org_core_app', 'browser_site_audit_outbox', 'SELECT'),
       has_table_privilege('org_core_app', 'browser_site_audit_outbox', 'UPDATE'),
       has_table_privilege('org_core_app', 'browser_site_audit_outbox', 'DELETE')`,
	).Scan(&insert, &read, &update, &remove); err != nil {
		t.Fatalf("inspect org_core_app privileges: %v", err)
	}
	if !insert || !read || update || remove {
		t.Fatalf("org_core_app holds insert=%v select=%v update=%v delete=%v on the outbox", insert, read, update, remove)
	}

	// 9. The organisation purge removes an intent that is still pending.
	service.SetAuditPublisher(nil)
	if _, err := service.SetPref(ctx, orgID, member, "member", "pending.example.com", PrefRequest{ExpectedRevision: &one, Level: LevelRead}); err != nil {
		t.Fatalf("choice write before the purge: %v", err)
	}
	if n := len(readAuditIntents(t, ctx, db, orgID)); n != 4 {
		t.Fatalf("before the purge there are %d intents, want 4", n)
	}
	var receipt string
	if err := db.Pool.QueryRow(ctx, `SELECT gdpr_hard_delete_organization($1)::text`, orgID).Scan(&receipt); err != nil {
		t.Fatalf("hard delete: %v", err)
	}
	var outcome struct {
		Success bool   `json:"success"`
		Error   string `json:"error"`
	}
	if err := json.Unmarshal([]byte(receipt), &outcome); err != nil || !outcome.Success {
		t.Fatalf("hard delete did not succeed: %s (%v)", receipt, err)
	}
	var left int
	if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM browser_site_audit_outbox WHERE org_id = $1`, orgID).Scan(&left); err != nil || left != 0 {
		t.Fatalf("%d intents survived the hard delete of the organisation (err=%v)", left, err)
	}
}
