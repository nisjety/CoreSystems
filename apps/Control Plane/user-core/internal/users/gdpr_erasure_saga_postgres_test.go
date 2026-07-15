package users

import (
	"context"
	"fmt"
	"os"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/jackc/pgx/v5/pgxpool"
)

func newErasurePostgresFixture(t *testing.T) (*pgxpool.Pool, *Repository) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping erasure saga PostgreSQL test")
	}
	ctx := context.Background()
	admin, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("open admin pool: %v", err)
	}
	t.Cleanup(admin.Close)

	schema := fmt.Sprintf("user_erasure_saga_%d", time.Now().UnixNano())
	if !regexp.MustCompile(`^[a-z0-9_]+$`).MatchString(schema) {
		t.Fatalf("unsafe fixture schema %q", schema)
	}
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create fixture schema: %v", err)
	}
	t.Cleanup(func() { _, _ = admin.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })

	config, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatalf("parse fixture DSN: %v", err)
	}
	config.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatalf("open fixture pool: %v", err)
	}
	t.Cleanup(pool.Close)

	fixtureSchema := `
		CREATE TABLE users (
			id TEXT PRIMARY KEY, email TEXT NOT NULL, name TEXT NOT NULL,
			password_hash TEXT NOT NULL DEFAULT '', avatar TEXT NOT NULL DEFAULT '',
			status TEXT NOT NULL DEFAULT 'active', email_verified BOOLEAN NOT NULL DEFAULT false,
			onboarding_complete BOOLEAN NOT NULL DEFAULT false, last_login_at TIMESTAMPTZ,
			onboarding_step TEXT, onboarding_state JSONB, onboarding_expires_at TIMESTAMPTZ,
			onboarding_completed_at TIMESTAMPTZ
		);
		CREATE TABLE user_profiles (user_id TEXT PRIMARY KEY, bio TEXT);
		CREATE TABLE user_sessions (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE user_activities (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE user_roles (user_id TEXT NOT NULL, role_id TEXT NOT NULL);
		CREATE TABLE user_devices (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE user_settings (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE provider_accounts (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE user_api_keys (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE user_activity_log (id TEXT PRIMARY KEY, user_id TEXT NOT NULL);
		CREATE TABLE user_org_memberships (
			id TEXT PRIMARY KEY, user_id TEXT NOT NULL, org_id TEXT NOT NULL,
			role TEXT NOT NULL, status TEXT NOT NULL, invited_by TEXT,
			created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
		);
		CREATE TABLE resource_grants (
			grant_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, resource_type TEXT NOT NULL,
			resource_id TEXT NOT NULL, subject_type TEXT NOT NULL, subject_id TEXT NOT NULL,
			role TEXT NOT NULL, granted_by TEXT NOT NULL DEFAULT '', granted_at TIMESTAMPTZ NOT NULL DEFAULT now()
		);`
	if _, err := pool.Exec(ctx, fixtureSchema); err != nil {
		t.Fatalf("create fixture tables: %v", err)
	}
	for _, migrationPath := range []string{
		"../../migrations/014_gdpr_audit_outbox.up.sql",
		"../../migrations/015_gdpr_erasure_saga.up.sql",
		"../../migrations/016_gdpr_erasure_fanout.up.sql",
	} {
		migration, err := os.ReadFile(migrationPath)
		if err != nil {
			t.Fatalf("read migration %s: %v", migrationPath, err)
		}
		if _, err := pool.Exec(ctx, string(migration)); err != nil {
			t.Fatalf("apply migration %s: %v", migrationPath, err)
		}
	}
	return pool, NewRepository(&database.DB{Pool: pool})
}

