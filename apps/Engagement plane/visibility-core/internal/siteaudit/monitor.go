package siteaudit

import (
	"context"
	"errors"
	"log"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

// Monitor is a single-process scheduler sharing Marketing's protected audit
// repository. The volume must never be mounted by multiple Marketing replicas.
type Monitor struct{ audits *Service }

func NewMonitor(audits *Service) *Monitor { return &Monitor{audits: audits} }
func defaultMonitor(w websites.Website) MonitorSnapshot {
	return MonitorSnapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "marketing_scheduler", Cadence: "daily", Status: "not_configured", LastOutcome: "none", History: []MonitorReceipt{}}
}
func (m *Monitor) Get(id, site string) (MonitorSnapshot, error) {
	w, err := m.audits.binding(id, site, 0)
	if err != nil {
		return MonitorSnapshot{}, err
	}
	for _, v := range m.audits.repo.monitors() {
		if v.WebsiteID != id {
			continue
		}
		out := cloneMonitor(v).MonitorSnapshot
		out.WebsiteRevision, out.Site = w.Revision, w.Site
		if v.WebsiteRevision != w.Revision || v.Site != w.Site {
			out.Status, out.Enabled, out.NextRunAt, out.Reason = "context_changed", false, "", "context_changed"
			out.History, out.LastAuditID, out.LastOutcome, out.FailureCount = []MonitorReceipt{}, "", "none", 0
		}
		return out, nil
	}
	return defaultMonitor(w), nil
}

// Update never fetches pages; explicit enable/resume waits a full interval.
func (m *Monitor) Update(id, site string, websiteRevision, scheduleRevision int64, enabled bool, cadence string) (MonitorSnapshot, error) {
	if websiteRevision < 1 || websiteRevision > 1<<53-1 || scheduleRevision < 0 || scheduleRevision >= 1<<53-1 || interval(cadence) == 0 {
		return MonitorSnapshot{}, ErrInvalid
	}
	s := m.audits
	s.mu.Lock()
	defer s.mu.Unlock()
	w, err := s.binding(id, site, websiteRevision)
	if err != nil {
		return MonitorSnapshot{}, err
	}
	if enabled && s.capture == nil {
		return MonitorSnapshot{}, ErrUnavailable
	}
	v := monitorRecord{MonitorSnapshot: defaultMonitor(w)}
	for _, old := range s.repo.monitors() {
		if old.WebsiteID == id {
			v = old
			break
		}
	}
	if v.Revision != scheduleRevision {
		return MonitorSnapshot{}, ErrConflict
	}
	if enabled && v.LastOutcome == "running" {
		return MonitorSnapshot{}, ErrCapacity
	}
	if v.Site != w.Site || v.WebsiteRevision != w.Revision {
		v.History, v.LastAuditID, v.LastOutcome = []MonitorReceipt{}, "", "none"
	}
	v.WebsiteID, v.Site, v.WebsiteRevision = w.ID, w.Site, w.Revision
	v.Revision, v.Configured, v.Enabled, v.Cadence, v.UpdatedAt = scheduleRevision+1, true, enabled, cadence, stamp(s.now())
	v.Status, v.NextRunAt, v.Reason, v.PendingFor, v.PendingAttempt, v.FailureCount = "paused", "", "", "", 0, 0
	if enabled {
		v.Status, v.NextRunAt = "active", stamp(s.now().Add(interval(cadence)))
	}
	if err = s.repo.setMonitor(v, scheduleRevision); err != nil {
		return MonitorSnapshot{}, err
	}
	return cloneMonitor(v).MonitorSnapshot, nil
}

