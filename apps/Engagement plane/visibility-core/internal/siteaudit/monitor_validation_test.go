package siteaudit

import (
	"encoding/json"
	"errors"
	"os"
	"testing"
	"time"
)

func TestMonitoringValidationRejectsCorruptStatesAndHistoryIsImmutable(t *testing.T) {
	s, m, clock, file := monitored(t, "")
	clock.advance(24 * time.Hour)
	if err := m.Tick(); err != nil {
		t.Fatal(err)
	}
	finish(t, s)
	v, _ := m.Get(testID, testSite)
	v.History[0].Status = "forged"
	copy, _ := m.Get(testID, testSite)
	if copy.History[0].Status != "complete" {
		t.Fatal("history mutation leaked")
	}
	valid := s.repo.monitors()[0]
	for _, change := range []func(*monitorRecord){
		func(v *monitorRecord) { v.Site = "http://localhost/" },
		func(v *monitorRecord) { v.WebsiteRevision = 0 },
		func(v *monitorRecord) { v.Source = "browser" },
		func(v *monitorRecord) { v.Revision = 0 },
		func(v *monitorRecord) { v.Configured = false },
		func(v *monitorRecord) { v.Cadence = "hourly" },
		func(v *monitorRecord) { v.UpdatedAt = "not-a-date" },
		func(v *monitorRecord) { v.Reason = "secret upstream error" },
		func(v *monitorRecord) { v.History = nil },
		func(v *monitorRecord) { v.FailureCount = 4 },
		func(v *monitorRecord) { v.PendingFor = "invalid"; v.PendingAttempt = 1 },
		func(v *monitorRecord) { v.PendingFor = v.UpdatedAt; v.PendingAttempt = 0 },
		func(v *monitorRecord) { v.PendingAttempt = 1 },
		func(v *monitorRecord) { v.NextRunAt = "invalid" },
		func(v *monitorRecord) { v.NextRunAt = "" },
		func(v *monitorRecord) { v.Enabled = false },
		func(v *monitorRecord) { v.Status = "unknown" },
		func(v *monitorRecord) { v.Status = "paused" },
		func(v *monitorRecord) { v.LastOutcome = "none" },
		func(v *monitorRecord) { v.LastAuditID = "audit_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
		func(v *monitorRecord) { v.History[0].AuditID = "forged" },
		func(v *monitorRecord) { v.History[0].ScheduledFor = "bad" },
		func(v *monitorRecord) { v.History[0].Attempt = 4 },
		func(v *monitorRecord) { v.History[0].CheckedPages = Limit + 1 },
		func(v *monitorRecord) { v.History[0].FindingCount = Limit*len(rules) + 1 },
		func(v *monitorRecord) { v.History[0].Status = "failed"; v.LastOutcome = "failed" },
		func(v *monitorRecord) { v.History[0].Status = "running"; v.LastOutcome = "running" },
		func(v *monitorRecord) { v.History[0].CompletedAt = "" },
		func(v *monitorRecord) { v.History = append(v.History, v.History[0]) },
	} {
		bad := cloneMonitor(valid)
		change(&bad)
		if validateMonitors([]monitorRecord{bad}) == nil {
			t.Fatalf("corruption accepted %+v", bad)
		}
	}
	if validateMonitors([]monitorRecord{valid, valid}) == nil {
		t.Fatal("duplicate schedule")
	}
	if validateMonitors(make([]monitorRecord, 65)) == nil {
		t.Fatal("unbounded schedules")
	}
	state := cloneState(s.repo.state)
	state.Monitoring[0].LastOutcome = "running"
	state.Monitoring[0].History[0].Status = "running"
	state.Monitoring[0].History[0].CompletedAt = ""
	state.Monitoring[0].History[0].CheckedPages = 0
	state.Monitoring[0].History[0].FindingCount = 0
	if validateState(state) == nil {
		t.Fatal("running schedule without running audit accepted")
	}
	raw, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = Open(file, "org-one"); err == nil {
		t.Fatal("corrupt on-disk monitoring accepted")
	}
}

func TestMonitoringRetainsTenReceiptsAndManualStartFlushesTerminal(t *testing.T) {
	s, m, clock, _ := monitored(t, "")
	for range 12 {
		clock.advance(24 * time.Hour)
		if err := m.Tick(); err != nil {
			t.Fatal(err)
		}
		finish(t, s)
	}
	v, _ := m.Get(testID, testSite)
	if len(v.History) != maxMonitorHistory || len(s.repo.list(testID, testSite)) != maxSiteRuns {
		t.Fatal("unbounded histories", v)
	}
	// Simulate a completed result retained only in memory after a failed write.
	s.mu.Lock()
	terminal := copySnapshot(s.repo.list(testID, testSite)[0])
	s.pendingTerminal = map[string]Snapshot{terminal.ID: terminal}
	s.mu.Unlock()
	s.repo.beforePersist = func(diskState) error { return errors.New("offline") }
	clock.advance(time.Minute)
	if _, err := s.Start(testID, testSite, 1); err == nil {
		t.Fatal("new capture despite pending failed persistence")
	}
	s.repo.beforePersist = nil
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	finish(t, s)
	s.mu.Lock()
	pending := len(s.pendingTerminal)
	s.mu.Unlock()
	if pending != 0 {
		t.Fatal("manual start did not flush")
	}
}
