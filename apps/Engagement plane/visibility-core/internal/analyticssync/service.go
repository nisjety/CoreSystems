package analyticssync

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
	repo      *Repository
	registry  Registry
	runner    Runner
	authority Authority
	now       func() time.Time
	wake      chan struct{}
	running   atomic.Bool
	timeout   time.Duration
}

func New(repo *Repository, registry Registry, runner Runner, authority Authority, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: registry, runner: runner, authority: authority, now: now, wake: make(chan struct{}, 2), timeout: jobTimeout}
}
func (s *Service) binding(id, site, q string, revision int64) (websites.Record, error) {
	canonical, err := pulse.CanonicalSite(site)
	if s.repo == nil || s.registry == nil || s.registry.OrganizationID() != s.repo.org || !idPattern.MatchString(id) || err != nil || canonical != site || (q != "FY" && (len(q) != 2 || q[0] != 'Q' || q[1] < '1' || q[1] > '4')) {
		return websites.Record{}, ErrInvalid
	}
	if _, err = pulse.QuarterPeriod(q, s.now()); err != nil {
		return websites.Record{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Record{}, err
	}
	if r.Website.ID != id || r.Website.Site != site || r.Website.Revision < 1 || revision > 0 && r.Website.Revision != revision {
		return websites.Record{}, ErrConflict
	}
	return r, nil
}
func matches(e entry, r websites.Record, q string, year int) bool {
	return e.WebsiteID == r.Website.ID && e.Site == r.Website.Site && e.WebsiteRevision == r.Website.Revision && e.Quarter == q && e.Year == year && e.Fingerprint == fingerprint(r)
}
func (s *Service) find(r websites.Record, q string) (entry, bool) {
	for _, e := range s.repo.read().Entries {
		if matches(e, r, q, s.now().UTC().Year()) {
			return e, true
		}
	}
	return entry{}, false
}

// Get reads lifecycle only and never schedules, authorizes or fetches metrics.
func (s *Service) Get(id, site, q string) (State, error) {
	r, err := s.binding(id, site, q, 0)
	if err != nil {
		return State{}, err
	}
	result := empty(r.Website, q)
	if e, ok := s.find(r, q); ok {
		result = clone(e.State)
	}
	again, err := s.binding(id, site, q, r.Website.Revision)
	if err != nil || fingerprint(again) != fingerprint(r) {
		return State{}, ErrConflict
	}
	return result, nil
}

// Read serves retained normalized metrics only after fresh Integration authority.
func (s *Service) Read(ctx context.Context, id, site, q string) (Bundle, error) {
	r, err := s.binding(id, site, q, 0)
	if err != nil {
		return Bundle{}, err
	}
	missing := MissingBundle(site, q, s.now(), "snapshot_pending")
	e, ok := s.find(r, q)
	if !ok {
		return missing, nil
	}
	if e.Bundle == nil {
		if e.ExpiresAt != "" {
			return MissingBundle(site, q, s.now(), "snapshot_expired"), nil
		}
		return missing, nil
	}
	if !parsed(e.ExpiresAt).After(s.now()) {
		return MissingBundle(site, q, s.now(), "snapshot_expired"), nil
	}
	if s.authority == nil {
		return MissingBundle(site, q, s.now(), "source_unavailable"), nil
	}
	access := s.authority.Check(ctx, r)
	again, err := s.binding(id, site, q, r.Website.Revision)
	if err != nil || fingerprint(again) != fingerprint(r) {
		return Bundle{}, ErrConflict
	}
	latest, stillCurrent := s.find(again, q)
	if !stillCurrent || latest.JobID != e.JobID || latest.Bundle == nil {
		return MissingBundle(site, q, s.now(), "snapshot_pending"), nil
	}
	if !parsed(latest.ExpiresAt).After(s.now()) {
		return MissingBundle(site, q, s.now(), "snapshot_expired"), nil
	}
	return maskAged(mask(clone(*e.Bundle), access, site, q, s.now(), "authorization_required"), site, q, s.now()), nil
}

// Integration may return an upstream cache receipt. Collection time must not
// extend that source's own observation age or an unseen provider revocation.
func maskAged(b Bundle, site, q string, now time.Time) Bundle {
	expired := func(status, observed string) bool {
		return status == "connected" && !parsed(observed).Add(metricsTTL).After(now)
	}
	missing := MissingBundle(site, q, now, "snapshot_expired")
	if expired(b.Pulse.Status, b.Pulse.UpdatedAt) || expired(b.Search.Status, b.Search.UpdatedAt) {
		b.Pulse, b.Search = missing.Pulse, missing.Search
	}
	if expired(b.GA4.Status, b.GA4.UpdatedAt) {
		b.GA4 = missing.GA4
	}
	if expired(b.Clarity.Status, b.Clarity.UpdatedAt) || expired(b.Clarity.Status, b.Clarity.Window.ObservedAt) {
		b.Clarity = missing.Clarity
	}
	return b
}

// Queue enrolls this exact website+quarter for periodic synchronization. It never
// calls providers; work begins only through the separately joined Run lifecycle.
func (s *Service) Queue(id, site string, revision int64, q string) (State, error) {
	if revision < 1 {
		return State{}, ErrInvalid
	}
	r, err := s.binding(id, site, q, revision)
	if err != nil {
		return State{}, err
	}
	var result State
	err = s.repo.update(func(v diskState) (diskState, error) {
		index := -1
		for i, e := range v.Entries {
			if e.WebsiteID == id && e.Quarter == q {
				index = i
				if matches(e, r, q, s.now().UTC().Year()) {
					if e.Status == "queued" || e.Status == "running" {
						e.Enabled = true
						v.Entries[i] = e
						result = clone(e.State)
						return v, nil
					}
					if s.now().Sub(parsed(e.CompletedAt)) < time.Minute {
						return v, ErrCooldown
					}
				}
				break
			}
		}
		if index < 0 && len(v.Entries) >= maxEntries {
			oldest := -1
			for i, e := range v.Entries {
				if e.Status == "queued" || e.Status == "running" {
					continue
				}
				if oldest < 0 || e.QueuedAt < v.Entries[oldest].QueuedAt {
					oldest = i
				}
			}
			if oldest < 0 {
				return v, ErrCapacity
			}
			index = oldest
		}
		result = empty(r.Website, q)
		if index >= 0 && matches(v.Entries[index], r, q, s.now().UTC().Year()) {
			carrySuccess(&result, v.Entries[index].State)
		}
		result.Enabled = true
		result.Status = "queued"
		result.JobID = newID()
		result.QueuedAt = stamp(s.now())
		next := entry{State: clone(result), Fingerprint: fingerprint(r), Year: s.now().UTC().Year()}
		if index < 0 {
			v.Entries = append(v.Entries, next)
		} else {
			v.Entries[index] = next
		}
		return v, nil
	})
	if err != nil && !errors.Is(err, errCommitted) {
		return State{}, err
	}
	s.notify()
	return result, nil
}
func (s *Service) SetEnabled(id, site string, revision int64, q string, enabled bool) (State, error) {
	if revision < 1 {
		return State{}, ErrInvalid
	}
	r, err := s.binding(id, site, q, revision)
	if err != nil {
		return State{}, err
	}
	if _, ok := s.find(r, q); !ok {
		if enabled {
			return s.Queue(id, site, revision, q)
		}
		return empty(r.Website, q), nil
	}
	var result State
	err = s.repo.update(func(v diskState) (diskState, error) {
		found := false
		for i, e := range v.Entries {
			if !matches(e, r, q, s.now().UTC().Year()) {
				continue
			}
			found = true
			e.Enabled = enabled
			e.NextSyncAt = ""
			if enabled && e.Status != "queued" && e.Status != "running" {
				e.NextSyncAt = stamp(s.now().Add(metricsTTL))
			}
			v.Entries[i] = e
			result = clone(e.State)
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
	return result, nil
}
func (s *Service) notify() {
	select {
	case s.wake <- struct{}{}:
	default:
	}
}

// Run processes only explicitly enrolled entries, never all registry websites.
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
func (s *Service) retry(ctx context.Context, fn func() error) bool {
	for {
		err := fn()
		if err == nil || errors.Is(err, errCommitted) {
			return true
		}
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return false
		case <-timer.C:
		}
	}
}
func (s *Service) recover() error {
	return s.repo.update(func(v diskState) (diskState, error) {
		for i, e := range v.Entries {
			if e.Bundle != nil && e.Enabled && parsed(e.NextSyncAt).After(s.now()) {
				next := nextCollection(*e.Bundle, parsed(e.CompletedAt), s.now())
				if next.Before(parsed(e.NextSyncAt)) {
					e.NextSyncAt = stamp(next)
					v.Entries[i] = e
				}
			}
			if e.Status != "running" {
				continue
			}
			if e.Attempts >= 3 {
				e = failedEntry(e, s.now(), "interrupted")
			} else {
				e.Status = "queued"
				e.StartedAt = ""
				for j, key := range stepKeys {
					e.Steps[j] = Step{Key: key, Status: "queued", LastSuccessAt: e.Steps[j].LastSuccessAt}
				}
			}
			v.Entries[i] = e
		}
		return v, nil
	})
}
func (s *Service) worker(ctx context.Context) {
	tick := time.NewTicker(time.Second)
	defer tick.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		e, ok := s.claim()
		if ok {
			s.execute(ctx, e)
			continue
		}
		select {
		case <-ctx.Done():
			return
		case <-s.wake:
		case <-tick.C:
		}
	}
}
func (s *Service) claim() (entry, bool) {
	v := s.repo.read()
	candidate := false
	for _, e := range v.Entries {
		if e.Enabled && (e.Status == "queued" || e.NextSyncAt != "" && !parsed(e.NextSyncAt).After(s.now())) {
			candidate = true
		}
		if e.Bundle != nil && s.now().Sub(parsed(e.CompletedAt)) >= retentionTTL {
			candidate = true
		}
	}
	if !candidate {
		return entry{}, false
	}
	var result entry
	found := false
	err := s.repo.update(func(v diskState) (diskState, error) {
		for i, e := range v.Entries {
			if e.Bundle != nil && s.now().Sub(parsed(e.CompletedAt)) >= retentionTTL {
				e.Bundle = nil
				v.Entries[i] = e
			}
			if found || !e.Enabled {
				continue
			}
			if e.Status != "queued" && (e.NextSyncAt == "" || parsed(e.NextSyncAt).After(s.now())) {
				continue
			}
			if e.Year != s.now().UTC().Year() {
				e.Enabled = false
				e.NextSyncAt = ""
				v.Entries[i] = e
				continue
			}
			r, err := s.binding(e.WebsiteID, e.Site, e.Quarter, e.WebsiteRevision)
			if err != nil || !matches(e, r, e.Quarter, e.Year) {
				e.Enabled = false
				e.NextSyncAt = ""
				if e.Status == "queued" {
					e.StartedAt = stamp(s.now())
					e = failedEntry(e, s.now(), "context_changed")
				}
				v.Entries[i] = e
				continue
			}
			if e.Status != "queued" {
				nextState := empty(r.Website, e.Quarter)
				carrySuccess(&nextState, e.State)
				e.State = nextState
				e.Enabled = true
				e.Status = "queued"
				e.JobID = newID()
				e.QueuedAt = stamp(s.now())
				e.Attempts = 0
			}
			e.Status = "running"
			e.StartedAt = stamp(s.now())
			e.CompletedAt = ""
			e.NextSyncAt = ""
			e.ExpiresAt = ""
			e.Bundle = nil
			e.Attempts++
			for j, key := range stepKeys {
				e.Steps[j] = Step{Key: key, Status: "running", LastSuccessAt: e.Steps[j].LastSuccessAt}
			}
			v.Entries[i] = e
			result = clone(e)
			found = true
		}
		return v, nil
	})
	return result, found && (err == nil || errors.Is(err, errCommitted))
}
func (s *Service) execute(parent context.Context, e entry) {
	ctx, cancel := context.WithTimeout(parent, s.timeout)
	defer cancel()
	r, err := s.binding(e.WebsiteID, e.Site, e.Quarter, e.WebsiteRevision)
	result := failedEntry(e, s.now(), "context_changed")
	if err == nil && matches(e, r, e.Quarter, e.Year) {
		access := Access{}
		if s.authority != nil {
			access = s.authority.Check(ctx, r)
		}
		bundle := MissingBundle(e.Site, e.Quarter, s.now(), "authorization_required")
		if access.SearchConsole || access.GA4 || access.Clarity {
			bundle, err = s.runner.Collect(ctx, r, e.Quarter)
		}
		again, bindErr := s.binding(e.WebsiteID, e.Site, e.Quarter, e.WebsiteRevision)
		if bindErr == nil && fingerprint(again) == e.Fingerprint {
			why := "source_unavailable"
			if ctx.Err() != nil {
				why = "timeout"
			}
			if err != nil || !validBundle(bundle, e.Site, e.Quarter, s.now()) {
				bundle = MissingBundle(e.Site, e.Quarter, s.now(), why)
			}
			bundle = mask(bundle, access, e.Site, e.Quarter, s.now(), "authorization_required")
			result = completedEntry(e, bundle, s.now())
		}
	}
	// Retry only durable commit, never repeat provider work on a storage failure.
	s.retry(parent, func() error {
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
	})
}
func failedEntry(e entry, now time.Time, why string) entry {
	e.Bundle = nil
	e.Status = "failed"
	e.CompletedAt = stamp(now)
	e.ExpiresAt = ""
	e.NextSyncAt = ""
	if e.StartedAt == "" {
		e.StartedAt = stamp(now)
	}
	if e.Enabled {
		e.NextSyncAt = stamp(now.Add(retryDelay))
	}
	for i, key := range stepKeys {
		e.Steps[i] = Step{Key: key, Status: "unavailable", Reason: why, ObservedAt: stamp(now), LastSuccessAt: e.Steps[i].LastSuccessAt}
	}
	return e
}
func completedEntry(e entry, b Bundle, now time.Time) entry {
	e = failedEntry(e, now, "source_unavailable")
	statuses := []struct{ status, reason, observed string }{{b.Pulse.Status, b.Pulse.Reason, b.Pulse.UpdatedAt}, {b.GA4.Status, b.GA4.Reason, b.GA4.UpdatedAt}, {b.Clarity.Status, b.Clarity.Reason, b.Clarity.UpdatedAt}}
	connected := 0
	for i, v := range statuses {
		why := v.reason
		if v.status == "connected" {
			connected++
			why = ""
		}
		if v.status == "unavailable" && !collectionReason(why) {
			why = "source_unavailable"
		}
		lastSuccess := e.Steps[i].LastSuccessAt
		if v.status == "connected" && !parsed(v.observed).IsZero() && !parsed(v.observed).After(now) && parsed(v.observed).After(parsed(lastSuccess)) {
			lastSuccess = v.observed
		}
		e.Steps[i] = Step{Key: stepKeys[i], Status: v.status, Reason: why, ObservedAt: stamp(now), LastSuccessAt: lastSuccess}
	}
	if connected == 0 {
		return e
	}
	e.Status = "partial"
	if connected == 3 {
		e.Status = "complete"
	}
	e.Bundle = &b
	e.ExpiresAt = stamp(now.Add(metricsTTL))
	if e.Enabled {
		e.NextSyncAt = stamp(nextCollection(b, now, now))
	}

	return e
}

// Lifecycle metadata survives retries without retaining or serving stale metrics.
func carrySuccess(next *State, previous State) {
	for i := range next.Steps {
		for _, old := range previous.Steps {
			if old.Key == next.Steps[i].Key {
				next.Steps[i].LastSuccessAt = old.LastSuccessAt
			}
		}
	}
}

func nextCollection(b Bundle, completed, now time.Time) time.Time {
	next := completed.Add(metricsTTL)
	observations := []struct{ status, observed string }{{b.Pulse.Status, b.Pulse.UpdatedAt}, {b.Search.Status, b.Search.UpdatedAt}, {b.GA4.Status, b.GA4.UpdatedAt}, {b.Clarity.Status, b.Clarity.UpdatedAt}, {b.Clarity.Status, b.Clarity.Window.ObservedAt}}
	for _, v := range observations {
		if v.status != "connected" || parsed(v.observed).IsZero() {
			continue
		}
		expires := parsed(v.observed).Add(metricsTTL)
		if !expires.After(now) {
			expires = now.Add(retryDelay)
		}
		if expires.Before(next) {
			next = expires
		}
	}
	return next
}
