package spaces

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS03-S1 live proofs for the Space effect-policy audit trail. Gated on
// TEST_DATABASE_URL (a disposable Postgres; all user-core migrations are
// applied here). space_effect_policy_events is append-only by trigger, so the
// rows these tests write are left behind on purpose under a unique org id.

type txAuditOutbox struct {
	fail  bool
	calls int
}

func (o *txAuditOutbox) EnqueueAuditTx(ctx context.Context, tx pgx.Tx, eventID, subject string, payload []byte) error {
	o.calls++
	if o.fail {
		return errors.New("forced outbox failure")
	}
	_, err := tx.Exec(ctx, `
		INSERT INTO user_audit_outbox (event_id, subject, payload)
		VALUES ($1, $2, $3::jsonb)
		ON CONFLICT (event_id) DO NOTHING`, eventID, subject, payload)
	return err
}

type policyAuditFixture struct {
	pool   *pgxpool.Pool
	repo   *Repository
	outbox *txAuditOutbox
	orgID  string
}

func newPolicyAuditFixture(t *testing.T) *policyAuditFixture {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live effect-policy audit test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	if err := (&database.DB{Pool: pool}).RunMigrations(ctx, "../../migrations"); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	fixture := &policyAuditFixture{
		pool:   pool,
		repo:   NewRepository(&database.DB{Pool: pool}),
		outbox: &txAuditOutbox{},
		orgID:  fmt.Sprintf("ws03s1-org-%d", time.Now().UnixNano()),
	}
	fixture.repo.SetPolicyAuditOutbox(fixture.outbox)
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id = $1`, fixture.orgID)
		_, _ = pool.Exec(ctx, `DELETE FROM user_audit_outbox WHERE payload->>'org_id' = $1`, fixture.orgID)
	})
	return fixture
}

func (f *policyAuditFixture) policy() EffectPolicy {
	return EffectPolicy{
		OrgID: f.orgID, PrivacyPolicyRef: "privacy:" + f.orgID + ":1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
	}
}

func (f *policyAuditFixture) counts(t *testing.T) (events, outbox int) {
	t.Helper()
	ctx := context.Background()
	if err := f.pool.QueryRow(ctx, `SELECT count(*) FROM space_effect_policy_events WHERE org_id = $1`, f.orgID).Scan(&events); err != nil {
		t.Fatalf("count events: %v", err)
	}
	if err := f.pool.QueryRow(ctx, `SELECT count(*) FROM user_audit_outbox WHERE payload->>'org_id' = $1`, f.orgID).Scan(&outbox); err != nil {
		t.Fatalf("count outbox: %v", err)
	}
	return events, outbox
}

func (f *policyAuditFixture) stored(t *testing.T) EffectPolicy {
	t.Helper()
	var p EffectPolicy
	if err := scanEffectPolicy(f.pool.QueryRow(context.Background(),
		`SELECT `+effectPolicyColumnList()+` FROM space_effect_policies WHERE org_id = $1`, f.orgID), &p); err != nil {
		t.Fatalf("read stored policy: %v", err)
	}
	return p
}

var operatorActor = EffectPolicyActor{Type: "service", ID: "control-space-policy"}

func TestEffectPolicyCreateAndUpdateWriteOneEventAndOneOutboxRowEach(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()

	policy := f.policy()
	policy.ThreadCreateEntitled = true
	changed, err := f.repo.UpsertEffectPolicy(ctx, policy, operatorActor)
	if err != nil || !changed {
		t.Fatalf("create: changed=%v err=%v", changed, err)
	}
	if events, outbox := f.counts(t); events != 1 || outbox != 1 {
		t.Fatalf("after create: events=%d outbox=%d, want 1 and 1", events, outbox)
	}

	policy.ThreadReadEntitled = true
	changed, err = f.repo.UpsertEffectPolicy(ctx, policy, operatorActor)
	if err != nil || !changed {
		t.Fatalf("update: changed=%v err=%v", changed, err)
	}
	if events, outbox := f.counts(t); events != 2 || outbox != 2 {
		t.Fatalf("after update: events=%d outbox=%d, want 2 and 2", events, outbox)
	}

	var kind, actorType, actorID, subject string
	if err := f.pool.QueryRow(ctx, `
		SELECT e.change_kind, e.actor_subject_type, e.actor_subject_id, o.subject
		FROM space_effect_policy_events e
		JOIN user_audit_outbox o ON o.event_id = 'space-effect-policy:' || e.event_id
		WHERE e.org_id = $1 ORDER BY e.event_id DESC LIMIT 1`, f.orgID).Scan(&kind, &actorType, &actorID, &subject); err != nil {
		t.Fatalf("join event and outbox: %v", err)
	}
	if kind != PolicyChangeOperatorUpdated || actorType != "service" || actorID != "control-space-policy" ||
		subject != AuditSubjectSpaceEffectPolicyUpdated {
		t.Fatalf("event/outbox drifted: kind=%s actor=%s/%s subject=%s", kind, actorType, actorID, subject)
	}
}

func TestEffectPolicyUnchangedReplayWritesNoEventOrOutboxRow(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()
	policy := f.policy()
	policy.ThreadCreateEntitled = true
	if _, err := f.repo.UpsertEffectPolicy(ctx, policy, operatorActor); err != nil {
		t.Fatalf("create: %v", err)
	}
	changed, err := f.repo.UpsertEffectPolicy(ctx, policy, operatorActor)
	if err != nil || changed {
		t.Fatalf("replay: changed=%v err=%v, want unchanged", changed, err)
	}
	if events, outbox := f.counts(t); events != 1 || outbox != 1 {
		t.Fatalf("a replay wrote rows: events=%d outbox=%d", events, outbox)
	}
}

func TestEffectPolicyPartialPutPreservesOmittedEntitlements(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()
	policy := f.policy()
	policy.ThreadCreateEntitled = true
	policy.RetrievalReadEntitled = true
	if _, err := f.repo.UpsertEffectPolicy(ctx, policy, operatorActor); err != nil {
		t.Fatalf("create: %v", err)
	}

	// An operator re-run states only thread_read; the rest of the body decodes to
	// false, but they were not stated and must keep their stored value.
	rerun := f.policy()
	rerun.ThreadReadEntitled = true
	changed, err := f.repo.UpsertEffectPolicyPartial(ctx, rerun, operatorActor, map[string]bool{"thread_read": true})
	if err != nil || !changed {
		t.Fatalf("partial update: changed=%v err=%v", changed, err)
	}
	stored := f.stored(t)
	if !stored.ThreadCreateEntitled || !stored.RetrievalReadEntitled || !stored.ThreadReadEntitled {
		t.Fatalf("an omitted entitlement was cleared or the stated one not set: %+v", EntitlementValues(stored))
	}

	// A key that IS stated as false does clear.
	rerun.ThreadCreateEntitled = false
	if _, err := f.repo.UpsertEffectPolicyPartial(ctx, rerun, operatorActor, map[string]bool{"thread_create": true, "thread_read": true}); err != nil {
		t.Fatalf("explicit clear: %v", err)
	}
	if f.stored(t).ThreadCreateEntitled {
		t.Fatal("an explicitly stated false did not clear the bit")
	}
}

func TestEffectPolicyForcedOutboxFailureRollsThePolicyBack(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()

	// A failing outbox on create leaves no policy row, no event row.
	f.outbox.fail = true
	if _, err := f.repo.UpsertEffectPolicy(ctx, f.policy(), operatorActor); err == nil {
		t.Fatal("a policy create with a failing outbox succeeded")
	}
	var rows int
	if err := f.pool.QueryRow(ctx, `SELECT count(*) FROM space_effect_policies WHERE org_id = $1`, f.orgID).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("policy row survived a failed audit: rows=%d err=%v", rows, err)
	}
	if events, outbox := f.counts(t); events != 0 || outbox != 0 {
		t.Fatalf("rows survived a failed audit: events=%d outbox=%d", events, outbox)
	}

	// And on update the stored policy is unchanged.
	f.outbox.fail = false
	original := f.policy()
	original.ThreadCreateEntitled = true
	if _, err := f.repo.UpsertEffectPolicy(ctx, original, operatorActor); err != nil {
		t.Fatalf("create: %v", err)
	}
	f.outbox.fail = true
	next := original
	next.ImportWriteEntitled = true
	if _, err := f.repo.UpsertEffectPolicy(ctx, next, operatorActor); err == nil {
		t.Fatal("a policy update with a failing outbox succeeded")
	}
	if f.stored(t).ImportWriteEntitled {
		t.Fatal("the policy change was not rolled back with its audit row")
	}
	if events, _ := f.counts(t); events != 1 {
		t.Fatalf("event row survived a rolled back update: events=%d", events)
	}
}

func TestEffectPolicyWithUnwiredOutboxFailsClosed(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.repo.SetPolicyAuditOutbox(nil)
	_, err := f.repo.UpsertEffectPolicy(context.Background(), f.policy(), operatorActor)
	if err == nil || !strings.Contains(err.Error(), "outbox is not wired") {
		t.Fatalf("unwired outbox: err = %v", err)
	}
	if events, outbox := f.counts(t); events != 0 || outbox != 0 {
		t.Fatalf("rows written with an unwired outbox: events=%d outbox=%d", events, outbox)
	}
	var rows int
	if err := f.pool.QueryRow(context.Background(), `SELECT count(*) FROM space_effect_policies WHERE org_id = $1`, f.orgID).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("policy row written with an unwired outbox: rows=%d err=%v", rows, err)
	}
}

func TestEffectPolicyEventsTableIsAppendOnly(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()
	policy := f.policy()
	if _, err := f.repo.UpsertEffectPolicy(ctx, policy, operatorActor); err != nil {
		t.Fatalf("create: %v", err)
	}
	if _, err := f.pool.Exec(ctx, `UPDATE space_effect_policy_events SET preset_id = 'x' WHERE org_id = $1`, f.orgID); err == nil ||
		!strings.Contains(err.Error(), "append-only") {
		t.Fatalf("UPDATE on the events table: err = %v, want an append-only refusal", err)
	}
	if _, err := f.pool.Exec(ctx, `DELETE FROM space_effect_policy_events WHERE org_id = $1`, f.orgID); err == nil ||
		!strings.Contains(err.Error(), "append-only") {
		t.Fatalf("DELETE on the events table: err = %v, want an append-only refusal", err)
	}
	if events, _ := f.counts(t); events != 1 {
		t.Fatalf("the events table lost its row: %d", events)
	}
}
