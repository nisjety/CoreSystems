package siteaudit

import (
	"context"
	"errors"
	"log"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Service struct {
	repo            *Repository
	registry        Registry
	capture         Capture
	now             func() time.Time
	mu              sync.Mutex
	active          int
	slots           chan struct{}
	pendingTerminal map[string]Snapshot
}

func New(repo *Repository, registry Registry, capture Capture, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: registry, capture: capture, now: now, slots: make(chan struct{}, 2), pendingTerminal: map[string]Snapshot{}}
}
func (s *Service) binding(id, site string, revision int64) (websites.Website, error) {
	if s.registry == nil || s.repo == nil || s.repo.org != s.registry.OrganizationID() || !websitePattern.MatchString(id) || !discovery.SafeURL(site, site) {
		return websites.Website{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Website{}, err
	}
	if r.Website.Site != site || revision > 0 && revision != r.Website.Revision {
		return websites.Website{}, ErrConflict
	}
	return r.Website, nil
}
func (s *Service) empty(w websites.Website) Snapshot {
	v := Snapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "quarry_capture", RuleVersion: RuleVersion, Status: "idle", Limit: Limit, Partial: true, Pages: []Page{}, Findings: []Finding{}, History: []Receipt{}}
	if s.capture == nil {
		v.Status = "failed"
		v.Reason = "not_configured"
	}
	return v
}
func history(runs []Snapshot, revision int64) []Receipt {
	out := []Receipt{}
	for _, r := range runs {
		if r.WebsiteRevision == revision {
			out = append(out, Receipt{ID: r.ID, Status: r.Status, StartedAt: r.StartedAt, CompletedAt: r.CompletedAt, PageCount: len(r.Pages), FindingCount: len(r.Findings)})
		}
	}
	return out
}
func (s *Service) Get(id, site string) (Snapshot, error) {
	w, err := s.binding(id, site, 0)
	if err != nil {
		return Snapshot{}, err
	}
	runs := s.repo.list(id, site)
	for _, v := range runs {
		if v.WebsiteRevision == w.Revision {
			v.History = history(runs, w.Revision)
			return v, nil
		}
	}
	return s.empty(w), nil
}
func (s *Service) GetRun(id, site, auditID string) (Snapshot, error) {
	w, err := s.binding(id, site, 0)
	if err != nil {
		return Snapshot{}, err
	}
	if !auditPattern.MatchString(auditID) {
		return Snapshot{}, ErrInvalid
	}
	runs := s.repo.list(id, site)
	for _, v := range runs {
		if v.ID == auditID {
			if v.WebsiteRevision != w.Revision {
				return Snapshot{}, ErrConflict
			}
			v.History = history(runs, w.Revision)
			return v, nil
		}
	}
	return Snapshot{}, ErrNotFound
}
func (s *Service) Start(id, site string, revision int64) (Snapshot, error) {
	if revision < 1 {
		return Snapshot{}, ErrInvalid
	}
	w, err := s.binding(id, site, revision)
	if err != nil {
		return Snapshot{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.flushTerminal(); err != nil {
		return Snapshot{}, err
	}
	if s.capture == nil {
		return s.empty(w), nil
	}
	runs := s.repo.list(id, site)
	for _, v := range runs {
		if v.WebsiteRevision == revision {
			if v.Status == "running" {
				v.History = history(runs, revision)
				return v, nil
			}
			if s.now().Sub(mustTime(v.StartedAt)) < time.Minute {
				return Snapshot{}, ErrCapacity
			}
			break
		}
	}
	if s.active >= 2 {
		return Snapshot{}, ErrCapacity
	}
	v := s.empty(w)
	v.ID = newID("audit_")
	v.Status = "running"
	v.StartedAt = stamp(s.now())
	err = s.repo.save(v)
	if err != nil && !errors.Is(err, errCommitted) {
		return Snapshot{}, err
	}
	s.active++
	go s.run(copySnapshot(v))
	// An ambiguous directory sync must not strand a run whose running receipt
	// was already atomically renamed. The caller can read status to confirm it.
	return v, err
}
func (s *Service) run(v Snapshot) {
	defer func() { s.mu.Lock(); s.active--; s.mu.Unlock() }()
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Minute)
	defer cancel()
	var pages []discovery.Page
	var err error
	var inventory *discovery.MapCoverage
	if detailed, ok := s.capture.(discovery.DetailedCapture); ok {
		pages, inventory, err = detailed.MapWithCoverage(ctx, v.Site)
	} else {
		pages, err = s.capture.Map(ctx, v.Site)
	}
	if err != nil {
		v.Status = "failed"
		v.Reason = reason(err)
	} else {
		v.Pages, v.Coverage = s.capturePages(ctx, v.Site, v.StartedAt, pages, func() bool { _, err := s.binding(v.WebsiteID, v.Site, v.WebsiteRevision); return err == nil })
		v.Coverage.Sitemap = inventory
		if inventory != nil && inventory.Truncated {
			v.Coverage.InventoryTruncated = true
		}
		v.Pages, v.Findings = evaluate(v.Pages)
		v.Status = "complete"
		checked := 0
		for _, p := range v.Pages {
			if p.Status == "checked" {
				checked++
			}
		}
		if checked == 0 {
			v.Status = "failed"
			v.Reason = failedPagesReason(v.Pages)
		}
	}
	v.CompletedAt = stamp(s.now())
	if _, err = s.binding(v.WebsiteID, v.Site, v.WebsiteRevision); err != nil {
		v.Status = "failed"
		v.Reason = "context_changed"
		v.Pages = []Page{}
		v.Findings = []Finding{}
		v.Coverage = nil
	}
	if err = s.repo.save(v); err != nil {
		log.Printf("marketing-core: audit %s result storage durability could not be confirmed", v.ID)
		if !errors.Is(err, errCommitted) {
			s.retainTerminal(v)
		}
	}
}
func failedPagesReason(pages []Page) string {
	if len(pages) == 0 {
		return "source_unavailable"
	}
	first := pages[0].Reason
	if first == "" || !validReason(first) {
		return "source_unavailable"
	}
	for _, p := range pages {
		if p.Status != "failed" || p.Reason != first {
			return "source_unavailable"
		}
	}
	return first
}
func (s *Service) capturePages(ctx context.Context, site, startedAt string, candidates []discovery.Page, allowed ...func() bool) ([]Page, *CrawlCoverage) {
	urls := []string{site}
	seen := map[string]bool{site: true}
	coverage := &CrawlCoverage{}
	for _, p := range candidates {
		if discovery.SafeURL(site, p.URL) && !seen[p.URL] {
			if len(urls) >= 1000 {
				coverage.InventoryTruncated = true
				break
			}
			urls = append(urls, p.URL)
			seen[p.URL] = true
		}
	}
	results := []Page{}
	for cursor := 0; cursor < len(urls) && len(results) < Limit && ctx.Err() == nil; {
		if len(allowed) > 0 && !allowed[0]() {
			break
		}
		end := min(cursor+2, len(urls), Limit)
		batch := make([]Page, end-cursor)
		var workers sync.WaitGroup
		for i := cursor; i < end; i++ {
			workers.Add(1)
			go func(index int) {
				defer workers.Done()
				batch[index-cursor] = s.captureOne(ctx, site, startedAt, urls[index])
			}(i)
		}
		workers.Wait()
		for _, p := range batch {
			results = append(results, p)
			coverage.ExtractionPartial = coverage.ExtractionPartial || p.ExtractionPartial
			if p.Reason == "quota_limited" || p.Reason == "access_denied" {
				coverage.StopReason = p.Reason
			}
			for _, link := range p.Links {
				if discovery.SafeURL(site, link) && !seen[link] {
					if len(urls) >= 1000 {
						coverage.InventoryTruncated = true
						continue
					}
					urls = append(urls, link)
					seen[link] = true
				}
			}
		}
		cursor = end
		if coverage.StopReason != "" {
			break
		}
	}
	coverage.Discovered = len(urls)
	coverage.Attempted = len(results)
	coverage.Pending = len(urls) - len(results)
	coverage.LimitReached = len(results) >= Limit && coverage.Pending > 0
	if ctx.Err() != nil {
		coverage.StopReason = "timeout"
	}
	for _, p := range results {
		if p.Status == "checked" {
			coverage.Checked++
		} else {
			coverage.Failed++
		}
	}
	return results, coverage
}
func (s *Service) captureOne(ctx context.Context, site, startedAt, raw string) Page {
	fail := func(why string) Page {
		return Page{URL: raw, Status: "failed", Reason: why, Links: []string{}, Checks: []Check{}}
	}
	select {
	case s.slots <- struct{}{}:
		defer func() { <-s.slots }()
	case <-ctx.Done():
		return fail("timeout")
	}
	deadline, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	p, err := s.capture.Capture(deadline, site, raw)
	if err != nil {
		return fail(reason(err))
	}
	result, err := NormalizeCapture(site, raw, p, s.now(), mustTime(startedAt))
	if err != nil {
		return fail(reason(err))
	}
	return result
}

// NormalizeCapture is shared by bounded audits and public homepage comparisons.
// Quarry owns network execution; both consumers validate the same evidence.
func NormalizeCapture(site, raw string, p CapturePage, now, notBefore time.Time) (Page, error) {
	if !validCapture(site, raw, p, now) || mustTime(p.ObservedAt).Before(notBefore) {
		return Page{}, &CaptureError{Reason: "invalid_source"}
	}
	if p.HTTPStatus < 400 && !strings.HasPrefix(strings.ToLower(p.ContentType), "text/html") && !strings.HasPrefix(strings.ToLower(p.ContentType), "application/xhtml+xml") {
		return Page{}, &CaptureError{Reason: "unsupported_content"}
	}
	links := []string{}
	seen := map[string]bool{}
	for _, link := range p.Links {
		if discovery.SafeURL(site, link) && !seen[link] {
			links = append(links, link)
			seen[link] = true
		}
	}
	canonical := ""
	if discovery.SafeURL(site, p.Canonical) {
		canonical = p.Canonical
	}
	return Page{URL: raw, FinalURL: p.FinalURL, HTTPStatus: p.HTTPStatus, Status: "checked", Title: strings.TrimSpace(p.Title), Description: strings.TrimSpace(p.Description), Canonical: canonical, ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, Links: links, Checks: []Check{}, SEO: copySEO(p.SEO), ExtractionPartial: p.Partial}, nil
}
func reason(err error) string {
	var capture *CaptureError
	if errors.As(err, &capture) && validReason(capture.Reason) && capture.Reason != "" {
		return capture.Reason
	}
	var mapError *discovery.CaptureError
	if errors.As(err, &mapError) && validReason(mapError.Reason) && mapError.Reason != "" {
		return mapError.Reason
	}
	if errors.Is(err, context.DeadlineExceeded) || errors.Is(err, context.Canceled) {
		return "timeout"
	}
	return "source_unavailable"
}
