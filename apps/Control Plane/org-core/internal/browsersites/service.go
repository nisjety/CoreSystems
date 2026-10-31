package browsersites

import (
	"context"
	"errors"
	"strings"
	"time"
)

// Store is the persistence the service needs. *Repository satisfies it; the HTTP
// tests use an in-memory store so the real service and routes run without a
// database.
type Store interface {
	Get(ctx context.Context, orgID string) (*PolicySet, error)
	Put(ctx context.Context, orgID, actorID string, expectedRevision int64, req PutRequest) (*PolicySet, error)
	Snapshot(ctx context.Context, orgID, userID string, hosts []string) (*PolicySet, map[string]Pref, error)
	ListPrefs(ctx context.Context, orgID, userID string) (PrefSet, error)
	PutPref(ctx context.Context, orgID, userID string, expectedRevision int64, pref Pref) (Pref, error)
	DeletePref(ctx context.Context, orgID, userID, host string) error
}

// Service is the one place the site policy is validated, written, resolved and
// explained. It takes its clock so a resolution's expiry is testable.
type Service struct {
	store Store
	now   func() time.Time
}

func NewService(store Store, now func() time.Time) *Service {
	return &Service{store: store, now: now}
}

// Get returns the stored policy for the admin read.
func (s *Service) Get(ctx context.Context, orgID string) (*PolicySet, error) {
	return s.store.Get(ctx, orgID)
}

// Put validates the body and replaces the organisation's policy. A body that fails
// validation is a *ValidationError and writes nothing.
func (s *Service) Put(ctx context.Context, orgID, actorID string, req PutRequest) (*PolicySet, error) {
	if strings.TrimSpace(actorID) == "" {
		return nil, errors.New("an actor id is required to write a browser site policy")
	}
	normalised, err := ValidatePut(req)
	if err != nil {
		return nil, err
	}
	return s.store.Put(ctx, orgID, actorID, *normalised.ExpectedRevision, normalised)
}

// Prefs returns one member's choices in the organisation.
func (s *Service) Prefs(ctx context.Context, orgID, userID string) (PrefSet, error) {
	return s.store.ListPrefs(ctx, orgID, userID)
}

// SetPref records one member's choice for host. The host is canonicalised first, so
// the stored key is the one the resolver compares against.
func (s *Service) SetPref(ctx context.Context, orgID, userID, host string, req PrefRequest) (Pref, error) {
	canonical, err := CanonicalHost(host)
	if err != nil {
		return Pref{}, err
	}
	input, err := ValidatePref(req, s.now())
	if err != nil {
		return Pref{}, err
	}
	return s.store.PutPref(ctx, orgID, userID, *req.ExpectedRevision, Pref{Host: canonical, Level: input.Level, ExpiresAt: input.ExpiresAt})
}

// DeletePref removes one member's choice for host.
func (s *Service) DeletePref(ctx context.Context, orgID, userID, host string) error {
	canonical, err := CanonicalHost(host)
	if err != nil {
		return err
	}
	return s.store.DeletePref(ctx, orgID, userID, canonical)
}

// Resolve answers for hosts for one caller. An empty userID is a service call and
// gets the admin's answer alone. The lookup is not cached: a decision about one
// site is not where staleness is acceptable, and the broker fails closed on error.
func (s *Service) Resolve(ctx context.Context, orgID, userID string, hosts []string) (Resolution, error) {
	canonical, err := CanonicalHostList(hosts)
	if err != nil {
		return Resolution{}, err
	}
	set, prefs, err := s.store.Snapshot(ctx, orgID, strings.TrimSpace(userID), canonical)
	if err != nil {
		return Resolution{}, err
	}
	resolution := Resolve(*set, prefs, canonical, s.now())
	resolution.OrganizationID = orgID
	resolution.UserID = strings.TrimSpace(userID)
	return resolution, nil
}

// Explain answers for one host and reports the layers behind the answer: the
// admin's rule and the caller's own choice, with its expiry.
func (s *Service) Explain(ctx context.Context, orgID, userID, host string) (Explanation, error) {
	canonical, err := CanonicalHost(host)
	if err != nil {
		return Explanation{}, err
	}
	userID = strings.TrimSpace(userID)
	set, prefs, err := s.store.Snapshot(ctx, orgID, userID, []string{canonical})
	if err != nil {
		return Explanation{}, err
	}
	now := s.now()
	explanation := Explanation{
		OrganizationID: orgID,
		UserID:         userID,
		Revision:       set.Revision,
		Mode:           set.Mode,
		Decision:       Decide(*set, prefs, canonical, now),
		OrgRule:        ruleFor(*set, canonical),
	}
	if pref, ok := prefs[canonical]; ok {
		explanation.UserLevel = pref.Level
		expires := pref.ExpiresAt
		explanation.UserExpiresAt = &expires
	}
	return explanation, nil
}
