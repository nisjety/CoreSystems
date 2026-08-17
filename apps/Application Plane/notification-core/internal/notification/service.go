package notification

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"reflect"
	"regexp"
	"strings"
	"time"
)

const (
	StatusAccepted   = "accepted"
	StatusSubmitted  = "submitted"
	StatusFailed     = "failed"
	StatusSuppressed = "suppressed"

	ProviderNovu          = "novu"
	ProviderLocalPolicy   = "local_policy"
	RecipientKindUser     = "user"
	RetentionModeStandard = "standard"
	RetentionModeZDR      = "zdr"

	SubjectNotificationRequestAccepted   = "verevon.application.notification.request.accepted"
	SubjectNotificationRequestSubmitted  = "verevon.application.notification.request.submitted"
	SubjectNotificationRequestFailed     = "verevon.application.notification.request.failed"
	SubjectNotificationRequestSuppressed = "verevon.application.notification.request.suppressed"
)

var ErrNotFound = errors.New("notification request not found")
var ErrAlreadyExists = errors.New("notification request already exists")
var ErrRecipientNotAuthorized = errors.New("recipient is not authorized for organization")

var identifierPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)

type ValidationError string

func (err ValidationError) Error() string {
	return string(err)
}

type IDGenerator func() string

type TimeSource func() time.Time

