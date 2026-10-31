package siteaudit

import (
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

func validMonitorSite(site string) bool { return discovery.SafeURL(site, site) }
func monitorIndex(state diskState, id string) int {
	for i, v := range state.Monitoring {
		if v.WebsiteID == id {
			return i
		}
	}
	return -1
}
func (r *Repository) monitors() []monitorRecord {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]monitorRecord, len(r.state.Monitoring))
	for i, v := range r.state.Monitoring {
		out[i] = cloneMonitor(v)
	}
	return out
}
func (r *Repository) setMonitor(v monitorRecord, revision int64) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next := cloneState(r.state)
	i := monitorIndex(next, v.WebsiteID)
	if i < 0 {
		if revision != 0 {
			return ErrConflict
		}
		if len(next.Monitoring) >= maxSchedules {
			return ErrCapacity
		}
		next.Monitoring = append(next.Monitoring, cloneMonitor(v))
	} else {
		if next.Monitoring[i].Revision != revision {
			return ErrConflict
		}
		next.Monitoring[i] = cloneMonitor(v)
	}
	next, err := fitInsightBudget(next, "")
	if err != nil {
		return err
	}
	return r.commit(next)
}

// claimMonitor is the exactly-one local transaction boundary: no page fetch may
// start until BOTH the occurrence and its running audit are durably committed.
func (r *Repository) claimMonitor(v monitorRecord, audit Snapshot, revision int64) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next := cloneState(r.state)
	i := monitorIndex(next, v.WebsiteID)
	if i < 0 || next.Monitoring[i].Revision != revision || !next.Monitoring[i].Enabled {
		return ErrConflict
	}
	var err error
	next, err = putSnapshot(next, audit)
	if err != nil {
		return err
	}
	next.Monitoring[i] = cloneMonitor(v)
	next, err = fitInsightBudget(next, audit.ID)
	if err != nil {
		return err
	}
	return r.commit(next)
}

func transientMonitorFailure(reason string) bool {
	switch reason {
	case "source_unavailable", "quota_limited", "timeout", "interrupted", "storage_unavailable":
		return true
	}
	return false
}

// Completion also changes the schedule in the SAME commit. Readers cannot see
// a completed occurrence whose scheduler still believes it must be dispatched.
func completeMonitor(next diskState, audit Snapshot) diskState {
	if audit.Status == "running" {
		return next
	}
	i := monitorIndex(next, audit.WebsiteID)
	if i < 0 {
		return next
	}
	v := cloneMonitor(next.Monitoring[i])
	if len(v.History) == 0 || v.History[0].AuditID != audit.ID || v.History[0].Status != "running" {
		return next
	}
	h := v.History[0]
	h.Status, h.CompletedAt, h.Reason = audit.Status, audit.CompletedAt, audit.Reason
	h.FindingCount = len(audit.Findings)
	for _, p := range audit.Pages {
		if p.Status == "checked" {
			h.CheckedPages++
		}
	}
	v.History[0] = h
	v.LastOutcome, v.UpdatedAt = audit.Status, audit.CompletedAt
	v.Revision++
	if audit.Status == "complete" {
		v.FailureCount = 0
	} else {
		v.FailureCount++
	}
	if v.Enabled {
		if audit.Reason == "context_changed" {
			v.Enabled, v.Status, v.Reason, v.NextRunAt = false, "context_changed", "context_changed", ""
		} else if audit.Status == "complete" {
			v.NextRunAt, v.Reason = stamp(mustTime(audit.CompletedAt).Add(interval(v.Cadence))), ""
			v.PendingFor, v.PendingAttempt = "", 0
		} else if transientMonitorFailure(audit.Reason) && h.Attempt < 3 {
			delay := 5 * time.Minute
			if h.Attempt == 2 {
				delay = 30 * time.Minute
			}
			v.NextRunAt, v.Reason = stamp(mustTime(audit.CompletedAt).Add(delay)), audit.Reason
			v.PendingFor, v.PendingAttempt = h.ScheduledFor, h.Attempt+1
		} else {
			v.Enabled, v.Status, v.Reason, v.NextRunAt = false, "blocked", audit.Reason, ""
		}
	}
	next.Monitoring[i] = v
	return next
}
