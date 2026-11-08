package workflows

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	commonpb "go.temporal.io/api/common/v1"
	enumspb "go.temporal.io/api/enums/v1"
	historypb "go.temporal.io/api/history/v1"
	taskqueuepb "go.temporal.io/api/taskqueue/v1"
	"go.temporal.io/api/temporalproto"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/converter"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/worker"
	"go.temporal.io/sdk/workflow"
	"google.golang.org/protobuf/types/known/durationpb"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// scheduledEnv registers fakes under the activity and child-workflow names the
// wrapper calls. Each fake records what it was given, so a test can assert on it.
type scheduledEnv struct {
	env        *testsuite.TestWorkflowEnvironment
	preflight  activities.PreflightResult
	register   []activities.RegisterRunInput
	events     []quarrycontracts.EventType
	eventsPay  []map[string]any
	crawlCalls []CrawlJobInput
	batchCalls []BatchJobInput
	scrapeIn   []ScrapeJobInput
	childErr   error
	// crawlBody, when set, replaces the default CrawlJobWF fake body.
	crawlBody func(workflow.Context, CrawlJobInput) error
}

func newScheduledEnv(t *testing.T) *scheduledEnv {
	t.Helper()
	var ts testsuite.WorkflowTestSuite
	se := &scheduledEnv{env: ts.NewTestWorkflowEnvironment(), preflight: activities.PreflightResult{Allow: true}}
	se.env.RegisterActivityWithOptions(func(_ context.Context, _ activities.PreflightInput) (activities.PreflightResult, error) {
		return se.preflight, nil
	}, activityName("PreflightFire"))
	se.env.RegisterActivityWithOptions(func(_ context.Context, in activities.RegisterRunInput) (activities.RegisterRunResult, error) {
		se.register = append(se.register, in)
		return activities.RegisterRunResult{JobID: "job_test", Status: in.Status}, nil
	}, activityName("RegisterScheduledRun"))
	se.env.RegisterActivityWithOptions(func(_ context.Context, _ string, evt quarrycontracts.Event) error {
		se.events = append(se.events, evt.Type)
		se.eventsPay = append(se.eventsPay, evt.Payload)
		return nil
	}, activityName("EmitEvent"))
	se.env.RegisterWorkflowWithOptions(func(ctx workflow.Context, in CrawlJobInput) error {
		se.crawlCalls = append(se.crawlCalls, in)
		if se.crawlBody != nil {
			return se.crawlBody(ctx, in)
		}
		return se.childErr
	}, workflow.RegisterOptions{Name: "CrawlJobWF"})
	se.env.RegisterWorkflowWithOptions(func(_ workflow.Context, in BatchJobInput) error {
		se.batchCalls = append(se.batchCalls, in)
		return nil
	}, workflow.RegisterOptions{Name: "BatchJobWF"})
	se.env.RegisterWorkflowWithOptions(func(_ workflow.Context, in ScrapeJobInput) error {
		se.scrapeIn = append(se.scrapeIn, in)
		return nil
	}, workflow.RegisterOptions{Name: "ScrapeJobWF"})
	acts := &activities.Activities{}
	se.env.RegisterWorkflowWithOptions(func(ctx workflow.Context, in ScheduledJobInput) error {
		return ScheduledJobWF(ctx, in, acts)
	}, workflow.RegisterOptions{Name: "ScheduledJobWF"})
	return se
}

// activityName keeps the activity registered under the method name the wrapper
// calls through the *activities.Activities method values.
func activityName(name string) activity.RegisterOptions { return activity.RegisterOptions{Name: name} }

func crawlFire(kind string) ScheduledJobInput {
	return ScheduledJobInput{
		ScheduleID:    "sch_test",
		OrgID:         "org_a",
		CreatedBy:     "user_1",
		Kind:          kind,
		OverlapPolicy: "skip",
		Params: map[string]any{
			"url":       "https://example.com",
			"org_id":    "org_a",
			"user_id":   "user_1",
			"max_pages": float64(5),
			"max_depth": float64(2),
		},
	}
}

