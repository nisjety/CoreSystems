package notification

import (
	"context"
	"errors"
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

func (f *fakeRepository) FindByIdempotencyKey(_ context.Context, idempotencyKey string) (*StoredRequest, error) {
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

	requestID, ok := f.byIdempotencyKey[idempotencyKey]
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
		IdempotencyKey: params.IdempotencyKey,
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
		f.byIdempotencyKey[params.IdempotencyKey] = params.ID
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

func (f *fakePublisher) Publish(_ context.Context, subject string, payload any) error {
	f.events = append(f.events, publishedEvent{subject: subject, payload: payload})
	return nil
}

func TestAcceptPersistsAndSubmitsRequest(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_123"}}
	publisher := &fakePublisher{}
	now := time.Date(2026, time.March, 31, 12, 0, 0, 0, time.UTC)

	service := NewServiceLegacy(repository, runtimeClient, publisher, func() string { return "req_123" }, func() time.Time { return now })

	response, err := service.Accept(context.Background(), Request{
		IdempotencyKey: "idem_123",
		RecipientID:    "user_123",
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

func TestAcceptMarksRequestFailedWhenRuntimeDispatchFails(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{err: errors.New("novu runtime unavailable")}
	publisher := &fakePublisher{}
	now := time.Date(2026, time.March, 31, 12, 30, 0, 0, time.UTC)

	service := NewServiceLegacy(repository, runtimeClient, publisher, func() string { return "req_456" }, func() time.Time { return now })

	response, err := service.Accept(context.Background(), Request{
		RecipientID: "user_456",
		Type:        "notification.created",
		Payload:     map[string]any{"title": "Hello"},
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
	if storedRequest.ErrorMessage != "novu runtime unavailable" {
		t.Fatalf("ErrorMessage = %q, want %q", storedRequest.ErrorMessage, "novu runtime unavailable")
	}
	if storedRequest.FailedAt == nil || !storedRequest.FailedAt.Equal(now) {
		t.Fatalf("FailedAt = %v, want %v", storedRequest.FailedAt, now)
	}
}

func TestAcceptReturnsExistingRequestForIdempotencyKey(t *testing.T) {
	repository := newFakeRepository()
	repository.stored["req_existing"] = &StoredRequest{
		ID:             "req_existing",
		IdempotencyKey: "idem_existing",
		RecipientID:    "user_789",
		Type:           "notification.created",
		Status:         StatusSubmitted,
		Provider:       "novu",
	}
	repository.byIdempotencyKey["idem_existing"] = "req_existing"
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_existing"}}
	publisher := &fakePublisher{}

	service := NewServiceLegacy(repository, runtimeClient, publisher, func() string { return "req_new" }, time.Now)

	response, err := service.Accept(context.Background(), Request{
		IdempotencyKey: "idem_existing",
		RecipientID:    "user_789",
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

func TestAcceptReturnsExistingRequestWhenCreateConflicts(t *testing.T) {
	repository := newFakeRepository()
	repository.stored["req_existing"] = &StoredRequest{
		ID:             "req_existing",
		IdempotencyKey: "idem_existing",
		RecipientID:    "user_789",
		Type:           "notification.created",
		Status:         StatusSubmitted,
		Provider:       ProviderNovu,
	}
	repository.byIdempotencyKey["idem_existing"] = "req_existing"
	repository.lookupSequence = []error{ErrNotFound}
	repository.createErr = ErrAlreadyExists
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu_req_existing"}}
	publisher := &fakePublisher{}

	service := NewServiceLegacy(repository, runtimeClient, publisher, func() string { return "req_new" }, time.Now)

	response, err := service.Accept(context.Background(), Request{
		IdempotencyKey: "idem_existing",
		RecipientID:    "user_789",
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
