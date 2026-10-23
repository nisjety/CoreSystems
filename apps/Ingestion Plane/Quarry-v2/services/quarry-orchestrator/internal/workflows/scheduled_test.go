package workflows

import (
	"context"
	"errors"
	"strings"
	"testing"

	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

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
	se.env.RegisterWorkflowWithOptions(func(_ workflow.Context, in CrawlJobInput) error {
		se.crawlCalls = append(se.crawlCalls, in)
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
