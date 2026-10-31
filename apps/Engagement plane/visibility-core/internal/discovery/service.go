// Package discovery owns an ephemeral, partial URL inventory. Quarry owns all
// website network access; these results are not page audits or health scores.
package discovery

import (
	"context"
	"errors"
	"regexp"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const Limit = 250

var ErrCooldown = errors.New("discovery cooldown or capacity limit")
var websiteID = regexp.MustCompile(`^web_[a-f0-9]{32}$`)

type Page struct {
	URL   string `json:"url"`
	Title string `json:"title,omitempty"`
}
type Snapshot struct {
	WebsiteID       string       `json:"websiteId"`
	Site            string       `json:"site"`
	WebsiteRevision int64        `json:"websiteRevision"`
	Source          string       `json:"source"`
	Status          string       `json:"status"`
	Limit           int          `json:"limit"`
	Partial         bool         `json:"partial"`
	Pages           []Page       `json:"pages"`
	DiscoveredAt    string       `json:"discoveredAt,omitempty"`
	Reason          string       `json:"reason,omitempty"`
	Coverage        *MapCoverage `json:"coverage,omitempty"`
}
type MapCoverage struct {
	SitemapFiles   int  `json:"sitemapFiles"`
	SitemapValid   int  `json:"sitemapValid"`
	SitemapPartial bool `json:"sitemapPartial"`
	Candidates     int  `json:"candidates"`
	RobotsFiltered int  `json:"robotsFiltered"`
	Truncated      bool `json:"truncated"`
}
type DetailedCapture interface {
	MapWithCoverage(context.Context, string) ([]Page, *MapCoverage, error)
}
type Registry interface {
	Resolve(string, string) (websites.Record, error)
}
type Capture interface {
	Map(context.Context, string) ([]Page, error)
}
type CaptureError struct{ Reason string }

func (e *CaptureError) Error() string { return "Quarry discovery unavailable" }

type entry struct {
	snapshot Snapshot
	started  time.Time
}
type Service struct {
	registry Registry
	capture  Capture
	now      func() time.Time
	mu       sync.Mutex
	entries  map[string]*entry
	active   int
}

func New(registry Registry, capture Capture, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{registry: registry, capture: capture, now: now, entries: map[string]*entry{}}
}
func (s *Service) resolve(id, site string) (websites.Website, error) {
	if !websiteID.MatchString(id) || s.registry == nil {
		return websites.Website{}, websites.ErrInvalid
	}
	record, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Website{}, err
	}
	if site == "" || record.Website.Site != site {
		return websites.Website{}, websites.ErrConflict
	}
	return record.Website, nil
}
func (s *Service) empty(w websites.Website) Snapshot {
	v := Snapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "quarry_map", Status: "idle", Limit: Limit, Partial: true, Pages: []Page{}}
	if s.capture == nil {
		v.Status = "failed"
		v.Reason = "not_configured"
	}
	return v
}
func copySnapshot(v Snapshot) Snapshot {
	v.Pages = append([]Page{}, v.Pages...)
	if v.Coverage != nil {
		c := *v.Coverage
		v.Coverage = &c
	}
	return v
}

// Get never initiates a network call. Registry revisions invalidate old reports.
func (s *Service) Get(id, site string) (Snapshot, error) {
	w, err := s.resolve(id, site)
	if err != nil {
		return Snapshot{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.entries[id]; e != nil && e.snapshot.Site == site && e.snapshot.WebsiteRevision == w.Revision {
		return copySnapshot(e.snapshot), nil
	}
	return s.empty(w), nil
}

// Start issues at most one map per site/minute and two maps per deployment.
// Captures have a fixed deadline independent of browser disconnects. They are
// deliberately ephemeral: restarting Marketing removes discovery snapshots.
func (s *Service) Start(id, site string, revision int64) (Snapshot, error) {
	w, err := s.resolve(id, site)
	if err != nil {
		return Snapshot{}, err
	}
	if revision < 1 || revision != w.Revision {
		return Snapshot{}, websites.ErrConflict
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.capture == nil {
		return s.empty(w), nil
	}
	old := s.entries[id]
	if old != nil && old.snapshot.Site == site && old.snapshot.WebsiteRevision == revision {
		if old.snapshot.Status == "running" {
			return copySnapshot(old.snapshot), nil
		}
		if s.now().Sub(old.started) < time.Minute {
			return Snapshot{}, ErrCooldown
		}
	}
	if s.active >= 2 || (old == nil && len(s.entries) >= 100) {
		return Snapshot{}, ErrCooldown
	}
	v := s.empty(w)
	v.Status = "running"
	e := &entry{snapshot: v, started: s.now()}
	s.entries[id] = e
	s.active++
	go s.run(e)
	return copySnapshot(v), nil
}
func (s *Service) run(e *entry) {
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	var pages []Page
	var coverage *MapCoverage
	var err error
	if detailed, ok := s.capture.(DetailedCapture); ok {
		pages, coverage, err = detailed.MapWithCoverage(ctx, e.snapshot.Site)
	} else {
		pages, err = s.capture.Map(ctx, e.snapshot.Site)
	}
	next := copySnapshot(e.snapshot)
	next.Status = "failed"
	next.Reason = "source_unavailable"
	var failure *CaptureError
	if errors.As(err, &failure) {
		switch failure.Reason {
		case "access_denied", "policy_blocked", "quota_limited", "invalid_source":
			next.Reason = failure.Reason
		}
	}
	if err == nil && ctx.Err() == nil {
		next.Status = "complete"
		next.Reason = ""
		next.Pages = sanitizePages(next.Site, pages)
		next.Coverage = coverage
		next.DiscoveredAt = s.now().UTC().Format(time.RFC3339Nano)
	}
	w, resolveErr := s.resolve(next.WebsiteID, next.Site)
	s.mu.Lock()
	defer s.mu.Unlock()
	s.active--
	if s.entries[next.WebsiteID] != e {
		return
	}
	if resolveErr != nil || w.Revision != next.WebsiteRevision {
		delete(s.entries, next.WebsiteID)
		return
	}
	s.entries[next.WebsiteID] = &entry{snapshot: next, started: e.started}
}
