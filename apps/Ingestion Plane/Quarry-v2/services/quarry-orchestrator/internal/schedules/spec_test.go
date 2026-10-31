package schedules

import (
	"context"
	"testing"
	"time"

	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/sdk/client"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/jobs"
)

// specHandle is a fake schedule handle whose Describe returns a fixed
// description, so a test can state what Temporal already holds.
type specHandle struct {
	client.ScheduleHandle
	desc     *client.ScheduleDescription
	updated  bool
	paused   bool
	unpaused bool
}

func (h *specHandle) Describe(context.Context) (*client.ScheduleDescription, error) {
	return h.desc, nil
}
func (h *specHandle) Update(_ context.Context, opts client.ScheduleUpdateOptions) error {
	h.updated = true
	if opts.DoUpdate != nil {
		_, err := opts.DoUpdate(client.ScheduleUpdateInput{Description: *h.desc})
		return err
	}
	return nil
}
func (h *specHandle) Pause(context.Context, client.SchedulePauseOptions) error {
	h.paused = true
	return nil
}
func (h *specHandle) Unpause(context.Context, client.ScheduleUnpauseOptions) error {
	h.unpaused = true
	return nil
}

type specClient struct {
	client.ScheduleClient
	h client.ScheduleHandle
}

func (c specClient) GetHandle(context.Context, string) client.ScheduleHandle { return c.h }

func desiredCrawl() ScheduleSpec {
	return ScheduleSpec{
		ID:             "sch_1",
		OrgID:          "org_a",
		Cron:           "*/5 * * * *",
		Workflow:       ScheduledJobWorkflow,
		Args:           []interface{}{map[string]any{"schedule_id": "sch_1", "org_id": "org_a", "kind": "crawl", "params": map[string]any{"url": "https://example.com"}}},
		Paused:         false,
		Overlap:        "cancel",
		TimeZone:       "Europe/Oslo",
		JitterS:        10,
		CatchupWindowS: 3600,
		PauseOnFailure: true,
	}
}

// describedFrom builds the description Temporal would hold for spec, through the
// same helpers create and update use.
func describedFrom(m *Manager, spec ScheduleSpec, paused bool) *client.ScheduleDescription {
	spc := m.scheduleSpecFor(spec)
	policy := m.policyFor(spec, nil)
	return &client.ScheduleDescription{Schedule: client.Schedule{
		Spec:   &spc,
		Action: m.scheduleActionFor(spec),
		Policy: policy,
		State:  &client.ScheduleState{Paused: paused},
	}}
}

func TestOverlapPolicy_Mapping(t *testing.T) {
	t.Parallel()
	cases := map[string]enumspb.ScheduleOverlapPolicy{
		"skip":   enumspb.SCHEDULE_OVERLAP_POLICY_SKIP,
		"allow":  enumspb.SCHEDULE_OVERLAP_POLICY_ALLOW_ALL,
		"cancel": enumspb.SCHEDULE_OVERLAP_POLICY_CANCEL_OTHER,
		"":       enumspb.SCHEDULE_OVERLAP_POLICY_SKIP,
		"bogus":  enumspb.SCHEDULE_OVERLAP_POLICY_SKIP,
	}
	for in, want := range cases {
		if got := overlapPolicy(in); got != want {
			t.Fatalf("overlap %q -> %v, want %v", in, got, want)
		}
	}
}

func TestNormalizeTimeZone_UTCIsTheDefault(t *testing.T) {
	t.Parallel()
	if normalizeTimeZone("UTC") != "" || normalizeTimeZone("") != "" || normalizeTimeZone("Europe/Oslo") != "Europe/Oslo" {
		t.Fatal("UTC and empty must map to the SDK default")
	}
}

// TestPrepareSpec_ResolvesCrawlBoundsAndTimeout: the reconciler writes the
// resolved bounds into the crawl params and the execution timeout into the args.
// The desired spec it was given is not modified.
func TestPrepareSpec_ResolvesCrawlBoundsAndTimeout(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{CrawlLimits: jobs.DefaultCrawlLimits()})
	spec := desiredCrawl()
	spec.Args = []interface{}{map[string]any{"kind": "crawl", "params": map[string]any{"url": "https://example.com", "max_pages": float64(0)}}}

	out := m.prepareSpec(spec)
	wrap := out.Args[0].(map[string]any)
	params := wrap["params"].(map[string]any)
	if params["max_pages"] != uint32(1000) || params["max_depth"] != uint32(5) {
		t.Fatalf("resolved bounds = %v / %v, want the defaults", params["max_pages"], params["max_depth"])
	}
	if out.ExecutionTimeoutS <= 0 || wrap["execution_timeout_s"] != out.ExecutionTimeoutS {
		t.Fatalf("execution timeout not stamped: %+v", wrap)
	}
	if wrap["overlap_policy"] != "cancel" {
		t.Fatalf("overlap policy not stamped: %v", wrap["overlap_policy"])
	}
	orig := spec.Args[0].(map[string]any)["params"].(map[string]any)
	if orig["max_pages"] != float64(0) {
		t.Fatal("prepareSpec must not mutate the desired spec's params")
	}
}

