package notification

import (
	"context"
	"errors"
	"fmt"
	"testing"
	"time"
)

type fakeRepository struct {
	stored             map[string]*StoredRequest
	byIdempotencyKey   map[string]string
	createErr          error
	lookupErr          error
	lookupSequence     []error
	markSubmittedErr   error
	markFailedErr      error
	createCalls        int
	markSubmittedCalls int
	markFailedCalls    int
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		stored:           make(map[string]*StoredRequest),
		byIdempotencyKey: make(map[string]string),
	}
}

func (f *fakeRepository) FindByIdempotencyKey(_ context.Context, organizationID, idempotencyKey string) (*StoredRequest, error) {
	if len(f.lookupSequence) > 0 {
		err := f.lookupSequence[0]
		f.lookupSequence = f.lookupSequence[1:]
		if err != nil {
			return nil, err
		}
	}

	if f.lookupErr != nil {
		return nil, f.lookupErr
	}

	requestID, ok := f.byIdempotencyKey[idempotencyLookupKey(organizationID, idempotencyKey)]
	if !ok {
		return nil, ErrNotFound
	}

	storedRequest, ok := f.stored[requestID]
	if !ok {
		return nil, ErrNotFound
	}

	return cloneStoredRequest(storedRequest), nil
}

func (f *fakeRepository) Create(_ context.Context, params CreateRequestParams) (*StoredRequest, error) {
	if f.createErr != nil {
		return nil, f.createErr
	}

	f.createCalls++
	storedRequest := &StoredRequest{
		ID:             params.ID,
		OrganizationID: params.OrganizationID,
		IdempotencyKey: params.IdempotencyKey,
		RequestSHA256:  params.RequestSHA256,
		RetentionMode:  params.RetentionMode,
		RecipientKind:  params.RecipientKind,
		RecipientID:    params.RecipientID,
		Type:           params.Type,
		Payload:        cloneMap(params.Payload),
		Source:         params.Source,
		Status:         params.Status,
		Provider:       params.Provider,
		CreatedAt:      params.OccurredAt,
		UpdatedAt:      params.OccurredAt,
	}
	f.stored[storedRequest.ID] = storedRequest
	if params.IdempotencyKey != "" {
		f.byIdempotencyKey[idempotencyLookupKey(params.OrganizationID, params.IdempotencyKey)] = params.ID
	}

	return cloneStoredRequest(storedRequest), nil
}

func (f *fakeRepository) MarkSubmitted(_ context.Context, requestID string, providerRequestID string, occurredAt time.Time) (*StoredRequest, error) {
	if f.markSubmittedErr != nil {
		return nil, f.markSubmittedErr
	}

	f.markSubmittedCalls++
	storedRequest, ok := f.stored[requestID]
	if !ok {
		return nil, ErrNotFound
	}

	storedRequest.Status = StatusSubmitted
	storedRequest.ProviderRequestID = providerRequestID
	storedRequest.SubmittedAt = &occurredAt
	storedRequest.UpdatedAt = occurredAt

	return cloneStoredRequest(storedRequest), nil
}

func (f *fakeRepository) MarkFailed(_ context.Context, requestID string, failureMessage string, occurredAt time.Time) (*StoredRequest, error) {
	if f.markFailedErr != nil {
		return nil, f.markFailedErr
	}

	f.markFailedCalls++
	storedRequest, ok := f.stored[requestID]
	if !ok {
		return nil, ErrNotFound
	}

	storedRequest.Status = StatusFailed
	storedRequest.ErrorMessage = failureMessage
	storedRequest.FailedAt = &occurredAt
	storedRequest.UpdatedAt = occurredAt

	return cloneStoredRequest(storedRequest), nil
}

type fakeRuntimeClient struct {
	result    *DispatchResult
	err       error
	callCount int
	requests  []DeliveryRequest
}

func (f *fakeRuntimeClient) Dispatch(_ context.Context, request DeliveryRequest) (*DispatchResult, error) {
	f.callCount++
	f.requests = append(f.requests, request)
	if f.err != nil {
		return nil, f.err
	}
	return f.result, nil
}

