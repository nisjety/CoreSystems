package modelentitlement

import (
	"context"
	"errors"
	"strings"
)

// Store is the persistence the service needs. *Repository satisfies it; tests
// use a fake so the service and the HTTP layer run without a database.
type Store interface {
	Get(ctx context.Context, orgID string) (*PolicySet, error)
	Put(ctx context.Context, orgID, actorID string, expectedRevision int64, req PutRequest) (*PolicySet, error)
}

// RoleReader reports a user's role in an organisation, or "" when the user is
// not an active member. *org.Service.CallerRole satisfies it, so the role comes
// from org-core's own membership table (projected from auth-core) and never
// from a header.
type RoleReader interface {
	CallerRole(ctx context.Context, orgID, userID string) (string, error)
}

// Service is the one place policy is validated, written and resolved.
type Service struct {
	store Store
	roles RoleReader
}

func NewService(store Store, roles RoleReader) *Service {
	return &Service{store: store, roles: roles}
}

// Get returns the stored policy set for the admin read.
func (s *Service) Get(ctx context.Context, orgID string) (*PolicySet, error) {
	return s.store.Get(ctx, orgID)
}

// Put validates the body and replaces the organisation's policy set. A body
// that fails validation is a *ValidationError and writes nothing.
func (s *Service) Put(ctx context.Context, orgID, actorID string, req PutRequest) (*PolicySet, error) {
	if strings.TrimSpace(actorID) == "" {
		return nil, errors.New("an actor id is required to write a model policy")
	}
	normalised, err := ValidatePut(req)
	if err != nil {
		return nil, err
	}
	return s.store.Put(ctx, orgID, actorID, *normalised.ExpectedRevision, normalised)
}

// Resolve answers what applies to userID in orgID. An empty userID is a service
// call and gets the organisation default only. The lookup is not cached: a
// resolution decides which model a person may use, and org-core is not where
// staleness is bounded (inference-core's TTL is).
func (s *Service) Resolve(ctx context.Context, orgID, userID string) (Resolution, error) {
	set, err := s.store.Get(ctx, orgID)
	if err != nil {
		return Resolution{}, err
	}
	caller := Caller{UserID: strings.TrimSpace(userID)}
	if caller.UserID != "" {
		role, err := s.roles.CallerRole(ctx, orgID, caller.UserID)
		if err != nil {
			return Resolution{}, err
		}
		caller.Role = role
	}
	return Resolve(set, caller), nil
}
