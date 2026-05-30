package notification

import (
	"context"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"
)

const (
	StatusAccepted  = "accepted"
	StatusSubmitted = "submitted"
	StatusFailed    = "failed"

	ProviderNovu = "novu"

	SubjectNotificationRequestAccepted  = "velion.application.notification.request.accepted"
	SubjectNotificationRequestSubmitted = "velion.application.notification.request.submitted"
	SubjectNotificationRequestFailed    = "velion.application.notification.request.failed"
)

var ErrNotFound = errors.New("notification request not found")
var ErrAlreadyExists = errors.New("notification request already exists")

type ValidationError string

func (err ValidationError) Error() string {
	return string(err)
}

type IDGenerator func() string

type TimeSource func() time.Time

type Repository interface {
	FindByIdempotencyKey(ctx context.Context, idempotencyKey string) (*StoredRequest, error)
	Create(ctx context.Context, params CreateRequestParams) (*StoredRequest, error)
	MarkSubmitted(ctx context.Context, requestID string, providerRequestID string, occurredAt time.Time) (*StoredRequest, error)
	MarkFailed(ctx context.Context, requestID string, failureMessage string, occurredAt time.Time) (*StoredRequest, error)
}

type RuntimeClient interface {
	Dispatch(ctx context.Context, request DeliveryRequest) (*DispatchResult, error)
}

type EventPublisher interface {
	Publish(ctx context.Context, subject string, payload any) error
}

type Request struct {
	IdempotencyKey string         `json:"idempotency_key,omitempty"`
	RecipientID    string         `json:"recipient_id"`
	Type           string         `json:"type"`
	Payload        map[string]any `json:"payload"`
	Source         string         `json:"source,omitempty"`
}

type CreateRequestParams struct {
	ID             string
	IdempotencyKey string
	RecipientID    string
	Type           string
	Payload        map[string]any
	Source         string
	Status         string
	Provider       string
	OccurredAt     time.Time
}

type DeliveryRequest struct {
	RequestID   string
	RecipientID string
	Type        string
	Payload     map[string]any
	Source      string
}

type DispatchResult struct {
	Provider          string
	ProviderRequestID string
}

type StoredRequest struct {
	ID                string         `json:"id"`
	IdempotencyKey    string         `json:"idempotency_key,omitempty"`
	RecipientID       string         `json:"recipient_id"`
	Type              string         `json:"type"`
	Payload           map[string]any `json:"payload"`
	Source            string         `json:"source,omitempty"`
	Status            string         `json:"status"`
	Provider          string         `json:"provider"`
	ProviderRequestID string         `json:"provider_request_id,omitempty"`
	ErrorMessage      string         `json:"error_message,omitempty"`
	CreatedAt         time.Time      `json:"created_at"`
	UpdatedAt         time.Time      `json:"updated_at"`
	SubmittedAt       *time.Time     `json:"submitted_at,omitempty"`
	FailedAt          *time.Time     `json:"failed_at,omitempty"`
}

type LifecycleEvent struct {
	RequestID         string    `json:"request_id"`
	RecipientID       string    `json:"recipient_id"`
	Type              string    `json:"type"`
	Source            string    `json:"source,omitempty"`
	Status            string    `json:"status"`
	Provider          string    `json:"provider"`
	ProviderRequestID string    `json:"provider_request_id,omitempty"`
	ErrorMessage      string    `json:"error_message,omitempty"`
	OccurredAt        time.Time `json:"occurred_at"`
}

type AcceptedRequest struct {
	RequestID string `json:"request_id"`
	Status    string `json:"status"`
}

type RuntimeDispatchError struct {
	cause error
}

func (err RuntimeDispatchError) Error() string {
	return err.cause.Error()
}

func (err RuntimeDispatchError) Unwrap() error {
	return err.cause
}

type Service struct {
	repository       Repository
	runtimeClient    RuntimeClient
	publisher        EventPublisher
	generateID       IDGenerator
	now              TimeSource
	feedSink         FeedSinkFn         // U5-2: post-dispatch hook for feed cache
	subscriberEnsure SubscriberEnsureFn // U5-2: pre-dispatch hook for identity row
}

// FeedSinkParams is the input to the feed-sink hook. Plain struct so
// notification-core stays decoupled from the feed package types.
type FeedSinkParams struct {
	RequestID             string
	RecipientID           string
	Type                  string
	Title                 string
	Body                  string
	CtaLabel              string
	CtaHref               string
	Payload               map[string]any
	ActorID               string
	ActorName             string
	ActorEmail            string
	ActorAvatar           string
	Provider              string
	ProviderTransactionID string
	Source                string
}

// FeedSinkFn writes a delivered notification into the local feed cache.
type FeedSinkFn func(ctx context.Context, params FeedSinkParams)

// SubscriberEnsureFn guarantees a subscriber row exists for a given
// recipient id; called before each dispatch so feed reads can join on
// subscriber data without dangling foreign keys.
type SubscriberEnsureFn func(ctx context.Context, recipientID string)

// Option configures a Service.
type Option func(*Service)

