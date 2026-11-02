package pagespeed

import (
	"context"
	"fmt"
	"sync/atomic"
	"testing"
	"time"
)

type runnerFunc func(context.Context, string, string) (*Report, string)

func (f runnerFunc) Run(ctx context.Context, site, strategy string) (*Report, string) {
	return f(ctx, site, strategy)
}

func TestJobsAreDetachedDeduplicatedAndTenantScoped(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	s := NewService(runnerFunc(func(ctx context.Context, site, strategy string) (*Report, string) {
		calls.Add(1)
		close(started)
		select {
		case <-release:
			score := 79.0
			return &Report{Performance: &score, FetchTime: "2026-10-21T12:00:00Z", LighthouseVersion: "13"}, ""
		case <-ctx.Done():
			return nil, "source_unavailable"
		}
	}), nil)
	defer s.Close()
	if s.Get("org-1", "https://aquatiq.com/", "mobile").Status != "idle" || calls.Load() != 0 {
		t.Fatal("GET should not invoke provider")
	}
	first, err := s.Start("org-1", "https://aquatiq.com/", "mobile")
	if err != nil || first.Status != "running" {
		t.Fatalf("start %+v %v", first, err)
	}
	<-started
	again, err := s.Start("org-1", "https://aquatiq.com/", "mobile")
	if err != nil || again.RequestedAt != first.RequestedAt || calls.Load() != 1 {
		t.Fatal("job not deduplicated")
	}
	if s.Get("org-2", "https://aquatiq.com/", "mobile").Status != "idle" {
		t.Fatal("cross-tenant result leak")
	}
	close(release)
	s.wg.Wait()
	complete := s.Get("org-1", "https://aquatiq.com/", "mobile")
	if complete.Status != "complete" || complete.Report == nil || complete.CompletedAt == "" {
		t.Fatalf("job not completed: %+v", complete)
	}
	complete.Report.FetchTime = "mutated"
	*complete.Report.Performance = 999
	if s.Get("org-1", "https://aquatiq.com/", "mobile").Report.FetchTime == "mutated" {
		t.Fatal("mutable cached result escaped")
	}
	if *s.Get("org-1", "https://aquatiq.com/", "mobile").Report.Performance != 79 {
		t.Fatal("mutable metric pointer escaped")
	}
}

func TestJobsUnavailableAndGlobalCacheLimit(t *testing.T) {
	s := NewService(nil, nil)
	defer s.Close()
	_, _ = s.Start("org-1", "https://aquatiq.com/", "mobile")
	s.wg.Wait()
	if got := s.Get("org-1", "https://aquatiq.com/", "mobile"); got.Status != "failed" || got.Reason != "not_configured" {
		t.Fatalf("nil runner result %+v", got)
	}
	s2 := NewService(runnerFunc(func(context.Context, string, string) (*Report, string) { return nil, "" }), nil)
	defer s2.Close()
	_, _ = s2.Start("org-1", "https://aquatiq.com/", "mobile")
	s2.wg.Wait()
	if got := s2.Get("org-1", "https://aquatiq.com/", "mobile"); got.Reason != "audit_failed" {
		t.Fatalf("empty result %+v", got)
	}
	s.mu.Lock()
	s.jobs = make(map[jobKey]job)
	for i := 0; i < maxJobs; i++ {
		s.jobs[jobKey{org: fmt.Sprint(i), site: "https://example.com/", strategy: "mobile"}] = job{snapshot: idle("https://example.com/", "mobile"), requested: time.Now()}
	}
	s.mu.Unlock()
	if _, err := s.Start("new-org", "https://new.com/", "mobile"); err != nil {
		t.Fatalf("completed cache entries blocked another tenant: %v", err)
	}
	s.wg.Wait()
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.jobs) != maxJobs {
		t.Fatalf("cache limit not enforced: %d", len(s.jobs))
	}
}

