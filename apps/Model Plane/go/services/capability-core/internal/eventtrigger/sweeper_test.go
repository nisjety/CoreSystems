package eventtrigger

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

const testOrg = "org-trg"

var testNow = time.Date(2026, 11, 8, 12, 0, 0, 0, time.UTC)

var testLimits = Limits{TriggerPerHour: 3, OrgPerHour: 5, MaxAttempts: 3}

// memRow is one fire row of the in-memory fire table.
type memRow struct {
	fire      Fire
	status    string
	outcome   string
	attempts  int
	settledAt time.Time
}

// memLedger stands in for the fire table. A batch restores its snapshot when it rolls back, as a transaction would.
type memLedger struct {
	rows   []memRow
	failOn string
}

func (l *memLedger) begin(context.Context) (batch, error) {
	return &memBatch{ledger: l, snapshot: append([]memRow(nil), l.rows...)}, nil
}

type memBatch struct {
	ledger    *memLedger
	snapshot  []memRow
	committed bool
}

func (b *memBatch) due(_ context.Context, limit int) ([]Fire, error) {
	var fires []Fire
	for _, row := range b.ledger.rows {
		if row.status != StatusPending {
			continue
		}
		fire := row.fire
		fire.Attempts = row.attempts
		fires = append(fires, fire)
		if len(fires) == limit {
			break
		}
	}
	return fires, nil
}

func (b *memBatch) counts(_ context.Context, fire Fire, since time.Time) (Counts, error) {
	var counts Counts
	for _, row := range b.ledger.rows {
		if row.status != StatusAuthorized || row.fire.OrgID != fire.OrgID || row.settledAt.Before(since) {
			continue
		}
		counts.Org++
		if row.fire.TriggerID == fire.TriggerID {
			counts.Trigger++
		}
	}
	return counts, nil
}

func (b *memBatch) settle(_ context.Context, fire Fire, settlement Settlement, now time.Time) error {
	if fire.ID == b.ledger.failOn {
		return errors.New("settle failed")
	}
	for i := range b.ledger.rows {
		if b.ledger.rows[i].fire.ID != fire.ID {
			continue
		}
		row := &b.ledger.rows[i]
		row.status, row.outcome, row.attempts = settlement.Status, settlement.Outcome, settlement.Attempts
		row.settledAt = time.Time{}
		if settlement.Status != StatusPending {
			row.settledAt = now
		}
		return nil
	}
	return errors.New("no such fire")
}

func (b *memBatch) commit(context.Context) error {
	b.committed = true
	return nil
}

func (b *memBatch) rollback(context.Context) {
	if !b.committed {
		b.ledger.rows = b.snapshot
	}
}

// fakeAuthorizer stands in for Control's fire decision. It records every intent it is asked about.
type fakeAuthorizer struct {
	intents []cron.FireIntent
	err     error
}

func (a *fakeAuthorizer) AuthorizeFire(_ context.Context, intent cron.FireIntent) error {
	a.intents = append(a.intents, intent)
	return a.err
}

// pendingRow is a pending fire of an active trigger bound to one connection, Space and member. Its event was created at
// the sweep's clock, so the event is inside the window.
func pendingRow(fireID, triggerID, eventID string) memRow {
	fire := Fire{ID: fireID, TriggerID: triggerID, OrgID: testOrg, WebhookEventID: eventID,
		ConnectionID: "conn-1", SpaceRef: "space-a", MemberUserID: "member-1", TriggerStatus: "active", EventAt: testNow}
	return memRow{fire: fire, status: StatusPending}
}

func newTestSweeper(l ledger, authority cron.FireAuthorizer, limits Limits) *Sweeper {
	sweeper := newSweeper(l, authority, limits)
	sweeper.now = func() time.Time { return testNow }
	return sweeper
}