type publishedEvent struct {
	subject string
	payload any
}

type fakePublisher struct {
	events []publishedEvent
}

type fakeRecipientResolver struct{}

func (fakeRecipientResolver) ResolveRecipient(_ context.Context, organizationID string, recipient Recipient) (*ResolvedRecipient, error) {
	return &ResolvedRecipient{
		Kind:                 recipient.Kind,
		ID:                   recipient.ID,
		ProviderSubscriberID: "provider:" + organizationID + ":" + recipient.ID,
	}, nil
}

func newLegacyTestService(repository Repository, runtime RuntimeClient, publisher EventPublisher, generateID IDGenerator, now TimeSource) *Service {
	return NewService(
		repository,
		runtime,
		publisher,
		WithIDGenerator(generateID),
		WithNow(now),
		WithRecipientResolver(fakeRecipientResolver{}),
	)
}

type fakeDeliveryQueue struct {
	attempts []DeliveryAttemptParams
}

func (f *fakeDeliveryQueue) EnqueueDeliveryAttempt(_ context.Context, params DeliveryAttemptParams) (*DeliveryAttempt, error) {
	f.attempts = append(f.attempts, params)
	return &DeliveryAttempt{ID: params.ID, NotificationID: params.NotificationID, AttemptNumber: params.AttemptNumber, Status: DeliveryAttemptPending}, nil
}

func (*fakeDeliveryQueue) ClaimDeliveryAttempt(context.Context, string, time.Time, time.Duration) (*DeliveryAttempt, error) {
	return nil, ErrNotFound
}

func (*fakeDeliveryQueue) MarkDeliverySubmitted(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error) {
	return nil, ErrNotFound
}

func (*fakeDeliveryQueue) MarkDeliveryUnknown(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error) {
	return nil, ErrNotFound
}

func (*fakeDeliveryQueue) MarkDeliveryAcknowledged(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error) {
	return nil, ErrNotFound
}

func (*fakeDeliveryQueue) MarkDeliveryFailed(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error) {
	return nil, ErrNotFound
}

func TestAcceptEnqueuesDurableAttemptWhenQueueIsConfigured(t *testing.T) {
	repository := newFakeRepository()
	queue := &fakeDeliveryQueue{}
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "must-not-dispatch"}}
	service := NewService(
		repository,
		runtimeClient,
		&fakePublisher{},
		WithIDGenerator(func() string { return "req_outbox" }),
		WithNow(func() time.Time { return time.Date(2026, time.August, 16, 21, 0, 0, 0, time.UTC) }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithDeliveryQueue(queue),
	)

	accepted, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_123"},
		Type:           "notification.created",
		Payload:        map[string]any{"title": "queued"},
	})
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if accepted == nil || accepted.Status != StatusAccepted {
		t.Fatalf("accepted = %#v, want accepted status", accepted)
	}
	if runtimeClient.callCount != 0 {
		t.Fatalf("runtime dispatch calls = %d, want 0", runtimeClient.callCount)
	}
	if len(queue.attempts) != 1 || queue.attempts[0].NotificationID != "req_outbox" || queue.attempts[0].AttemptNumber != 1 {
		t.Fatalf("queued attempts = %#v, want one attempt for req_outbox", queue.attempts)
	}
}

func scopedRequest(userID string) Request {
	return Request{
		OrganizationID: "org_123",
		Recipient: Recipient{
			Kind: RecipientKindUser,
			ID:   userID,
		},
	}
}

func idempotencyLookupKey(organizationID, idempotencyKey string) string {
	return organizationID + "\x00" + idempotencyKey
}

func (f *fakePublisher) Publish(_ context.Context, subject string, payload any) error {
	f.events = append(f.events, publishedEvent{subject: subject, payload: payload})
	return nil
}