type Repository interface {
	FindByIdempotencyKey(ctx context.Context, organizationID, idempotencyKey string) (*StoredRequest, error)
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

type PreferenceGate interface {
	IsNotificationEnabled(ctx context.Context, organizationID, recipientID, eventType string) (bool, error)
}

type Recipient struct {
	Kind string `json:"kind"`
	ID   string `json:"id"`
}

type ResolvedRecipient struct {
	Kind                 string
	ID                   string
	ProviderSubscriberID string
}

type RecipientResolver interface {
	ResolveRecipient(ctx context.Context, organizationID string, recipient Recipient) (*ResolvedRecipient, error)
}

type RecipientResolveFn func(ctx context.Context, organizationID string, recipient Recipient) (*ResolvedRecipient, error)

func (fn RecipientResolveFn) ResolveRecipient(ctx context.Context, organizationID string, recipient Recipient) (*ResolvedRecipient, error) {
	return fn(ctx, organizationID, recipient)
}

type Request struct {
	OrganizationID string         `json:"organization_id"`
	IdempotencyKey string         `json:"idempotency_key,omitempty"`
	Recipient      Recipient      `json:"recipient"`
	Type           string         `json:"type"`
	Payload        map[string]any `json:"payload"`
	Source         string         `json:"source,omitempty"`
	RetentionMode  string         `json:"retention_mode,omitempty"`
	requestSHA256  string
}

type CreateRequestParams struct {
	ID             string
	OrganizationID string
	IdempotencyKey string
	RequestSHA256  string
	RetentionMode  string
	RecipientKind  string
	RecipientID    string
	Type           string
	Payload        map[string]any
	Source         string
	Status         string
	Provider       string
	OccurredAt     time.Time
}

type DeliveryRequest struct {
	RequestID           string
	OrganizationID      string
	RecipientKind       string
	RecipientID         string
	ProviderRecipientID string
	Type                string
	Payload             map[string]any
	Source              string
	RetentionMode       string
}

type DispatchResult struct {
	Provider          string
	ProviderRequestID string
}

type StoredRequest struct {
	ID                string         `json:"id"`
	OrganizationID    string         `json:"organization_id"`
	IdempotencyKey    string         `json:"idempotency_key,omitempty"`
	RequestSHA256     string         `json:"-"`
	RetentionMode     string         `json:"retention_mode"`
	RecipientKind     string         `json:"recipient_kind"`
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
	OrganizationID    string    `json:"organization_id"`
	RecipientKind     string    `json:"recipient_kind"`
	RecipientID       string    `json:"recipient_id"`
	Type              string    `json:"type"`
	Source            string    `json:"source,omitempty"`
	Status            string    `json:"status"`
	Provider          string    `json:"provider"`
	ProviderRequestID string    `json:"provider_request_id,omitempty"`
	ErrorMessage      string    `json:"error_message,omitempty"`
	RetentionMode     string    `json:"retention_mode"`
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
	repository        Repository
	deliveryQueue     DeliveryQueue
	runtimeClient     RuntimeClient
	publisher         EventPublisher
	generateID        IDGenerator
	now               TimeSource
	feedSink          FeedSinkFn // U5-2: post-dispatch hook for feed cache
	recipientResolver RecipientResolver
	preferenceGate    PreferenceGate
}

// FeedSinkParams is the input to the feed-sink hook. Plain struct so
// notification-core stays decoupled from the feed package types.
type FeedSinkParams struct {
	RequestID             string
	OrganizationID        string
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
	DeliveryStatus        string
	SubmittedAt           time.Time
	DeliveredAt           *time.Time
}

// FeedSinkFn writes a provider-submitted notification into the local feed cache.
type FeedSinkFn func(ctx context.Context, params FeedSinkParams)

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

// WithFeedSink registers a post-dispatch hook that mirrors the submitted
// notification into the local feed cache. Delivery is confirmed separately.
func WithFeedSink(fn FeedSinkFn) Option {
	return func(s *Service) { s.feedSink = fn }
}

func WithRecipientResolver(resolver RecipientResolver) Option {
	return func(s *Service) { s.recipientResolver = resolver }
}

func WithPreferenceGate(gate PreferenceGate) Option {
	return func(s *Service) { s.preferenceGate = gate }
}

// WithDeliveryQueue enables the durable outbox path. It is intentionally an
// opt-in wiring change so old synchronous test/compatibility callers do not
// silently change delivery semantics before the worker and callback contract
// are deployed together.
func WithDeliveryQueue(queue DeliveryQueue) Option {
	return func(s *Service) { s.deliveryQueue = queue }
}

// NewService constructs the service with optional behaviour hooks.
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
	if s.deliveryQueue != nil && validatedRequest.RetentionMode == RetentionModeZDR {
		// The durable queue intentionally stores no payload. Until a provider
		// contract supplies an encrypted, expiry-bound transient descriptor,
		// accepting ZDR work into an async queue would strand it or require
		// persisting content that ZDR forbids.
		return nil, errors.New("asynchronous ZDR notification delivery is unavailable")
	}
	resolvedRecipient, err := s.resolveRecipient(ctx, validatedRequest)
	if err != nil {
		return nil, err
	}

	if validatedRequest.IdempotencyKey != "" {
		existingRequest, err := s.repository.FindByIdempotencyKey(
			ctx,
			validatedRequest.OrganizationID,
			validatedRequest.IdempotencyKey,
		)
		switch {
		case err == nil:
			return s.resumeExisting(ctx, existingRequest, validatedRequest, resolvedRecipient)
		case errors.Is(err, ErrNotFound):
			// Continue and create a new request.
		case err != nil:
			return nil, fmt.Errorf("find notification request by idempotency key: %w", err)
		}
	}

	enabled, err := s.isNotificationEnabled(ctx, validatedRequest)
	if err != nil {
		return nil, err
	}

	occurredAt := s.now().UTC()
	status := StatusAccepted
	provider := ProviderNovu
	if !enabled {
		status = StatusSuppressed
		provider = ProviderLocalPolicy
	}
	persistedPayload := validatedRequest.Payload
	if validatedRequest.RetentionMode == RetentionModeZDR {
		persistedPayload = nil
	}
	storedRequest, err := s.repository.Create(ctx, CreateRequestParams{
		ID:             s.generateID(),
		OrganizationID: validatedRequest.OrganizationID,
		IdempotencyKey: validatedRequest.IdempotencyKey,
		RequestSHA256:  validatedRequest.requestSHA256,
		RetentionMode:  validatedRequest.RetentionMode,
		RecipientKind:  resolvedRecipient.Kind,
		RecipientID:    resolvedRecipient.ID,
		Type:           validatedRequest.Type,
		Payload:        copyPayload(persistedPayload),
		Source:         validatedRequest.Source,
		Status:         status,
		Provider:       provider,
		OccurredAt:     occurredAt,
	})
	if err != nil {
		if validatedRequest.IdempotencyKey != "" && errors.Is(err, ErrAlreadyExists) {
			existingRequest, lookupErr := s.repository.FindByIdempotencyKey(
				ctx,
				validatedRequest.OrganizationID,
				validatedRequest.IdempotencyKey,
			)
			if lookupErr != nil {
				return nil, fmt.Errorf("resolve duplicate notification request: %w", lookupErr)
			}
			return s.resumeExisting(ctx, existingRequest, validatedRequest, resolvedRecipient)
		}
		return nil, fmt.Errorf("create notification request: %w", err)
	}

	if !enabled {
		s.publishLifecycleEvent(ctx, SubjectNotificationRequestSuppressed, storedRequest, occurredAt)
		return buildAcceptedResponse(storedRequest), nil
	}

	s.publishLifecycleEvent(ctx, SubjectNotificationRequestAccepted, storedRequest, occurredAt)
	if s.deliveryQueue != nil {
		if err := s.enqueueDeliveryAttempt(ctx, storedRequest, occurredAt); err != nil {
			return nil, fmt.Errorf("enqueue notification delivery attempt: %w", err)
		}
		return buildAcceptedResponse(storedRequest), nil
	}

	return s.dispatchAndFinalize(ctx, storedRequest, resolvedRecipient.ProviderSubscriberID, validatedRequest.Payload, occurredAt)
}

