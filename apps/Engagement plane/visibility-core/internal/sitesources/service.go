package sitesources

import (
	"context"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Service struct {
	mu                     sync.Mutex
	store                  *repository
	registry               Registry
	collector              Collector
	now                    func() time.Time
	attempts               map[string]time.Time
	running                map[string]bool
	oauth                  OAuthAuthority
	collections            *collectionRepository
	collectionWake         chan struct{}
	collectionRunning      atomic.Bool
	collectionCancels      map[string]context.CancelFunc
	collectionStorageFault bool
	collectionLastLog      time.Time
}

func NewWithOAuth(path, org string, registry Registry, collector Collector, authority OAuthAuthority, now func() time.Time) (*Service, error) {
	s, err := New(path, org, registry, collector, now)
	if err != nil {
		return nil, err
	}
	s.oauth = authority
	s.collections, err = openCollections(filepath.Join(filepath.Dir(path), "site-source-jobs.json"), org)
	if err != nil {
		return nil, err
	}
	s.collectionWake = make(chan struct{}, 2)
	s.collectionCancels = map[string]context.CancelFunc{}
	return s, nil
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
	return &Service{store: store, registry: registry, collector: collector, now: now, attempts: map[string]time.Time{}, running: map[string]bool{}}, nil
}
func (s *Service) resolve(id, site, p string) (websites.Record, error) {
	if !idPattern.MatchString(id) || !provider(p) {
		return websites.Record{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Record{}, err
	}
	return r, nil
}
func (s *Service) Get(id, site, p string) (Report, error) {
	return s.GetContext(context.Background(), id, site, p)
}
func (s *Service) GetContext(ctx context.Context, id, site, p string) (Report, error) {
	r, err := s.resolve(id, site, p)
	if err != nil {
		return Report{}, err
	}
	auth, authErr := s.authorize(ctx, r, p)
	current, err := s.resolve(id, site, p)
	if err != nil {
		return Report{}, err
	}
	if current.Website.Revision != r.Website.Revision {
		return Report{}, websites.ErrConflict
	}
	now := s.now().UTC()
	blank := Report{WebsiteID: id, Site: site, WebsiteRevision: r.Website.Revision, Provider: p, ObservedAt: stamp(now), Window: period(now), Status: "not_loaded"}
	if authErr != nil || auth.Managed && (!auth.Active || auth.SnapshotObservedAt == "") {
		return blank, nil
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, snapshot := range s.store.state.Entries {
		observed, _ := time.Parse(time.RFC3339Nano, snapshot.ObservedAt)
		if snapshot.WebsiteID == id && snapshot.Site == site && snapshot.Provider == p && snapshot.WebsiteRevision == r.Website.Revision && !observed.After(now) && now.Sub(observed) < Freshness && snapshot.Window == period(now) && (!auth.Managed || auth.SnapshotObservedAt == snapshot.ObservedAt && auth.SnapshotDigest == Fingerprint(snapshot)) {
			return clone(snapshot), nil
		}
	}
	return blank, nil
}
func (s *Service) authorize(ctx context.Context, r websites.Record, p string) (Authorization, error) {
	if p != "algolia" || s.oauth == nil {
		return Authorization{}, nil
	}
	return s.oauth.Authorize(ctx, r)
}
func (s *Service) Sync(ctx context.Context, id, site string, revision int64, p string) (Report, error) {
	r, err := s.resolve(id, site, p)
	if err != nil {
		return Report{}, err
	}
	if revision != r.Website.Revision {
		return Report{}, websites.ErrConflict
	}
	key := id + ":" + p
	now := s.now().UTC()
	s.mu.Lock()
	for previous, attempted := range s.attempts {
		if !s.running[previous] && now.Sub(attempted) >= time.Minute {
			delete(s.attempts, previous)
		}
	}
	if s.running[key] || !s.attempts[key].IsZero() && now.Sub(s.attempts[key]) < time.Minute {
		s.mu.Unlock()
		return Report{}, ErrCooldown
	}
	if s.attempts[key].IsZero() && len(s.attempts) >= maxEntries {
		s.mu.Unlock()
		return Report{}, ErrCapacity
	}
	s.attempts[key] = now
	s.running[key] = true
	s.mu.Unlock()
	defer func() { s.mu.Lock(); delete(s.running, key); s.mu.Unlock() }()
	bounded, cancel := context.WithTimeout(ctx, 35*time.Second)
	defer cancel()
	authority, err := s.authorize(bounded, r, p)
	if err != nil || authority.Managed && !authority.Active {
		return Report{}, ErrUnavailable
	}
	var result Report
	if authority.Managed {
		result, err = s.oauth.CollectOAuth(bounded, r, authority)
	} else if s.collector != nil {
		result, err = s.collector.Collect(bounded, r, p)
	} else {
		return Report{}, ErrUnavailable
	}
	if err != nil {
		return Report{}, ErrUnavailable
	}
	current, err := s.resolve(id, site, p)
	if err != nil {
		return Report{}, err
	}
	if current.Website.Revision != revision {
		return Report{}, websites.ErrConflict
	}
	latest, err := s.authorize(bounded, current, p)
	if err != nil || latest.Managed != authority.Managed || authority.Managed && (!latest.Active || latest.Fingerprint != authority.Fingerprint || latest.Generation != authority.Generation) {
		return Report{}, ErrUnavailable
	}
	now = s.now().UTC()
	observed, _ := time.Parse(time.RFC3339Nano, result.ObservedAt)
	if !validReport(result) || result.Status == "not_loaded" || result.WebsiteID != id || result.Site != site || result.WebsiteRevision != revision || result.Provider != p || result.Window != period(now) || observed.After(now) || observed.Before(now.Add(-5*time.Minute)) {
		return Report{}, ErrUnavailable
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err = s.store.save(clone(result), now); err != nil {
		return Report{}, err
	}
	if authority.Managed {
		if err = s.oauth.BindSnapshot(current, authority, result); err != nil {
			return Report{}, ErrUnavailable
		}
	}
	return clone(result), nil
}
