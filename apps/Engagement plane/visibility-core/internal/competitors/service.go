// Package competitors owns explicit, site-scoped public homepage comparisons.
// It never connects a competitor's private account or estimates their traffic.
package competitors

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"regexp"
	"sync/atomic"
	"time"
)

var ErrInvalid = errors.New("invalid comparison context")
var ErrCapacity = errors.New("comparison limit reached")
var orgPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
var jobPattern = regexp.MustCompile(`^cmp_[a-f0-9]{32}$`)
var idPattern = regexp.MustCompile(`^web_[a-f0-9]{32}$`)

const maxBytes = 16 << 20
const MaxPerSite = 3
const MaxPairsPerTarget = 5

type Registry interface {
	OrganizationID() string
	Resolve(string, string) (websites.Record, error)
}
type Capture interface {
	Capture(context.Context, string, string) (siteaudit.CapturePage, error)
}
type Page struct {
	URL           string         `json:"url"`
	FinalURL      string         `json:"finalUrl"`
	Title         string         `json:"title"`
	Description   string         `json:"description"`
	Canonical     string         `json:"canonical"`
	HTTPStatus    int            `json:"httpStatus"`
	InternalLinks int            `json:"internalLinks"`
	ObservedAt    string         `json:"observedAt"`
	RunID         string         `json:"runId"`
	Fingerprint   string         `json:"fingerprint"`
	SEO           *siteaudit.SEO `json:"seo,omitempty"`
}
type ComparisonHistory struct {
	ObservedAt string `json:"observedAt"`
	Own        Page   `json:"own"`
	Competitor Page   `json:"competitor"`
	Changed    bool   `json:"changed"`
}
type Entry struct {
	OwnURL          string              `json:"ownUrl,omitempty"`
	TargetURL       string              `json:"targetUrl,omitempty"`
	History         []ComparisonHistory `json:"history,omitempty"`
	JobID           string              `json:"jobId"`
	WebsiteID       string              `json:"websiteId"`
	Site            string              `json:"site"`
	WebsiteRevision int64               `json:"websiteRevision"`
	Target          string              `json:"target"`
	Status          string              `json:"status"`
	RequestedAt     string              `json:"requestedAt"`
	LastAttemptAt   string              `json:"lastAttemptAt,omitempty"`
	NextRunAt       string              `json:"nextRunAt"`
	Reason          string              `json:"reason,omitempty"`
	Own             *Page               `json:"own,omitempty"`
	Competitor      *Page               `json:"competitor,omitempty"`
}
type Snapshot struct {
	WebsiteID       string  `json:"websiteId"`
	Site            string  `json:"site"`
	WebsiteRevision int64   `json:"websiteRevision"`
	Source          string  `json:"source"`
	Limit           int     `json:"limit"`
	Entries         []Entry `json:"entries"`
}
type diskState struct {
	Version        int     `json:"version"`
	OrganizationID string  `json:"organizationId"`
	Entries        []Entry `json:"entries"`
}