func TestAcceptPersistsAndSubmitsRequest(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_123"}}
	publisher := &fakePublisher{}
	now := time.Date(2026, time.March, 31, 12, 0, 0, 0, time.UTC)

	service := newLegacyTestService(repository, runtimeClient, publisher, func() string { return "req_123" }, func() time.Time { return now })

	response, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		IdempotencyKey: "idem_123",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_123"},
		Type:           "notification.created",
		Payload:        map[string]any{"title": "Hello"},
		Source:         "planner-sync-core",
	})
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if response == nil {
		t.Fatal("Accept() response = nil, want non-nil")
	}
	if response.RequestID != "req_123" {
		t.Fatalf("RequestID = %q, want %q", response.RequestID, "req_123")
	}
	if response.Status != StatusSubmitted {
		t.Fatalf("Status = %q, want %q", response.Status, StatusSubmitted)
	}
	if repository.createCalls != 1 {
		t.Fatalf("createCalls = %d, want %d", repository.createCalls, 1)
	}
	if repository.markSubmittedCalls != 1 {
		t.Fatalf("markSubmittedCalls = %d, want %d", repository.markSubmittedCalls, 1)
	}
	if runtimeClient.callCount != 1 {
		t.Fatalf("runtimeClient.callCount = %d, want %d", runtimeClient.callCount, 1)
	}
	if len(publisher.events) != 2 {
		t.Fatalf("len(publisher.events) = %d, want %d", len(publisher.events), 2)
	}
	if publisher.events[0].subject != SubjectNotificationRequestAccepted {
		t.Fatalf("first subject = %q, want %q", publisher.events[0].subject, SubjectNotificationRequestAccepted)
	}
	if publisher.events[1].subject != SubjectNotificationRequestSubmitted {
		t.Fatalf("second subject = %q, want %q", publisher.events[1].subject, SubjectNotificationRequestSubmitted)
	}

	storedRequest := repository.stored["req_123"]
	if storedRequest == nil {
		t.Fatal("stored request = nil, want non-nil")
	}
	if storedRequest.ProviderRequestID != "novu_req_123" {
		t.Fatalf("ProviderRequestID = %q, want %q", storedRequest.ProviderRequestID, "novu_req_123")
	}
	if storedRequest.SubmittedAt == nil || !storedRequest.SubmittedAt.Equal(now) {
		t.Fatalf("SubmittedAt = %v, want %v", storedRequest.SubmittedAt, now)
	}
}

func TestAcceptFeedProjectionRecordsProviderSubmissionNotDelivery(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "provider-tx-1"}}
	now := time.Date(2026, time.July, 13, 13, 0, 0, 0, time.UTC)
	var projected FeedSinkParams
	service := NewService(
		repository,
		runtimeClient,
		&fakePublisher{},
		WithIDGenerator(func() string { return "req_feed" }),
		WithNow(func() time.Time { return now }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithFeedSink(func(_ context.Context, params FeedSinkParams) { projected = params }),
	)
	request := scopedRequest("user-1")
	request.Type = "notification.created"
	if _, err := service.Accept(context.Background(), request); err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if projected.DeliveryStatus != StatusSubmitted {
		t.Fatalf("feed delivery status = %q, want %q", projected.DeliveryStatus, StatusSubmitted)
	}
	if !projected.SubmittedAt.Equal(now) {
		t.Fatalf("feed submitted_at = %v, want %v", projected.SubmittedAt, now)
	}
	if projected.DeliveredAt != nil {
		t.Fatalf("feed delivered_at = %v, want nil without provider callback", projected.DeliveredAt)
	}
}