func (s *Service) resumeExisting(
	ctx context.Context,
	existingRequest *StoredRequest,
	request Request,
	resolvedRecipient *ResolvedRecipient,
) (*AcceptedRequest, error) {
	if existingRequest == nil ||
		existingRequest.OrganizationID != request.OrganizationID ||
		existingRequest.RecipientKind != resolvedRecipient.Kind ||
		existingRequest.RecipientID != resolvedRecipient.ID ||
		existingRequest.Type != request.Type ||
		!idempotencyRequestMatches(existingRequest, request) {
		return nil, ValidationError("idempotency_key conflicts with an existing notification request")
	}

	switch existingRequest.Status {
	case StatusSubmitted, StatusSuppressed:
		return buildAcceptedResponse(existingRequest), nil
	case StatusAccepted:
		if s.deliveryQueue != nil {
			if err := s.enqueueDeliveryAttempt(ctx, existingRequest, s.now().UTC()); err != nil {
				return nil, fmt.Errorf("enqueue existing notification delivery attempt: %w", err)
			}
			return buildAcceptedResponse(existingRequest), nil
		}
		fallthrough
	case StatusFailed:
		enabled, err := s.isNotificationEnabled(ctx, request)
		if err != nil {
			return nil, err
		}
		if !enabled {
			return buildAcceptedResponse(existingRequest), RuntimeDispatchError{
				cause: errors.New("notification retry suppressed by local preference"),
			}
		}
		return s.dispatchAndFinalize(ctx, existingRequest, resolvedRecipient.ProviderSubscriberID, request.Payload, s.now().UTC())
	default:
		return buildAcceptedResponse(existingRequest), RuntimeDispatchError{
			cause: fmt.Errorf("notification has non-delivery status %q", existingRequest.Status),
		}
	}
}

func (s *Service) enqueueDeliveryAttempt(ctx context.Context, storedRequest *StoredRequest, occurredAt time.Time) error {
	if s.deliveryQueue == nil {
		return errors.New("notification delivery queue is not configured")
	}
	if storedRequest == nil || strings.TrimSpace(storedRequest.ID) == "" {
		return errors.New("notification request is required")
	}
	_, err := s.deliveryQueue.EnqueueDeliveryAttempt(ctx, DeliveryAttemptParams{
		ID:             storedRequest.ID + ":attempt:1",
		NotificationID: storedRequest.ID,
		AttemptNumber:  1,
		OccurredAt:     occurredAt,
	})
	return err
}

func (s *Service) isNotificationEnabled(ctx context.Context, request Request) (bool, error) {
	if s.preferenceGate == nil {
		return true, nil
	}
	enabled, err := s.preferenceGate.IsNotificationEnabled(
		ctx,
		request.OrganizationID,
		request.Recipient.ID,
		request.Type,
	)
	if err != nil {
		return false, fmt.Errorf("check local notification preference: %w", err)
	}
	return enabled, nil
}