func cloneState(v diskState) diskState {
	raw, _ := json.Marshal(v)
	var out diskState
	_ = json.Unmarshal(raw, &out)
	return out
}
func stamp(v time.Time) string  { return v.UTC().Format(time.RFC3339Nano) }
func parsed(v string) time.Time { t, _ := time.Parse(time.RFC3339Nano, v); return t }
func validPage(site string, p *Page) bool {
	if p == nil {
		return false
	}
	// Reuse the audit owner's provenance, host, text, status and time validation.
	raw := siteaudit.CapturePage{URL: p.URL, FinalURL: p.FinalURL, Title: p.Title, Description: p.Description, Canonical: p.Canonical, HTTPStatus: p.HTTPStatus, ContentType: "text/html", ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, Links: []string{}, SEO: p.SEO}
	normalized, err := siteaudit.NormalizeCapture(site, p.URL, raw, parsed(p.ObservedAt), time.Time{})
	return err == nil && normalized.Canonical == p.Canonical && p.InternalLinks >= 0 && p.InternalLinks <= 100
}
func validState(v diskState) error {
	if v.Version != 1 || !orgPattern.MatchString(v.OrganizationID) || v.Entries == nil || len(v.Entries) > 960 {
		return ErrInvalid
	}
	seen := map[string]bool{}
	counts := map[string]map[string]int{}
	for _, e := range v.Entries {
		site, err := pulse.CanonicalSite(e.Site)
		target, te := pulse.CanonicalSite(e.Target)
		ownURL, targetURL := pair(e)
		key := e.WebsiteID + "|" + target + "|" + ownURL + "|" + targetURL
		if !jobPattern.MatchString(e.JobID) || err != nil || te != nil || site != e.Site || target != e.Target || pulse.GA4ReportingSiteAllowed(site, target) || !idPattern.MatchString(e.WebsiteID) || e.WebsiteRevision < 1 || seen[key] || !discovery.SafeURL(site, ownURL) || !discovery.SafeURL(target, targetURL) || parsed(e.RequestedAt).IsZero() || parsed(e.NextRunAt).IsZero() {
			return ErrInvalid
		}
		seen[key] = true
		if counts[e.WebsiteID] == nil {
			counts[e.WebsiteID] = map[string]int{}
		}
		counts[e.WebsiteID][target]++
		if len(counts[e.WebsiteID]) > MaxPerSite || counts[e.WebsiteID][target] > MaxPairsPerTarget {
			return ErrCapacity
		}
		if e.Status != "queued" && e.Status != "running" && e.Status != "complete" && e.Status != "failed" {
			return ErrInvalid
		}
		if e.LastAttemptAt != "" && parsed(e.LastAttemptAt).IsZero() {
			return ErrInvalid
		}
		if e.Status != "queued" && (e.LastAttemptAt == "" || parsed(e.LastAttemptAt).Before(parsed(e.RequestedAt))) {
			return ErrInvalid
		}
		if (e.Status == "complete" || e.Status == "failed") && parsed(e.NextRunAt).Before(parsed(e.LastAttemptAt)) {
			return ErrInvalid
		}
		if e.Status == "failed" {
			if e.Reason != "source_unavailable" {
				return ErrInvalid
			}
		} else if e.Reason != "" {
			return ErrInvalid
		}
		if e.Own != nil || e.Competitor != nil {
			if !validPage(site, e.Own) || !validPage(target, e.Competitor) {
				return ErrInvalid
			}
		}
		if e.Own != nil && (e.Own.URL != ownURL || e.Competitor.URL != targetURL) {
			return ErrInvalid
		}
		if len(e.History) > 20 {
			return ErrInvalid
		}
		for _, h := range e.History {
			if parsed(h.ObservedAt).IsZero() || !validPage(site, &h.Own) || !validPage(target, &h.Competitor) || h.Own.URL != ownURL || h.Competitor.URL != targetURL {
				return ErrInvalid
			}
		}
		if e.Status == "complete" && e.Own == nil {
			return ErrInvalid
		}
	}
	return nil
}

type Service struct {
	repo     *Repository
	registry Registry
	capture  Capture
	now      func() time.Time
	running  atomic.Bool
	wake     chan struct{}
}