func TestSaturatedTenantCacheEvictsOldestCompletedButNeverRunning(t *testing.T) {
	now := time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC)
	s := NewService(runnerFunc(func(ctx context.Context, _, _ string) (*Report, string) {
		<-ctx.Done()
		return nil, "source_unavailable"
	}), func() time.Time { return now })
	defer s.Close()
	activeSite := "https://active.example.com/"
	if _, err := s.Start("active-org", activeSite, "mobile"); err != nil {
		t.Fatal(err)
	}
	s.mu.Lock()
	for i := 1; i < maxJobs; i++ {
		site := fmt.Sprintf("https://site-%d.example.com/", i)
		snapshot := idle(site, "mobile")
		snapshot.Status = "failed"
		snapshot.Reason = "quota_limited"
		s.jobs[jobKey{"saturating-org", site, "mobile"}] = job{snapshot: snapshot, requested: now.Add(-time.Duration(i) * time.Second)}
	}
	s.mu.Unlock()
	if _, err := s.Start("other-org", "https://other.example.com/", "mobile"); err != nil {
		t.Fatalf("one tenant exhausted another tenant's cache capacity: %v", err)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.jobs) != maxJobs {
		t.Fatalf("cache grew beyond bound: %d", len(s.jobs))
	}
	if _, ok := s.jobs[jobKey{"active-org", activeSite, "mobile"}]; !ok {
		t.Fatal("running audit was evicted")
	}
	oldest := jobKey{"saturating-org", fmt.Sprintf("https://site-%d.example.com/", maxJobs-1), "mobile"}
	if _, ok := s.jobs[oldest]; ok {
		t.Fatal("oldest completed audit was retained instead of evicted")
	}
}

func TestJobLimitsCooldownExpiryAndCancellation(t *testing.T) {
	now := time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC)
	s := NewService(runnerFunc(func(ctx context.Context, _, _ string) (*Report, string) {
		<-ctx.Done()
		return nil, "source_unavailable"
	}), func() time.Time { return now })
	for i, site := range []string{"https://aquatiq.com/", "https://example.com/"} {
		if _, err := s.Start("org-1", site, "mobile"); err != nil {
			t.Fatalf("job %d failed: %v", i, err)
		}
	}
	if _, err := s.Start("org-1", "https://third.com/", "mobile"); err != ErrBusy {
		t.Fatalf("missing concurrency cap: %v", err)
	}
	s.Close()
	failed := s.Get("org-1", "https://aquatiq.com/", "mobile")
	if failed.Status != "failed" || failed.Reason != "source_unavailable" {
		t.Fatalf("cancel result %+v", failed)
	}
	if _, err := s.Start("org-1", "https://aquatiq.com/", "mobile"); err != ErrClosed {
		t.Fatal("closed service accepted work")
	}

	var calls atomic.Int32
	s2 := NewService(runnerFunc(func(context.Context, string, string) (*Report, string) { calls.Add(1); return nil, "quota_limited" }), func() time.Time { return now })
	defer s2.Close()
	_, _ = s2.Start("org-1", "https://aquatiq.com/", "mobile")
	s2.wg.Wait()
	_, _ = s2.Start("org-1", "https://aquatiq.com/", "mobile")
	s2.wg.Wait()
	if calls.Load() != 1 {
		t.Fatal("cooldown not respected")
	}
	now = now.Add(61 * time.Second)
	_, _ = s2.Start("org-1", "https://aquatiq.com/", "mobile")
	s2.wg.Wait()
	if calls.Load() != 2 {
		t.Fatal("cooldown did not expire")
	}
	now = now.Add(2 * time.Hour)
	if s2.Get("org-1", "https://aquatiq.com/", "mobile").Status != "idle" {
		t.Fatal("TTL not applied")
	}
	if _, err := s2.Start("", "https://aquatiq.com/", "mobile"); err == nil {
		t.Fatal("empty tenant accepted")
	}
	if _, err := s2.Start("org-1", "https://localhost/", "mobile"); err == nil {
		t.Fatal("invalid site accepted")
	}
}