// WithIDGenerator overrides the default request-id generator (mostly for tests).
func WithIDGenerator(gen IDGenerator) Option {
	return func(s *Service) {
		if gen != nil {
			s.generateID = gen
		}
	}
}

// WithNow overrides the default time source (mostly for tests).
func WithNow(now TimeSource) Option {
	return func(s *Service) {
		if now != nil {
			s.now = now
		}
	}
}

// WithFeedSink registers a post-dispatch hook that mirrors the delivered
// notification into the local feed cache.
func WithFeedSink(fn FeedSinkFn) Option {
	return func(s *Service) { s.feedSink = fn }
}

// WithSubscriberEnsurer registers a pre-dispatch hook that ensures a
// subscriber row exists for the recipient.
func WithSubscriberEnsurer(fn SubscriberEnsureFn) Option {
	return func(s *Service) { s.subscriberEnsure = fn }
}

// NewService constructs the service with optional behaviour hooks. The
// legacy (generateID, now) positional args are still accepted in the old
// signature via test callers — they're handled by the back-compat helper
// `NewServiceLegacy` below.
func NewService(repository Repository, runtimeClient RuntimeClient, publisher EventPublisher, opts ...Option) *Service {
	s := &Service{
		repository:    repository,
		runtimeClient: runtimeClient,
		publisher:     publisher,
		generateID:    defaultIDGenerator,
		now:           time.Now,
	}
	for _, opt := range opts {
		opt(s)
	}
	return s
}

// NewServiceLegacy preserves the old 5-arg signature used by the test
// suite (server_test.go). Prefer NewService + Options in new code.
func NewServiceLegacy(repository Repository, runtimeClient RuntimeClient, publisher EventPublisher, generateID IDGenerator, now TimeSource) *Service {
	return NewService(repository, runtimeClient, publisher,
		WithIDGenerator(generateID),
		WithNow(now),
	)
}

func (s *Service) Accept(ctx context.Context, request Request) (*AcceptedRequest, error) {
	if ctx == nil {
		ctx = context.Background()
	}

	validatedRequest, err := validateRequest(request)
	if err != nil {
		return nil, err
	}
	if s.repository == nil {
		return nil, errors.New("notification repository is not configured")
	}

	if validatedRequest.IdempotencyKey != "" {
		existingRequest, err := s.repository.FindByIdempotencyKey(ctx, validatedRequest.IdempotencyKey)
		switch {
		case err == nil:
			return buildAcceptedResponse(existingRequest), nil
		case errors.Is(err, ErrNotFound):
			// Continue and create a new request.
		case err != nil:
			return nil, fmt.Errorf("find notification request by idempotency key: %w", err)
		}
	}

	occurredAt := s.now().UTC()
	storedRequest, err := s.repository.Create(ctx, CreateRequestParams{
		ID:             s.generateID(),
		IdempotencyKey: validatedRequest.IdempotencyKey,
		RecipientID:    validatedRequest.RecipientID,
		Type:           validatedRequest.Type,
		Payload:        copyPayload(validatedRequest.Payload),
		Source:         validatedRequest.Source,
		Status:         StatusAccepted,
		Provider:       ProviderNovu,
		OccurredAt:     occurredAt,
	})
	if err != nil {
		if validatedRequest.IdempotencyKey != "" && errors.Is(err, ErrAlreadyExists) {
			existingRequest, lookupErr := s.repository.FindByIdempotencyKey(ctx, validatedRequest.IdempotencyKey)
			if lookupErr != nil {
				return nil, fmt.Errorf("resolve duplicate notification request: %w", lookupErr)
			}
			return buildAcceptedResponse(existingRequest), nil
		}
		return nil, fmt.Errorf("create notification request: %w", err)
	}

	s.publishLifecycleEvent(ctx, SubjectNotificationRequestAccepted, storedRequest, occurredAt)

	// U5-2: ensure a local subscriber row exists for the recipient before
	// the dispatch. Best-effort; failures log inside the hook.
	if s.subscriberEnsure != nil {
		s.subscriberEnsure(ctx, storedRequest.RecipientID)
	}

	if s.runtimeClient == nil {
		return buildAcceptedResponse(storedRequest), nil
	}

	dispatchResult, err := s.runtimeClient.Dispatch(ctx, DeliveryRequest{
		RequestID:   storedRequest.ID,
		RecipientID: storedRequest.RecipientID,
		Type:        storedRequest.Type,
		Payload:     copyPayload(storedRequest.Payload),
		Source:      storedRequest.Source,
	})
	if err != nil {
		failedRequest, markErr := s.repository.MarkFailed(ctx, storedRequest.ID, err.Error(), occurredAt)
		if markErr != nil {
			return nil, fmt.Errorf("mark notification request failed: %w", markErr)
		}

		s.publishLifecycleEvent(ctx, SubjectNotificationRequestFailed, failedRequest, occurredAt)
		return buildAcceptedResponse(failedRequest), RuntimeDispatchError{cause: err}
	}

	providerRequestID := ""
	if dispatchResult != nil {
		providerRequestID = strings.TrimSpace(dispatchResult.ProviderRequestID)
	}

	submittedRequest, err := s.repository.MarkSubmitted(ctx, storedRequest.ID, providerRequestID, occurredAt)
	if err != nil {
		return nil, fmt.Errorf("mark notification request submitted: %w", err)
	}

	s.publishLifecycleEvent(ctx, SubjectNotificationRequestSubmitted, submittedRequest, occurredAt)

	// U5-2: mirror the delivered notification into the feed cache so the
	// /notifications endpoint serves it without a Novu round-trip.
	// Display fields (title/body/cta) are derived from the payload on a
	// best-effort basis — workflows should publish them in payload so the
	// in-app view has something to render.
	if s.feedSink != nil {
		feedParams := FeedSinkParams{
			RequestID:             submittedRequest.ID,
			RecipientID:           submittedRequest.RecipientID,
			Type:                  submittedRequest.Type,
			Payload:               copyPayload(submittedRequest.Payload),
			Provider:              submittedRequest.Provider,
			ProviderTransactionID: submittedRequest.ProviderRequestID,
			Source:                submittedRequest.Source,
		}
		extractDisplayFields(submittedRequest.Payload, &feedParams)
		s.feedSink(ctx, feedParams)
	}

	return buildAcceptedResponse(submittedRequest), nil
}