// TestScheduledJobWF_HappyPathCrawl: an allowed fire registers a running job and
// runs CrawlJobWF with the job id and the resolved bounds from params.
func TestScheduledJobWF_HappyPathCrawl(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	se.env.ExecuteWorkflow("ScheduledJobWF", crawlFire("crawl"))

	if !se.env.IsWorkflowCompleted() {
		t.Fatal("workflow did not complete")
	}
	if err := se.env.GetWorkflowError(); err != nil {
		t.Fatalf("workflow error: %v", err)
	}
	if len(se.register) != 1 || se.register[0].Status != "running" || se.register[0].ScheduleID != "sch_test" {
		t.Fatalf("register calls = %+v, want one running fire", se.register)
	}
	if len(se.crawlCalls) != 1 {
		t.Fatalf("CrawlJobWF ran %d times, want 1", len(se.crawlCalls))
	}
	child := se.crawlCalls[0]
	if child.JobID != "job_test" || child.OrgID != "org_a" || child.MaxPages != 5 || child.MaxDepth != 2 {
		t.Fatalf("child input = %+v", child)
	}
	if !strings.HasPrefix(child.RunID, "run_") || strings.Contains(child.RunID, "-") {
		t.Fatalf("run id %q must be run_ plus the execution id without dashes", child.RunID)
	}
}

// TestScheduledJobWF_RunIDIsDerivedFromExecution: the run id registered with
// control is the run id the child's events use.
func TestScheduledJobWF_RunIDIsDerivedFromExecution(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	se.env.ExecuteWorkflow("ScheduledJobWF", crawlFire("crawl"))

	if len(se.register) != 1 || len(se.crawlCalls) != 1 {
		t.Fatalf("unexpected calls: register=%d crawl=%d", len(se.register), len(se.crawlCalls))
	}
	if se.register[0].RunID != se.crawlCalls[0].RunID || se.register[0].RunKey != se.register[0].RunID {
		t.Fatalf("run id mismatch: register=%+v child=%q", se.register[0], se.crawlCalls[0].RunID)
	}
}

// TestScheduledJobWF_PreflightDenySkipsAndRegistersSkipped: a denied fire starts
// no child and records a terminal skipped job with the reason class.
func TestScheduledJobWF_PreflightDenySkipsAndRegistersSkipped(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	se.preflight = activities.PreflightResult{Allow: false, Reason: ScheduledReasonPreflightDenied}
	se.env.ExecuteWorkflow("ScheduledJobWF", crawlFire("crawl"))

	if err := se.env.GetWorkflowError(); err != nil {
		t.Fatalf("a denied fire is not a failure: %v", err)
	}
	if len(se.register) != 1 || se.register[0].Status != "skipped" || se.register[0].Reason != ScheduledReasonPreflightDenied {
		t.Fatalf("register = %+v, want one skipped fire with the reason", se.register)
	}
	if len(se.crawlCalls) != 0 {
		t.Fatal("a denied fire must not start a child")
	}
}

// TestScheduledJobWF_ChildFailureEmitsRunFailed: a child error becomes a run_failed
// event with the schedule fields and a reason class, and no URL.
func TestScheduledJobWF_ChildFailureEmitsRunFailed(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	se.childErr = errors.New("boom https://example.com/private")
	se.env.ExecuteWorkflow("ScheduledJobWF", crawlFire("crawl"))

	if se.env.GetWorkflowError() == nil {
		t.Fatal("a failed child must fail the fire, so pause-on-failure can see it")
	}
	if len(se.events) != 1 || se.events[0] != quarrycontracts.EvtRunFailed {
		t.Fatalf("events = %v, want one run_failed", se.events)
	}
	pay := se.eventsPay[0]
	if pay["schedule_id"] != "sch_test" || pay["org_id"] != "org_a" || pay["reason"] != ScheduledReasonChildFailed {
		t.Fatalf("payload = %v", pay)
	}
	if _, has := pay["url"]; has {
		t.Fatal("the failure event must not carry a URL")
	}
}

