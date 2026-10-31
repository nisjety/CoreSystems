package sitesources

import (
	"context"
	"errors"
	"sync"
	"time"
)

// Run reconciles only persisted explicit Algolia mappings; dashboard GETs do not
// enroll work. Both workers and in-flight requests are joined on shutdown.
func (s *Service) Run(ctx context.Context) {
	if s.collections == nil || !s.collectionRunning.CompareAndSwap(false, true) {
		return
	}
	defer s.collectionRunning.Store(false)
	s.mu.Lock()
	entries := append([]collectionJob{}, s.collections.state.Entries...)
	for i, j := range entries {
		if j.Status == "syncing" {
			j.Status = "queued"
			j.NextRunAt = stamp(s.now())
			j.Reason = "provider_unavailable"
			entries[i] = j
		}
	}
	err := s.saveCollectionsLocked(entries)
	s.mu.Unlock()
	for err != nil && !errors.Is(err, errCommitted) {
		timer := time.NewTimer(time.Second)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
		s.mu.Lock()
		err = s.saveCollectionsLocked(entries)
		s.mu.Unlock()
	}
	if ctx.Err() != nil {
		return
	}
	var wg sync.WaitGroup
	for range 2 {
		wg.Add(1)
		go func() { defer wg.Done(); s.collectionWorker(ctx) }()
	}
	tick := time.NewTicker(time.Second)
	defer tick.Stop()
	for {
		select {
		case <-ctx.Done():
			wg.Wait()
			return
		case <-tick.C:
			_ = s.reconcileCollections() // The writer owns bounded sanitized diagnostics.
			s.notifyCollection()
		}
	}
}
func (s *Service) collectionWorker(ctx context.Context) {
	tick := time.NewTicker(time.Second)
	defer tick.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		if j, ok := s.claimCollection(); ok {
			s.executeCollection(ctx, j)
			continue
		}
		select {
		case <-ctx.Done():
			return
		case <-s.collectionWake:
		case <-tick.C:
		}
	}
}
func (s *Service) executeCollection(parent context.Context, j collectionJob) {
	ctx, cancel := context.WithTimeout(parent, 40*time.Second)
	defer cancel()
	s.mu.Lock()
	s.collectionCancels[j.RunID] = cancel
	s.mu.Unlock()
	defer func() { s.mu.Lock(); delete(s.collectionCancels, j.RunID); s.mu.Unlock() }()
	e := jobEnrollment(j)
	if !s.currentEnrollment(e) {
		s.finishCollection(j, Report{}, "context_changed")
		return
	}
	before, err := s.collectionAccess(ctx, e)
	if err != nil {
		s.finishCollection(j, Report{}, "provider_unavailable")
		return
	}
	if !before.Active {
		s.finishCollection(j, Report{}, "reconnect_required")
		return
	}
	report, err := s.Sync(ctx, j.WebsiteID, j.Site, j.WebsiteRevision, "algolia")
	if errors.Is(err, ErrCooldown) {
		s.finishCollection(j, Report{}, "cooldown")
		return
	}
	if err != nil {
		s.finishCollection(j, Report{}, "provider_unavailable")
		return
	}
	after, err := s.collectionAccess(ctx, e)
	if err != nil || !after.Active || after.Generation != before.Generation || ctx.Err() != nil {
		s.finishCollection(j, Report{}, "reconnect_required")
		return
	}
	s.finishCollection(j, report, "")
}
func (s *Service) finishCollection(j collectionJob, report Report, why string) {
	if !s.currentEnrollment(jobEnrollment(j)) {
		return
	}
	now := s.now().UTC()
	j.Status = "needs_attention"
	j.Reason = why
	delay := time.Minute
	if j.Attempts > 1 {
		delay = 5 * time.Minute
	}
	if j.Attempts > 2 {
		delay = 30 * time.Minute
	}
	if j.Attempts > 3 {
		delay = time.Hour
	}
	j.NextRunAt = stamp(now.Add(delay))
	switch why {
	case "cooldown":
		j.Status = "queued"
		j.Reason = ""
		j.NextRunAt = stamp(now.Add(time.Minute))
	case "reconnect_required":
		j.Status = "reconnect_required"
		j.NextRunAt = stamp(now.Add(time.Hour))
	case "":
		if report.Status == "not_configured" {
			j.Reason = "not_configured"
		} else if report.Status != "connected" {
			j.Reason = "provider_unavailable"
		} else {
			j.LastSuccessAt = report.ObservedAt
			if report.Algolia != nil && successful(report.Algolia.Analytics.Status) && connectedTerms(report.Algolia.TopSearches) && connectedTerms(report.Algolia.NoResultTerms) {
				j.Status = "ready"
				j.Reason = ""
				j.Attempts = 0
				j.NextRunAt = stamp(nextCollection(now, report))
			} else {
				j.Reason = "partial_report"
				j.NextRunAt = stamp(now.Add(time.Hour))
			}
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	entries := append([]collectionJob{}, s.collections.state.Entries...)
	for i, current := range entries {
		if current.RunID == j.RunID && current.Status == "syncing" {
			entries[i] = j
			_ = s.saveCollectionsLocked(entries)
			return
		}
	}
}