func TestPrepareSpec_PassesOtherWorkflowsThrough(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{})
	spec := ScheduleSpec{ID: "sch_m", Workflow: "ChangeMonitorWF", Args: []interface{}{"a"}}
	if out := m.prepareSpec(spec); out.Workflow != "ChangeMonitorWF" || out.Args[0] != "a" || out.ExecutionTimeoutS != 0 {
		t.Fatalf("change_monitor spec changed: %+v", out)
	}
}

// TestCanonical_MatchesDescription: a spec and the description Temporal would hold
// for it compare equal, so an unchanged schedule is not updated.
func TestCanonical_MatchesDescription(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{CrawlLimits: jobs.DefaultCrawlLimits(), TaskQueue: "tasks"})
	spec := m.prepareSpec(desiredCrawl())
	want, err := m.canonicalFromSpec(spec)
	if err != nil {
		t.Fatalf("canonical spec: %v", err)
	}
	have, err := m.canonicalFromDescription(describedFrom(m, spec, false), spec)
	if err != nil {
		t.Fatalf("canonical description: %v", err)
	}
	if want != have {
		t.Fatalf("canonical forms differ:\nwant %s\nhave %s", want, have)
	}
}

// TestUpdate_SkippedWhenUnchanged: with the description matching the spec and the
// pause state matching, the reconciler makes no Update and no pause call.
func TestUpdate_SkippedWhenUnchanged(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{CrawlLimits: jobs.DefaultCrawlLimits(), TaskQueue: "tasks"})
	spec := m.prepareSpec(desiredCrawl())
	h := &specHandle{desc: describedFrom(m, spec, false)}
	if err := m.update(context.Background(), specClient{h: h}, spec); err != nil {
		t.Fatalf("update: %v", err)
	}
	if h.updated || h.paused || h.unpaused {
		t.Fatalf("expected no calls, got updated=%v paused=%v unpaused=%v", h.updated, h.paused, h.unpaused)
	}
}

// TestUpdate_PausesOnlyWhenStateDiffers: a changed cron calls Update, and the
// pause state is left alone because it already matches.
func TestUpdate_PausesOnlyWhenStateDiffers(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{CrawlLimits: jobs.DefaultCrawlLimits(), TaskQueue: "tasks"})
	spec := m.prepareSpec(desiredCrawl())
	h := &specHandle{desc: describedFrom(m, spec, false)}
	spec.Cron = "0 * * * *"
	if err := m.update(context.Background(), specClient{h: h}, spec); err != nil {
		t.Fatalf("update: %v", err)
	}
	if !h.updated || h.paused || h.unpaused {
		t.Fatalf("expected update only, got updated=%v paused=%v unpaused=%v", h.updated, h.paused, h.unpaused)
	}
}

// TestPauseIfRunning_NoWorkflowPausesExistingSchedule: a spec with no mapped
// workflow pauses an existing Temporal schedule that is still running.
func TestPauseIfRunning_NoWorkflowPausesExistingSchedule(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{})
	h := &specHandle{desc: &client.ScheduleDescription{Schedule: client.Schedule{State: &client.ScheduleState{Paused: false}}}}
	m.pauseIfRunning(context.Background(), specClient{h: h}, "sch_1")
	if !h.paused {
		t.Fatal("an existing schedule with no workflow must be paused")
	}
	h2 := &specHandle{desc: &client.ScheduleDescription{Schedule: client.Schedule{State: &client.ScheduleState{Paused: true}}}}
	m.pauseIfRunning(context.Background(), specClient{h: h2}, "sch_1")
	if h2.paused {
		t.Fatal("an already paused schedule must not be paused again")
	}
}

// TestCreate_AppliesOverlapTimezoneJitterCatchup: the desired options reach
// Temporal's schedule options and the action timeout.
func TestCreate_AppliesOverlapTimezoneJitterCatchup(t *testing.T) {
	t.Parallel()
	m := New(nil, Config{CrawlLimits: jobs.DefaultCrawlLimits(), TaskQueue: "tasks"})
	spec := m.prepareSpec(desiredCrawl())
	spc := m.scheduleSpecFor(spec)
	if spc.TimeZoneName != "Europe/Oslo" || spc.Jitter != 10*time.Second {
		t.Fatalf("spec = %+v", spc)
	}
	act := m.scheduleActionFor(spec)
	if act.WorkflowExecutionTimeout <= 0 || act.Workflow != ScheduledJobWorkflow {
		t.Fatalf("action = %+v", act)
	}
	// Compared as the enum, not its String(): the SDK prints it as "CancelOther".
	if overlapPolicy(spec.Overlap) != enumspb.SCHEDULE_OVERLAP_POLICY_CANCEL_OTHER {
		t.Fatalf("cancel overlap = %v, must map to CANCEL_OTHER", overlapPolicy(spec.Overlap))
	}
	if pol := m.policyFor(spec, nil); pol.CatchupWindow != time.Hour || !pol.PauseOnFailure {
		t.Fatalf("policy = %+v", pol)
	}
}
