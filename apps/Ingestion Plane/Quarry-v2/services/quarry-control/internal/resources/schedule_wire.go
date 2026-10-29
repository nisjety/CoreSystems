package resources

import "github.com/triodelab/quarry-v2/services/quarry-control/internal/store"

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
	// Orchestrator contract — what Temporal schedule to materialize.
	Workflow string `json:"workflow"`
	Args     []any  `json:"args"`
	Paused   bool   `json:"paused"`
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
}

// workflowFor maps a schedule's target_kind to the Temporal workflow name +
// args the orchestrator should materialize. Only change_monitor is mapped in
// the W2 MVP — scrape/crawl/batch schedules are NOT run_id-safe for recurring
// dispatch yet, so they return ("", nil) and the orchestrator skips them
// (logged) rather than creating a workflowless Temporal schedule.
func workflowFor(s store.Schedule) (string, []any) {
	switch s.TargetKind {
	case store.TargetKindChangeMonitor:
		return "ChangeMonitorWF", []any{changeMonitorArgs{OrgID: s.OrgID, URL: s.TargetRef, CreatedBy: s.CreatedBy}}
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
		ID:         string(s.ID),
		Name:       string(s.ID),
		OrgID:      s.OrgID,
		Cron:       s.Cron,
		TargetKind: s.TargetKind,
		TargetRef:  s.TargetRef,
		Enabled:    s.Enabled,
		CreatedAt:  s.CreatedAt,
		Workflow:   wf,
		Args:       args,
		Paused:     !s.Enabled,
	}
}