func TestAcceptZDRDispatchesWithoutPersistingOrProjectingContent(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "provider-zdr-1"}}
	publisher := &fakePublisher{}
	feedCalls := 0
	service := NewService(
		repository,
		runtimeClient,
		publisher,
		WithIDGenerator(func() string { return "req_zdr" }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithFeedSink(func(context.Context, FeedSinkParams) { feedCalls++ }),
	)
	request := scopedRequest("user-zdr")
	request.Type = "ticket.triaged"
	request.RetentionMode = RetentionModeZDR
	request.Payload = map[string]any{"message": "sensitive ticket summary"}

	result, err := service.Accept(context.Background(), request)
	if err != nil || result == nil || result.Status != StatusSubmitted {
		t.Fatalf("Accept() = (%#v, %v), want submitted", result, err)
	}
	stored := repository.stored["req_zdr"]
	if stored == nil {
		t.Fatal("stored request = nil")
	}
	if stored.RetentionMode != RetentionModeZDR {
		t.Fatalf("stored retention mode = %q, want %q", stored.RetentionMode, RetentionModeZDR)
	}
	if len(stored.Payload) != 0 {
		t.Fatalf("stored ZDR payload = %#v, want empty", stored.Payload)
	}
	if len(runtimeClient.requests) != 1 || runtimeClient.requests[0].Payload["message"] != "sensitive ticket summary" {
		t.Fatalf("runtime requests = %#v, want transient delivery payload", runtimeClient.requests)
	}
	if feedCalls != 0 {
		t.Fatalf("feed calls = %d, want 0 for ZDR", feedCalls)
	}
	for _, published := range publisher.events {
		event, ok := published.payload.(LifecycleEvent)
		if !ok {
			continue
		}
		if event.RecipientID != "" {
			t.Fatalf("ZDR lifecycle recipient_id = %q, want redacted", event.RecipientID)
		}
	}
}

func TestAcceptMarksRequestFailedWhenRuntimeDispatchFails(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{err: errors.New("novu runtime unavailable")}
	publisher := &fakePublisher{}
	now := time.Date(2026, time.March, 31, 12, 30, 0, 0, time.UTC)

	service := newLegacyTestService(repository, runtimeClient, publisher, func() string { return "req_456" }, func() time.Time { return now })

	response, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_456"},
		Type:           "notification.created",
		Payload:        map[string]any{"title": "Hello"},
	})
	if err == nil {
		t.Fatal("Accept() error = nil, want error")
	}
	if !IsRuntimeDispatchError(err) {
		t.Fatalf("IsRuntimeDispatchError(%v) = false, want true", err)
	}
	if response == nil {
		t.Fatal("Accept() response = nil, want non-nil")
	}
	if response.RequestID != "req_456" {
		t.Fatalf("RequestID = %q, want %q", response.RequestID, "req_456")
	}
	if response.Status != StatusFailed {
		t.Fatalf("Status = %q, want %q", response.Status, StatusFailed)
	}
	if repository.markFailedCalls != 1 {
		t.Fatalf("markFailedCalls = %d, want %d", repository.markFailedCalls, 1)
	}
	if len(publisher.events) != 2 {
		t.Fatalf("len(publisher.events) = %d, want %d", len(publisher.events), 2)
	}
	if publisher.events[1].subject != SubjectNotificationRequestFailed {
		t.Fatalf("second subject = %q, want %q", publisher.events[1].subject, SubjectNotificationRequestFailed)
	}

	storedRequest := repository.stored["req_456"]
	if storedRequest == nil {
		t.Fatal("stored request = nil, want non-nil")
	}
	if storedRequest.ErrorMessage != "delivery failed" {
		t.Fatalf("ErrorMessage = %q, want redacted delivery failure", storedRequest.ErrorMessage)
	}
	if storedRequest.FailedAt == nil || !storedRequest.FailedAt.Equal(now) {
		t.Fatalf("FailedAt = %v, want %v", storedRequest.FailedAt, now)
	}
}

