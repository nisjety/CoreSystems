package pagespeed

import (
	"context"
	"errors"
	"sync"
	"time"
)

var (
	ErrBusy   = errors.New("PageSpeed audit capacity is busy")
	ErrClosed = errors.New("PageSpeed audits are unavailable")
)

const cacheTTL = time.Hour
const retryCooldown = time.Minute
const maxJobs = 128
const maxConcurrent = 2

type Snapshot struct {
	Site        string  `json:"site"`
	Source      string  `json:"source"`
	Strategy    string  `json:"strategy"`
	Status      string  `json:"status"`
	RequestedAt string  `json:"requestedAt,omitempty"`
	CompletedAt string  `json:"completedAt,omitempty"`
	Reason      string  `json:"reason,omitempty"`
	Report      *Report `json:"report,omitempty"`
}

type Runner interface {
	Run(context.Context, string, string) (*Report, string)
}
type jobKey struct{ org, site, strategy string }
type job struct {
	snapshot             Snapshot
	requested, completed time.Time
}

// Service keeps bounded, ephemeral diagnostic projections. Restarting the
// process clears these jobs; they are not durable workspace records.
type Service struct {
	mu      sync.Mutex
	jobs    map[jobKey]job
	runner  Runner
	now     func() time.Time
	ctx     context.Context
	cancel  context.CancelFunc
	wg      sync.WaitGroup
	running int
	closed  bool
}

func NewService(runner Runner, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Service{jobs: make(map[jobKey]job), runner: runner, now: now, ctx: ctx, cancel: cancel}
}

func idle(site, strategy string) Snapshot {
	return Snapshot{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "idle"}
}

func (s *Service) Get(org, site, strategy string) Snapshot {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.prune(s.now())
	if current, ok := s.jobs[jobKey{org, site, strategy}]; ok {
		return clone(current.snapshot)
	}
	return idle(site, strategy)
}

// Start schedules a read-only provider measurement; no OAuth grant or provider
// settings are changed. Pending and recent jobs are reused to protect quota.
func (s *Service) Start(org, site, strategy string) (Snapshot, error) {
	if org == "" || len(org) > 128 || ValidateInput(site, strategy) != nil {
		return Snapshot{}, errors.New("invalid PageSpeed audit input")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return Snapshot{}, ErrClosed
	}
	now := s.now().UTC()
	s.prune(now)
	key := jobKey{org, site, strategy}
	prior, exists := s.jobs[key]
	if exists && (prior.snapshot.Status == "running" || now.Sub(prior.requested) < retryCooldown) {
		return clone(prior.snapshot), nil
	}
	if s.running >= maxConcurrent {
		return Snapshot{}, ErrBusy
	}
	// A tenant's retained completed reports must not reserve global admission
	// capacity for the whole TTL. Evict the oldest non-running report when a
	// different key needs space, while preserving every in-flight audit.
	if !exists && len(s.jobs) >= maxJobs && !s.evictOldestCompleted() {
		return Snapshot{}, ErrBusy
	}
	snapshot := idle(site, strategy)
	snapshot.Status = "running"
	snapshot.RequestedAt = now.Format(time.RFC3339Nano)
	s.jobs[key] = job{snapshot: snapshot, requested: now}
	s.running++
	s.wg.Add(1)
	go s.run(key, now)
	return snapshot, nil
}

func (s *Service) run(key jobKey, requested time.Time) {
	defer s.wg.Done()
	ctx, cancel := context.WithTimeout(s.ctx, 90*time.Second)
	defer cancel()
	var report *Report
	reason := "not_configured"
	if s.runner != nil {
		report, reason = s.runner.Run(ctx, key.site, key.strategy)
	}
	if ctx.Err() != nil {
		report = nil
		reason = "source_unavailable"
	}
	if report == nil && reason == "" {
		reason = "audit_failed"
	}
	if reason != "" {
		report = nil
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	completed := s.now().UTC()
	snapshot := Snapshot{Site: key.site, Source: "google_pagespeed", Strategy: key.strategy, Status: "complete", RequestedAt: requested.Format(time.RFC3339Nano), CompletedAt: completed.Format(time.RFC3339Nano), Report: report}
	if reason != "" {
		snapshot.Status = "failed"
		snapshot.Reason = reason
	}
	s.jobs[key] = job{snapshot: clone(snapshot), requested: requested, completed: completed}
	s.running--
}

func (s *Service) prune(now time.Time) {
	for key, current := range s.jobs {
		if current.snapshot.Status != "running" && now.Sub(current.requested) >= cacheTTL {
			delete(s.jobs, key)
		}
	}
}

func (s *Service) evictOldestCompleted() bool {
	var oldestKey jobKey
	var oldestTime time.Time
	found := false
	for key, current := range s.jobs {
		if current.snapshot.Status == "running" {
			continue
		}
		if !found || current.requested.Before(oldestTime) {
			oldestKey, oldestTime, found = key, current.requested, true
		}
	}
	if found {
		delete(s.jobs, oldestKey)
	}
	return found
}

func clone(snapshot Snapshot) Snapshot {
	if snapshot.Report == nil {
		return snapshot
	}
	r := snapshot.Report
	copied := Report{Performance: copyNumber(r.Performance), Accessibility: copyNumber(r.Accessibility), BestPractices: copyNumber(r.BestPractices), SEO: copyNumber(r.SEO), LCPMs: copyNumber(r.LCPMs), CLS: copyNumber(r.CLS), TBTMs: copyNumber(r.TBTMs), FCPMs: copyNumber(r.FCPMs), SpeedIndexMs: copyNumber(r.SpeedIndexMs), FetchTime: r.FetchTime, LighthouseVersion: r.LighthouseVersion, Checks: cloneChecks(r.Checks)}
	copied.ReportingOrigin = r.ReportingOrigin
	return Snapshot{Site: snapshot.Site, Source: snapshot.Source, Strategy: snapshot.Strategy, Status: snapshot.Status, RequestedAt: snapshot.RequestedAt, CompletedAt: snapshot.CompletedAt, Reason: snapshot.Reason, Report: &copied}
}

func copyNumber(value *float64) *float64 {
	if value == nil {
		return nil
	}
	copied := *value
	return &copied
}

func (s *Service) Close() {
	s.mu.Lock()
	s.closed = true
	s.cancel()
	s.mu.Unlock()
	s.wg.Wait()
}
