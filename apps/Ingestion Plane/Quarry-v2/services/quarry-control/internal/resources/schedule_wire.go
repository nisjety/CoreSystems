package resources

import (
	"time"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// scheduleWire is the orchestrator-facing serialization of a schedule. The
// orchestrator's reconciler (quarry-orchestrator/internal/schedules) decodes
// {id, name, cron, workflow, args, paused} and creates a Temporal schedule
// from it. We DERIVE workflow/args/paused on serialize rather than persisting
// them, so the schedules table stays the single source of truth for a
// schedule's identity + cadence and the control→orchestrator contract is a
// pure projection.
type scheduleWire struct {
	ID         string `json:"id"`
	Name       string `json:"name"`
	OrgID      string `json:"org_id"`
	Cron       string `json:"cron"`
	TargetKind string `json:"target_kind"`
	TargetRef  string `json:"target_ref"`
	Enabled    bool   `json:"enabled"`
	CreatedAt  int64  `json:"created_at"`
	// Tag is the monitor's operator label (E3.5-6). Omitted when untagged, so an
	// untagged row serialises exactly as before. The reconciler does not read it.
	Tag string `json:"tag,omitempty"`
	// Orchestrator contract — what Temporal schedule to materialize.
	Workflow string `json:"workflow"`
	Args     []any  `json:"args"`
	Paused   bool   `json:"paused"`
	// Executable-schedule fields (WS15.S2): the calendar and overlap options the
	// reconciler gives Temporal. The defaults are the schedule row's defaults.
	Overlap        string `json:"overlap"`
	TimeZone       string `json:"timezone"`
	JitterS        int64  `json:"jitter_s"`
	CatchupWindowS int64  `json:"catchup_window_s"`
	PauseOnFailure bool   `json:"pause_on_failure"`
}

// changeMonitorArgs is the [0] element of the Temporal workflow Args for a
// change_monitor schedule. The orchestrator's ChangeMonitorWF input struct
// decodes this object by matching json tags. run_id is intentionally ABSENT:
// each scheduled fire derives a fresh, replay-safe run_id from its own
// Temporal workflow execution, so recurring fires never collapse onto one
// run timeline (which would also dedupe the second fire's events).
type changeMonitorArgs struct {
	OrgID     string `json:"org_id"`
	URL       string `json:"url"`
	CreatedBy string `json:"created_by,omitempty"`
	// Zdr is the posture stamped at schedule create (WS4.H, migration 023).
	// A pointer with omitempty: a row created before the stamp (nil) emits
	// byte-identical args to what shipped before, so an existing Temporal
	// schedule sees no spec change on the next reconcile pass.
	Zdr *bool `json:"zdr,omitempty"`
}

// workflowFor maps a schedule's target_kind to the Temporal workflow name +
// args the orchestrator should materialize. Only change_monitor is mapped in
// the W2 MVP — scrape/crawl/batch schedules are NOT run_id-safe for recurring
// dispatch yet, so they return ("", nil) and the orchestrator skips them
// (logged) rather than creating a workflowless Temporal schedule.
func workflowFor(s store.Schedule) (string, []any) {
	switch s.TargetKind {
	case store.TargetKindChangeMonitor:
		return "ChangeMonitorWF", []any{changeMonitorArgs{OrgID: s.OrgID, URL: s.TargetRef, CreatedBy: s.CreatedBy, Zdr: s.Zdr}}
	case "scrape", "crawl", "batch":
		// Dark until QUARRY_SCHEDULED_JOBS_ENABLED is 1 (WS15.S2): with the switch
		// off these kinds stay unmapped and the reconciler creates nothing for them.
		if !ScheduledJobsEnabled() {
			return "", nil
		}
		return "ScheduledJobWF", []any{scheduledJobArgs{
			ScheduleID: string(s.ID),
			OrgID:      s.OrgID,
			CreatedBy:  s.CreatedBy,
			Kind:       s.TargetKind,
			Params:     scheduledParams(s),
		}}
	default:
		return "", nil
	}
}

// toScheduleWire renders a stored Schedule into the orchestrator wire shape.
func toScheduleWire(s store.Schedule) scheduleWire {
	wf, args := workflowFor(s)
	if args == nil {
		args = []any{}
	}
	return scheduleWire{
		ID:             string(s.ID),
		Name:           string(s.ID),
		OrgID:          s.OrgID,
		Cron:           s.Cron,
		TargetKind:     s.TargetKind,
		TargetRef:      s.TargetRef,
		Enabled:        s.Enabled,
		CreatedAt:      s.CreatedAt,
		Tag:            s.Tag,
		Workflow:       wf,
		Args:           args,
		Paused:         !s.Enabled,
		Overlap:        s.OverlapPolicy,
		TimeZone:       s.Timezone,
		JitterS:        s.JitterS,
		CatchupWindowS: s.CatchupWindowS,
		PauseOnFailure: s.PauseOnFailure,
	}
}

// scheduleView is the tenant projection of a schedule (WS15.S1), returned by
// GET /v1/schedules?org_id=, GET /v1/schedules/{id} and POST /v1/schedules. It
// carries the fields the gateway's ScheduleItem and the edge's ScheduleSummary
// read. The legacy fields (id, target_kind, target_ref, enabled) stay for the
// monitoring filter and other existing readers.
type scheduleView struct {
	ScheduleID     string `json:"schedule_id"`
	Name           string `json:"name"`
	Kind           string `json:"kind"`
	Cron           string `json:"cron"`
	Timezone       string `json:"timezone"`
	OverlapPolicy  string `json:"overlap_policy"`
	CatchupWindowS int64  `json:"catchup_window_s"`
	PauseOnFailure bool   `json:"pause_on_failure"`
	JitterS        int64  `json:"jitter_s"`
	Status         string `json:"status"`
	Tag            string `json:"tag,omitempty"`
	// NeedsActivation is true for a non-monitor schedule that was created
	// disabled and never toggled (updated_at still equals created_at). Two
	// operations in the same second are indistinguishable here; the column
	// cannot record more without a new migration.
	NeedsActivation     bool           `json:"needs_activation"`
	Config              map[string]any `json:"config"`
	CreatedAt           string         `json:"created_at"`
	UpdatedAt           string         `json:"updated_at"`
	LastRunAt           *string        `json:"last_run_at"`
	NextRunAt           *string        `json:"next_run_at"`
	SkippedOverlapCount int64          `json:"skipped_overlap_count"`

	ID         string `json:"id"`
	TargetKind string `json:"target_kind"`
	TargetRef  string `json:"target_ref"`
	Enabled    bool   `json:"enabled"`
}

// toScheduleView renders a stored schedule for its tenant. A schedule with no
// name shows its target, so the list never shows an empty label.
func toScheduleView(s store.Schedule) scheduleView {
	name := s.Name
	if name == "" {
		name = s.TargetRef
	}
	cfg := s.Config
	if cfg == nil {
		cfg = map[string]any{}
	}
	status := "paused"
	if s.Enabled {
		status = "active"
	}
	return scheduleView{
		ScheduleID:     string(s.ID),
		Name:           name,
		Kind:           s.TargetKind,
		Cron:           s.Cron,
		Timezone:       s.Timezone,
		OverlapPolicy:  s.OverlapPolicy,
		CatchupWindowS: s.CatchupWindowS,
		PauseOnFailure: s.PauseOnFailure,
		JitterS:        s.JitterS,
		Status:         status,
		Tag:            s.Tag,
		NeedsActivation: !s.Enabled && s.TargetKind != store.TargetKindChangeMonitor &&
			s.UpdatedAt == s.CreatedAt,
		Config:              cfg,
		CreatedAt:           rfc3339(s.CreatedAt),
		UpdatedAt:           rfc3339(s.UpdatedAt),
		LastRunAt:           rfc3339Ptr(s.LastRunAt),
		NextRunAt:           rfc3339Ptr(s.NextRunAt),
		SkippedOverlapCount: s.SkippedOverlapCount,
		ID:                  string(s.ID),
		TargetKind:          s.TargetKind,
		TargetRef:           s.TargetRef,
		Enabled:             s.Enabled,
	}
}

func rfc3339(unix int64) string {
	return time.Unix(unix, 0).UTC().Format(time.RFC3339)
}

func rfc3339Ptr(unix *int64) *string {
	if unix == nil {
		return nil
	}
	s := rfc3339(*unix)
	return &s
}