func TestAcceptReturnsExistingRequestForIdempotencyKey(t *testing.T) {
	repository := newFakeRepository()
	repository.stored["req_existing"] = &StoredRequest{
		ID:             "req_existing",
		OrganizationID: "org_123",
		IdempotencyKey: "idem_existing",
		RecipientKind:  RecipientKindUser,
		RecipientID:    "user_789",
		Type:           "notification.created",
		Status:         StatusSubmitted,
		Provider:       "novu",
	}
	repository.byIdempotencyKey[idempotencyLookupKey("org_123", "idem_existing")] = "req_existing"
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_existing"}}
	publisher := &fakePublisher{}

	service := newLegacyTestService(repository, runtimeClient, publisher, func() string { return "req_new" }, time.Now)

	response, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		IdempotencyKey: "idem_existing",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_789"},
		Type:           "notification.created",
	})
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if response == nil {
		t.Fatal("Accept() response = nil, want non-nil")
	}
	if response.RequestID != "req_existing" {
		t.Fatalf("RequestID = %q, want %q", response.RequestID, "req_existing")
	}
	if response.Status != StatusSubmitted {
		t.Fatalf("Status = %q, want %q", response.Status, StatusSubmitted)
	}
	if repository.createCalls != 0 {
		t.Fatalf("createCalls = %d, want %d", repository.createCalls, 0)
	}
	if runtimeClient.callCount != 0 {
		t.Fatalf("runtimeClient.callCount = %d, want %d", runtimeClient.callCount, 0)
	}
	if len(publisher.events) != 0 {
		t.Fatalf("len(publisher.events) = %d, want %d", len(publisher.events), 0)
	}
}

func TestAcceptRejectsSameTenantIdempotencyKeyWithDifferentPayload(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_1"}}
	service := newLegacyTestService(repository, runtimeClient, &fakePublisher{}, func() string { return "req_1" }, time.Now)

	first := scopedRequest("user_789")
	first.IdempotencyKey = "idem_conflict"
	first.Type = "notification.created"
	first.Payload = map[string]any{"title": "First"}
	if _, err := service.Accept(context.Background(), first); err != nil {
		t.Fatalf("first Accept() error = %v", err)
	}

	second := scopedRequest("user_789")
	second.IdempotencyKey = "idem_conflict"
	second.Type = "notification.created"
	second.Payload = map[string]any{"title": "Different"}
	result, err := service.Accept(context.Background(), second)
	if result != nil || !IsValidationError(err) {
		t.Fatalf("second Accept() = (%#v, %v), want nil validation conflict", result, err)
	}
	if runtimeClient.callCount != 1 {
		t.Fatalf("runtime calls = %d, want 1", runtimeClient.callCount)
	}
}

func TestAcceptScopesIdempotencyKeyByOrganization(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req"}}
	nextID := 0
	service := newLegacyTestService(repository, runtimeClient, &fakePublisher{}, func() string {
		nextID++
		return fmt.Sprintf("req_%d", nextID)
	}, time.Now)

	first := scopedRequest("user_789")
	first.IdempotencyKey = "shared_key"
	first.Type = "notification.created"
	second := first
	second.OrganizationID = "org_456"

	firstResult, firstErr := service.Accept(context.Background(), first)
	secondResult, secondErr := service.Accept(context.Background(), second)
	if firstErr != nil || secondErr != nil {
		t.Fatalf("Accept errors = (%v, %v), want nil", firstErr, secondErr)
	}
	if firstResult.RequestID == secondResult.RequestID {
		t.Fatalf("request IDs = %q and %q, want distinct tenant-scoped records", firstResult.RequestID, secondResult.RequestID)
	}
	if runtimeClient.callCount != 2 {
		t.Fatalf("runtime calls = %d, want 2", runtimeClient.callCount)
	}
}

func TestAcceptRetriesExistingFailedRequestWithTheSameProviderRequestID(t *testing.T) {
	repository := newFakeRepository()
	repository.stored["req_failed"] = &StoredRequest{
		ID:             "req_failed",
		OrganizationID: "org_123",
		IdempotencyKey: "idem_failed",
		RecipientKind:  RecipientKindUser,
		RecipientID:    "user_789",
		Type:           "notification.created",
		Status:         StatusFailed,
		Provider:       ProviderNovu,
	}
	repository.byIdempotencyKey[idempotencyLookupKey("org_123", "idem_failed")] = "req_failed"
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "must-not-send"}}
	service := newLegacyTestService(repository, runtimeClient, &fakePublisher{}, func() string { return "req_new" }, time.Now)

	response, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		IdempotencyKey: "idem_failed",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_789"},
		Type:           "notification.created",
	})
	if err != nil || response == nil || response.Status != StatusSubmitted {
		t.Fatalf("Accept() = (%#v, %v), want submitted recovery", response, err)
	}
	if runtimeClient.callCount != 1 || runtimeClient.requests[0].RequestID != "req_failed" {
		t.Fatalf("runtime requests = %#v, want one retry for req_failed", runtimeClient.requests)
	}
}