func TestErasureSagaSnapshotsEveryActiveOrganizationBeforeAuthMutation(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-multi-org", "org-primary")
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO user_org_memberships (id, user_id, org_id, role, status)
		VALUES ('membership-u-multi-org-second', 'u-multi-org', 'org-secondary', 'member', 'active'),
		       ('membership-u-multi-org-inactive', 'u-multi-org', 'org-inactive', 'member', 'removed')
	`); err != nil {
		t.Fatalf("insert secondary memberships: %v", err)
	}

	operationID := erasureOperationID("u-multi-org", ErasureModeHardDelete)
	auth := &fakeAuthErasureExecutor{execute: func(ctx context.Context, _ ErasureMode, _ string) ([]byte, error) {
		var orgs []string
		rows, err := pool.Query(ctx, `
			SELECT org_id FROM user_erasure_fanout
			WHERE operation_id=$1 ORDER BY org_id
		`, operationID)
		if err != nil {
			return nil, err
		}
		defer rows.Close()
		for rows.Next() {
			var orgID string
			if err := rows.Scan(&orgID); err != nil {
				return nil, err
			}
			orgs = append(orgs, orgID)
		}
		if strings.Join(orgs, ",") != "org-primary,org-secondary" {
			return nil, fmt.Errorf("durable pre-mutation org snapshot = %v", orgs)
		}
		return []byte(`{"success":true,"user_id":"u-multi-org"}`), nil
	}}
	fanout := &fakeErasureFanoutPublisher{}
	svc := NewService(repo, nil, nil)
	svc.authEraser = auth
	svc.erasureFanout = fanout

	receipt, err := svc.HardEraseUser(context.Background(), "u-multi-org", "u-multi-org", "self", "org-primary")
	if err != nil {
		t.Fatalf("hard erase multi-org subject: %v", err)
	}
	if !receipt.Success || fanout.calls != 2 {
		t.Fatalf("receipt=%+v fanout calls=%d, want two acknowledged child events", receipt, fanout.calls)
	}
	if fanout.eventIDs[0] == fanout.eventIDs[1] || len(fanout.eventIDs[0]) > 128 || len(fanout.eventIDs[1]) > 128 {
		t.Fatalf("child IDs must be unique and bounded: %q %q", fanout.eventIDs[0], fanout.eventIDs[1])
	}
	var published, total int
	if err := pool.QueryRow(context.Background(), `
		SELECT count(*) FILTER (WHERE published_at IS NOT NULL), count(*)
		FROM user_erasure_fanout WHERE operation_id=$1
	`, operationID).Scan(&published, &total); err != nil {
		t.Fatalf("read fanout children: %v", err)
	}
	if published != 2 || total != 2 {
		t.Fatalf("published=%d total=%d, want all two child PubAcks persisted", published, total)
	}
}

func TestErasureSagaDoesNotCompleteUntilEveryChildHasPubAck(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-partial-fanout", "org-one")
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO user_org_memberships (id, user_id, org_id, role, status)
		VALUES ('membership-u-partial-fanout-two', 'u-partial-fanout', 'org-two', 'member', 'active')
	`); err != nil {
		t.Fatalf("insert second membership: %v", err)
	}
	svc := NewService(repo, nil, nil)
	svc.authEraser = &fakeAuthErasureExecutor{execute: func(context.Context, ErasureMode, string) ([]byte, error) {
		return []byte(`{"success":true,"user_id":"u-partial-fanout"}`), nil
	}}
	svc.erasureFanout = &fakeErasureFanoutPublisher{failOnCall: 2}

	if _, err := svc.HardEraseUser(context.Background(), "u-partial-fanout", "u-partial-fanout", "self", "org-one"); err == nil {
		t.Fatal("missing second PubAck must fail the attempt")
	}
	operationID := erasureOperationID("u-partial-fanout", ErasureModeHardDelete)
	var completed bool
	var published int
	if err := pool.QueryRow(context.Background(), `
		SELECT operation.completed_at IS NOT NULL,
		       (SELECT count(*) FROM user_erasure_fanout child
		        WHERE child.operation_id=operation.operation_id AND child.published_at IS NOT NULL)
		FROM user_erasure_operations operation WHERE operation_id=$1
	`, operationID).Scan(&completed, &published); err != nil {
		t.Fatalf("read partial fanout state: %v", err)
	}
	if completed || published != 1 {
		t.Fatalf("completed=%v published=%d, want incomplete with exactly one PubAck", completed, published)
	}
}

