package schedules

// Spec helpers for the executable schedules of WS15.S2: the Temporal overlap
// and time-zone mapping, the stamping of the fire's configuration-dependent
// fields (resolved crawl bounds, execution timeout), and the canonical form the
// reconciler compares to decide whether a Temporal schedule must be updated.
//
// Change detection compares what Temporal holds (Describe) with the desired
// spec, not a memo: the SDK (v1.29) cannot update a schedule's memo, so a
// spec_hash memo would go stale after the first change and churn every pass.

import (
	"encoding/json"
	"fmt"
	"time"

	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/sdk/client"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/timeouts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/workflows"
)

// ScheduledJobWorkflow is the wrapper a scrape, crawl or batch schedule fires
// (workflows.ScheduledJobWF, registered by name in main.go).
const ScheduledJobWorkflow = "ScheduledJobWF"

// scheduledActionGrace is added to the job's execution timeout for the schedule
// action timeout, so the child's own timeout ends the run first.
const scheduledActionGrace = 60 * time.Second

// overlapPolicy maps the stored overlap policy (skip, allow, cancel) to Temporal.
// An unknown or empty value is skip, the schedule default.
func overlapPolicy(s string) enumspb.ScheduleOverlapPolicy {
	switch s {
	case "allow":
		return enumspb.SCHEDULE_OVERLAP_POLICY_ALLOW_ALL
	case workflows.ScheduledOverlapCancel:
		return enumspb.SCHEDULE_OVERLAP_POLICY_CANCEL_OTHER
	default:
		return enumspb.SCHEDULE_OVERLAP_POLICY_SKIP
	}
}

// normalizeTimeZone returns the zone Temporal should be given. UTC and empty are
// both the SDK default, so they map to "" and an existing schedule sees no change.
func normalizeTimeZone(tz string) string {
	if tz == "" || tz == "UTC" {
		return ""
	}
	return tz
}

// prepareSpec stamps the fields of a scheduled-job fire that depend on the
// orchestrator's configuration: the resolved crawl bounds (written into params),
// the overlap policy, the execution timeout and the action timeout. The values
// are part of the schedule args, so a configuration change is a spec change and
// is applied on the next pass. Other workflows pass through unchanged.
func (m *Manager) prepareSpec(spec ScheduleSpec) ScheduleSpec {
	if spec.Workflow != ScheduledJobWorkflow || len(spec.Args) == 0 {
		return spec
	}
	orig, ok := spec.Args[0].(map[string]any)
	if !ok {
		return spec
	}
	// The desired spec is shared with the caller (it is read again for the
	// pause state and the log), so the stamped fields go into a copy of the
	// wrapper, never into the map the caller holds.
	wrap := copyMap(orig)
	kind, _ := wrap["kind"].(string)
	params := copyMap(wrap["params"])

	if kind == "crawl" {
		resolved := m.cfg.CrawlLimits.Resolve(
			workflows.UintFromParam(params["max_pages"]),
			workflows.UintFromParam(params["max_depth"]),
		)
		params["max_pages"] = resolved.Pages
		params["max_depth"] = resolved.Depth
	}
	execS := int64(timeouts.ExecutionTimeoutFor(kind, params) / time.Second)

	wrap["params"] = params
	wrap["overlap_policy"] = spec.Overlap
	wrap["execution_timeout_s"] = execS

	args := make([]interface{}, len(spec.Args))
	copy(args, spec.Args)
	args[0] = wrap

	spec.Args = args
	spec.ExecutionTimeoutS = execS
	return spec
}

// actionTimeout is the Temporal schedule action timeout for a job timeout of
// execS seconds (zero when the job timeout is unset).
func actionTimeout(execS int64) time.Duration {
	if execS <= 0 {
		return 0
	}
	return time.Duration(execS)*time.Second + scheduledActionGrace
}

// copyMap returns a shallow copy of a JSON object param, never nil.
func copyMap(v any) map[string]any {
	out := map[string]any{}
	if src, ok := v.(map[string]any); ok {
		for k, val := range src {
			out[k] = val
		}
	}
	return out
}

// canonicalSchedule is the comparable form of a schedule, built from the desired
// spec and from a Temporal description the same way.
type canonicalSchedule struct {
	Cron           string `json:"cron"`
	TimeZone       string `json:"timezone"`
	JitterS        int64  `json:"jitter_s"`
	Overlap        int32  `json:"overlap"`
	CatchupS       int64  `json:"catchup_window_s"`
	PauseOnFailure bool   `json:"pause_on_failure"`
	Workflow       string `json:"workflow"`
	TaskQueue      string `json:"task_queue"`
	ActionTimeoutS int64  `json:"action_timeout_s"`
	Args           string `json:"args"`
}

// canonicalFromSpec renders the desired spec. A zero catch-up window is left out
// of the comparison: Temporal reports its own default when none was set.
func (m *Manager) canonicalFromSpec(spec ScheduleSpec) (string, error) {
	args, err := json.Marshal(spec.Args)
	if err != nil {
		return "", err
	}
	c := canonicalSchedule{
		Cron:           spec.Cron,
		TimeZone:       normalizeTimeZone(spec.TimeZone),
		JitterS:        spec.JitterS,
		Overlap:        int32(overlapPolicy(spec.Overlap)),
		CatchupS:       spec.CatchupWindowS,
		PauseOnFailure: spec.PauseOnFailure,
		Workflow:       spec.Workflow,
		TaskQueue:      m.cfg.TaskQueue,
		ActionTimeoutS: int64(actionTimeout(spec.ExecutionTimeoutS) / time.Second),
		Args:           string(args),
	}
	return marshalCanonical(c)
}

// canonicalFromDescription renders what Temporal holds for the schedule, in the
// same shape as canonicalFromSpec.
func (m *Manager) canonicalFromDescription(desc *client.ScheduleDescription, spec ScheduleSpec) (string, error) {
	var c canonicalSchedule
	if desc == nil {
		return "", fmt.Errorf("nil description")
	}
	if desc.Schedule.Spec != nil {
		if len(desc.Schedule.Spec.CronExpressions) > 0 {
			c.Cron = desc.Schedule.Spec.CronExpressions[0]
		}
		c.TimeZone = normalizeTimeZone(desc.Schedule.Spec.TimeZoneName)
		c.JitterS = int64(desc.Schedule.Spec.Jitter / time.Second)
	}
	if p := desc.Schedule.Policy; p != nil {
		c.Overlap = int32(p.Overlap)
		c.PauseOnFailure = p.PauseOnFailure
		if spec.CatchupWindowS > 0 {
			c.CatchupS = int64(p.CatchupWindow / time.Second)
		}
	}
	if wa, ok := desc.Schedule.Action.(*client.ScheduleWorkflowAction); ok && wa != nil {
		c.Workflow = fmt.Sprint(wa.Workflow)
		c.TaskQueue = wa.TaskQueue
		c.ActionTimeoutS = int64(wa.WorkflowExecutionTimeout / time.Second)
		args, err := json.Marshal(wa.Args)
		if err != nil {
			return "", err
		}
		c.Args = string(args)
	}
	return marshalCanonical(c)
}

func marshalCanonical(c canonicalSchedule) (string, error) {
	b, err := json.Marshal(c)
	if err != nil {
		return "", err
	}
	return string(b), nil
}