func (s *Service) dispatchAndFinalize(
	ctx context.Context,
	storedRequest *StoredRequest,
	providerRecipientID string,
	deliveryPayload map[string]any,
	occurredAt time.Time,
) (*AcceptedRequest, error) {
	if s.runtimeClient == nil {
		failure := errors.New("notification runtime is not configured")
		failedRequest, markErr := s.repository.MarkFailed(ctx, storedRequest.ID, failure.Error(), occurredAt)
		if markErr != nil {
			return nil, fmt.Errorf("mark notification request failed: %w", markErr)
		}
		s.publishLifecycleEvent(ctx, SubjectNotificationRequestFailed, failedRequest, occurredAt)
		return buildAcceptedResponse(failedRequest), RuntimeDispatchError{cause: failure}
	}

	dispatchResult, err := s.runtimeClient.Dispatch(ctx, DeliveryRequest{
		RequestID:           storedRequest.ID,
		OrganizationID:      storedRequest.OrganizationID,
		RecipientKind:       storedRequest.RecipientKind,
		RecipientID:         storedRequest.RecipientID,
		ProviderRecipientID: providerRecipientID,
		Type:                storedRequest.Type,
		Payload:             copyPayload(deliveryPayload),
		Source:              storedRequest.Source,
		RetentionMode:       storedRequest.RetentionMode,
	})
	if err != nil {
		failedRequest, markErr := s.repository.MarkFailed(
			ctx,
			storedRequest.ID,
			sanitizeLifecycleErrorMessage(err.Error()),
			occurredAt,
		)
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

	// Mirror the provider-submitted notification into the feed cache so the
	// /notifications endpoint serves it without a Novu round-trip.
	// Display fields (title/body/cta) are derived from the payload on a
	// best-effort basis — workflows should publish them in payload so the
	// in-app view has something to render.
	if s.feedSink != nil && submittedRequest.RetentionMode != RetentionModeZDR {
		feedParams := FeedSinkParams{
			RequestID:             submittedRequest.ID,
			OrganizationID:        submittedRequest.OrganizationID,
			RecipientID:           submittedRequest.RecipientID,
			Type:                  submittedRequest.Type,
			Payload:               copyPayload(submittedRequest.Payload),
			Provider:              submittedRequest.Provider,
			ProviderTransactionID: submittedRequest.ProviderRequestID,
			Source:                submittedRequest.Source,
			DeliveryStatus:        StatusSubmitted,
			SubmittedAt:           occurredAt,
		}
		if submittedRequest.SubmittedAt != nil {
			feedParams.SubmittedAt = submittedRequest.SubmittedAt.UTC()
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
		OrganizationID: strings.TrimSpace(request.OrganizationID),
		IdempotencyKey: strings.TrimSpace(request.IdempotencyKey),
		Recipient: Recipient{
			Kind: strings.TrimSpace(request.Recipient.Kind),
			ID:   strings.TrimSpace(request.Recipient.ID),
		},
		Type:          strings.TrimSpace(request.Type),
		Payload:       copyPayload(request.Payload),
		Source:        strings.TrimSpace(request.Source),
		RetentionMode: strings.ToLower(strings.TrimSpace(request.RetentionMode)),
	}
	if validatedRequest.RetentionMode == "" {
		validatedRequest.RetentionMode = RetentionModeStandard
	}
	if validatedRequest.RetentionMode != RetentionModeStandard && validatedRequest.RetentionMode != RetentionModeZDR {
		return Request{}, ValidationError("retention_mode must be standard or zdr")
	}

	if err := validateIdentifier("organization_id", validatedRequest.OrganizationID); err != nil {
		return Request{}, err
	}
	if validatedRequest.Recipient.Kind != RecipientKindUser {
		return Request{}, ValidationError("recipient.kind must be user")
	}
	if err := validateIdentifier("recipient.id", validatedRequest.Recipient.ID); err != nil {
		return Request{}, err
	}
	if validatedRequest.Type == "" {
		return Request{}, ValidationError("type is required")
	}
	if len(validatedRequest.Type) > 128 {
		return Request{}, ValidationError("type is too long")
	}
	if len(validatedRequest.IdempotencyKey) > 256 {
		return Request{}, ValidationError("idempotency_key is too long")
	}
	if len(validatedRequest.Source) > 128 {
		return Request{}, ValidationError("source is too long")
	}
	requestSHA256, err := fingerprintRequest(validatedRequest)
	if err != nil {
		return Request{}, ValidationError("payload is invalid")
	}
	validatedRequest.requestSHA256 = requestSHA256

	return validatedRequest, nil
}

func fingerprintRequest(request Request) (string, error) {
	canonical := struct {
		OrganizationID string         `json:"organization_id"`
		RecipientKind  string         `json:"recipient_kind"`
		RecipientID    string         `json:"recipient_id"`
		Type           string         `json:"type"`
		Payload        map[string]any `json:"payload"`
		Source         string         `json:"source"`
		RetentionMode  string         `json:"retention_mode"`
	}{
		OrganizationID: request.OrganizationID,
		RecipientKind:  request.Recipient.Kind,
		RecipientID:    request.Recipient.ID,
		Type:           request.Type,
		Payload:        request.Payload,
		Source:         request.Source,
		RetentionMode:  request.RetentionMode,
	}
	encoded, err := json.Marshal(canonical)
	if err != nil {
		return "", err
	}
	digest := sha256.Sum256(encoded)
	return fmt.Sprintf("%x", digest), nil
}

func idempotencyRequestMatches(existing *StoredRequest, request Request) bool {
	if existing.RequestSHA256 != "" {
		return existing.RequestSHA256 == request.requestSHA256
	}
	// Scoped rows created before request_sha256 was deployed are compared
	// structurally. Never assume an empty fingerprint is a match.
	return existing.Source == request.Source && reflect.DeepEqual(existing.Payload, request.Payload)
}

func validateIdentifier(field, value string) error {
	if value == "" {
		return ValidationError(field + " is required")
	}
	if !identifierPattern.MatchString(value) {
		return ValidationError(field + " is invalid")
	}
	return nil
}

func (s *Service) resolveRecipient(ctx context.Context, request Request) (*ResolvedRecipient, error) {
	if s.recipientResolver == nil {
		return nil, errors.New("notification recipient resolver is not configured")
	}
	resolved, err := s.recipientResolver.ResolveRecipient(ctx, request.OrganizationID, request.Recipient)
	if err != nil {
		if errors.Is(err, ErrRecipientNotAuthorized) {
			return nil, ErrRecipientNotAuthorized
		}
		return nil, fmt.Errorf("resolve notification recipient: %w", err)
	}
	if resolved == nil || resolved.Kind != RecipientKindUser || resolved.ID != request.Recipient.ID || strings.TrimSpace(resolved.ProviderSubscriberID) == "" {
		return nil, ErrRecipientNotAuthorized
	}
	return &ResolvedRecipient{
		Kind:                 resolved.Kind,
		ID:                   resolved.ID,
		ProviderSubscriberID: strings.TrimSpace(resolved.ProviderSubscriberID),
	}, nil
}

func (s *Service) publishLifecycleEvent(ctx context.Context, subject string, storedRequest *StoredRequest, occurredAt time.Time) {
	if s.publisher == nil || storedRequest == nil {
		return
	}

	recipientID := storedRequest.RecipientID
	if storedRequest.RetentionMode == RetentionModeZDR {
		recipientID = ""
	}
	if err := s.publisher.Publish(ctx, subject, LifecycleEvent{
		RequestID:         storedRequest.ID,
		OrganizationID:    storedRequest.OrganizationID,
		RecipientKind:     storedRequest.RecipientKind,
		RecipientID:       recipientID,
		Type:              storedRequest.Type,
		Source:            storedRequest.Source,
		Status:            storedRequest.Status,
		Provider:          storedRequest.Provider,
		ProviderRequestID: storedRequest.ProviderRequestID,
		ErrorMessage:      sanitizeLifecycleErrorMessage(storedRequest.ErrorMessage),
		RetentionMode:     storedRequest.RetentionMode,
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
