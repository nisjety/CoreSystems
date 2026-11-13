package cron

import (
	"context"
	"testing"
	"time"
)

// V1 R14 (owner accepted recording the outcome): session-core now settles a scheduled agent run
// whose completed step withheld effects as a needs_review run, and records its cron fire with
// status "failed" and outcome "needs_review" (scheduled_result_outcome / scheduled_fire_status in
// session-core grpc.rs). tripsFailureLimit reads the fire status only, so a needs_review fire
// counts as a failure toward the paused_failures auto-pause. These tests pin what the code does.

// Three fires that each needed review (recorded failed) pause the schedule, exactly as three
// failed fires do; the schedule's notice says paused_failures.
func TestThreeNeedsReviewFiresCountTowardTheFailurePause(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	live := guardTemplate(t, now.Add(30*24*time.Hour))
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{authorizer: authorizer}).WithContractGuards(true)
	// The status column session-core writes for a needs_review fire.
	tx := &guardBatchTx{statuses: []string{"failed", "failed", "failed"}}
	var pauses []pauseNotice
	fired, err := sweeper.sweepDue(ctx, tx, guardDue(live, now.Add(-time.Minute)), now, &pauses)
	if err != nil {
		t.Fatalf("sweepDue: %v", err)
	}
	if fired || authorizer.calls != 0 || len(pauses) != 1 || pauses[0].outcome != outcomePausedFailures {
		t.Fatalf("want a paused_failures pause, got fired=%v calls=%d pauses=%+v", fired, authorizer.calls, pauses)
	}
}

// Without the approval contract guards (CRON_CONTRACT_GUARDS off) the failure count is never read,
// so needs_review fires never pause a schedule: the switch-off path is unchanged.
func TestNeedsReviewFiresDoNotPauseWithoutTheContractGuards(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	live := guardTemplate(t, now.Add(30*24*time.Hour))
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{authorizer: authorizer}).WithContractGuards(false)
	tx := &guardBatchTx{statuses: []string{"failed", "failed", "failed"}}
	var pauses []pauseNotice
	fired, err := sweeper.sweepDue(ctx, tx, guardDue(live, now.Add(-time.Minute)), now, &pauses)
	if err != nil {
		t.Fatalf("sweepDue: %v", err)
	}
	if !fired || len(pauses) != 0 {
		t.Fatalf("guards off: fired=%v pauses=%+v; want the fire and no pause", fired, pauses)
	}
}

// A completed fire between needs_review fires still breaks the run, so two reviews around a clean
// run do not pause.
func TestACompletedFireBreaksANeedsReviewRun(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	live := guardTemplate(t, now.Add(30*24*time.Hour))
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{authorizer: authorizer}).WithContractGuards(true)
	tx := &guardBatchTx{statuses: []string{"failed", "completed", "failed"}}
	var pauses []pauseNotice
	fired, err := sweeper.sweepDue(ctx, tx, guardDue(live, now.Add(-time.Minute)), now, &pauses)
	if err != nil || !fired || len(pauses) != 0 {
		t.Fatalf("fired=%v pauses=%+v err=%v", fired, pauses, err)
	}
}
