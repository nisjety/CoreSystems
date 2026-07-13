package subscribers

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"
)

// IdentifyClient is the subset of the Novu client we need: push the
// subscriber identity to Novu so workflows can address them by name etc.
// We keep the surface minimal so the runtime package can supply either
// the real Novu adapter or a stub in tests.
type IdentifyClient interface {
	IdentifySubscriber(ctx context.Context, params IdentifyParams) error
}

type Repository interface {
	Upsert(ctx context.Context, params UpsertParams) (*Subscriber, error)
	Get(ctx context.Context, userID string) (*Subscriber, error)
	GetActiveForOrganization(ctx context.Context, organizationID, userID string) (*Subscriber, error)
	UpsertMembership(ctx context.Context, params MembershipParams) (*Membership, error)
	ListActiveMembershipsForUser(ctx context.Context, userID string) ([]Membership, error)
}

// IdentifyParams is the input shape we pass to the runtime. Mirrors Novu's
// subscriber upsert payload, but kept as a plain struct so the runtime
// layer owns the SDK type mapping.
type IdentifyParams struct {
	SubscriberID string
	Email        string
	Phone        string
	FirstName    string
	LastName     string
	Avatar       string
	Locale       string
	Timezone     string
	// Data is forwarded as Novu subscriber "data" — arbitrary metadata.
	Data map[string]any
}

type Service struct {
	repo     Repository
	runtime  IdentifyClient
	now      func() time.Time
	notifier func(error) // optional async error sink; nil = log.Printf
}

func NewService(repo Repository, runtime IdentifyClient) *Service {
	return &Service{
		repo:    repo,
		runtime: runtime,
		now:     time.Now,
	}
}

// Upsert inserts or updates the local row, then asynchronously syncs the
// identity to Novu. Upstream callers (NATS handlers, HTTP recipient-upsert
// endpoint) get a fast write — the Novu sync happens best-effort.
func (s *Service) Upsert(ctx context.Context, params UpsertParams) (*Subscriber, error) {
	if s == nil {
		return nil, errors.New("subscribers service not configured")
	}

	sub, err := s.repo.Upsert(ctx, params)
	if err != nil {
		return nil, fmt.Errorf("upsert subscriber: %w", err)
	}

	// Sync every active org-scoped provider identity. Delivery never targets
	// the legacy global subscriber id because preferences and payloads belong
	// to one organization.
	if s.runtime != nil {
		memberships, listErr := s.repo.ListActiveMembershipsForUser(ctx, sub.UserID)
		if listErr != nil {
			return nil, fmt.Errorf("list subscriber memberships: %w", listErr)
		}
		for _, membership := range memberships {
			go s.syncToNovu(*sub, membership)
		}
	}

	return sub, nil
}

// Get returns the cached subscriber or ErrNotFound.
func (s *Service) Get(ctx context.Context, userID string) (*Subscriber, error) {
	if s == nil {
		return nil, errors.New("subscribers service not configured")
	}
	return s.repo.Get(ctx, userID)
}

// ResolveUser returns the provider subscriber ID only when Control-derived
// local state says the user is an active member of the requested organization.
// A signed gateway session proves who made the request, but is not a durable
// membership grant: stale active-organization session state must not preserve
// access after Control removes a member.
func (s *Service) ResolveUser(ctx context.Context, organizationID, userID string) (string, error) {
	if s == nil {
		return "", errors.New("subscribers service not configured")
	}
	organizationID = strings.TrimSpace(organizationID)
	userID = strings.TrimSpace(userID)
	subscriber, err := s.repo.GetActiveForOrganization(ctx, organizationID, userID)
	if err != nil {
		return "", err
	}
	if subscriber.NovuSubscriberID == "" {
		return "", ErrNotFound
	}
	return subscriber.NovuSubscriberID, nil
}

func (s *Service) UpsertMembership(ctx context.Context, params MembershipParams) (*Membership, error) {
	if s == nil {
		return nil, errors.New("subscribers service not configured")
	}
	providerSubscriberID := providerSubscriberID(params.OrganizationID, params.UserID)
	membership, err := s.repo.UpsertMembership(ctx, MembershipParams{
		OrganizationID:       params.OrganizationID,
		UserID:               params.UserID,
		ProviderSubscriberID: providerSubscriberID,
		Role:                 params.Role,
		Status:               params.Status,
		AuthorityRevision:    params.AuthorityRevision,
		SourceEventID:        params.SourceEventID,
		OccurredAt:           params.OccurredAt,
	})
	if err != nil {
		return nil, err
	}
	if membership.Status == MembershipStatusActive && s.runtime != nil {
		subscriber, getErr := s.repo.Get(ctx, membership.UserID)
		if getErr != nil {
			return nil, fmt.Errorf("resolve membership identity: %w", getErr)
		}
		go s.syncToNovu(*subscriber, *membership)
	}
	return membership, nil
}

func (s *Service) syncToNovu(sub Subscriber, membership Membership) {
	// Detached context — the goroutine outlives the original caller.
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	err := s.runtime.IdentifySubscriber(ctx, IdentifyParams{
		SubscriberID: membership.ProviderSubscriberID,
		Email:        sub.Email,
		Phone:        sub.Phone,
		FirstName:    sub.FirstName,
		LastName:     sub.LastName,
		Avatar:       sub.Avatar,
		Locale:       sub.Locale,
		Timezone:     sub.Timezone,
		Data: map[string]any{
			"velion_user_id": sub.UserID,
			"velion_org_id":  membership.OrganizationID,
			"velion_role":    membership.Role,
		},
	})
	if err != nil {
		if s.notifier != nil {
			s.notifier(err)
		} else {
			log.Printf("[notification-core/subscribers] Novu identify failed for %s: %v", sub.UserID, err)
		}
		return
	}

}

func providerSubscriberID(organizationID, userID string) string {
	canonical := strings.TrimSpace(organizationID) + "\x00" + strings.TrimSpace(userID)
	digest := sha256.Sum256([]byte(canonical))
	return fmt.Sprintf("velion:%x", digest)
}