func TestAcceptRecoversAcceptedRequestAfterFinalizationFailure(t *testing.T) {
	repository := newFakeRepository()
	repository.stored["req_accepted"] = &StoredRequest{
		ID:             "req_accepted",
		OrganizationID: "org_123",
		IdempotencyKey: "idem_accepted",
		RecipientKind:  RecipientKindUser,
		RecipientID:    "user_789",
		Type:           "notification.created",
		Status:         StatusAccepted,
		Provider:       ProviderNovu,
	}
	repository.byIdempotencyKey[idempotencyLookupKey("org_123", "idem_accepted")] = "req_accepted"
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_existing"}}
	service := newLegacyTestService(repository, runtimeClient, &fakePublisher{}, func() string { return "req_new" }, time.Now)

	response, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		IdempotencyKey: "idem_accepted",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_789"},
		Type:           "notification.created",
	})
	if err != nil || response == nil || response.Status != StatusSubmitted {
		t.Fatalf("Accept() = (%#v, %v), want submitted recovery", response, err)
	}
	if runtimeClient.callCount != 1 || runtimeClient.requests[0].RequestID != "req_accepted" {
		t.Fatalf("runtime requests = %#v, want one retry for req_accepted", runtimeClient.requests)
	}
}

func TestAcceptReturnsExistingRequestWhenCreateConflicts(t *testing.T) {
	repository := newFakeRepository()
	repository.stored["req_existing"] = &StoredRequest{
		ID:             "req_existing",
		OrganizationID: "org_123",
		IdempotencyKey: "idem_existing",
		RecipientKind:  RecipientKindUser,
		RecipientID:    "user_789",
		Type:           "notification.created",
		Status:         StatusSubmitted,
		Provider:       ProviderNovu,
	}
	repository.byIdempotencyKey[idempotencyLookupKey("org_123", "idem_existing")] = "req_existing"
	repository.lookupSequence = []error{ErrNotFound}
	repository.createErr = ErrAlreadyExists
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_existing"}}
	publisher := &fakePublisher{}

	service := newLegacyTestService(repository, runtimeClient, publisher, func() string { return "req_new" }, time.Now)

	response, err := service.Accept(context.Background(), Request{
		OrganizationID: "org_123",
		IdempotencyKey: "idem_existing",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_789"},
		Type:           "notification.created",
	})
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if response == nil {
		t.Fatal("Accept() response = nil, want non-nil")
	}
	if response.RequestID != "req_existing" {
		t.Fatalf("RequestID = %q, want %q", response.RequestID, "req_existing")
	}
	if response.Status != StatusSubmitted {
		t.Fatalf("Status = %q, want %q", response.Status, StatusSubmitted)
	}
	if runtimeClient.callCount != 0 {
		t.Fatalf("runtimeClient.callCount = %d, want %d", runtimeClient.callCount, 0)
	}
	if len(publisher.events) != 0 {
		t.Fatalf("len(publisher.events) = %d, want %d", len(publisher.events), 0)
	}
}

func cloneStoredRequest(storedRequest *StoredRequest) *StoredRequest {
	if storedRequest == nil {
		return nil
	}

	cloned := *storedRequest
	cloned.Payload = cloneMap(storedRequest.Payload)
	return &cloned
}

func cloneMap(values map[string]any) map[string]any {
	if values == nil {
		return nil
	}

	cloned := make(map[string]any, len(values))
	for key, value := range values {
		cloned[key] = value
	}
	return cloned
}