func TestRunOnceAuthorisesAFireUnderBudget(t *testing.T) {
	mem := &memLedger{rows: []memRow{pendingRow("trgfire_1", "trg_1", "wh-1")}}
	authority := &fakeAuthorizer{}
	authorized, err := newTestSweeper(mem, authority, testLimits).RunOnce(context.Background())
	if err != nil || authorized != 1 {
		t.Fatalf("RunOnce = %d, %v; want 1, nil", authorized, err)
	}
	row := mem.rows[0]
	if row.status != StatusAuthorized || row.outcome != OutcomeAuthorized || row.attempts != 1 || !row.settledAt.Equal(testNow) {
		t.Fatalf("row = %+v; want authorized, one attempt, settled now", row)
	}
	if len(authority.intents) != 1 {
		t.Fatalf("Control was asked %d times; want 1", len(authority.intents))
	}
	intent := authority.intents[0]
	if intent.OrgID != testOrg || intent.SpaceRef != "space-a" || intent.SubjectID != "member-1" ||
		intent.ScheduleID != "trg_1" || intent.FireKey != "wh-1" || intent.IdempotencyKey != "trg_1:wh-1" {
		t.Fatalf("intent = %+v", intent)
	}
	if !strings.HasPrefix(intent.TemplateDigest, "sha256:") || len(intent.TemplateDigest) != len("sha256:")+64 {
		t.Fatalf("template digest %q is not a sha256 digest", intent.TemplateDigest)
	}
}

func TestRunOnceRefusesAFireWhoseTriggerStoppedWithoutAskingControl(t *testing.T) {
	row := pendingRow("trgfire_1", "trg_1", "wh-1")
	row.fire.TriggerStatus = "paused"
	mem := &memLedger{rows: []memRow{row}}
	authority := &fakeAuthorizer{}
	authorized, err := newTestSweeper(mem, authority, testLimits).RunOnce(context.Background())
	if err != nil || authorized != 0 {
		t.Fatalf("RunOnce = %d, %v; want 0, nil", authorized, err)
	}
	if got := mem.rows[0]; got.status != StatusRefused || got.outcome != OutcomeTriggerNotActive {
		t.Fatalf("row = %+v; want refused trigger_not_active", got)
	}
	if len(authority.intents) != 0 {
		t.Fatal("Control was asked about a trigger that is not active")
	}
}

func TestRunOnceStopsATriggerAtItsHourlyBudgetWithinOneBatch(t *testing.T) {
	mem := &memLedger{rows: []memRow{
		pendingRow("trgfire_1", "trg_1", "wh-1"),
		pendingRow("trgfire_2", "trg_1", "wh-2"),
		pendingRow("trgfire_3", "trg_1", "wh-3"),
	}}
	authority := &fakeAuthorizer{}
	limits := Limits{TriggerPerHour: 1, OrgPerHour: 10, MaxAttempts: 3}
	authorized, err := newTestSweeper(mem, authority, limits).RunOnce(context.Background())
	if err != nil || authorized != 1 {
		t.Fatalf("RunOnce = %d, %v; want 1, nil", authorized, err)
	}
	if len(authority.intents) != 1 {
		t.Fatalf("Control was asked %d times; a spent budget must not reach Control", len(authority.intents))
	}
	for _, row := range mem.rows[1:] {
		if row.status != StatusRefused || row.outcome != OutcomeTriggerBudget {
			t.Fatalf("row = %+v; want refused trigger_budget_reached", row)
		}
	}
}

func TestRunOnceStopsAnOrgAtItsHourlyQuotaAcrossItsTriggers(t *testing.T) {
	mem := &memLedger{rows: []memRow{
		pendingRow("trgfire_1", "trg_1", "wh-1"),
		pendingRow("trgfire_2", "trg_2", "wh-1"),
		pendingRow("trgfire_3", "trg_3", "wh-1"),
	}}
	authority := &fakeAuthorizer{}
	limits := Limits{TriggerPerHour: 3, OrgPerHour: 2, MaxAttempts: 3}
	authorized, err := newTestSweeper(mem, authority, limits).RunOnce(context.Background())
	if err != nil || authorized != 2 {
		t.Fatalf("RunOnce = %d, %v; want 2, nil", authorized, err)
	}
	if got := mem.rows[2]; got.status != StatusRefused || got.outcome != OutcomeOrgQuota {
		t.Fatalf("row = %+v; want refused org_quota_reached", got)
	}
}

func TestRunOnceCountsOnlyFiresAuthorisedInTheLastHour(t *testing.T) {
	old := pendingRow("trgfire_0", "trg_1", "wh-0")
	old.status = StatusAuthorized
	old.outcome = OutcomeAuthorized
	old.attempts = 1
	old.settledAt = testNow.Add(-2 * time.Hour)
	mem := &memLedger{rows: []memRow{old, pendingRow("trgfire_1", "trg_1", "wh-1")}}
	limits := Limits{TriggerPerHour: 1, OrgPerHour: 10, MaxAttempts: 3}
	authorized, err := newTestSweeper(mem, &fakeAuthorizer{}, limits).RunOnce(context.Background())
	if err != nil || authorized != 1 {
		t.Fatalf("RunOnce = %d, %v; an authorisation older than the hour must not spend the budget", authorized, err)
	}
}