// TestScheduledJobWF_BatchAndScrapeKindsMapToTheirChildren: each kind starts its
// own job workflow with the shared builder's input.
func TestScheduledJobWF_BatchAndScrapeKindsMapToTheirChildren(t *testing.T) {
	t.Parallel()

	se := newScheduledEnv(t)
	batch := crawlFire("batch")
	batch.Params = map[string]any{"urls": []any{"https://a.example", "https://b.example"}, "org_id": "org_a"}
	se.env.ExecuteWorkflow("ScheduledJobWF", batch)
	if err := se.env.GetWorkflowError(); err != nil {
		t.Fatalf("batch fire: %v", err)
	}
	if len(se.batchCalls) != 1 || len(se.batchCalls[0].URLs) != 2 {
		t.Fatalf("BatchJobWF calls = %+v", se.batchCalls)
	}

	se2 := newScheduledEnv(t)
	scrape := crawlFire("scrape")
	scrape.Params = map[string]any{"url": "https://example.com", "org_id": "org_a"}
	se2.env.ExecuteWorkflow("ScheduledJobWF", scrape)
	if err := se2.env.GetWorkflowError(); err != nil {
		t.Fatalf("scrape fire: %v", err)
	}
	if len(se2.scrapeIn) != 1 || se2.scrapeIn[0].URL != "https://example.com" {
		t.Fatalf("ScrapeJobWF calls = %+v", se2.scrapeIn)
	}
}

// TestScheduledJobWF_UnknownKindFailsBeforeAChild: an unsupported kind is
// refused after its run is registered, and the run ends as failed.
func TestScheduledJobWF_UnknownKindFailsBeforeAChild(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	se.env.ExecuteWorkflow("ScheduledJobWF", crawlFire("change_monitor"))
	if se.env.GetWorkflowError() == nil {
		t.Fatal("an unknown kind must fail the fire")
	}
	if len(se.events) != 1 || se.events[0] != quarrycontracts.EvtRunFailed {
		t.Fatalf("events = %v, want run_failed", se.events)
	}
}

// cancelFire is a crawl fire whose schedule overlap policy is "cancel".
func cancelFire() ScheduledJobInput {
	in := crawlFire("crawl")
	in.OverlapPolicy = ScheduledOverlapCancel
	return in
}

// TestScheduledJobWF_OverlapCancelSignalsChild: when the schedule's overlap
// policy cancels a fire, the wrapper signals the running child to cancel on a
// disconnected context, waits for it to stop, and ends the fire without a
// failure event (a cancelled fire is not a failed fire).
func TestScheduledJobWF_OverlapCancelSignalsChild(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	var gotCancelSignal bool
	se.crawlBody = func(ctx workflow.Context, _ CrawlJobInput) error {
		// The child also sees the cancellation request, so it waits for the
		// signal on a context that survives it.
		disc, cancel := workflow.NewDisconnectedContext(ctx)
		defer cancel()
		workflow.GetSignalChannel(disc, SignalCancel).Receive(disc, nil)
		gotCancelSignal = true
		return nil
	}
	se.env.RegisterDelayedCallback(func() { se.env.CancelWorkflow() }, time.Minute)
	se.env.ExecuteWorkflow("ScheduledJobWF", cancelFire())

	if !se.env.IsWorkflowCompleted() {
		t.Fatal("workflow did not complete")
	}
	if err := se.env.GetWorkflowError(); err != nil {
		t.Fatalf("a cancelled fire must end without an error: %v", err)
	}
	if !gotCancelSignal {
		t.Fatal("the child never received the cancel signal")
	}
	if len(se.events) != 0 {
		t.Fatalf("events = %v, a cancelled fire emits neither run_failed nor run_timed_out", se.events)
	}
}

// TestScheduledJobWF_OverlapCancelWaitIsBounded: a child that ignores the signal
// does not hold the wrapper open. The wrapper stops waiting after
// scheduledCancelWait and still ends the fire without an error.
func TestScheduledJobWF_OverlapCancelWaitIsBounded(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	se.crawlBody = func(ctx workflow.Context, _ CrawlJobInput) error {
		disc, cancel := workflow.NewDisconnectedContext(ctx)
		defer cancel()
		return workflow.Sleep(disc, 24*time.Hour)
	}
	se.env.RegisterDelayedCallback(func() { se.env.CancelWorkflow() }, time.Minute)
	se.env.ExecuteWorkflow("ScheduledJobWF", cancelFire())

	if !se.env.IsWorkflowCompleted() {
		t.Fatal("workflow did not complete")
	}
	if err := se.env.GetWorkflowError(); err != nil {
		t.Fatalf("a cancelled fire must end without an error: %v", err)
	}
	if len(se.events) != 0 {
		t.Fatalf("events = %v, want none", se.events)
	}
}

