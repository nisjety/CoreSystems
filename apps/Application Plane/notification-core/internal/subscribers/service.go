package subscribers

import (
	"context"
	"errors"
	"fmt"
	"log"
	"time"
)

// IdentifyClient is the subset of the Novu client we need: push the
// subscriber identity to Novu so workflows can address them by name etc.
// We keep the surface minimal so the runtime package can supply either
// the real Novu adapter or a stub in tests.
type IdentifyClient interface {
	IdentifySubscriber(ctx context.Context, params IdentifyParams) error
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
	repo     *PGRepository
	runtime  IdentifyClient
	now      func() time.Time
	notifier func(error) // optional async error sink; nil = log.Printf
}

func NewService(repo *PGRepository, runtime IdentifyClient) *Service {
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

	// Best-effort Novu sync. We don't fail the local upsert if Novu is
	// down — the next event for this subscriber will retry the sync.
	if s.runtime != nil {
		go s.syncToNovu(*sub)
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

// EnsureForRecipient guarantees a row exists for a recipient_id used in
// triggers. It does not enrich identity — callers responsible for richer
// data should call Upsert. Used by the feed-write path: every delivered
// notification implies its recipient must have a subscriber row.
func (s *Service) EnsureForRecipient(ctx context.Context, recipientID string) (*Subscriber, error) {
	if s == nil {
		return nil, errors.New("subscribers service not configured")
	}

	sub, err := s.repo.Get(ctx, recipientID)
	if err == nil {
		return sub, nil
	}
	if !errors.Is(err, ErrNotFound) {
		return nil, err
	}

	// Insert a stub row. Later events with richer identity will merge.
	return s.repo.Upsert(ctx, UpsertParams{UserID: recipientID})
}

func (s *Service) syncToNovu(sub Subscriber) {
	// Detached context — the goroutine outlives the original caller.
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	err := s.runtime.IdentifySubscriber(ctx, IdentifyParams{
		SubscriberID: sub.NovuSubscriberID,
		Email:        sub.Email,
		Phone:        sub.Phone,
		FirstName:    sub.FirstName,
		LastName:     sub.LastName,
		Avatar:       sub.Avatar,
		Locale:       sub.Locale,
		Timezone:     sub.Timezone,
		Data: map[string]any{
			"velion_user_id": sub.UserID,
			"velion_org_id":  sub.OrgID,
			"velion_role":    sub.Role,
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

	if err := s.repo.MarkSynced(ctx, sub.UserID, s.now()); err != nil {
		log.Printf("[notification-core/subscribers] mark synced failed for %s: %v", sub.UserID, err)
	}
}