func TestAnAuthorityErrorLeavesTheFirePendingUntilItsAttemptsAreUsed(t *testing.T) {
	mem := &memLedger{rows: []memRow{pendingRow("trgfire_1", "trg_1", "wh-1")}}
	authority := &fakeAuthorizer{err: errors.New("Control unavailable")}
	limits := Limits{TriggerPerHour: 3, OrgPerHour: 5, MaxAttempts: 2}
	sweeper := newTestSweeper(mem, authority, limits)

	authorized, err := sweeper.RunOnce(context.Background())
	if err != nil || authorized != 0 {
		t.Fatalf("first sweep = %d, %v; a per-fire authority error is not a sweep error", authorized, err)
	}
	if got := mem.rows[0]; got.status != StatusPending || got.attempts != 1 || !got.settledAt.IsZero() {
		t.Fatalf("row after one failed attempt = %+v; want pending with one attempt", got)
	}

	if _, err := sweeper.RunOnce(context.Background()); err != nil {
		t.Fatalf("second sweep: %v", err)
	}
	if got := mem.rows[0]; got.status != StatusRefused || got.outcome != OutcomeAuthorityFailed || got.attempts != 2 {
		t.Fatalf("row after its attempts = %+v; want refused authority_unavailable", got)
	}
	if len(authority.intents) != 2 {
		t.Fatalf("Control was asked %d times; want 2", len(authority.intents))
	}
}

func TestAFailedSettlementRollsTheWholeBatchBack(t *testing.T) {
	mem := &memLedger{
		rows:   []memRow{pendingRow("trgfire_1", "trg_1", "wh-1"), pendingRow("trgfire_2", "trg_1", "wh-2")},
		failOn: "trgfire_2",
	}
	authorized, err := newTestSweeper(mem, &fakeAuthorizer{}, testLimits).RunOnce(context.Background())
	if err == nil || authorized != 0 {
		t.Fatalf("RunOnce = %d, %v; want a failed sweep", authorized, err)
	}
	for _, row := range mem.rows {
		if row.status != StatusPending || row.attempts != 0 || !row.settledAt.IsZero() {
			t.Fatalf("row = %+v; a failed batch must leave every fire pending and unsettled", row)
		}
	}
}

func TestNewSweeperNeedsADatabaseAndAnAuthorizer(t *testing.T) {
	if _, err := NewSweeper(nil, &fakeAuthorizer{}, testLimits); err == nil {
		t.Fatal("a sweeper without a database was built")
	}
	if _, err := NewSweeper(nil, nil, testLimits); err == nil {
		t.Fatal("a sweeper without an authorizer was built")
	}
}

func TestRefusalChecksTheTriggerBeforeTheBudgets(t *testing.T) {
	fire := pendingRow("trgfire_1", "trg_1", "wh-1").fire
	fire.TriggerStatus = "disabled"
	if got := refusalFor(fire, Counts{Trigger: 99, Org: 99}, testLimits, testNow); got != OutcomeTriggerNotActive {
		t.Fatalf("refusalFor = %q; want trigger_not_active", got)
	}
	fire.TriggerStatus = "active"
	if got := refusalFor(fire, Counts{Trigger: 3}, testLimits, testNow); got != OutcomeTriggerBudget {
		t.Fatalf("refusalFor = %q; want trigger_budget_reached", got)
	}
	if got := refusalFor(fire, Counts{Org: 5}, testLimits, testNow); got != OutcomeOrgQuota {
		t.Fatalf("refusalFor = %q; want org_quota_reached", got)
	}
	if got := refusalFor(fire, Counts{Trigger: 2, Org: 4}, testLimits, testNow); got != "" {
		t.Fatalf("refusalFor = %q; a fire inside both budgets may be asked", got)
	}
}

func TestTriggerDigestChangesWithTheBinding(t *testing.T) {
	base := Fire{TriggerID: "trg_1", ConnectionID: "conn-1", SpaceRef: "space-a", MemberUserID: "member-1"}
	same := base
	if triggerDigest(base) != triggerDigest(same) {
		t.Fatal("the same binding gave two digests")
	}
	moved := base
	moved.ConnectionID = "conn-2"
	if triggerDigest(base) == triggerDigest(moved) {
		t.Fatal("a different connection gave the same digest")
	}
	if !strings.HasPrefix(triggerDigest(base), "sha256:") {
		t.Fatalf("digest %q is not a sha256 digest", triggerDigest(base))
	}
}