// TestScheduledJobWF_NonCancelPolicyDoesNotSignalChild: only the "cancel"
// overlap policy signals the child; any other policy that sees a cancelled
// context returns quietly.
func TestScheduledJobWF_NonCancelPolicyDoesNotSignalChild(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	var gotCancelSignal bool
	se.crawlBody = func(ctx workflow.Context, _ CrawlJobInput) error {
		disc, cancel := workflow.NewDisconnectedContext(ctx)
		defer cancel()
		sel := workflow.NewSelector(disc)
		sel.AddReceive(workflow.GetSignalChannel(disc, SignalCancel), func(c workflow.ReceiveChannel, _ bool) {
			c.Receive(disc, nil)
			gotCancelSignal = true
		})
		sel.AddFuture(workflow.NewTimer(disc, time.Hour), func(workflow.Future) {})
		sel.Select(disc)
		return nil
	}
	se.env.RegisterDelayedCallback(func() { se.env.CancelWorkflow() }, time.Minute)
	in := crawlFire("crawl")
	in.OverlapPolicy = "buffer_one"
	se.env.ExecuteWorkflow("ScheduledJobWF", in)

	if err := se.env.GetWorkflowError(); err != nil {
		t.Fatalf("workflow error: %v", err)
	}
	if gotCancelSignal {
		t.Fatal("only the cancel overlap policy may signal the child")
	}
}

// TestScheduledJobWF_ChildTimeoutEmitsRunTimedOut: a child that hits the fire's
// execution timeout becomes a run_timed_out event with the timed-out reason
// class, the schedule fields and no URL, and the fire still fails so the
// schedule's pause-on-failure option can see it.
func TestScheduledJobWF_ChildTimeoutEmitsRunTimedOut(t *testing.T) {
	t.Parallel()
	se := newScheduledEnv(t)
	// The test environment does not enforce a child's execution timeout, so the
	// child closes the way the server closes a timed-out child: with a timeout
	// error.
	se.crawlBody = func(ctx workflow.Context, _ CrawlJobInput) error {
		return temporal.NewTimeoutError(enumspb.TIMEOUT_TYPE_START_TO_CLOSE, nil)
	}
	in := crawlFire("crawl")
	in.ExecutionTimeoutS = 60
	se.env.ExecuteWorkflow("ScheduledJobWF", in)

	if !se.env.IsWorkflowCompleted() {
		t.Fatal("workflow did not complete")
	}
	werr := se.env.GetWorkflowError()
	if werr == nil {
		t.Fatal("a timed-out child must fail the fire")
	}
	var timeoutErr *temporal.TimeoutError
	if !errors.As(werr, &timeoutErr) {
		t.Fatalf("workflow error %v does not wrap a timeout", werr)
	}
	if len(se.events) != 1 || se.events[0] != quarrycontracts.EvtRunTimedOut {
		t.Fatalf("events = %v, want one run_timed_out", se.events)
	}
	pay := se.eventsPay[0]
	if pay["schedule_id"] != "sch_test" || pay["org_id"] != "org_a" || pay["created_by"] != "user_1" ||
		pay["reason"] != ScheduledReasonChildTimedOut {
		t.Fatalf("payload = %v", pay)
	}
	if _, has := pay["url"]; has {
		t.Fatal("the timeout event must not carry a URL")
	}
}

// replayHistoryFixture is the recorded history of one allowed crawl fire. It
// lives next to the schedule tests (testdata of internal/schedules).
const replayHistoryFixture = "../schedules/testdata/scheduled_job_crawl_history.json"

// replayScheduledWF registers the wrapper the way main.go does and replays a
// history against it.
func replayScheduledWF(history *historypb.History) error {
	r := worker.NewWorkflowReplayer()
	acts := &activities.Activities{}
	r.RegisterWorkflowWithOptions(func(ctx workflow.Context, in ScheduledJobInput) error {
		return ScheduledJobWF(ctx, in, acts)
	}, workflow.RegisterOptions{Name: "ScheduledJobWF"})
	return r.ReplayWorkflowHistory(nil, history)
}

func loadReplayHistory(t *testing.T) *historypb.History {
	t.Helper()
	raw, err := os.ReadFile(replayHistoryFixture)
	if err != nil {
		t.Fatalf("read fixture: %v", err)
	}
	history := &historypb.History{}
	if err := (temporalproto.CustomJSONUnmarshalOptions{DiscardUnknown: true}).Unmarshal(raw, history); err != nil {
		t.Fatalf("decode fixture: %v", err)
	}
	return history
}