// extractDisplayFields pulls common display fields out of the payload by
// well-known key names: `title`, `body`, `message`, `cta_label`, `cta_href`,
// `actor_id`, `actor_name`, `actor_email`, `actor_avatar`. This convention
// keeps the contract simple: upstream callers just include these fields
// in their payload (alongside any structured data) and notification-core
// surfaces them in the feed.
func extractDisplayFields(payload map[string]any, params *FeedSinkParams) {
	if payload == nil || params == nil {
		return
	}
	if v, ok := payload["title"].(string); ok {
		params.Title = v
	}
	if v, ok := payload["body"].(string); ok {
		params.Body = v
	} else if v, ok := payload["message"].(string); ok {
		params.Body = v
	}
	if v, ok := payload["cta_label"].(string); ok {
		params.CtaLabel = v
	}
	if v, ok := payload["cta_href"].(string); ok {
		params.CtaHref = v
	}
	if v, ok := payload["actor_id"].(string); ok {
		params.ActorID = v
	}
	if v, ok := payload["actor_name"].(string); ok {
		params.ActorName = v
	}
	if v, ok := payload["actor_email"].(string); ok {
		params.ActorEmail = v
	}
	if v, ok := payload["actor_avatar"].(string); ok {
		params.ActorAvatar = v
	}
}

func defaultIDGenerator() string {
	return fmt.Sprintf("req_%d", time.Now().UnixNano())
}

func IsValidationError(err error) bool {
	var validationErr ValidationError
	return errors.As(err, &validationErr)
}

func IsRuntimeDispatchError(err error) bool {
	var runtimeDispatchErr RuntimeDispatchError
	return errors.As(err, &runtimeDispatchErr)
}

func buildAcceptedResponse(storedRequest *StoredRequest) *AcceptedRequest {
	if storedRequest == nil {
		return nil
	}

	return &AcceptedRequest{
		RequestID: storedRequest.ID,
		Status:    storedRequest.Status,
	}
}

func validateRequest(request Request) (Request, error) {
	validatedRequest := Request{
		IdempotencyKey: strings.TrimSpace(request.IdempotencyKey),
		RecipientID:    strings.TrimSpace(request.RecipientID),
		Type:           strings.TrimSpace(request.Type),
		Payload:        copyPayload(request.Payload),
		Source:         strings.TrimSpace(request.Source),
	}

	if validatedRequest.RecipientID == "" {
		return Request{}, ValidationError("recipient_id is required")
	}
	if validatedRequest.Type == "" {
		return Request{}, ValidationError("type is required")
	}

	return validatedRequest, nil
}

func (s *Service) publishLifecycleEvent(ctx context.Context, subject string, storedRequest *StoredRequest, occurredAt time.Time) {
	if s.publisher == nil || storedRequest == nil {
		return
	}

	if err := s.publisher.Publish(ctx, subject, LifecycleEvent{
		RequestID:         storedRequest.ID,
		RecipientID:       storedRequest.RecipientID,
		Type:              storedRequest.Type,
		Source:            storedRequest.Source,
		Status:            storedRequest.Status,
		Provider:          storedRequest.Provider,
		ProviderRequestID: storedRequest.ProviderRequestID,
		ErrorMessage:      sanitizeLifecycleErrorMessage(storedRequest.ErrorMessage),
		OccurredAt:        occurredAt,
	}); err != nil {
		log.Printf("notification-core: failed to publish %s event for request %s: %v", subject, storedRequest.ID, err)
	}
}

func copyPayload(payload map[string]any) map[string]any {
	if payload == nil {
		return nil
	}

	cloned := make(map[string]any, len(payload))
	for key, value := range payload {
		cloned[key] = value
	}

	return cloned
}

func sanitizeLifecycleErrorMessage(errorMessage string) string {
	if strings.TrimSpace(errorMessage) == "" {
		return ""
	}

	return "delivery failed"
}
