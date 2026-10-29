package algoliamapping

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Service struct {
	mu       sync.Mutex
	store    *repository
	registry Registry
	client   Client
}

func New(path, org string, registry Registry, client Client) (*Service, error) {
	if registry == nil || registry.OrganizationID() != org {
		return nil, ErrInvalid
	}
	r, err := open(path, org)
	if err != nil {
		return nil, err
	}
	return &Service{store: r, registry: registry, client: client}, nil
}
func (s *Service) resolve(id, site string) (websites.Record, error) {
	if !idPattern.MatchString(id) {
		return websites.Record{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return websites.Record{}, err
	}
	return r, nil
}
func (s *Service) read(id string) (entry, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, e := range s.store.state.Entries {
		if e.Mapping.WebsiteID == id {
			return e, true
		}
	}
	return entry{}, false
}
func empty(w websites.Website) Mapping {
	return Mapping{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Status: "not_mapped"}
}
func (s *Service) Get(ctx context.Context, id, site string) (Mapping, error) {
	r, err := s.resolve(id, site)
	if err != nil {
		return Mapping{}, err
	}
	e, found := s.read(id)
	if !found || e.Mapping.Site != site || e.Mapping.WebsiteRevision != r.Website.Revision || e.Mapping.Status == "not_mapped" {
		return empty(r.Website), nil
	}
	result := e.Mapping
	a, err := s.access(ctx, result.ConnectionID)
	if err != nil {
		result.Status = "access_check_required"
	} else if a.Status != "active" {
		result.Status = "reconnect_required"
	}
	current, err := s.resolve(id, site)
	if err != nil {
		return Mapping{}, err
	}
	latest, ok := s.read(id)
	if !ok || current.Website.Revision != r.Website.Revision || fingerprint(latest) != fingerprint(e) {
		return Mapping{}, websites.ErrConflict
	}
	return result, nil
}
func (s *Service) Put(ctx context.Context, id, site string, revision int64, connection, app, index string) (Mapping, error) {
	r, err := s.resolve(id, site)
	if err != nil {
		return Mapping{}, err
	}
	if revision != r.Website.Revision {
		return Mapping{}, websites.ErrConflict
	}
	if !connectionPattern.MatchString(connection) || !appPattern.MatchString(app) || !indexPattern.MatchString(index) {
		return Mapping{}, ErrInvalid
	}
	prior, existed := s.read(id)
	before, err := s.access(ctx, connection)
	if err != nil || before.Status != "active" || s.client == nil {
		return Mapping{}, ErrUnavailable
	}
	allowed, err := s.client.Allowed(ctx, connection, app, index)
	if err != nil || !allowed {
		return Mapping{}, ErrUnavailable
	}
	after, err := s.access(ctx, connection)
	if err != nil || after.Status != "active" || after.Generation != before.Generation {
		return Mapping{}, ErrUnavailable
	}
	current, err := s.resolve(id, site)
	if err != nil {
		return Mapping{}, err
	}
	if current.Website.Revision != revision {
		return Mapping{}, websites.ErrConflict
	}
	value := Mapping{WebsiteID: id, Site: site, WebsiteRevision: revision, Status: "mapped", ConnectionID: connection, ApplicationID: app, IndexName: index}
	s.mu.Lock()
	defer s.mu.Unlock()
	var latest entry
	found := false
	for _, candidate := range s.store.state.Entries {
		if candidate.Mapping.WebsiteID == id {
			latest = candidate
			found = true
			break
		}
	}
	if found != existed || found && fingerprint(latest) != fingerprint(prior) {
		return Mapping{}, websites.ErrConflict
	}
	if err = s.store.save(entry{Mapping: value, Version: nonce()}); err != nil {
		return Mapping{}, err
	}
	return value, nil
}
func (s *Service) Delete(id, site string, revision int64) (Mapping, error) {
	r, err := s.resolve(id, site)
	if err != nil {
		return Mapping{}, err
	}
	if revision != r.Website.Revision {
		return Mapping{}, websites.ErrConflict
	}
	value := empty(r.Website)
	s.mu.Lock()
	defer s.mu.Unlock()
	if err = s.store.save(entry{Mapping: value, Version: nonce()}); err != nil {
		return Mapping{}, err
	}
	return value, nil
}
func (s *Service) access(ctx context.Context, connection string) (Access, error) {
	if s.client == nil {
		return Access{}, ErrUnavailable
	}
	bounded, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	a, err := s.client.Access(bounded, connection)
	if err != nil || a.ConnectionID != connection || a.Status != "active" && a.Status != "reconnect_required" || !generationPattern.MatchString(a.Generation) {
		return Access{}, ErrUnavailable
	}
	return a, nil
}
func fingerprint(e entry) string {
	raw, _ := json.Marshal(struct {
		Mapping Mapping
		Version string
	}{e.Mapping, e.Version})
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
func nonce() string {
	var value [32]byte
	_, _ = rand.Read(value[:])
	return hex.EncodeToString(value[:])
}

// Enrollments are local persisted mapping intentions. No provider call or grant
// assertion happens here; the worker independently checks access before work.
func (s *Service) Enrollments() []sitesources.Enrollment {
	s.mu.Lock()
	entries := append([]entry{}, s.store.state.Entries...)
	s.mu.Unlock()
	result := []sitesources.Enrollment{}
	for _, e := range entries {
		if e.Mapping.Status != "mapped" {
			continue
		}
		r, err := s.resolve(e.Mapping.WebsiteID, e.Mapping.Site)
		if err != nil || r.Website.Revision != e.Mapping.WebsiteRevision {
			continue
		}
		result = append(result, sitesources.Enrollment{WebsiteID: e.Mapping.WebsiteID, Site: e.Mapping.Site, WebsiteRevision: e.Mapping.WebsiteRevision, Fingerprint: fingerprint(e)})
	}
	return result
}
func (s *Service) Authorize(ctx context.Context, r websites.Record) (sitesources.Authorization, error) {
	w := r.Website
	current, err := s.resolve(w.ID, w.Site)
	if err != nil {
		return sitesources.Authorization{}, err
	}
	if current.Website.Revision != w.Revision {
		return sitesources.Authorization{}, websites.ErrConflict
	}
	e, found := s.read(w.ID)
	if !found {
		return sitesources.Authorization{}, nil
	}
	auth := sitesources.Authorization{Managed: true, Fingerprint: fingerprint(e)}
	if e.Mapping.Status != "mapped" || e.Mapping.Site != w.Site || e.Mapping.WebsiteRevision != w.Revision {
		return auth, nil
	}
	a, err := s.access(ctx, e.Mapping.ConnectionID)
	if err != nil {
		return sitesources.Authorization{}, err
	}
	current, err = s.resolve(w.ID, w.Site)
	if err != nil {
		return sitesources.Authorization{}, err
	}
	if current.Website.Revision != w.Revision {
		return sitesources.Authorization{}, websites.ErrConflict
	}
	latest, ok := s.read(w.ID)
	if !ok || fingerprint(latest) != auth.Fingerprint {
		return sitesources.Authorization{}, websites.ErrConflict
	}
	auth.Active = a.Status == "active"
	auth.Generation = a.Generation
	if auth.Active && e.SnapshotFingerprint == auth.Fingerprint && e.SnapshotGeneration == auth.Generation {
		auth.SnapshotObservedAt = e.SnapshotObservedAt
		auth.SnapshotDigest = e.SnapshotDigest
	}
	return auth, nil
}
func (s *Service) CollectOAuth(ctx context.Context, r websites.Record, a sitesources.Authorization) (sitesources.Report, error) {
	e, found := s.read(r.Website.ID)
	if !found || !a.Active || fingerprint(e) != a.Fingerprint || s.client == nil {
		return sitesources.Report{}, ErrUnavailable
	}
	return s.client.Report(ctx, r, e.Mapping)
}
func (s *Service) BindSnapshot(r websites.Record, a sitesources.Authorization, report sitesources.Report) error {
	w := r.Website
	current, err := s.resolve(w.ID, w.Site)
	if err != nil {
		return err
	}
	if current.Website.Revision != w.Revision {
		return websites.ErrConflict
	}
	if !a.Managed || !a.Active || !fingerprintPattern.MatchString(a.Fingerprint) || !generationPattern.MatchString(a.Generation) || report.WebsiteID != w.ID || report.Site != w.Site || report.WebsiteRevision != w.Revision || report.Provider != "algolia" {
		return ErrInvalid
	}
	if _, err = time.Parse(time.RFC3339Nano, report.ObservedAt); err != nil {
		return ErrInvalid
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, e := range s.store.state.Entries {
		if e.Mapping.WebsiteID == w.ID {
			if fingerprint(e) != a.Fingerprint || e.Mapping.Status != "mapped" {
				return websites.ErrConflict
			}
			next := e
			next.SnapshotFingerprint = a.Fingerprint
			next.SnapshotGeneration = a.Generation
			next.SnapshotObservedAt = report.ObservedAt
			next.SnapshotDigest = sitesources.Fingerprint(report)
			return s.store.save(next)
		}
	}
	return websites.ErrConflict
}