func TestGDPRDeliveryHealthAndBoundedRequeuePreserveEvidence(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-operator", "org-operator")
	operation := ErasureOperation{
		OperationID: erasureOperationID("u-operator", ErasureModeHardDelete),
		UserID:      "u-operator", Mode: ErasureModeHardDelete,
		ActorID: "admin-operator", ActorRole: "admin", OrgID: "org-operator",
	}
	if _, err := repo.BeginErasureOperation(context.Background(), operation); err != nil {
		t.Fatalf("begin operator fixture: %v", err)
	}
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO user_audit_outbox (event_id, subject, payload, attempts, terminal_at, last_error, created_at)
		VALUES ('audit-terminal', $1, '{"subject_id":"u-operator"}', 20, now(), 'broker refused', now()-interval '10 minutes')
	`, ErasureAuditSubject); err != nil {
		t.Fatalf("seed terminal audit delivery: %v", err)
	}
	if _, err := pool.Exec(context.Background(), `
		UPDATE user_erasure_fanout
		SET attempts=20, terminal_at=now(), last_error='consumer unavailable', created_at=now()-interval '12 minutes'
		WHERE operation_id=$1
	`, operation.OperationID); err != nil {
		t.Fatalf("seed terminal fanout delivery: %v", err)
	}

	svc := NewService(repo, nil, nil)
	health, err := svc.GDPRDeliveryHealth(context.Background())
	if err != nil {
		t.Fatalf("delivery health: %v", err)
	}
	if !health.Degraded || health.AuditTerminal != 1 || health.FanoutTerminal != 1 || health.OldestLag < 10*time.Minute {
		t.Fatalf("unexpected delivery health: %+v", health)
	}
	if _, err := svc.RequeueGDPRDeliveries(context.Background(), "audit", []string{"audit-terminal"}); err != nil {
		t.Fatalf("requeue audit: %v", err)
	}
	var payload string
	var auditTerminal bool
	var requeues int
	if err := pool.QueryRow(context.Background(), `
		SELECT payload::text, terminal_at IS NOT NULL, requeue_count
		FROM user_audit_outbox WHERE event_id='audit-terminal'
	`).Scan(&payload, &auditTerminal, &requeues); err != nil {
		t.Fatalf("read requeued audit: %v", err)
	}
	if !strings.Contains(payload, "u-operator") || auditTerminal || requeues != 1 {
		t.Fatalf("audit evidence/requeue state payload=%s terminal=%v requeues=%d", payload, auditTerminal, requeues)
	}

	var childID string
	if err := pool.QueryRow(context.Background(), `SELECT child_event_id FROM user_erasure_fanout WHERE operation_id=$1`, operation.OperationID).Scan(&childID); err != nil {
		t.Fatalf("read child ID: %v", err)
	}
	if _, err := pool.Exec(context.Background(), `UPDATE user_erasure_operations SET next_attempt_at=now()+interval '1 hour' WHERE operation_id=$1`, operation.OperationID); err != nil {
		t.Fatalf("delay parent before operator requeue: %v", err)
	}
	if _, err := svc.RequeueGDPRDeliveries(context.Background(), "fanout", []string{childID}); err != nil {
		t.Fatalf("requeue fanout: %v", err)
	}
	var fanoutTerminal, parentDue bool
	if err := pool.QueryRow(context.Background(), `
		SELECT terminal_at IS NOT NULL, requeue_count FROM user_erasure_fanout WHERE child_event_id=$1
	`, childID).Scan(&fanoutTerminal, &requeues); err != nil {
		t.Fatalf("read requeued fanout: %v", err)
	}
	if fanoutTerminal || requeues != 1 {
		t.Fatalf("fanout terminal=%v requeues=%d", fanoutTerminal, requeues)
	}
	if err := pool.QueryRow(context.Background(), `SELECT next_attempt_at <= now() FROM user_erasure_operations WHERE operation_id=$1`, operation.OperationID).Scan(&parentDue); err != nil {
		t.Fatalf("read requeued parent schedule: %v", err)
	}
	if !parentDue {
		t.Fatal("requeued fanout child did not wake its parent operation")
	}

	if _, err := repo.RequeueGDPRDeliveries(context.Background(), "fanout", make([]string, 101)); err == nil {
		t.Fatal("operator requeue accepted more than 100 identifiers")
	}
	if _, err := repo.RequeueGDPRDeliveries(context.Background(), "unknown", []string{"audit-terminal"}); err == nil {
		t.Fatal("operator requeue accepted an unknown delivery kind")
	}
}

func insertErasureSubject(t *testing.T, pool *pgxpool.Pool, userID, orgID string) {
	t.Helper()
	ctx := context.Background()
	statements := []struct {
		query string
		args  []any
	}{
		{`INSERT INTO users (id, email, name) VALUES ($1, $2, 'Fixture User')`, []any{userID, userID + "@example.test"}},
		{`INSERT INTO user_org_memberships (id, user_id, org_id, role, status) VALUES ($1, $2, $3, 'member', 'active')`, []any{"membership-" + userID, userID, orgID}},
		{`INSERT INTO resource_grants (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role) VALUES ($1, $2, 'document', 'doc-1', 'user', $3, 'view')`, []any{"grant-" + userID, orgID, userID}},
		{`INSERT INTO user_api_keys (id, user_id) VALUES ($1, $2)`, []any{"key-" + userID, userID}},
		{`INSERT INTO user_activity_log (id, user_id) VALUES ($1, $2)`, []any{"activity-" + userID, userID}},
	}
	for _, statement := range statements {
		if _, err := pool.Exec(ctx, statement.query, statement.args...); err != nil {
			t.Fatalf("insert erasure subject: %v", err)
		}
	}
}

func TestErasureSagaPersistsBeforeMutationAndResumesWithoutRepeatingStages(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-resume", "org-verified")
	ctx := context.Background()

	operationID := erasureOperationID("u-resume", ErasureModeHardDelete)
	auth := &fakeAuthErasureExecutor{}
	auth.execute = func(ctx context.Context, mode ErasureMode, userID string) ([]byte, error) {
		var count int
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM user_erasure_operations WHERE operation_id=$1`, operationID).Scan(&count); err != nil {
			return nil, err
		}
		if count != 1 {
			return nil, fmt.Errorf("durable operation ledger was not written before Auth mutation")
		}
		return []byte(`{"success":true,"user_id":"u-resume","deleted_records":{"sessions":["sensitive-session-id"]}}`), nil
	}
	fanout := &fakeErasureFanoutPublisher{failures: 1}
	svc := NewService(repo, nil, nil)
	svc.authEraser = auth
	svc.erasureFanout = fanout

	orgID, err := svc.ResolveErasureAuditOrg(ctx, "u-resume", "org-from-untrusted-header")
	if err != nil || orgID != "org-verified" {
		t.Fatalf("resolved org = %q err=%v, want verified membership org", orgID, err)
	}
	if verifiedOrg, err := svc.ResolveErasureAuditOrg(ctx, "u-resume", "org-verified"); err != nil || verifiedOrg != "org-verified" {
		t.Fatalf("verified delegated membership org = %q err=%v", verifiedOrg, err)
	}
	if _, err := svc.ResolveErasureAuditOrg(ctx, "", ""); err == nil {
		t.Fatal("blank audit subject was accepted")
	}
	if _, err := svc.ResolveErasureAuditOrg(ctx, "missing-user", ""); err == nil {
		t.Fatal("ownerless subject was assigned an unverified audit org")
	}

	if _, err := svc.HardEraseUser(ctx, "u-resume", "u-resume", "self", orgID); err == nil {
		t.Fatal("first attempt should surface the fan-out failure while retaining resumable state")
	}
	if auth.calls != 1 || fanout.calls != 1 {
		t.Fatalf("first attempt calls: auth=%d fanout=%d", auth.calls, fanout.calls)
	}

	var authDone, localDone, auditDone bool
	var fanoutDone, completed bool
	var retained string
	if err := pool.QueryRow(ctx, `
		SELECT auth_completed_at IS NOT NULL, local_completed_at IS NOT NULL,
		       audit_enqueued_at IS NOT NULL, fanout_published_at IS NOT NULL,
		       completed_at IS NOT NULL, to_jsonb(op)::text
		FROM user_erasure_operations op WHERE operation_id=$1
	`, operationID).Scan(&authDone, &localDone, &auditDone, &fanoutDone, &completed, &retained); err != nil {
		t.Fatalf("read operation: %v", err)
	}
	if !authDone || !localDone || !auditDone || fanoutDone || completed {
		t.Fatalf("unexpected stage state auth=%v local=%v audit=%v fanout=%v completed=%v", authDone, localDone, auditDone, fanoutDone, completed)
	}
	if strings.Contains(retained, "sensitive-session-id") || strings.Contains(retained, "deleted_records") {
		t.Fatalf("raw Auth receipt was retained in operation ledger: %s", retained)
	}

	for _, table := range []string{"users", "user_org_memberships", "resource_grants", "user_api_keys", "user_activity_log"} {
		var count int
		if err := pool.QueryRow(ctx, "SELECT count(*) FROM "+table).Scan(&count); err != nil {
			t.Fatalf("count %s: %v", table, err)
		}
		if count != 0 {
			t.Fatalf("%s still contains %d erased-subject rows", table, count)
		}
	}
	var auditPayload string
	if err := pool.QueryRow(ctx, `SELECT payload::text FROM user_audit_outbox WHERE event_id=$1`, operationID+":audit").Scan(&auditPayload); err != nil {
		t.Fatalf("read audit intent: %v", err)
	}
	if strings.Contains(auditPayload, "sensitive-session-id") || strings.Contains(auditPayload, "auth_db_receipt") {
		t.Fatalf("audit intent retained raw Auth receipt: %s", auditPayload)
	}

	if _, err := pool.Exec(ctx, `UPDATE user_erasure_operations SET next_attempt_at=now()-interval '1 second' WHERE operation_id=$1`, operationID); err != nil {
		t.Fatalf("make retry due: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE user_erasure_fanout SET next_attempt_at=now()-interval '1 second' WHERE operation_id=$1 AND published_at IS NULL`, operationID); err != nil {
		t.Fatalf("make fanout retry due: %v", err)
	}
	claimed, found, err := repo.ClaimNextErasureOperation(ctx)
	if err != nil || !found {
		t.Fatalf("restart worker claim found=%v err=%v", found, err)
	}
	resumed, err := svc.processClaimedErasure(ctx, claimed)
	if err != nil {
		t.Fatalf("restart worker resume erasure: %v", err)
	}
	receipt := erasureReceipt(resumed)
	if !receipt.Success || receipt.OperationID != operationID || !receipt.AuditEnqueued || !receipt.FanoutPublished {
		t.Fatalf("completed receipt = %+v", receipt)
	}
	if auth.calls != 1 || fanout.calls != 2 {
		t.Fatalf("resume repeated completed stages: auth=%d fanout=%d", auth.calls, fanout.calls)
	}

	if _, err := svc.HardEraseUser(ctx, "u-resume", "another-admin", "admin", orgID); err != nil {
		t.Fatalf("idempotent completed retry: %v", err)
	}
	if auth.calls != 1 || fanout.calls != 2 {
		t.Fatalf("completed retry caused side effects: auth=%d fanout=%d", auth.calls, fanout.calls)
	}
	var operations, audits int
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM user_erasure_operations WHERE user_id='u-resume' AND mode='hard_delete'`).Scan(&operations)
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM user_audit_outbox WHERE event_id=$1`, operationID+":audit").Scan(&audits)
	if operations != 1 || audits != 1 {
		t.Fatalf("idempotency rows: operations=%d audits=%d", operations, audits)
	}
}

