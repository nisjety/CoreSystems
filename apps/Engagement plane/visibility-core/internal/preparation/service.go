package preparation

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
	return &Service{repo: repo, registry: registry, runner: runner, now: now, timeout: jobTimeout, wake: make(chan struct{}, 2)}
}
func (s *Service) binding(id, site string, revision int64) (websites.Website, error) {
	canonical, err := pulse.CanonicalSite(site)
	if s.repo == nil || s.registry == nil || s.repo.org != s.registry.OrganizationID() || !websiteIDPattern.MatchString(id) || err != nil || canonical != site {
		return websites.Website{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Website{}, err
	}
	if r.Website.ID != id || r.Website.Site != site || r.Website.Revision < 1 || revision > 0 && r.Website.Revision != revision {
		return websites.Website{}, ErrConflict
	}
	return r.Website, nil
}

// Get reads receipts only. It never starts work or returns another revision's result.
func (s *Service) Get(id, site string) (Snapshot, error) {
	w, err := s.binding(id, site, 0)
	if err != nil {
		return Snapshot{}, err
	}
	result := empty(w)
	for _, r := range s.repo.read().Jobs {
		if r.WebsiteID == id && r.Site == site && r.WebsiteRevision == w.Revision {
			result = clone(r.Snapshot)
		}
	}
	if _, err = s.binding(id, site, w.Revision); err != nil {
		return Snapshot{}, err
	}
	return result, nil
}

// Queue records an explicit preparation request before any provider work begins.
func (s *Service) Queue(id, site string, revision int64) (Snapshot, error) {
	if revision < 1 {
		return Snapshot{}, ErrInvalid
	}
	w, err := s.binding(id, site, revision)
	if err != nil {
		return Snapshot{}, err
	}
	var result Snapshot
	err = s.repo.update(func(state diskState) (diskState, error) {
		index := -1
		for i, r := range state.Jobs {
			if r.WebsiteID == id {
				index = i
				if r.Site == site && r.WebsiteRevision == revision {
					if r.Status == "queued" || r.Status == "running" {
						result = clone(r.Snapshot)
						return state, nil
					}
					completed, _ := parseTime(r.CompletedAt)
					if s.now().Sub(completed) < time.Minute {
						return state, ErrCooldown
					}
				}
				break
			}
		}
		if index < 0 && len(state.Jobs) >= maxJobs {
			return state, ErrCapacity
		}
		result = empty(w)
		result.Status = "queued"
		result.JobID = newID()
		result.QueuedAt = stamp(s.now())
		next := record{Snapshot: clone(result)}
		if index < 0 {
			state.Jobs = append(state.Jobs, next)
		} else {
			state.Jobs[index] = next
		}
		return state, nil
	})
	if err != nil {
		return Snapshot{}, err
	}
	select {
	case s.wake <- struct{}{}:
	default:
	}
	return result, nil
}

// Run owns two bounded workers and waits for their exit. A second Run is a no-op.
func (s *Service) Run(ctx context.Context) {
	if s.repo == nil || !s.running.CompareAndSwap(false, true) {
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
		if err := fn(); err == nil || errors.Is(err, errCommitted) {
			return true
		}
		timer := time.NewTimer(250 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return false
		case <-timer.C:
		}
	}
}
func (s *Service) recover() error {
	return s.repo.update(func(state diskState) (diskState, error) {
		for i, r := range state.Jobs {
			if r.Status != "running" {
				continue
			}
			for j, step := range r.Steps {
				if step.Status == "running" {
					r.Steps[j] = Step{Key: step.Key, Status: "failed", Reason: "interrupted", ObservedAt: stamp(s.now())}
				}
			}
			r.Status = "queued"
			if r.Attempts >= 3 {
				for j, step := range r.Steps {
					if step.Status == "queued" {
						r.Steps[j] = Step{Key: step.Key, Status: "unavailable", Reason: "interrupted", ObservedAt: stamp(s.now())}
					}
				}
				finish(&r, s.now())
			} else {
				r.StartedAt = ""
			}
			state.Jobs[i] = r
		}
		return state, nil
	})
}
func (s *Service) worker(ctx context.Context) {
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		job, ok := s.claim()
		if ok {
			s.execute(ctx, job)
			continue
		}
		select {
		case <-ctx.Done():
			return
		case <-s.wake:
		case <-ticker.C:
		}
	}
}
func (s *Service) claim() (record, bool) {
	queued := false
	for _, r := range s.repo.read().Jobs {
		queued = queued || r.Status == "queued"
	}
	if !queued {
		return record{}, false
	}
	var result record
	found := false
	err := s.repo.update(func(state diskState) (diskState, error) {
		for i, r := range state.Jobs {
			if r.Status != "queued" {
				continue
			}
			r.Status = "running"
			r.Attempts++
			if r.StartedAt == "" {
				r.StartedAt = stamp(s.now())
			}
			state.Jobs[i] = r
			result = r
			found = true
			break
		}
		return state, nil
	})
	return result, found && (err == nil || errors.Is(err, errCommitted))
}
func (s *Service) save(ctx context.Context, job record) bool {
	return s.retry(ctx, func() error {
		return s.repo.update(func(state diskState) (diskState, error) {
			for i, r := range state.Jobs {
				if r.JobID == job.JobID {
					state.Jobs[i] = job
					return state, nil
				}
			}
			return state, nil
		})
	})
}
func (s *Service) execute(parent context.Context, job record) {
	ctx, cancel := context.WithTimeout(parent, s.timeout)
	defer cancel()
	for i, step := range job.Steps {
		if step.Status != "queued" {
			continue
		}
		w, err := s.binding(job.WebsiteID, job.Site, job.WebsiteRevision)
		if err != nil {
			s.abort(parent, job, "context_changed")
			return
		}
		if ctx.Err() != nil {
			if parent.Err() == nil {
				s.abort(parent, job, "timeout")
			}
			return
		}
		job.Steps[i] = Step{Key: step.Key, Status: "running"}
		if !s.save(parent, job) {
			return
		}
		// The source deadline bounds network work, not recovery of a receipt.
		// A storage outage may outlast it; never start another source afterward.
		if ctx.Err() != nil {
			s.abort(parent, job, "timeout")
			return
		}
		result := StepResult{Status: "unavailable", Reason: "not_configured"}
		if s.runner != nil {
			result, err = s.runner.Run(ctx, w, step.Key)
			if err != nil || !validResult(result) {
				result = StepResult{Status: "failed", Reason: "source_unavailable"}
			}
		}
		if ctx.Err() != nil {
			result = StepResult{Status: "failed", Reason: "timeout"}
		}
		if _, err = s.binding(job.WebsiteID, job.Site, job.WebsiteRevision); err != nil {
			s.abort(parent, job, "context_changed")
			return
		}
		job.Steps[i] = Step{Key: step.Key, Status: result.Status, Reason: result.Reason, ObservedAt: stamp(s.now())}
		if !s.save(parent, job) {
			return
		}
	}
	if _, err := s.binding(job.WebsiteID, job.Site, job.WebsiteRevision); err != nil {
		s.abort(parent, job, "context_changed")
		return
	}
	finish(&job, s.now())
	s.save(parent, job)
}
func (s *Service) abort(ctx context.Context, job record, reason string) {
	for i, step := range job.Steps {
		if step.Status == "queued" || step.Status == "running" {
			job.Steps[i] = Step{Key: step.Key, Status: "failed", Reason: reason, ObservedAt: stamp(s.now())}
		}
	}
	finish(&job, s.now())
	s.save(ctx, job)
}
func finish(job *record, now time.Time) {
	complete := 0
	for _, step := range job.Steps {
		if step.Status == "complete" {
			complete++
		}
	}
	job.Status = "failed"
	if complete == 3 {
		job.Status = "complete"
	} else if complete > 0 {
		job.Status = "partial"
	}
	job.CompletedAt = stamp(now)
}
