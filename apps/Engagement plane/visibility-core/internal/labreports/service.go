// Package labreports owns durable, per-site Lighthouse projections and daily
// collection. Integration Core remains the only provider/network authority.
package labreports

import (
	"context"
	"encoding/json"
	"errors"
	"regexp"
	"sync"
	"sync/atomic"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

var ErrInvalid = errors.New("invalid lab report context")
var ErrCapacity = errors.New("lab report capacity exceeded")
var orgPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
var idPattern = regexp.MustCompile(`^web_[a-f0-9]{32}$`)

const maxBytes = 1 << 20
const maxReports = 128 // Two devices for each of the registry’s 64 sites.
const FreshFor = 24 * time.Hour

type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
	List() []websites.Website
}
type Source interface {
	GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
	StartPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
}
type record struct {
	WebsiteID   string                     `json:"websiteId"`
	Site        string                     `json:"site"`
	Revision    int64                      `json:"revision"`
	Strategy    string                     `json:"strategy"`
	Status      string                     `json:"status"`
	RequestedAt string                     `json:"requestedAt"`
	StartedAt   string                     `json:"startedAt,omitempty"`
	CompletedAt string                     `json:"completedAt,omitempty"`
	NextRunAt   string                     `json:"nextRunAt"`
	Reason      string                     `json:"reason,omitempty"`
	Successful  *pulse.PageSpeedProjection `json:"successful,omitempty"`
}
type diskState struct {
	Version        int      `json:"version"`
	OrganizationID string   `json:"organizationId"`
	Reports        []record `json:"reports"`
}

func cloneState(v diskState) diskState {
	raw, _ := json.Marshal(v)
	var copy diskState
	_ = json.Unmarshal(raw, &copy)
	return copy
}
func timestamp(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }
func parse(v string) time.Time     { t, _ := time.Parse(time.RFC3339Nano, v); return t }
func validState(v diskState) error {
	if v.Version != 1 || !orgPattern.MatchString(v.OrganizationID) || v.Reports == nil || len(v.Reports) > maxReports {
		return ErrInvalid
	}
	seen := map[string]bool{}
	for _, r := range v.Reports {
		canonical, err := pulse.CanonicalSite(r.Site)
		key := r.WebsiteID + ":" + r.Strategy
		if err != nil || canonical != r.Site || !idPattern.MatchString(r.WebsiteID) || r.Revision < 1 || (r.Strategy != "mobile" && r.Strategy != "desktop") || seen[key] || parse(r.RequestedAt).IsZero() || parse(r.NextRunAt).IsZero() {
			return ErrInvalid
		}
		seen[key] = true
		if r.StartedAt != "" && (parse(r.StartedAt).IsZero() || parse(r.StartedAt).Before(parse(r.RequestedAt))) {
			return ErrInvalid
		}
		if r.Status != "queued" && r.Status != "running" && r.Status != "complete" && r.Status != "failed" {
			return ErrInvalid
		}
		if r.Status == "running" && r.StartedAt == "" {
			return ErrInvalid
		}
		if r.Status == "failed" {
			if !validReason(r.Reason) {
				return ErrInvalid
			}
		} else if r.Reason != "" {
			return ErrInvalid
		}
		if r.Status == "complete" || r.Status == "failed" {
			if parse(r.CompletedAt).IsZero() || r.StartedAt == "" || parse(r.CompletedAt).Before(parse(r.StartedAt)) {
				return ErrInvalid
			}
		}
		if r.Successful != nil && (!pulse.ValidPageSpeedProjection(*r.Successful) || r.Successful.Status != "complete" || r.Successful.Site != r.Site || r.Successful.Strategy != r.Strategy || r.Successful.Cache != nil) {
			return ErrInvalid
		}
		if r.Status == "complete" && r.Successful == nil {
			return ErrInvalid
		}
	}
	return nil
}
func validReason(s string) bool {
	return s == "quota_limited" || s == "not_configured" || s == "source_unavailable" || s == "audit_failed"
}

type Service struct {
	repo     *Repository
	registry Registry
	source   Source
	now      func() time.Time
	running  atomic.Bool
	wake     chan struct{}
}

