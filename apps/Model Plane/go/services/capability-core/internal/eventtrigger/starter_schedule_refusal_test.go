package eventtrigger

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// The starter refuses a fire whose bound schedule is switched off or auto-paused. It never starts that fire, and it does
// not leave it authorised for a later switch-on. These tests drive the starter over an in-memory ledger that applies the
// schedule condition of each claim, and pin the claim query with a capturing transaction, so no database is needed.

// schedGateFire is one authorised, unstarted fire the in-memory ledger holds. triggerStatus is the status of the trigger the
// fire belongs to. deleted reports that its schedule is soft-deleted, and eventAt is when its webhook event was created.
type schedGateFire struct {
	candidate     StartCandidate
	enabled       bool
	deleted       bool
	triggerStatus string
	settledAt     time.Time
	eventAt       time.Time
	status        string
	outcome       string
	taskID        string
}

// schedGateLedger is the in-memory starter ledger. Its batches claim the fires as the Postgres claims would, and it
// counts the tasks written.
type schedGateLedger struct {
	fires []*schedGateFire
	tasks int
}

func (l *schedGateLedger) begin(context.Context) (startBatch, error) {
	return &schedGateBatch{ledger: l}, nil
}

type schedGateBatch struct {
	ledger *schedGateLedger
}

// claim returns the first authorised fire, not excluded, that match accepts.
func (b *schedGateBatch) claim(excluded []string, match func(*schedGateFire) bool) *StartCandidate {
	for _, f := range b.ledger.fires {
		if f.status != "authorized" || !match(f) {
			continue
		}
		skipped := false
		for _, id := range excluded {
			skipped = skipped || id == f.candidate.FireID
		}
		if !skipped {
			candidate := f.candidate
			return &candidate
		}
	}
	return nil
}

// The claims partition the authorised fires by trigger, schedule state and window, as their SQL does: next and nextDisabled
// need an active trigger, a schedule that is not soft-deleted and a fire authorised and created inside the window;
// nextDeleted takes an active trigger's fire whose schedule is soft-deleted, whatever its age; nextInactive needs a paused
// or disabled trigger; and nextExpired takes an active trigger's fire authorised, or created, before the window.
func (b *schedGateBatch) next(_ context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error) {
	return b.claim(excluded, func(f *schedGateFire) bool {
		return f.triggerStatus == "active" && f.enabled && !f.deleted && schedGateInWindow(f, notBefore)
	}), nil
}

func (b *schedGateBatch) nextDisabled(_ context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error) {
	return b.claim(excluded, func(f *schedGateFire) bool {
		return f.triggerStatus == "active" && !f.enabled && !f.deleted && schedGateInWindow(f, notBefore)
	}), nil
}

func (b *schedGateBatch) nextDeleted(_ context.Context, excluded []string) (*StartCandidate, error) {
	return b.claim(excluded, func(f *schedGateFire) bool {
		return f.triggerStatus == "active" && f.deleted
	}), nil
}

func (b *schedGateBatch) nextInactive(_ context.Context, excluded []string) (*StartCandidate, error) {
	return b.claim(excluded, func(f *schedGateFire) bool { return f.triggerStatus != "active" }), nil
}

func (b *schedGateBatch) nextExpired(_ context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error) {
	return b.claim(excluded, func(f *schedGateFire) bool {
		return f.triggerStatus == "active" && (f.settledAt.Before(notBefore) || f.eventAt.Before(notBefore))
	}), nil
}

// schedGateInWindow is the window the claims bind: the fire was authorised and its event created no earlier than notBefore.
func schedGateInWindow(f *schedGateFire, notBefore time.Time) bool {
	return !f.settledAt.Before(notBefore) && !f.eventAt.Before(notBefore)
}

func (b *schedGateBatch) insert(_ context.Context, candidate StartCandidate, _ string, _ time.Time) (string, error) {
	b.ledger.tasks++
	return "task_" + candidate.FireID, nil
}

func (b *schedGateBatch) markStarted(_ context.Context, fireID, taskID string, _ time.Time) error {
	f := b.find(fireID)
	if f == nil || f.status != "authorized" {
		return errors.New("the fake fire is not authorised and unstarted")
	}
	f.status, f.taskID = "started", taskID
	return nil
}

func (b *schedGateBatch) refuse(_ context.Context, fireID, outcome string, _ time.Time) error {
	f := b.find(fireID)
	if f == nil || f.status != "authorized" {
		return errors.New("the fake fire is not authorised and unstarted")
	}
	f.status, f.outcome = "refused", outcome
	return nil
}

func (b *schedGateBatch) commit(context.Context) error { return nil }

func (b *schedGateBatch) rollback(context.Context) {}

func (b *schedGateBatch) find(fireID string) *schedGateFire {
	for _, f := range b.ledger.fires {
		if f.candidate.FireID == fireID {
			return f
		}
	}
	return nil
}

// schedGateAuthorised is an authorised fire with a valid key, of an active trigger, whose schedule has the given enabled
// state and whose consent contract has not lapsed (a template with no approval_contract carries none).
func schedGateAuthorised(fireID string, enabled bool) *schedGateFire {
	return &schedGateFire{
		candidate: StartCandidate{
			FireID:         fireID,
			TriggerID:      "trg_gate",
			WebhookEventID: "evt_" + fireID,
			Schedule: cron.DueSchedule{
				ID:        "sched_gate",
				OrgID:     "org_gate",
				SpaceRef:  "space_gate",
				SubjectID: "user_gate",
				Template:  json.RawMessage(`{}`),
			},
		},
		enabled:       enabled,
		triggerStatus: "active",
		settledAt:     time.Now().UTC(),
		eventAt:       time.Now().UTC(),
		status:        "authorized",
	}
}

