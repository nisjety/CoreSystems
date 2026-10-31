package webvitals

import (
	"context"
	"fmt"
	"log"
	"math"
	"sort"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Service struct {
	mu        sync.Mutex
	store     *repository
	registry  Registry
	collector Collector
	now       func() time.Time
	attempts  map[string]time.Time
	running   map[string]bool
}

func New(path, org string, registry Registry, collector Collector, now func() time.Time) (*Service, error) {
	if registry == nil || registry.OrganizationID() != org {
		return nil, ErrInvalid
	}
	store, err := open(path, org)
	if err != nil {
		return nil, err
	}
	if now == nil {
		now = time.Now
	}
	if err = store.prune(now().UTC()); err != nil {
		return nil, err
	}
	return &Service{store: store, registry: registry, collector: collector, now: now, attempts: map[string]time.Time{}, running: map[string]bool{}}, nil
}
func (s *Service) resolve(id, site, env string) (websites.Record, error) {
	if !idPattern.MatchString(id) || !environment(env) {
		return websites.Record{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Record{}, err
	}
	return r, nil
}
func (s *Service) Get(id, site, env string) (Report, error) {
	r, err := s.resolve(id, site, env)
	if err != nil {
		return Report{}, err
	}
	now := s.now().UTC()
	s.mu.Lock()
	defer s.mu.Unlock()
	if err = s.store.prune(now); err != nil {
		return Report{}, err
	}
	return s.report(r.Website, env, now), nil
}

// Run only expires local payloads. It does not refresh providers or create
// observations; collection remains an explicit, permission-checked action.
func (s *Service) Run(ctx context.Context) {
	ticker := time.NewTicker(time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.mu.Lock()
			err := s.store.prune(s.now().UTC())
			s.mu.Unlock()
			if err != nil {
				log.Printf("marketing-core: first-party measurement retention cleanup unavailable")
			}
		}
	}
}
func (s *Service) Sync(ctx context.Context, id, site string, revision int64, env string) (Report, error) {
	r, err := s.resolve(id, site, env)
	if err != nil {
		return Report{}, err
	}
	if revision != r.Website.Revision {
		return Report{}, websites.ErrConflict
	}
	key := id + ":" + env
	now := s.now().UTC()
	s.mu.Lock()
	if s.running[key] || !s.attempts[key].IsZero() && now.Sub(s.attempts[key]) < time.Minute {
		s.mu.Unlock()
		return Report{}, ErrCooldown
	}
	if len(s.attempts) >= maxEntries && s.attempts[key].IsZero() {
		s.mu.Unlock()
		return Report{}, ErrCapacity
	}
	s.attempts[key] = now
	s.running[key] = true
	s.mu.Unlock()
	defer func() { s.mu.Lock(); delete(s.running, key); s.mu.Unlock() }()
	if s.collector == nil {
		return Report{}, ErrInvalid
	}
	bounded, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	data, err := s.collector.Collect(bounded, r, env)
	if err != nil {
		return Report{}, err
	}
	current, err := s.resolve(id, site, env)
	if err != nil {
		return Report{}, err
	}
	if current.Website.Revision != revision {
		return Report{}, websites.ErrConflict
	}
	now = s.now().UTC()
	next, err := normalize(data, current.Website, env, now)
	if err != nil {
		return Report{}, fmt.Errorf("%w: %w", ErrUnavailable, err)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err = s.store.save(next, now); err != nil {
		return Report{}, err
	}
	return s.report(current.Website, env, now), nil
}
func (s *Service) report(w websites.Website, env string, now time.Time) Report {
	v := Report{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "first_party_web_vitals", Environment: env, Status: "not_configured", Window: Window{windowStart(now).Format("2006-01-02"), now.Format("2006-01-02")}, AsOf: stamp(now), RetentionDays: RetentionDays, Groups: []Group{{Viewport: "narrow"}, {Viewport: "wide"}}}
	for _, entry := range s.store.state.Entries {
		if entry.WebsiteID != w.ID || entry.Site != w.Site || entry.WebsiteRevision != w.Revision || entry.Environment != env || entry.Status != "connected" {
			continue
		}
		v.Status = "awaiting_samples"
		v.LastCollectedAt = entry.ObservedAt
		v.ReportingOrigin = entry.ReportingOrigin
		for i, g := range v.Groups {
			values := map[string][]float64{"LCP": {}, "INP": {}, "CLS": {}}
			for _, sample := range entry.Samples {
				at, _ := time.Parse(time.RFC3339Nano, sample.ReceivedAt)
				if sample.Viewport == g.Viewport && !at.Before(windowStart(now)) && !at.After(now) {
					values[sample.Metric] = append(values[sample.Metric], sample.Value)
					v.SampleCount++
				}
			}
			v.Groups[i].Metrics = Metrics{LCP: statistic(values["LCP"]), INP: statistic(values["INP"]), CLS: statistic(values["CLS"])}
		}
		if v.SampleCount > 0 {
			v.Status = "connected"
		}
		break
	}
	return v
}
func statistic(values []float64) Statistic {
	if len(values) == 0 {
		return Statistic{}
	}
	sort.Float64s(values)
	value := values[int(math.Ceil(float64(len(values))*.75))-1]
	return Statistic{Count: len(values), P75: &value}
}