func New(repo *Repository, registry Registry, source Source, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: registry, source: source, now: now, wake: make(chan struct{}, 1)}
}
func (s *Service) binding(site, strategy string) (websites.Website, error) {
	if s.repo == nil || s.registry == nil || s.repo.org != s.registry.OrganizationID() || (strategy != "mobile" && strategy != "desktop") {
		return websites.Website{}, ErrInvalid
	}
	r, err := s.registry.Resolve("", site)
	if err != nil || r.Website.Site != site {
		return websites.Website{}, ErrInvalid
	}
	return r.Website, nil
}
func matches(r record, w websites.Website, strategy string) bool {
	return r.WebsiteID == w.ID && r.Site == w.Site && r.Revision == w.Revision && r.Strategy == strategy
}
func (s *Service) Get(_ context.Context, site, strategy string) pulse.PageSpeedProjection {
	value := pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "idle"}
	w, err := s.binding(site, strategy)
	if err != nil {
		return pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "failed", Reason: "source_unavailable"}
	}
	for _, r := range s.repo.read().Reports {
		if !matches(r, w, strategy) {
			continue
		}
		value.Status = r.Status
		if r.Status == "queued" {
			value.Status = "running"
		}
		value.RequestedAt = r.RequestedAt
		value.CompletedAt = r.CompletedAt
		value.Reason = r.Reason
		cache := &pulse.PageSpeedCache{Stale: true, RefreshStatus: r.Status, LastAttemptAt: r.StartedAt, NextRefreshAt: r.NextRunAt, RefreshReason: r.Reason}
		if r.Successful != nil {
			value = *r.Successful
			cache.LastSuccessAt = value.Report.FetchTime
			age := s.now().Sub(parse(value.Report.FetchTime))
			cache.Stale = age >= FreshFor || age < -5*time.Minute
		}
		value.Cache = cache
		break
	}
	if current, checkErr := s.binding(site, strategy); checkErr != nil || current.Revision != w.Revision || current.ID != w.ID {
		return pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "failed", Reason: "source_unavailable"}
	}
	return value
}
func (s *Service) Queue(site, strategy string) pulse.PageSpeedProjection {
	w, err := s.binding(site, strategy)
	if err != nil {
		return s.Get(context.Background(), site, strategy)
	}
	if err = s.queue(w, strategy, true); err != nil && !errors.Is(err, errCommitted) {
		value := s.Get(context.Background(), site, strategy)
		if value.Cache != nil {
			value.Cache.RefreshStatus = "failed"
			value.Cache.RefreshReason = "source_unavailable"
			return value
		}
		return pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "failed", Reason: "source_unavailable"}
	}
	return s.Get(context.Background(), site, strategy)
}
func (s *Service) queue(w websites.Website, strategy string, manual bool) error {
	active := map[string]bool{}
	for _, site := range s.registry.List() {
		active[site.ID] = true
	}
	err := s.repo.update(func(v diskState) (diskState, error) {
		kept := v.Reports[:0]
		for _, row := range v.Reports {
			if active[row.WebsiteID] {
				kept = append(kept, row)
			}
		}
		v.Reports = kept
		now := s.now()
		index := -1
		var successful *pulse.PageSpeedProjection
		for i, r := range v.Reports {
			if r.WebsiteID != w.ID || r.Strategy != strategy {
				continue
			}
			index = i
			if matches(r, w, strategy) {
				if r.Status == "queued" || r.Status == "running" || manual && now.Sub(parse(r.RequestedAt)) < time.Minute || !manual && now.Before(parse(r.NextRunAt)) {
					return v, nil
				}
				successful = r.Successful
			}
			break
		}
		if index < 0 && len(v.Reports) >= maxReports {
			return v, ErrCapacity
		}
		r := record{WebsiteID: w.ID, Site: w.Site, Revision: w.Revision, Strategy: strategy, Status: "queued", RequestedAt: timestamp(now), NextRunAt: timestamp(now), Successful: successful}
		if index < 0 {
			v.Reports = append(v.Reports, r)
		} else {
			v.Reports[index] = r
		}
		return v, nil
	})
	if err == nil {
		select {
		case s.wake <- struct{}{}:
		default:
		}
	}
	return err
}
func (s *Service) ensure() {
	if s.registry == nil {
		return
	}
	for _, w := range s.registry.List() {
		for _, device := range []string{"mobile", "desktop"} {
			_ = s.queue(w, device, false)
		}
	}
}
func (s *Service) Run(ctx context.Context) {
	if s.repo == nil || !s.running.CompareAndSwap(false, true) {
		return
	}
	defer s.running.Store(false)
	// A process crash cannot leave a durable job permanently marked running.
	for {
		err := s.repo.update(func(v diskState) (diskState, error) {
			for i := range v.Reports {
				if v.Reports[i].Status == "running" {
					v.Reports[i].Status = "queued"
					v.Reports[i].StartedAt = ""
				}
			}
			return v, nil
		})
		if err == nil || errors.Is(err, errCommitted) {
			break
		}
		if !wait(ctx, time.Second) {
			return
		}
	}
	s.ensure()
	var wg sync.WaitGroup
	for range 2 {
		wg.Add(1)
		go func() { defer wg.Done(); s.worker(ctx) }()
	}
	ticker := time.NewTicker(time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			wg.Wait()
			return
		case <-ticker.C:
			s.ensure()
		}
	}
}
func (s *Service) claim() (record, bool) {
	var found record
	ok := false
	err := s.repo.update(func(v diskState) (diskState, error) {
		for i, r := range v.Reports {
			if r.Status == "queued" {
				r.Status = "running"
				r.StartedAt = timestamp(s.now())
				v.Reports[i] = r
				found = r
				ok = true
				break
			}
		}
		return v, nil
	})
	return found, ok && (err == nil || errors.Is(err, errCommitted))
}
func (s *Service) worker(ctx context.Context) {
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		r, ok := s.claim()
		if ok {
			s.collect(ctx, r)
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
func wait(ctx context.Context, duration time.Duration) bool {
	timer := time.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-timer.C:
		return true
	}
}
func (s *Service) current(r record) bool {
	w, err := s.binding(r.Site, r.Strategy)
	return err == nil && matches(r, w, r.Strategy)
}
func (s *Service) collect(parent context.Context, r record) {
	if parent.Err() != nil {
		return
	}
	result := pulse.PageSpeedProjection{}
	if s.current(r) && s.source != nil {
		ctx, cancel := context.WithTimeout(parent, 3*time.Minute)
		defer cancel()
		result = s.source.StartPageSpeed(ctx, r.Site, r.Strategy)
		for result.Status == "running" && wait(ctx, time.Second) {
			if !s.current(r) {
				result = pulse.PageSpeedProjection{}
				break
			}
			result = s.source.GetPageSpeed(ctx, r.Site, r.Strategy)
		}
		if ctx.Err() != nil {
			result = pulse.PageSpeedProjection{}
		}
	}
	if parent.Err() != nil {
		return
	} // Restart recovers the durable running receipt.
	if !s.current(r) {
		result = pulse.PageSpeedProjection{}
	}
	// Retry only the persistence step; never repeat a provider call after a disk failure.
	for {
		err := s.finish(r, result)
		if err == nil || errors.Is(err, errCommitted) || !wait(parent, time.Second) {
			return
		}
	}
}
func (s *Service) finish(job record, result pulse.PageSpeedProjection) error {
	return s.repo.update(func(v diskState) (diskState, error) {
		for i, r := range v.Reports {
			if r.WebsiteID != job.WebsiteID || r.Strategy != job.Strategy || r.Revision != job.Revision || r.Site != job.Site || r.RequestedAt != job.RequestedAt || r.Status != "running" {
				continue
			}
			now := s.now()
			r.CompletedAt = timestamp(now)
			r.Status = "failed"
			r.Reason = "source_unavailable"
			r.NextRunAt = timestamp(now.Add(time.Hour))
			if validReason(result.Reason) {
				r.Reason = result.Reason
			}
			measured := time.Time{}
			if result.Report != nil {
				measured = parse(result.Report.FetchTime)
			}
			if result.Cache == nil && pulse.ValidPageSpeedProjection(result) && result.Status == "complete" && result.Site == r.Site && result.Strategy == r.Strategy && !measured.After(now.Add(5*time.Minute)) && (r.Successful == nil || !measured.Before(parse(r.Successful.Report.FetchTime))) {
				result.Cache = nil
				r.Successful = &result
				r.Status = "complete"
				r.Reason = ""
				next := measured.Add(FreshFor)
				if !next.After(now) {
					next = now.Add(time.Hour)
				}
				r.NextRunAt = timestamp(next)
			}
			v.Reports[i] = r
			break
		}
		return v, nil
	})
}
