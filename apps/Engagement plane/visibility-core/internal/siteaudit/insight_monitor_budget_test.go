package siteaudit

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

func nearCapacityState(t *testing.T, s *Service, r *Repository, base Snapshot) diskState {
	t.Helper()
	makeState := func(extra int) diskState {
		state := cloneState(r.state)
		state.Runs = []Snapshot{copySnapshot(base)}
		for runIndex := range 2 {
			v := insightRun(t, s, 11+runIndex, "Healthy")
			v.Pages = []Page{}
			for pageIndex := range Limit {
				p := base.Pages[0]
				p.URL = fmt.Sprintf("%slarge-%d", testSite, pageIndex)
				p.FinalURL, p.Canonical = p.URL, p.URL
				p.Title = fmt.Sprintf("Page %d", pageIndex)
				p.Description = "D"
				p.ObservedAt = v.StartedAt
				p.Links = []string{}
				for linkIndex := range maxLinks {
					p.Links = append(p.Links, fmt.Sprintf("%starget-%s-%d", testSite, strings.Repeat("a", extra+1), linkIndex))
				}
				v.Pages = append(v.Pages, p)
			}
			v.Pages, v.Findings = evaluate(v.Pages)
			state.Runs = append(state.Runs, v)
		}
		return state
	}
	initial := makeState(0)
	raw, _ := json.Marshal(initial)
	target := maxFileBytes - 32
	extra := (target - len(raw)) / (2 * Limit * maxLinks)
	state := makeState(extra)
	raw, _ = json.Marshal(state)
	remaining := target - len(raw)
	if remaining < 0 || remaining > 2*Limit*maxLinks {
		t.Fatal("byte-bound fixture calculation", remaining)
	}
	for ri := range state.Runs {
		if ri == 0 {
			continue
		}
		for pi := range state.Runs[ri].Pages {
			if remaining == 0 {
				break
			}
			n := min(remaining, 2047)
			state.Runs[ri].Pages[pi].Description = strings.Repeat("d", n+1)
			remaining -= n
		}
	}
	if remaining != 0 {
		t.Fatal("unfilled fixture budget", remaining)
	}
	if err := validateState(state); err != nil {
		t.Fatal("invalid near-capacity fixture", err)
	}
	raw, _ = json.Marshal(state)
	if len(raw) != target {
		t.Fatal("fixture not at fixed file cap", len(raw))
	}
	return state
}
func TestMonitorBudgetFitKeepsClaimAtomicAndPreservesRunningReport(t *testing.T) {
	s, r, _, _ := testService(t, source())
	base := insightRun(t, s, 10, "")
	saveInsight(t, r, base)
	r.state = nearCapacityState(t, s, r, base)
	m := NewMonitor(s)
	before, _ := json.Marshal(r.state)
	r.beforePersist = func(diskState) error { return errors.New("offline") }
	if _, err := m.Update(testID, testSite, 1, 0, false, "daily"); err == nil {
		t.Fatal("failed schedule commit accepted")
	}
	after, _ := json.Marshal(r.state)
	if string(before) != string(after) {
		t.Fatal("failed set leaked budget mutations")
	}
	r.beforePersist = nil
	configured, err := m.Update(testID, testSite, 1, 0, true, "daily")
	if err != nil {
		t.Fatal("schedule stranded at byte cap", err)
	}
	r.state = nearCapacityState(t, s, r, base)
	audit := s.emptyWebsiteFixture()
	audit.ID, audit.Status, audit.StartedAt = newID("audit_"), "running", configured.NextRunAt
	record := r.monitors()[0]
	oldRevision := record.Revision
	record.Revision++
	record.UpdatedAt = audit.StartedAt
	record.LastAuditID, record.LastOutcome = audit.ID, "running"
	record.PendingFor, record.PendingAttempt = audit.StartedAt, 1
	record.NextRunAt = stamp(mustTime(audit.StartedAt).Add(24 * time.Hour))
	record.History = []MonitorReceipt{{AuditID: audit.ID, ScheduledFor: audit.StartedAt, StartedAt: audit.StartedAt, Status: "running", Attempt: 1}}
	before, _ = json.Marshal(r.state)
	r.beforePersist = func(diskState) error { return errors.New("offline") }
	if err = r.claimMonitor(record, audit, oldRevision); err == nil {
		t.Fatal("uncommitted claim accepted")
	}
	after, _ = json.Marshal(r.state)
	if string(before) != string(after) {
		t.Fatal("failed claim leaked pruning or running receipt")
	}
	r.beforePersist = nil
	if err = r.claimMonitor(record, audit, oldRevision); err != nil {
		t.Fatal("due claim stranded at cap", err)
	}
	raw, _ := json.Marshal(r.state)
	if len(raw) > maxFileBytes || len(r.state.Runs) != 1 || r.state.Runs[0].ID != audit.ID || r.state.Runs[0].Status != "running" || r.state.Monitoring[0].LastAuditID != audit.ID {
		t.Fatal("claim/pruning not atomic or running evidence lost")
	}
}