// TestScheduledJobWF_ReplaysRecordedHistory: the recorded history of an allowed
// crawl fire (preflight, register, CrawlJobWF child, completion) replays
// against the current wrapper code with no non-determinism. A change that adds,
// removes or reorders a command in the wrapper fails this test before it can
// strand fires already in flight.
func TestScheduledJobWF_ReplaysRecordedHistory(t *testing.T) {
	t.Parallel()
	history := loadReplayHistory(t)
	if err := replayScheduledWF(history); err != nil {
		t.Fatalf("replay of the recorded history failed: %v", err)
	}
}

// TestScheduledJobWF_ReplayRejectsAChangedCommand: the fixture has teeth. The
// same history with the first activity renamed must fail to replay.
func TestScheduledJobWF_ReplayRejectsAChangedCommand(t *testing.T) {
	t.Parallel()
	history := loadReplayHistory(t)
	changed := false
	for _, ev := range history.Events {
		if a := ev.GetActivityTaskScheduledEventAttributes(); a != nil && a.ActivityType.Name == "PreflightFire" {
			a.ActivityType.Name = "PreflightFireRenamed"
			changed = true
			break
		}
	}
	if !changed {
		t.Fatal("fixture holds no PreflightFire activity")
	}
	if err := replayScheduledWF(history); err == nil {
		t.Fatal("replay of a changed history must report non-determinism")
	}
}

