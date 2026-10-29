package notification

import (
	"context"
	"errors"
	"testing"
	"time"
)

type feedProjectionQueueFake struct {
	projection *FeedProjection
	projected  int
	unknown    int
	lastError  string
}

func (f *feedProjectionQueueFake) ClaimFeedProjection(context.Context, string, time.Time, time.Duration) (*FeedProjection, error) {
	if f.projection == nil {
		return nil, ErrNotFound
	}
	projection := *f.projection
	f.projection = nil
	return &projection, nil
}

func (f *feedProjectionQueueFake) MarkFeedProjectionProjected(_ context.Context, _, _ string, _ time.Time) (*FeedProjection, error) {
	f.projected++
	return &FeedProjection{Status: FeedProjectionProjected}, nil
}

func (f *feedProjectionQueueFake) MarkFeedProjectionUnknown(_ context.Context, _, _, errorCode string, _ time.Time) (*FeedProjection, error) {
	f.unknown++
	f.lastError = errorCode
	return &FeedProjection{Status: FeedProjectionUnknown, ErrorCode: errorCode}, nil
}

type feedProjectionRequestFake struct {
	request *StoredRequest
}

func (f feedProjectionRequestFake) FindByID(context.Context, string) (*StoredRequest, error) {
	if f.request == nil {
		return nil, ErrNotFound
	}
	request := *f.request
	request.Payload = copyPayload(f.request.Payload)
	return &request, nil
}

type feedProjectionSinkFake struct {
	params []FeedSinkParams
	err    error
}

func (f *feedProjectionSinkFake) Project(_ context.Context, params FeedSinkParams) error {
	f.params = append(f.params, params)
	return f.err
}

func TestFeedProjectionWorkerProjectsProviderSubmission(t *testing.T) {
	now := time.Date(2026, time.August, 17, 12, 0, 0, 0, time.UTC)
	queue := &feedProjectionQueueFake{projection: &FeedProjection{
		ID: "projection-1", AttemptID: "attempt-1", NotificationID: "request-1",
		ProviderRequestID: "provider-1", DeliveryStatus: FeedDeliverySubmitted,
		Status: FeedProjectionPending,
	}}
	sink := &feedProjectionSinkFake{}
	worker := NewFeedProjectionWorker(
		queue,
		feedProjectionRequestFake{request: &StoredRequest{
			ID: "request-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser,
			RecipientID: "user-1", Type: "notification.created", Source: "test",
			Payload: map[string]any{"title": "hello"}, RetentionMode: RetentionModeStandard,
			SubmittedAt: &now,
		}},
		sink.Project,
		WithFeedProjectionWorkerClock(func() time.Time { return now }),
	)

	processed, err := worker.RunOnce(context.Background(), "feed-worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want one processed projection", processed, err)
	}
	if queue.projected != 1 || queue.unknown != 0 {
		t.Fatalf("projected=%d unknown=%d, want projected only", queue.projected, queue.unknown)
	}
	if len(sink.params) != 1 {
		t.Fatalf("sink calls=%d, want one", len(sink.params))
	}
	if sink.params[0].RequestID != "request-1" || sink.params[0].ProviderTransactionID != "provider-1" || sink.params[0].DeliveryStatus != FeedDeliverySubmitted {
		t.Fatalf("projected params = %#v", sink.params[0])
	}
}

func TestFeedProjectionWorkerProjectsAlreadyAcknowledgedReceipt(t *testing.T) {
	now := time.Date(2026, time.August, 17, 12, 0, 0, 0, time.UTC)
	deliveredAt := now.Add(time.Second)
	queue := &feedProjectionQueueFake{projection: &FeedProjection{
		ID: "projection-1", AttemptID: "attempt-1", NotificationID: "request-1",
		ProviderRequestID: "provider-1", ProviderReceiptDigest: "receipt-1",
		DeliveryStatus: FeedDeliveryDelivered, DeliveredAt: &deliveredAt,
		Status: FeedProjectionPending,
	}}
	sink := &feedProjectionSinkFake{}
	worker := NewFeedProjectionWorker(
		queue,
		feedProjectionRequestFake{request: &StoredRequest{
			ID: "request-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser,
			RecipientID: "user-1", Type: "notification.created", RetentionMode: RetentionModeStandard,
			SubmittedAt: &now,
		}},
		sink.Project,
	)

	processed, err := worker.RunOnce(context.Background(), "feed-worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want delivered projection", processed, err)
	}
	if len(sink.params) != 1 || sink.params[0].DeliveryStatus != FeedDeliveryDelivered || sink.params[0].DeliveredAt == nil || !sink.params[0].DeliveredAt.Equal(deliveredAt) {
		t.Fatalf("delivered params = %#v, want delivered receipt", sink.params)
	}
}

func TestFeedProjectionWorkerRetainsUnknownOnProjectionFailure(t *testing.T) {
	queue := &feedProjectionQueueFake{projection: &FeedProjection{
		ID: "projection-1", AttemptID: "attempt-1", NotificationID: "request-1",
		ProviderRequestID: "provider-1", DeliveryStatus: FeedDeliverySubmitted,
		Status: FeedProjectionPending,
	}}
	sink := &feedProjectionSinkFake{err: errors.New("feed database unavailable")}
	worker := NewFeedProjectionWorker(
		queue,
		feedProjectionRequestFake{request: &StoredRequest{
			ID: "request-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser,
			RecipientID: "user-1", Type: "notification.created", RetentionMode: RetentionModeStandard,
		}},
		sink.Project,
	)

	processed, err := worker.RunOnce(context.Background(), "feed-worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want handled unknown projection", processed, err)
	}
	if queue.unknown != 1 || queue.lastError != "feed_projection_ambiguous" {
		t.Fatalf("unknown=%d error=%q, want ambiguous projection", queue.unknown, queue.lastError)
	}
	if queue.projected != 0 {
		t.Fatalf("projected=%d, want zero", queue.projected)
	}
}

func TestFeedProjectionWorkerRefusesZDRProjection(t *testing.T) {
	queue := &feedProjectionQueueFake{projection: &FeedProjection{
		ID: "projection-1", AttemptID: "attempt-1", NotificationID: "request-1",
		ProviderRequestID: "provider-1", DeliveryStatus: FeedDeliverySubmitted,
		Status: FeedProjectionPending,
	}}
	sink := &feedProjectionSinkFake{}
	worker := NewFeedProjectionWorker(
		queue,
		feedProjectionRequestFake{request: &StoredRequest{
			ID: "request-1", OrganizationID: "org-1", RecipientKind: RecipientKindUser,
			RecipientID: "user-1", Type: "notification.created", RetentionMode: RetentionModeZDR,
		}},
		sink.Project,
	)

	processed, err := worker.RunOnce(context.Background(), "feed-worker-1")
	if err != nil || !processed {
		t.Fatalf("RunOnce() = (%v, %v), want handled ZDR refusal", processed, err)
	}
	if queue.unknown != 1 || queue.lastError != "zdr_feed_projection_unavailable" {
		t.Fatalf("unknown=%d error=%q, want ZDR refusal", queue.unknown, queue.lastError)
	}
	if len(sink.params) != 0 {
		t.Fatalf("sink calls=%d, want zero for ZDR", len(sink.params))
	}
}
