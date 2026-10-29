package notification

import (
	"context"
	"errors"
	"testing"
	"time"
)

type workerQueueFake struct {
	attempt       *DeliveryAttempt
	submitted     int
	unknown       int
	failed        int
	lastProvider  string
	lastErrorCode string
}

func (f *workerQueueFake) EnqueueDeliveryAttempt(context.Context, DeliveryAttemptParams) (*DeliveryAttempt, error) {
	return nil, errors.New("not used")
}

func (f *workerQueueFake) ClaimDeliveryAttempt(context.Context, string, time.Time, time.Duration) (*DeliveryAttempt, error) {
	if f.attempt == nil {
		return nil, ErrNotFound
	}
	attempt := *f.attempt
	f.attempt = nil
	return &attempt, nil
}

func (f *workerQueueFake) MarkDeliverySubmitted(_ context.Context, _ string, _ string, providerRequestID string, _ time.Time) (*DeliveryAttempt, error) {
	f.submitted++
	f.lastProvider = providerRequestID
	return &DeliveryAttempt{Status: DeliveryAttemptSentUnconfirmed, ProviderRequestID: providerRequestID}, nil
}

func (f *workerQueueFake) MarkDeliveryUnknown(_ context.Context, _ string, _ string, errorCode string, _ time.Time) (*DeliveryAttempt, error) {
	f.unknown++
	f.lastErrorCode = errorCode
	return &DeliveryAttempt{Status: DeliveryAttemptUnknown, ErrorCode: errorCode}, nil
}

func (f *workerQueueFake) MarkDeliveryAcknowledged(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error) {
	return nil, errors.New("not used")
}

func (f *workerQueueFake) MarkDeliveryFailed(_ context.Context, _ string, _ string, errorCode string, _ time.Time) (*DeliveryAttempt, error) {
	f.failed++
	f.lastErrorCode = errorCode
	return &DeliveryAttempt{Status: DeliveryAttemptFailed, ErrorCode: errorCode}, nil
}

type workerRequestRepositoryFake struct {
	request *StoredRequest
}

func (f *workerRequestRepositoryFake) FindByID(context.Context, string) (*StoredRequest, error) {
	if f.request == nil {
		return nil, ErrNotFound
	}
	copy := *f.request
	copy.Payload = cloneMap(f.request.Payload)
	return &copy, nil
}

type workerRuntimeFake struct {
	result *DispatchResult
	err    error
	calls  int
}

func (f *workerRuntimeFake) Dispatch(context.Context, DeliveryRequest) (*DispatchResult, error) {
	f.calls++
	return f.result, f.err
}

func TestDeliveryWorkerMarksProviderAcceptanceUnconfirmed(t *testing.T) {
	queue := &workerQueueFake{attempt: &DeliveryAttempt{ID: "attempt-1", NotificationID: "req-1", Status: DeliveryAttemptClaimed}}
	repository := &workerRequestRepositoryFake{request: &StoredRequest{
		ID: "req-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser, RecipientID: "user-1",
		Type: "notification.created", Payload: map[string]any{"title": "hello"}, RetentionMode: RetentionModeStandard,
	}}
	runtime := &workerRuntimeFake{result: &DispatchResult{ProviderRequestID: "provider-1"}}
	worker := NewDeliveryWorker(repository, queue, runtime, fakeRecipientResolver{}, WithDeliveryWorkerClock(func() time.Time { return time.Unix(100, 0).UTC() }))

	processed, err := worker.RunOnce(context.Background(), "worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want processed without error", processed, err)
	}
	if runtime.calls != 1 || queue.submitted != 1 || queue.lastProvider != "provider-1" {
		t.Fatalf("runtime=%d submitted=%d provider=%q", runtime.calls, queue.submitted, queue.lastProvider)
	}
	if queue.unknown != 0 || queue.failed != 0 {
		t.Fatalf("unknown=%d failed=%d, want zero", queue.unknown, queue.failed)
	}
}

func TestDeliveryWorkerMarksProviderErrorUnknown(t *testing.T) {
	queue := &workerQueueFake{attempt: &DeliveryAttempt{ID: "attempt-1", NotificationID: "req-1", Status: DeliveryAttemptClaimed}}
	repository := &workerRequestRepositoryFake{request: &StoredRequest{
		ID: "req-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser, RecipientID: "user-1",
		Type: "notification.created", Payload: map[string]any{"title": "hello"}, RetentionMode: RetentionModeStandard,
	}}
	runtime := &workerRuntimeFake{err: errors.New("provider timeout")}
	worker := NewDeliveryWorker(repository, queue, runtime, fakeRecipientResolver{}, WithDeliveryWorkerClock(func() time.Time { return time.Unix(100, 0).UTC() }))

	processed, err := worker.RunOnce(context.Background(), "worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want processed without error", processed, err)
	}
	if queue.unknown != 1 || queue.lastErrorCode != "provider_dispatch_ambiguous" {
		t.Fatalf("unknown=%d error=%q, want ambiguous unknown", queue.unknown, queue.lastErrorCode)
	}
	if queue.failed != 0 {
		t.Fatalf("failed=%d, want zero for ambiguous provider result", queue.failed)
	}
}

func TestDeliveryWorkerRequiresProviderCorrelation(t *testing.T) {
	queue := &workerQueueFake{attempt: &DeliveryAttempt{ID: "attempt-1", NotificationID: "req-1", Status: DeliveryAttemptClaimed}}
	repository := &workerRequestRepositoryFake{request: &StoredRequest{
		ID: "req-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser, RecipientID: "user-1",
		Type: "notification.created", Payload: map[string]any{"title": "hello"}, RetentionMode: RetentionModeStandard,
	}}
	runtime := &workerRuntimeFake{result: &DispatchResult{}}
	worker := NewDeliveryWorker(repository, queue, runtime, fakeRecipientResolver{}, WithDeliveryWorkerClock(func() time.Time { return time.Unix(100, 0).UTC() }))

	processed, err := worker.RunOnce(context.Background(), "worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want provider correlation failure handled as unknown", processed, err)
	}
	if queue.unknown != 1 || queue.lastErrorCode != "provider_request_id_missing" {
		t.Fatalf("unknown=%d error=%q, want provider_request_id_missing", queue.unknown, queue.lastErrorCode)
	}
	if queue.submitted != 0 {
		t.Fatalf("submitted=%d, want zero without provider correlation", queue.submitted)
	}
}

func TestDeliveryWorkerRefusesZDRPayloadFromDurableQueue(t *testing.T) {
	queue := &workerQueueFake{attempt: &DeliveryAttempt{ID: "attempt-1", NotificationID: "req-1", Status: DeliveryAttemptClaimed}}
	repository := &workerRequestRepositoryFake{request: &StoredRequest{
		ID: "req-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser, RecipientID: "user-1",
		Type: "notification.created", RetentionMode: RetentionModeZDR,
	}}
	runtime := &workerRuntimeFake{result: &DispatchResult{ProviderRequestID: "must-not-dispatch"}}
	worker := NewDeliveryWorker(repository, queue, runtime, fakeRecipientResolver{}, WithDeliveryWorkerClock(func() time.Time { return time.Unix(100, 0).UTC() }))

	processed, err := worker.RunOnce(context.Background(), "worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want processed without error", processed, err)
	}
	if runtime.calls != 0 || queue.failed != 1 || queue.lastErrorCode != "zdr_async_delivery_unavailable" {
		t.Fatalf("runtime=%d failed=%d error=%q", runtime.calls, queue.failed, queue.lastErrorCode)
	}
}
