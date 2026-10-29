package publicsignals

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Service struct {
	repo     *Repository
	registry Registry
	runner   Runner
	now      func() time.Time
	timeout  time.Duration
	wake     chan struct{}
	running  atomic.Bool
}

func New(repo *Repository, registry Registry, runner Runner, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: registry, runner: runner, now: now, timeout: 40 * time.Second, wake: make(chan struct{}, 2)}
}
func (s *Service) binding(id, site string, revision int64) (websites.Record, error) {
	canonical, err := pulse.CanonicalSite(site)
	if s.repo == nil || s.registry == nil || s.registry.OrganizationID() != s.repo.org || !idPattern.MatchString(id) || err != nil || canonical != site {
		return websites.Record{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Record{}, err
	}
	if r.Website.ID != id || r.Website.Site != site || r.Website.Revision < 1 || revision > 0 && revision != r.Website.Revision {
		return websites.Record{}, ErrConflict
	}
	return r, nil
}
func matches(v State, w websites.Website) bool {
	return v.WebsiteID == w.ID && v.Site == w.Site && v.WebsiteRevision == w.Revision
}

// Get is a pure saved projection, never a provider request or queue operation.
func (s *Service) Get(id, site string) (State, error) {
	r, err := s.binding(id, site, 0)
	if err != nil {
		return State{}, err
	}
	result := empty(r.Website)
	for _, v := range s.repo.read().Entries {
		if matches(v, r.Website) {
			result = clone(v)
		}
	}
	if result.Report != nil && (!parse(result.ExpiresAt).After(s.now()) || !parse(result.Report.ObservedAt).Add(successTTL).After(s.now())) {
		result.CachedReport = retainedReport(result, s.now())
		result.Report = nil
	}
	if _, err = s.binding(id, site, r.Website.Revision); err != nil {
		return State{}, err
	}
	for _, latest := range s.repo.read().Entries {
		if matches(latest, r.Website) && latest.JobID != result.JobID {
			return State{}, ErrConflict
		}
	}
	if result.Report != nil && (!parse(result.ExpiresAt).After(s.now()) || !parse(result.Report.ObservedAt).Add(successTTL).After(s.now())) {
		result.CachedReport = retainedReport(result, s.now())
		result.Report = nil
	}
	if result.CachedReport != nil && !parse(result.CachedReport.ObservedAt).Add(retentionTTL).After(s.now()) {
		result.CachedReport = nil
	}
	return result, nil
}

// A prior observation is a separately labelled cache, never the new job's proof.
// Retention follows the original observation, not retries or refresh receipts.
func retainedReport(e State, now time.Time) *Report {
	result := e.CachedReport
	if e.Report != nil && (result == nil || parse(e.Report.ObservedAt).After(parse(result.ObservedAt))) {
		result = e.Report
	}
	if result == nil || parse(result.ObservedAt).After(now) || !parse(result.ObservedAt).Add(retentionTTL).After(now) {
		return nil
	}
	copy := *result
	return &copy
}
func (s *Service) Queue(id, site string, revision int64) (State, error) {
	if revision < 1 {
		return State{}, ErrInvalid
	}
	r, err := s.binding(id, site, revision)
	if err != nil {
		return State{}, err
	}
	var result State
	err = s.repo.update(func(v diskState) (diskState, error) {
		index := -1
		for i, e := range v.Entries {
			if e.WebsiteID != id {
				continue
			}
			index = i
			if matches(e, r.Website) {
				if e.Status == "queued" || e.Status == "running" {
					e.Enabled = true
					v.Entries[i] = e
					result = clone(e)
					return v, nil
				}
				if s.now().Sub(parse(e.CompletedAt)) < time.Minute {
					return v, ErrCooldown
				}
			}
			break
		}
		if index < 0 && len(v.Entries) >= maxEntries {
			for i, e := range v.Entries {
				if e.Status == "queued" || e.Status == "running" {
					continue
				}
				if index < 0 || e.CompletedAt < v.Entries[index].CompletedAt {
					index = i
				}
			}
			if index < 0 {
				return v, ErrCapacity
			}
		}
		result = empty(r.Website)
		if index >= 0 && matches(v.Entries[index], r.Website) {
			result.CachedReport = retainedReport(v.Entries[index], s.now())
		}
		result.Enabled = true
		result.Status = "queued"
		result.JobID = newID()
		result.QueuedAt = stamp(s.now())
		if index < 0 {
			v.Entries = append(v.Entries, clone(result))
		} else {
			v.Entries[index] = clone(result)
		}
		return v, nil
	})
	if err != nil && !errors.Is(err, errCommitted) {
		return State{}, err
	}
	s.notify()
	return result, nil
}
func (s *Service) SetEnabled(id, site string, revision int64, enabled bool) (State, error) {
	if revision < 1 {
		return State{}, ErrInvalid
	}
	r, err := s.binding(id, site, revision)
	if err != nil {
		return State{}, err
	}
	current, err := s.Get(id, site)
	if err != nil {
		return State{}, err
	}
	if current.Status == "idle" {
		if enabled {
			return s.Queue(id, site, revision)
		}
		return current, nil
	}
	if enabled && current.Status == "complete" && current.Report == nil {
		return s.Queue(id, site, revision)
	}
	err = s.repo.update(func(v diskState) (diskState, error) {
		found := false
		for i, e := range v.Entries {
			if !matches(e, r.Website) {
				continue
			}
			found = true
			e.Enabled = enabled
			e.NextSyncAt = ""
			if enabled && (e.Status == "complete" || e.Status == "failed") {
				e.NextSyncAt = stamp(s.now().Add(successTTL))
				if e.Status == "failed" {
					e.NextSyncAt = stamp(s.now().Add(failureDelay))
				}
			}
			v.Entries[i] = e
		}
		if !found {
			return v, ErrConflict
		}
		return v, nil
	})
	if err != nil && !errors.Is(err, errCommitted) {
		return State{}, err
	}
	s.notify()
	return s.Get(id, site)
}
func (s *Service) notify() {
	select {
	case s.wake <- struct{}{}:
	default:
	}
}
func (s *Service) retry(ctx context.Context, fn func() error) bool {
	for {
		err := fn()
		if err == nil || errors.Is(err, errCommitted) {
			return true
		}
		if errors.Is(err, ErrInvalid) || errors.Is(err, ErrCapacity) {
			return false
		}
		t := time.NewTimer(100 * time.Millisecond)
		select {
		case <-ctx.Done():
			t.Stop()
			return false
		case <-t.C:
		}
	}
}

// Run owns two joined workers and recovers only previously enrolled jobs.
func (s *Service) Run(ctx context.Context) {
	if s.repo == nil || s.runner == nil || !s.running.CompareAndSwap(false, true) {
		return
	}
	defer s.running.Store(false)
	if !s.retry(ctx, s.recover) {
		return
	}
	var wg sync.WaitGroup
	for range 2 {
		wg.Add(1)
		go func() { defer wg.Done(); s.worker(ctx) }()
	}
	wg.Wait()
}
func (s *Service) recover() error {
	return s.repo.update(func(v diskState) (diskState, error) {
		for i, e := range v.Entries {
			if e.Status == "running" {
				v.Entries[i] = failed(e, s.now(), "interrupted")
			}
		}
		return v, nil
	})
}
func (s *Service) worker(ctx context.Context) {
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		if e, ok := s.claim(); ok {
			s.execute(ctx, e)
			continue
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		case <-s.wake:
		}
	}
}
func (s *Service) claim() (State, bool) {
	candidate := false
	for _, e := range s.repo.read().Entries {
		if e.Enabled && (e.Status == "queued" || e.NextSyncAt != "" && !parse(e.NextSyncAt).After(s.now())) || e.Report != nil && s.now().Sub(parse(e.CompletedAt)) >= retentionTTL || e.CachedReport != nil && !parse(e.CachedReport.ObservedAt).Add(retentionTTL).After(s.now()) {
			candidate = true
		}
	}
	if !candidate {
		return State{}, false
	}
	var result State
	found := false
	err := s.repo.update(func(v diskState) (diskState, error) {
		for i, e := range v.Entries {
			if e.CachedReport != nil && !parse(e.CachedReport.ObservedAt).Add(retentionTTL).After(s.now()) {
				e.CachedReport = nil
				v.Entries[i] = e
			}
			if e.Report != nil && s.now().Sub(parse(e.CompletedAt)) >= retentionTTL {
				e.Report = nil
				v.Entries[i] = e
			}
			if found || !e.Enabled || e.Status != "queued" && (e.NextSyncAt == "" || parse(e.NextSyncAt).After(s.now())) {
				continue
			}
			r, err := s.binding(e.WebsiteID, e.Site, e.WebsiteRevision)
			if err != nil {
				e.Enabled = false
				e.NextSyncAt = ""
				if e.Status == "queued" {
					e.StartedAt = stamp(s.now())
					e = failed(e, s.now(), "context_changed")
				}
				v.Entries[i] = e
				continue
			}
			if e.Status != "queued" {
				cached := retainedReport(e, s.now())
				e = empty(r.Website)
				e.CachedReport = cached
				e.Enabled = true
				e.Status = "queued"
				e.JobID = newID()
				e.QueuedAt = stamp(s.now())
			}
			e.Status = "running"
			e.StartedAt = stamp(s.now())
			v.Entries[i] = e
			result = clone(e)
			found = true
		}
		return v, nil
	})
	return result, found && (err == nil || errors.Is(err, errCommitted))
}
func (s *Service) execute(parent context.Context, e State) {
	ctx, cancel := context.WithTimeout(parent, s.timeout)
	defer cancel()
	var report Report
	var collectErr error
	why := "context_changed"
	r, err := s.binding(e.WebsiteID, e.Site, e.WebsiteRevision)
	if err == nil {
		report, collectErr = s.runner.Collect(ctx, r)
		if _, err = s.binding(e.WebsiteID, e.Site, e.WebsiteRevision); err == nil {
			why = "source_unavailable"
			if ctx.Err() != nil {
				why = "timeout"
			}
		}
	}
	// One observation defines the entire terminal receipt. Independent clock
	// reads can otherwise put expiry a few nanoseconds beyond its strict bound.
	completedNow := s.now()
	if completedNow.Before(parse(e.StartedAt)) {
		completedNow = parse(e.StartedAt)
	}
	result := failed(e, completedNow, why)
	if err == nil && collectErr == nil && ctx.Err() == nil && ValidReport(report, e.Site, completedNow) && !parse(report.ObservedAt).Before(parse(e.StartedAt)) {
		result.Status = "complete"
		result.Reason = ""
		result.Report = &report
		result.ExpiresAt = stamp(completedNow.Add(successTTL))
		if result.Enabled {
			delay := successTTL
			for _, device := range report.CrUX {
				if device.Reason == "source_unavailable" || device.Reason == "timeout" || device.Reason == "quota_limited" {
					delay = failureDelay
				}
			}
			if report.CommonCrawl.Reason == "source_unavailable" || report.CommonCrawl.Reason == "timeout" || report.CommonCrawl.Reason == "quota_limited" {
				delay = failureDelay
			}
			result.NextSyncAt = stamp(completedNow.Add(delay))
		}
	}
	if !validState(diskState{Version: 1, OrganizationID: s.repo.org, Entries: []State{result}}) {
		result = failed(e, completedNow, "source_unavailable")
	}
	commit := func() error {
		return s.repo.update(func(v diskState) (diskState, error) {
			for i, current := range v.Entries {
				if current.JobID != e.JobID || current.Status != "running" {
					continue
				}
				next := clone(result)
				next.Enabled = current.Enabled
				if !next.Enabled {
					next.NextSyncAt = ""
				}
				v.Entries[i] = next
			}
			return v, nil
		})
	}
	commitErr := commit()
	if errors.Is(commitErr, ErrInvalid) || errors.Is(commitErr, ErrCapacity) {
		// A permanent payload/size error is not a transient storage outage.
		// Record one bounded failure receipt without re-running the provider.
		result = failed(e, completedNow, "source_unavailable")
		commitErr = commit()
	}
	if commitErr != nil && !errors.Is(commitErr, errCommitted) && !errors.Is(commitErr, ErrInvalid) && !errors.Is(commitErr, ErrCapacity) {
		s.retry(parent, commit)
	}
}
func failed(e State, now time.Time, why string) State {
	e.Status = "failed"
	e.Reason = why
	e.Report = nil
	e.ExpiresAt = ""
	e.CompletedAt = stamp(now)
	if e.StartedAt == "" {
		e.StartedAt = stamp(now)
	}
	e.NextSyncAt = ""
	if e.Enabled {
		e.NextSyncAt = stamp(now.Add(failureDelay))
	}
	return e
}