func TestAnonymizeSagaScrubsLocalPIIAndCredentials(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-anonymize", "org-safe")
	ctx := context.Background()
	auth := &fakeAuthErasureExecutor{execute: func(context.Context, ErasureMode, string) ([]byte, error) {
		return []byte(`{"success":true,"user_id":"u-anonymize","anonymized_email":"must-not-be-retained@example.test"}`), nil
	}}
	svc := NewService(repo, nil, nil)
	svc.authEraser = auth
	svc.erasureFanout = &fakeErasureFanoutPublisher{}

	receipt, err := svc.AnonymizeUser(ctx, "u-anonymize", "u-anonymize", "self", "org-safe")
	if err != nil {
		t.Fatalf("anonymize: %v", err)
	}
	if !receipt.Success || receipt.LocalDeleted {
		t.Fatalf("anonymize receipt = %+v", receipt)
	}
	var email, name, passwordHash, avatar, status string
	if err := pool.QueryRow(ctx, `SELECT email, name, password_hash, avatar, status FROM users WHERE id='u-anonymize'`).Scan(&email, &name, &passwordHash, &avatar, &status); err != nil {
		t.Fatalf("read anonymized user: %v", err)
	}
	if email == "u-anonymize@example.test" || name != "Deleted User" || passwordHash != "" || avatar != "" || status != "blocked" {
		t.Fatalf("local PII not scrubbed: email=%q name=%q password=%q avatar=%q status=%q", email, name, passwordHash, avatar, status)
	}
	var memberships int
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM user_org_memberships WHERE user_id='u-anonymize'`).Scan(&memberships)
	if memberships != 1 {
		t.Fatalf("anonymize must preserve the referential membership row, got %d", memberships)
	}
	for _, table := range []string{"resource_grants", "user_api_keys", "user_activity_log"} {
		var count int
		if table == "resource_grants" {
			_ = pool.QueryRow(ctx, `SELECT count(*) FROM resource_grants WHERE subject_id='u-anonymize'`).Scan(&count)
		} else {
			_ = pool.QueryRow(ctx, "SELECT count(*) FROM "+table+" WHERE user_id='u-anonymize'").Scan(&count)
		}
		if count != 0 {
			t.Fatalf("%s retained %d anonymized-user rows", table, count)
		}
	}
}

func TestErasureSagaRejectsFalseAuthReceiptBeforeLocalMutation(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-auth-fail", "org-safe")
	ctx := context.Background()

	auth := &fakeAuthErasureExecutor{execute: func(context.Context, ErasureMode, string) ([]byte, error) {
		return []byte(`{"success":false,"user_id":"u-auth-fail","error":"auth refused"}`), nil
	}}
	svc := NewService(repo, nil, nil)
	svc.authEraser = auth
	svc.erasureFanout = &fakeErasureFanoutPublisher{}

	if _, err := svc.HardEraseUser(ctx, "u-auth-fail", "u-auth-fail", "self", "org-safe"); err == nil || !strings.Contains(err.Error(), "reported failure") {
		t.Fatalf("false Auth receipt error = %v", err)
	}
	var users, operations, authDone, localDone int
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM users WHERE id='u-auth-fail'`).Scan(&users)
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM user_erasure_operations WHERE user_id='u-auth-fail'`).Scan(&operations)
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM user_erasure_operations WHERE user_id='u-auth-fail' AND auth_completed_at IS NOT NULL`).Scan(&authDone)
	_ = pool.QueryRow(ctx, `SELECT count(*) FROM user_erasure_operations WHERE user_id='u-auth-fail' AND local_completed_at IS NOT NULL`).Scan(&localDone)
	if users != 1 || operations != 1 || authDone != 0 || localDone != 0 {
		t.Fatalf("false receipt state users=%d operations=%d authDone=%d localDone=%d", users, operations, authDone, localDone)
	}
	if _, err := svc.HardEraseUser(ctx, "u-auth-fail", "u-auth-fail", "self", "org-safe"); err == nil || !strings.Contains(err.Error(), "pending retry") {
		t.Fatalf("immediate duplicate must expose pending durable state, got %v", err)
	}
	if auth.calls != 1 {
		t.Fatalf("pending duplicate repeated Auth mutation: calls=%d", auth.calls)
	}
}