// schedGateClaimTx is a pgx.Tx that records the SQL and the arguments of a claim and answers no row.
type schedGateClaimTx struct {
	pgx.Tx
	sql  string
	args []any
}

func (tx *schedGateClaimTx) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	tx.sql = sql
	tx.args = args
	return schedGateNoRow{}
}

type schedGateNoRow struct{}

func (schedGateNoRow) Scan(...any) error { return pgx.ErrNoRows }

// A fire whose schedule is switched off is refused with its own outcome. It writes no task and is not left authorised.
func TestStarterRefusesAFireWhoseScheduleIsSwitchedOffWithoutStartingIt(t *testing.T) {
	l := &schedGateLedger{fires: []*schedGateFire{schedGateAuthorised("trgfire_off", false)}}
	started, err := newStarter(l).RunOnce(context.Background())
	if err != nil || started != 0 {
		t.Fatalf("RunOnce = %d, %v; want no run started and no error", started, err)
	}
	if f := l.fires[0]; f.status != "refused" || f.outcome != OutcomeScheduleDisabled {
		t.Fatalf("fire is %s with outcome %q; want refused with %q", f.status, f.outcome, OutcomeScheduleDisabled)
	}
	if l.tasks != 0 {
		t.Fatalf("%d tasks written for a switched-off schedule; want none", l.tasks)
	}
}

// An enabled fire still starts in the same pass that refuses a switched-off one, and only the enabled fire takes a task.
func TestStarterStartsAnEnabledFireAndRefusesADisabledOneInTheSamePass(t *testing.T) {
	l := &schedGateLedger{fires: []*schedGateFire{
		schedGateAuthorised("trgfire_off", false),
		schedGateAuthorised("trgfire_on", true),
	}}
	started, err := newStarter(l).RunOnce(context.Background())
	if err != nil || started != 1 {
		t.Fatalf("RunOnce = %d, %v; want one run started and no error", started, err)
	}
	if on := l.fires[1]; on.status != "started" || on.taskID != "task_trgfire_on" {
		t.Fatalf("enabled fire is %s with task %q; want started with its task", on.status, on.taskID)
	}
	if off := l.fires[0]; off.status != "refused" || off.outcome != OutcomeScheduleDisabled {
		t.Fatalf("switched-off fire is %s with outcome %q; want refused with %q", off.status, off.outcome, OutcomeScheduleDisabled)
	}
	if l.tasks != 1 {
		t.Fatalf("%d tasks written; want one, for the enabled fire", l.tasks)
	}
}

// A fire that is already refused for its key or its consent keeps that outcome when its schedule is also switched off.
func TestAFireWhoseScheduleIsSwitchedOffKeepsItsEarlierRefusal(t *testing.T) {
	cases := []struct {
		name    string
		mutate  func(*schedGateFire)
		outcome string
	}{
		{
			name:    "an invalid fire key",
			mutate:  func(f *schedGateFire) { f.candidate.WebhookEventID = "evt/bad" },
			outcome: OutcomeFireKeyInvalid,
		},
		{
			name:    "a lapsed consent contract",
			mutate:  func(f *schedGateFire) { f.candidate.Schedule.Template = json.RawMessage(`{"approval_contract":5}`) },
			outcome: OutcomeApprovalExpired,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := schedGateAuthorised("trgfire_pre", false)
			tc.mutate(f)
			l := &schedGateLedger{fires: []*schedGateFire{f}}
			if _, err := newStarter(l).RunOnce(context.Background()); err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if f.status != "refused" || f.outcome != tc.outcome || l.tasks != 0 {
				t.Fatalf("fire is %s with outcome %q and %d tasks; want refused with %q and no task",
					f.status, f.outcome, l.tasks, tc.outcome)
			}
		})
	}
}

// next and nextDisabled run the same claim and bind opposite schedule states, so a switched-off schedule's fire is claimed
// only by the path that refuses it.
func TestNextAndNextDisabledShareTheClaimAndBindOppositeScheduleStates(t *testing.T) {
	enabledTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: enabledTx}).next(context.Background(), nil, time.Time{}); err != nil {
		t.Fatalf("next: %v", err)
	}
	disabledTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: disabledTx}).nextDisabled(context.Background(), nil, time.Time{}); err != nil {
		t.Fatalf("nextDisabled: %v", err)
	}
	if enabledTx.sql != disabledTx.sql {
		t.Fatalf("next and nextDisabled run different claims:\n%s\n---\n%s", enabledTx.sql, disabledTx.sql)
	}
	// The schedule and the trigger are locked with the fire, so a switch-off or a pause cannot land between the check and the
	// task.
	if sql := strings.Join(strings.Fields(enabledTx.sql), " "); !strings.Contains(sql, "FOR UPDATE OF f, s, t SKIP LOCKED") {
		t.Fatalf("the claim does not lock the schedule and trigger rows with the fire:\n%s", sql)
	}
	if len(enabledTx.args) != 3 || enabledTx.args[1] != true {
		t.Fatalf("next binds %v; want the schedule state true and the window", enabledTx.args)
	}
	if len(disabledTx.args) != 3 || disabledTx.args[1] != false {
		t.Fatalf("nextDisabled binds %v; want the schedule state false and the window", disabledTx.args)
	}
}