// TestRegenerateScheduledReplayFixture rewrites the recorded history. It runs
// only with QUARRY_REGEN_REPLAY_FIXTURE=1. The history is built event by event in
// the shape the Temporal server records for the fire, not captured from a live
// cluster; replay is what proves it matches the workflow.
func TestRegenerateScheduledReplayFixture(t *testing.T) {
	if os.Getenv("QUARRY_REGEN_REPLAY_FIXTURE") != "1" {
		t.Skip("set QUARRY_REGEN_REPLAY_FIXTURE=1 to rewrite the recorded history")
	}
	dc := converter.GetDefaultDataConverter()
	payloads := func(v ...any) *commonpb.Payloads {
		p, err := dc.ToPayloads(v...)
		if err != nil {
			t.Fatalf("encode payload: %v", err)
		}
		return p
	}
	base := time.Date(2026, 11, 1, 9, 0, 0, 0, time.UTC)
	tq := &taskqueuepb.TaskQueue{Name: "quarry-orchestrator", Kind: enumspb.TASK_QUEUE_KIND_NORMAL}
	var events []*historypb.HistoryEvent
	add := func(typ enumspb.EventType, set func(*historypb.HistoryEvent)) int64 {
		ev := &historypb.HistoryEvent{
			EventId:   int64(len(events) + 1),
			EventTime: timestamppb.New(base.Add(time.Duration(len(events)) * 100 * time.Millisecond)),
			EventType: typ,
			TaskId:    int64(1048576 + len(events)),
		}
		set(ev)
		events = append(events, ev)
		return ev.EventId
	}
	// wft appends the three events of one workflow task and returns the
	// scheduled-event id.
	wft := func() {
		sched := add(enumspb.EVENT_TYPE_WORKFLOW_TASK_SCHEDULED, func(e *historypb.HistoryEvent) {
			e.Attributes = &historypb.HistoryEvent_WorkflowTaskScheduledEventAttributes{
				WorkflowTaskScheduledEventAttributes: &historypb.WorkflowTaskScheduledEventAttributes{
					TaskQueue: tq, StartToCloseTimeout: durationpb.New(10 * time.Second), Attempt: 1,
				},
			}
		})
		started := add(enumspb.EVENT_TYPE_WORKFLOW_TASK_STARTED, func(e *historypb.HistoryEvent) {
			e.Attributes = &historypb.HistoryEvent_WorkflowTaskStartedEventAttributes{
				WorkflowTaskStartedEventAttributes: &historypb.WorkflowTaskStartedEventAttributes{
					ScheduledEventId: sched, Identity: "quarry-orchestrator@worker", RequestId: "req-wft-" + fmt.Sprint(sched),
				},
			}
		})
		add(enumspb.EVENT_TYPE_WORKFLOW_TASK_COMPLETED, func(e *historypb.HistoryEvent) {
			e.Attributes = &historypb.HistoryEvent_WorkflowTaskCompletedEventAttributes{
				WorkflowTaskCompletedEventAttributes: &historypb.WorkflowTaskCompletedEventAttributes{
					ScheduledEventId: sched, StartedEventId: started, Identity: "quarry-orchestrator@worker",
				},
			}
		})
	}
	// activity appends scheduled, started and completed events for one call.
	activity := func(name string, in, out any) {
		wtc := events[len(events)-1].EventId
		sched := add(enumspb.EVENT_TYPE_ACTIVITY_TASK_SCHEDULED, func(e *historypb.HistoryEvent) {
			e.Attributes = &historypb.HistoryEvent_ActivityTaskScheduledEventAttributes{
				ActivityTaskScheduledEventAttributes: &historypb.ActivityTaskScheduledEventAttributes{
					ActivityId:                   fmt.Sprint(len(events) + 1),
					ActivityType:                 &commonpb.ActivityType{Name: name},
					TaskQueue:                    tq,
					Input:                        payloads(in),
					ScheduleToCloseTimeout:       durationpb.New(0),
					ScheduleToStartTimeout:       durationpb.New(0),
					StartToCloseTimeout:          durationpb.New(5 * time.Minute),
					WorkflowTaskCompletedEventId: wtc,
					RetryPolicy: &commonpb.RetryPolicy{
						InitialInterval:    durationpb.New(time.Second),
						BackoffCoefficient: 2,
						MaximumInterval:    durationpb.New(time.Minute),
						MaximumAttempts:    5,
					},
				},
			}
		})
		started := add(enumspb.EVENT_TYPE_ACTIVITY_TASK_STARTED, func(e *historypb.HistoryEvent) {
			e.Attributes = &historypb.HistoryEvent_ActivityTaskStartedEventAttributes{
				ActivityTaskStartedEventAttributes: &historypb.ActivityTaskStartedEventAttributes{
					ScheduledEventId: sched, Identity: "quarry-orchestrator@worker", RequestId: "req-act-" + fmt.Sprint(sched), Attempt: 1,
				},
			}
		})
		add(enumspb.EVENT_TYPE_ACTIVITY_TASK_COMPLETED, func(e *historypb.HistoryEvent) {
			e.Attributes = &historypb.HistoryEvent_ActivityTaskCompletedEventAttributes{
				ActivityTaskCompletedEventAttributes: &historypb.ActivityTaskCompletedEventAttributes{
					Result: payloads(out), ScheduledEventId: sched, StartedEventId: started, Identity: "quarry-orchestrator@worker",
				},
			}
		})
	}

	const (
		runID = "7c1d6a52-0f3e-4c3b-9a0e-5b2f8d4e9a11"
		jobID = "job_recorded"
	)
	fire := ScheduledJobInput{
		ScheduleID:        "sch_recorded",
		OrgID:             "org_a",
		CreatedBy:         "user_1",
		Kind:              "crawl",
		OverlapPolicy:     "skip",
		ExecutionTimeoutS: 3600,
		Params: map[string]any{
			"url": "https://example.com", "org_id": "org_a", "user_id": "user_1",
			"max_pages": float64(5), "max_depth": float64(2),
		},
	}
	childRunID := "run_" + strings.ReplaceAll(runID, "-", "")

	add(enumspb.EVENT_TYPE_WORKFLOW_EXECUTION_STARTED, func(e *historypb.HistoryEvent) {
		e.Attributes = &historypb.HistoryEvent_WorkflowExecutionStartedEventAttributes{
			WorkflowExecutionStartedEventAttributes: &historypb.WorkflowExecutionStartedEventAttributes{
				WorkflowType:             &commonpb.WorkflowType{Name: "ScheduledJobWF"},
				TaskQueue:                tq,
				Input:                    payloads(fire),
				WorkflowExecutionTimeout: durationpb.New(time.Hour),
				WorkflowRunTimeout:       durationpb.New(time.Hour),
				WorkflowTaskTimeout:      durationpb.New(10 * time.Second),
				OriginalExecutionRunId:   runID,
				FirstExecutionRunId:      runID,
				Attempt:                  1,
				Identity:                 "schedule",
			},
		}
	})
	wft()
	activity("PreflightFire", activities.PreflightInput{ScheduleID: fire.ScheduleID, OrgID: fire.OrgID},
		activities.PreflightResult{Allow: true, Posture: "unchecked", Entitlement: "floor"})
	wft()
	activity("RegisterScheduledRun", activities.RegisterRunInput{
		ScheduleID: fire.ScheduleID, RunKey: childRunID, RunID: childRunID, Status: "running", WorkflowID: "sch_recorded-fire",
	}, activities.RegisterRunResult{JobID: jobID, Status: "running"})
	wft()
	initiated := add(enumspb.EVENT_TYPE_START_CHILD_WORKFLOW_EXECUTION_INITIATED, func(e *historypb.HistoryEvent) {
		e.Attributes = &historypb.HistoryEvent_StartChildWorkflowExecutionInitiatedEventAttributes{
			StartChildWorkflowExecutionInitiatedEventAttributes: &historypb.StartChildWorkflowExecutionInitiatedEventAttributes{
				Namespace:                    "default",
				WorkflowId:                   "quarry-job-" + jobID,
				WorkflowType:                 &commonpb.WorkflowType{Name: "CrawlJobWF"},
				TaskQueue:                    tq,
				Input:                        payloads(map[string]any{"job_id": jobID}),
				WorkflowExecutionTimeout:     durationpb.New(time.Hour),
				ParentClosePolicy:            enumspb.PARENT_CLOSE_POLICY_REQUEST_CANCEL,
				WorkflowTaskCompletedEventId: events[len(events)-1].EventId,
				WorkflowIdReusePolicy:        enumspb.WORKFLOW_ID_REUSE_POLICY_ALLOW_DUPLICATE_FAILED_ONLY,
			},
		}
	})
	childStarted := add(enumspb.EVENT_TYPE_CHILD_WORKFLOW_EXECUTION_STARTED, func(e *historypb.HistoryEvent) {
		e.Attributes = &historypb.HistoryEvent_ChildWorkflowExecutionStartedEventAttributes{
			ChildWorkflowExecutionStartedEventAttributes: &historypb.ChildWorkflowExecutionStartedEventAttributes{
				Namespace:         "default",
				InitiatedEventId:  initiated,
				WorkflowExecution: &commonpb.WorkflowExecution{WorkflowId: "quarry-job-" + jobID, RunId: "b3f1c9a4-2d7e-4f60-8a15-6e0c3d9b7f22"},
				WorkflowType:      &commonpb.WorkflowType{Name: "CrawlJobWF"},
			},
		}
	})
	wft()
	add(enumspb.EVENT_TYPE_CHILD_WORKFLOW_EXECUTION_COMPLETED, func(e *historypb.HistoryEvent) {
		e.Attributes = &historypb.HistoryEvent_ChildWorkflowExecutionCompletedEventAttributes{
			ChildWorkflowExecutionCompletedEventAttributes: &historypb.ChildWorkflowExecutionCompletedEventAttributes{
				Namespace:         "default",
				InitiatedEventId:  initiated,
				StartedEventId:    childStarted,
				WorkflowExecution: &commonpb.WorkflowExecution{WorkflowId: "quarry-job-" + jobID, RunId: "b3f1c9a4-2d7e-4f60-8a15-6e0c3d9b7f22"},
				WorkflowType:      &commonpb.WorkflowType{Name: "CrawlJobWF"},
			},
		}
	})
	wft()
	add(enumspb.EVENT_TYPE_WORKFLOW_EXECUTION_COMPLETED, func(e *historypb.HistoryEvent) {
		e.Attributes = &historypb.HistoryEvent_WorkflowExecutionCompletedEventAttributes{
			WorkflowExecutionCompletedEventAttributes: &historypb.WorkflowExecutionCompletedEventAttributes{
				WorkflowTaskCompletedEventId: events[len(events)-1].EventId,
			},
		}
	})

	history := &historypb.History{Events: events}
	if err := replayScheduledWF(history); err != nil {
		t.Fatalf("the built history does not replay: %v", err)
	}
	out, err := (temporalproto.CustomJSONMarshalOptions{Indent: "  "}).Marshal(history)
	if err != nil {
		t.Fatalf("encode history: %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(replayHistoryFixture), 0o755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	if err := os.WriteFile(replayHistoryFixture, append(out, '\n'), 0o644); err != nil {
		t.Fatalf("write fixture: %v", err)
	}
}
