package store

// Job control (WS15.L2): a caller asks for a pause, a resume or a cancel by
// writing a desired state on the job record; the orchestrator's control loop
// reads the pending requests, signals the workflow and acknowledges them. The
// rules that decide whether a request is allowed live here, in one place, so
// the in-memory and Postgres stores cannot disagree.

import (
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// Control verbs as the wire names them.
const (
	ControlPause  = "pause"
	ControlResume = "resume"
	ControlCancel = "cancel"
)

// ControlError is a request the job's state does not allow. Status is the job's
// status at the time, Reason a closed word: "ended", "not_controllable",
// "not_running", "not_paused", "unknown_action". The handler answers 409 with
// both in details.
type ControlError struct {
	Status string
	Reason string
}

func (e *ControlError) Error() string { return "job control refused: " + e.Reason }

// ControlPlan is what a verb does to a job. EndNow means the job is cancelled
// at once (it never started); otherwise Desired is the desired state to write.
// NoOp means the request repeats one already pending: nothing changes.
type ControlPlan struct {
	Desired string
	EndNow  bool
	NoOp    bool
}

// PlanControl decides what verb does to job j, or refuses it with a
// *ControlError. It is pure: the stores apply the plan.
func PlanControl(j Job, action string) (ControlPlan, error) {
	refuse := func(reason string) error {
		return &ControlError{Status: j.Status, Reason: reason}
	}
	if IsTerminalJobStatus(j.Status) {
		return ControlPlan{}, refuse("ended")
	}
	pending := j.DesiredSeq > j.AppliedSeq
	// A scrape is a single page with no workflow loop to pause; the only verb it
	// takes is a cancel before it started.
	if j.Kind == "scrape" && !(action == ControlCancel && j.Status == "accepted") {
		return ControlPlan{}, refuse("not_controllable")
	}
	switch action {
	case ControlCancel:
		if j.Status == "accepted" {
			return ControlPlan{Desired: "cancelled", EndNow: true}, nil
		}
		if j.DesiredState == "cancelled" && pending {
			return ControlPlan{Desired: "cancelled", NoOp: true}, nil
		}
		return ControlPlan{Desired: "cancelled"}, nil
	case ControlPause:
		if j.Status != "running" {
			return ControlPlan{}, refuse("not_running")
		}
		if j.DesiredState == "paused" && pending {
			return ControlPlan{Desired: "paused", NoOp: true}, nil
		}
		return ControlPlan{Desired: "paused"}, nil
	case ControlResume:
		if j.Status != "paused" {
			return ControlPlan{}, refuse("not_paused")
		}
		if j.DesiredState == "running" && pending {
			return ControlPlan{Desired: "running", NoOp: true}, nil
		}
		return ControlPlan{Desired: "running"}, nil
	}
	return ControlPlan{}, refuse("unknown_action")
}

// ApplyControl returns job j after plan. The stores call it inside their write,
// so the memory and Postgres stores set the same fields.
func ApplyControl(j Job, plan ControlPlan, now time.Time) Job {
	if plan.NoOp {
		return j
	}
	if plan.EndNow {
		ms := now.UnixMilli()
		j.Status = "cancelled"
		j.TerminalReason = "run_cancelled"
		j.FinishedAt = &ms
		j.DesiredState = plan.Desired
		j.DesiredSeq++
		j.AppliedSeq = j.DesiredSeq
		return j
	}
	j.DesiredState = plan.Desired
	j.DesiredSeq++
	return j
}

// SetControl implements JobsStore for the in-memory store.
func (m *memJobs) SetControl(orgID string, id quarrycontracts.ID, action string) (Job, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	j, ok := m.items[id]
	if !ok || j.OrgID != orgID {
		return Job{}, ErrNotFound
	}
	plan, err := PlanControl(j, action)
	if err != nil {
		return Job{}, err
	}
	j = ApplyControl(j, plan, time.Now())
	m.items[id] = j
	return j, nil
}

// AckControl implements JobsStore for the in-memory store.
func (m *memJobs) AckControl(id quarrycontracts.ID, appliedSeq int64) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	j, ok := m.items[id]
	if !ok {
		return ErrNotFound
	}
	if appliedSeq > j.AppliedSeq {
		j.AppliedSeq = appliedSeq
		m.items[id] = j
	}
	return nil
}

// PendingControl implements JobsStore for the in-memory store.
func (m *memJobs) PendingControl(limit int) []Job {
	m.mu.RLock()
	defer m.mu.RUnlock()
	out := []Job{}
	for _, id := range m.order {
		j, ok := m.items[id]
		if !ok || j.DesiredSeq <= j.AppliedSeq {
			continue
		}
		out = append(out, j)
		if limit > 0 && len(out) >= limit {
			break
		}
	}
	return out
}

// ListRunning implements JobsStore for the in-memory store.
func (m *memJobs) ListRunning(limit int) []Job {
	m.mu.RLock()
	defer m.mu.RUnlock()
	out := []Job{}
	for _, id := range m.order {
		j, ok := m.items[id]
		if !ok || (j.Status != "running" && j.Status != "paused") || j.WorkflowID == "" {
			continue
		}
		out = append(out, j)
		if limit > 0 && len(out) >= limit {
			break
		}
	}
	return out
}

// Finalize implements JobsStore for the in-memory store. A job that already
// ended is returned unchanged with changed=false.
func (m *memJobs) Finalize(id quarrycontracts.ID, status, reason string, finishedAt int64) (Job, bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	j, ok := m.items[id]
	if !ok {
		return Job{}, false, ErrNotFound
	}
	if IsTerminalJobStatus(j.Status) {
		return j, false, nil
	}
	j.Status = status
	j.TerminalReason = reason
	if j.FinishedAt == nil {
		ms := finishedAt
		j.FinishedAt = &ms
	}
	m.items[id] = j
	return j, true, nil
}