func TestErasureWorkerResumesPersistedOperationAfterRestart(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-worker", "org-worker")
	ctx := context.Background()
	svc := NewService(repo, nil, nil)
	svc.authEraser = &fakeAuthErasureExecutor{execute: func(context.Context, ErasureMode, string) ([]byte, error) {
		return []byte(`{"success":true,"user_id":"u-worker"}`), nil
	}}
	svc.erasureFanout = &fakeErasureFanoutPublisher{}
	requested := ErasureOperation{
		OperationID: erasureOperationID("u-worker", ErasureModeHardDelete),
		UserID:      "u-worker", Mode: ErasureModeHardDelete,
		ActorID: "u-worker", ActorRole: "self", OrgID: "org-worker",
	}
	if _, err := repo.BeginErasureOperation(ctx, requested); err != nil {
		t.Fatalf("persist restart fixture: %v", err)
	}

	svc.StartErasureSaga()
	svc.StartErasureSaga()
	t.Cleanup(svc.CloseErasureSaga)
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		operation, err := repo.GetErasureOperation(ctx, requested.OperationID)
		if err != nil {
			t.Fatalf("read worker operation: %v", err)
		}
		if operation.CompletedAt != nil {
			svc.CloseErasureSaga()
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatal("restart worker did not complete the persisted erasure operation")
}

func TestPGXAuthErasureExecutorUsesTypedParameterizedProcedures(t *testing.T) {
	pool, _ := newErasurePostgresFixture(t)
	ctx := context.Background()
	if _, err := pool.Exec(ctx, `
		CREATE FUNCTION gdpr_hard_delete_user(requested_id TEXT) RETURNS JSONB
		LANGUAGE sql AS 'SELECT jsonb_build_object(''success'', true, ''user_id'', requested_id)';
		CREATE FUNCTION gdpr_anonymize_user(requested_id TEXT) RETURNS JSONB
		LANGUAGE sql AS 'SELECT jsonb_build_object(''success'', true, ''user_id'', requested_id)';
	`); err != nil {
		t.Fatalf("create Auth procedure fixtures: %v", err)
	}
	executor := &pgxAuthErasureExecutor{pool: pool}
	for _, mode := range []ErasureMode{ErasureModeHardDelete, ErasureModeAnonymize} {
		receipt, err := executor.Execute(ctx, mode, "u-procedure")
		if err != nil {
			t.Fatalf("execute %s: %v", mode, err)
		}
		if err := validateAuthErasureReceipt(receipt, "u-procedure"); err != nil {
			t.Fatalf("validate %s receipt: %v", mode, err)
		}
	}
	if _, err := executor.Execute(ctx, "unsupported", "u-procedure"); err == nil {
		t.Fatal("unsupported Auth erasure mode was accepted")
	}

	svc := NewService(nil, nil, nil)
	svc.SetAuthPool(pool)
	if svc.authEraser == nil {
		t.Fatal("SetAuthPool did not install the typed executor")
	}
	svc.SetAuthPool(nil)
	if svc.authEraser != nil {
		t.Fatal("SetAuthPool(nil) did not fail closed")
	}
}