func New(repo *Repository, r Registry, c Capture, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: r, capture: c, now: now, wake: make(chan struct{}, 1)}
}
func (s *Service) binding(id, site string, rev int64) (websites.Website, error) {
	if s.repo == nil || s.registry == nil || s.repo.org != s.registry.OrganizationID() || !idPattern.MatchString(id) {
		return websites.Website{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Website{}, err
	}
	if r.Website.Site != site || rev > 0 && r.Website.Revision != rev {
		return websites.Website{}, websites.ErrConflict
	}
	return r.Website, nil
}
func (s *Service) Get(id, site string) (Snapshot, error) {
	w, err := s.binding(id, site, 0)
	if err != nil {
		return Snapshot{}, err
	}
	v := Snapshot{WebsiteID: id, Site: site, WebsiteRevision: w.Revision, Source: "quarry_comparison", Limit: MaxPerSite, Entries: []Entry{}}
	for _, e := range s.repo.read().Entries {
		if e.WebsiteID == id && e.Site == site && e.WebsiteRevision == w.Revision {
			v.Entries = append(v.Entries, e)
		}
	}
	if _, err = s.binding(id, site, w.Revision); err != nil {
		return Snapshot{}, err
	}
	return v, nil
}
func (s *Service) Command(id, site string, rev int64, target, operation string, paths ...string) (Snapshot, error) {
	w, err := s.binding(id, site, rev)
	if err != nil {
		return Snapshot{}, err
	}
	ownURL, targetURL := site, target
	if len(paths) != 0 {
		if len(paths) != 2 {
			return Snapshot{}, ErrInvalid
		}
		ownURL, targetURL = paths[0], paths[1]
	}
	normalized, err := pulse.CanonicalSite(target)
	if err != nil || normalized != target || !discovery.SafeURL(site, ownURL) || !discovery.SafeURL(target, targetURL) || pulse.GA4ReportingSiteAllowed(site, target) || (operation != "refresh" && operation != "remove") {
		return Snapshot{}, ErrInvalid
	}
	err = s.repo.update(func(v diskState) (diskState, error) {
		kept := v.Entries[:0]
		var old *Entry
		targets := map[string]bool{}
		pairCount := 0
		for _, e := range v.Entries {
			if e.WebsiteID == id {
				if e.Site != site || e.WebsiteRevision != rev {
					continue
				}
				ownPage, targetPage := pair(e)
				if e.Target == target && ownPage == ownURL && targetPage == targetURL {
					copy := e
					old = &copy
					continue
				}
				targets[e.Target] = true
				if e.Target == target {
					pairCount++
				}
			}
			kept = append(kept, e)
		}
		v.Entries = kept
		if operation == "remove" {
			return v, nil
		}
		if !targets[target] && len(targets) >= MaxPerSite || pairCount >= MaxPairsPerTarget {
			return v, ErrCapacity
		}
		if old != nil && (old.Status == "queued" || old.Status == "running" || s.now().Sub(parsed(old.RequestedAt)) < time.Minute) {
			v.Entries = append(v.Entries, *old)
			return v, nil
		}
		e := Entry{JobID: newJobID(), WebsiteID: id, Site: site, WebsiteRevision: w.Revision, Target: target, OwnURL: ownURL, TargetURL: targetURL, Status: "queued", RequestedAt: stamp(s.now()), NextRunAt: stamp(s.now())}
		if old != nil {
			e.History = old.History
			e.Own = old.Own
			e.Competitor = old.Competitor
		}
		v.Entries = append(v.Entries, e)
		return v, nil
	})
	if err != nil && !errors.Is(err, errCommitted) {
		return Snapshot{}, err
	}
	select {
	case s.wake <- struct{}{}:
	default:
	}
	return s.Get(id, site)
}
func (s *Service) Run(ctx context.Context) {
	if !s.running.CompareAndSwap(false, true) {
		return
	}
	defer s.running.Store(false)
	for {
		err := s.repo.update(func(v diskState) (diskState, error) {
			for i := range v.Entries {
				if v.Entries[i].Status == "running" {
					v.Entries[i].Status = "queued"
				}
			}
			return v, nil
		})
		if err == nil || errors.Is(err, errCommitted) {
			break
		}
		if !wait(ctx) {
			return
		}
	}
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		job, ok := s.claim()
		if ok {
			s.collect(ctx, job)
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
func (s *Service) claim() (Entry, bool) {
	var out Entry
	found := false
	err := s.repo.update(func(v diskState) (diskState, error) {
		for i, e := range v.Entries {
			if e.Status == "queued" || e.Status != "running" && !parsed(e.NextRunAt).After(s.now()) {
				if _, err := s.binding(e.WebsiteID, e.Site, e.WebsiteRevision); err != nil {
					continue
				}
				var nonce [16]byte
				if _, err := rand.Read(nonce[:]); err != nil {
					return v, err
				}
				e.JobID = "cmp_" + hex.EncodeToString(nonce[:])
				e.Status = "running"
				e.Reason = ""
				e.LastAttemptAt = stamp(s.now())
				v.Entries[i] = e
				out = e
				found = true
				break
			}
		}
		return v, nil
	})
	return out, found && (err == nil || errors.Is(err, errCommitted))
}
func wait(ctx context.Context) bool {
	t := time.NewTimer(time.Second)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}
func (s *Service) capturePage(ctx context.Context, site, rawURL string, started time.Time) *Page {
	if s.capture == nil {
		return nil
	}
	raw, err := s.capture.Capture(ctx, site, rawURL)
	if err != nil {
		return nil
	}
	p, err := siteaudit.NormalizeCapture(site, rawURL, raw, s.now(), started)
	if err != nil {
		return nil
	}
	return &Page{URL: p.URL, FinalURL: p.FinalURL, Title: p.Title, Description: p.Description, Canonical: p.Canonical, HTTPStatus: p.HTTPStatus, InternalLinks: len(p.Links), ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, SEO: siteaudit.CloneSEO(p.SEO)}
}
func (s *Service) collect(parent context.Context, job Entry) {
	if parent.Err() != nil {
		return
	}
	if _, err := s.binding(job.WebsiteID, job.Site, job.WebsiteRevision); err != nil {
		return
	}
	ctx, cancel := context.WithTimeout(parent, 35*time.Second)
	defer cancel()
	started := s.now()
	ownURL, targetURL := pair(job)
	own := s.capturePage(ctx, job.Site, ownURL, started)
	var other *Page
	if _, err := s.binding(job.WebsiteID, job.Site, job.WebsiteRevision); err == nil && ctx.Err() == nil {
		other = s.capturePage(ctx, job.Target, targetURL, started)
	}
	if parent.Err() != nil {
		return
	}
	for {
		err := s.repo.update(func(v diskState) (diskState, error) {
			for i, e := range v.Entries {
				if e.WebsiteID != job.WebsiteID || e.Site != job.Site || e.Target != job.Target || e.WebsiteRevision != job.WebsiteRevision || e.RequestedAt != job.RequestedAt || e.Status != "running" || e.JobID != job.JobID {
					continue
				}
				if _, err := s.binding(e.WebsiteID, e.Site, e.WebsiteRevision); err != nil {
					return v, nil
				}
				e.Status = "failed"
				e.Reason = "source_unavailable"
				e.NextRunAt = stamp(s.now().Add(time.Hour))
				if own != nil && other != nil {
					changed := e.Own != nil && (contentChanged(e.Own, own) || contentChanged(e.Competitor, other))
					e.History = append([]ComparisonHistory{{ObservedAt: stamp(s.now()), Own: *own, Competitor: *other, Changed: changed}}, e.History...)
					if len(e.History) > 20 {
						e.History = e.History[:20]
					}
					e.Status = "complete"
					e.Reason = ""
					e.Own = own
					e.Competitor = other
					e.NextRunAt = stamp(s.now().Add(24 * time.Hour))
				}
				v.Entries[i] = e
				break
			}
			return v, nil
		})
		if err == nil || errors.Is(err, errCommitted) || !wait(parent) {
			return
		}
	}
}

func newJobID() string {
	var v [16]byte
	if _, err := rand.Read(v[:]); err != nil {
		panic("cryptographic entropy unavailable")
	}
	return "cmp_" + hex.EncodeToString(v[:])
}

func pair(e Entry) (string, string) {
	a, b := e.OwnURL, e.TargetURL
	if a == "" {
		a = e.Site
	}
	if b == "" {
		b = e.Target
	}
	return a, b
}
func contentChanged(a, b *Page) bool {
	if a == nil || b == nil {
		return true
	}
	return a.Fingerprint != b.Fingerprint || a.HTTPStatus != b.HTTPStatus || a.Title != b.Title || a.Description != b.Description || a.Canonical != b.Canonical
}