// Tick is clock-injected and makes at most two dispatches through the existing
// shared run/capture bounds. It coalesces missed intervals instead of replaying.
func (m *Monitor) Tick() error {
	return m.tick(context.Background())
}
func (m *Monitor) tick(ctx context.Context) error {
	s := m.audits
	s.mu.Lock()
	defer s.mu.Unlock()
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if err := s.flushTerminal(); err != nil {
		return err
	}
	for _, v := range s.repo.monitors() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if !v.Enabled {
			continue
		}
		if _, err := s.binding(v.WebsiteID, v.Site, v.WebsiteRevision); err != nil {
			old := v.Revision
			v.Revision++
			v.Enabled, v.Status, v.NextRunAt, v.Reason, v.UpdatedAt = false, "context_changed", "", "context_changed", stamp(s.now())
			if err = s.repo.setMonitor(v, old); err != nil {
				return err
			}
			continue
		}
		if v.LastOutcome == "running" || v.NextRunAt == "" || mustTime(v.NextRunAt).After(s.now()) {
			continue
		}
		if err := m.dispatch(ctx, v); err != nil {
			return err
		}
	}
	return nil
}
func (m *Monitor) deferRun(v monitorRecord) error {
	old := v.Revision
	if v.PendingFor == "" {
		v.PendingFor, v.PendingAttempt = v.NextRunAt, 1
	}
	v.Revision++
	v.NextRunAt, v.Reason, v.UpdatedAt = stamp(m.audits.now().Add(time.Minute)), "capacity_deferred", stamp(m.audits.now())
	return m.audits.repo.setMonitor(v, old)
}
func (m *Monitor) dispatch(ctx context.Context, v monitorRecord) error {
	s := m.audits
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if s.capture == nil {
		old := v.Revision
		v.Revision++
		v.Enabled, v.Status, v.NextRunAt, v.Reason, v.UpdatedAt = false, "blocked", "", "not_configured", stamp(s.now())
		return s.repo.setMonitor(v, old)
	}
	if s.active >= 2 {
		return m.deferRun(v)
	}
	for _, run := range s.repo.list(v.WebsiteID, v.Site) {
		if run.WebsiteRevision != v.WebsiteRevision {
			continue
		}
		if run.Status == "running" || s.now().Sub(mustTime(run.StartedAt)) < time.Minute {
			return m.deferRun(v)
		}
		break
	}
	w, err := s.binding(v.WebsiteID, v.Site, v.WebsiteRevision)
	if err != nil {
		return err
	}
	audit := s.empty(w)
	audit.ID, audit.Status, audit.StartedAt = newID("audit_"), "running", stamp(s.now())
	old := v.Revision
	if v.PendingFor == "" {
		v.PendingFor, v.PendingAttempt = v.NextRunAt, 1
	}
	h := MonitorReceipt{AuditID: audit.ID, ScheduledFor: v.PendingFor, StartedAt: audit.StartedAt, Status: "running", Attempt: v.PendingAttempt}
	v.History = append([]MonitorReceipt{h}, v.History...)
	if len(v.History) > maxMonitorHistory {
		v.History = append([]MonitorReceipt{}, v.History[:maxMonitorHistory]...)
	}
	v.Revision++
	v.LastAuditID, v.LastOutcome, v.NextRunAt, v.Reason, v.UpdatedAt = audit.ID, "running", stamp(s.now().Add(interval(v.Cadence))), "", audit.StartedAt
	if ctx.Err() != nil {
		return ctx.Err()
	}
	err = s.repo.claimMonitor(v, audit, old)
	if err != nil && !errors.Is(err, errCommitted) {
		return err
	}
	s.active++
	go s.run(copySnapshot(audit))
	return err
}

// Run owns only polling lifecycle; interrupted captures retain durable receipts
// and are recovered by Open on restart. A cancelled context starts no new tick.
func (m *Monitor) Run(ctx context.Context) {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()
	m.runPolling(ctx, ticker.C)
}
func (m *Monitor) runPolling(ctx context.Context, ticks <-chan time.Time) {
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticks:
			if ctx.Err() != nil {
				return
			}
			if err := m.tick(ctx); err != nil && ctx.Err() == nil {
				log.Printf("marketing-core: monitoring persistence or dispatch could not be confirmed")
			}
		}
	}
}
