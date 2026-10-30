package siteaudit

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

func TestMonitoringIsExplicitDurableAndDoesNotCatchUp(t *testing.T) {
	s, _, _, file := testService(t, source())
	var mu sync.Mutex
	now := testNow
	s.now = func() time.Time { mu.Lock(); defer mu.Unlock(); return now }
	advance := func(d time.Duration) { mu.Lock(); now = now.Add(d); mu.Unlock() }
	c := source()
	base := c.captureFn
	c.captureFn = func(ctx context.Context, site, raw string) (CapturePage, error) {
		v, err := base(ctx, site, raw)
		v.ObservedAt = stamp(s.now())
		return v, err
	}
	s.capture = c
	m := NewMonitor(s)
	v, err := m.Get(testID, testSite)
	if err != nil || v.Configured || v.Enabled || v.Status != "not_configured" {
		t.Fatal(v, err)
	}
	v, err = m.Update(testID, testSite, 1, 0, true, "daily")
	if err != nil || !v.Enabled || v.Revision != 1 || v.NextRunAt != stamp(testNow.Add(24*time.Hour)) {
		t.Fatal(v, err)
	}
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	if audit, _ := s.Get(testID, testSite); audit.Status != "idle" {
		t.Fatal("enable fetched immediately")
	}
	advance(8 * 24 * time.Hour)
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	audit := finish(t, s)
	v, _ = m.Get(testID, testSite)
	if audit.Status != "complete" || len(v.History) != 1 || v.LastOutcome != "complete" || v.History[0].Attempt != 1 || v.NextRunAt != stamp(s.now().Add(24*time.Hour)) {
		t.Fatal(audit.Status, v)
	}
	repo, err := Open(file, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	restored := New(repo, s.registry, c, s.now)
	m = NewMonitor(restored)
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	v, _ = m.Get(testID, testSite)
	if len(v.History) != 1 {
		t.Fatal("restart duplicated occurrence", v)
	}
	if _, err = m.Update(testID, testSite, 1, 0, false, "daily"); !errors.Is(err, ErrConflict) {
		t.Fatal("stale schedule update", err)
	}
	v, err = m.Update(testID, testSite, 1, v.Revision, false, "weekly")
	if err != nil || v.Enabled || v.Status != "paused" || v.NextRunAt != "" {
		t.Fatal(v, err)
	}
}

func TestMonitoringRejectsUnconfiguredAndChangedBindings(t *testing.T) {
	s, _, registry, _ := testService(t, nil)
	m := NewMonitor(s)
	if _, err := m.Update(testID, testSite, 1, 0, true, "daily"); !errors.Is(err, ErrUnavailable) {
		t.Fatal(err)
	}
	s.capture = source()
	for _, cadence := range []string{"", "hourly", "DAILY"} {
		if _, err := m.Update(testID, testSite, 1, 0, true, cadence); !errors.Is(err, ErrInvalid) {
			t.Fatal(err)
		}
	}
	v, err := m.Update(testID, testSite, 1, 0, true, "weekly")
	if err != nil {
		t.Fatal(err)
	}
	registry.mu.Lock()
	registry.revision = 2
	registry.mu.Unlock()
	s.now = func() time.Time { return testNow.Add(8 * 24 * time.Hour) }
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	v, _ = m.Get(testID, testSite)
	if v.Enabled || v.Status != "context_changed" || v.WebsiteRevision != 2 || v.NextRunAt != "" {
		t.Fatal(v)
	}
	if audit, _ := s.Get(testID, testSite); audit.Status != "idle" {
		t.Fatal("changed binding fetched")
	}
	if _, err = m.Update(testID, testSite, 1, v.Revision, true, "weekly"); !errors.Is(err, ErrConflict) {
		t.Fatal(err)
	}
}
