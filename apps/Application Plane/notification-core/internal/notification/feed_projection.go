package notification

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"
)

// FeedProjectionStatus is the durable state of the local Activity/Inbox
// projection. It is separate from provider delivery: a provider receipt can
// arrive before or after the local projection worker and must remain
// replayable without duplicating the user-visible item.
type FeedProjectionStatus string

const (
	FeedProjectionPending   FeedProjectionStatus = "pending"
	FeedProjectionClaimed   FeedProjectionStatus = "claimed"
	FeedProjectionProjected FeedProjectionStatus = "projected"
	FeedProjectionUnknown   FeedProjectionStatus = "unknown"

	FeedDeliverySubmitted = "submitted"
	FeedDeliveryDelivered = "delivered"
)

// FeedProjection contains control metadata only. The notification payload is
// loaded from notification_requests by the worker and is never copied into
// the projection queue, preserving the existing ZDR boundary.
type FeedProjection struct {
	ID                    string
	AttemptID             string
	NotificationID        string
	ProviderRequestID     string
	ProviderReceiptDigest string
	Status                FeedProjectionStatus
	DeliveryStatus        string
	WorkerID              string
	LeaseExpiresAt        *time.Time
	ErrorCode             string
	CreatedAt             time.Time
	UpdatedAt             time.Time
	SubmittedAt           *time.Time
	DeliveredAt           *time.Time
	NextAttemptAt         time.Time
}

// FeedProjectionQueue is the lease-fenced control store for the local feed
// projection. PGRepository implements it together with DeliveryQueue.
type FeedProjectionQueue interface {
	ClaimFeedProjection(context.Context, string, time.Time, time.Duration) (*FeedProjection, error)
	MarkFeedProjectionProjected(context.Context, string, string, time.Time) (*FeedProjection, error)
	MarkFeedProjectionUnknown(context.Context, string, string, string, time.Time) (*FeedProjection, error)
}

// FeedProjector is deliberately independent of the feed package so the
// notification state machine does not own the Application feed schema.
type FeedProjector func(context.Context, FeedSinkParams) error

type FeedProjectionWorker struct {
	queue             FeedProjectionQueue
	requestRepository DeliveryRequestRepository
	project           FeedProjector
	now               TimeSource
	lease             time.Duration
}

type FeedProjectionWorkerOption func(*FeedProjectionWorker)

func WithFeedProjectionWorkerClock(now TimeSource) FeedProjectionWorkerOption {
	return func(worker *FeedProjectionWorker) {
		if now != nil {
			worker.now = now
		}
	}
}

func WithFeedProjectionWorkerLease(lease time.Duration) FeedProjectionWorkerOption {
	return func(worker *FeedProjectionWorker) {
		if lease > 0 {
			worker.lease = lease
		}
	}
}

func NewFeedProjectionWorker(
	queue FeedProjectionQueue,
	requestRepository DeliveryRequestRepository,
	project FeedProjector,
	opts ...FeedProjectionWorkerOption,
) *FeedProjectionWorker {
	worker := &FeedProjectionWorker{
		queue:             queue,
		requestRepository: requestRepository,
		project:           project,
		now:               time.Now,
		lease:             30 * time.Second,
	}
	for _, opt := range opts {
		opt(worker)
	}
	return worker
}

// RunOnce claims and projects one provider-submitted notification. Any
// projector error is retained as unknown: the feed write may have committed
// before the transport returned the error, so retrying the idempotent Create
// path is the only safe recovery.
func (worker *FeedProjectionWorker) RunOnce(ctx context.Context, workerID string) (bool, error) {
	if worker == nil || worker.queue == nil || worker.requestRepository == nil || worker.project == nil {
		return false, errors.New("feed projection worker is not configured")
	}
	now := worker.now().UTC()
	projection, err := worker.queue.ClaimFeedProjection(ctx, workerID, now, worker.lease)
	if errors.Is(err, ErrNotFound) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("claim feed projection: %w", err)
	}
	if projection == nil || strings.TrimSpace(projection.ID) == "" || strings.TrimSpace(projection.NotificationID) == "" {
		return false, errors.New("claimed feed projection is malformed")
	}

	request, err := worker.requestRepository.FindByID(ctx, projection.NotificationID)
	if err != nil {
		_, markErr := worker.queue.MarkFeedProjectionUnknown(ctx, projection.ID, workerID, "notification_request_unavailable", now)
		if markErr != nil {
			return true, fmt.Errorf("load notification request: %w; mark unknown: %v", err, markErr)
		}
		return true, nil
	}
	if request == nil || request.RetentionMode == RetentionModeZDR {
		_, markErr := worker.queue.MarkFeedProjectionUnknown(ctx, projection.ID, workerID, "zdr_feed_projection_unavailable", now)
		if markErr != nil {
			return true, fmt.Errorf("reject ZDR feed projection: %w", markErr)
		}
		return true, nil
	}

	submittedAt := now
	if request.SubmittedAt != nil && !request.SubmittedAt.IsZero() {
		submittedAt = request.SubmittedAt.UTC()
	}
	params := FeedSinkParams{
		RequestID:             request.ID,
		OrganizationID:        request.OrganizationID,
		RecipientID:           request.RecipientID,
		Type:                  request.Type,
		Payload:               copyPayload(request.Payload),
		Provider:              request.Provider,
		ProviderTransactionID: projection.ProviderRequestID,
		Source:                request.Source,
		DeliveryStatus:        projection.DeliveryStatus,
		SubmittedAt:           submittedAt,
		DeliveredAt:           projection.DeliveredAt,
	}
	if params.DeliveryStatus == "" {
		params.DeliveryStatus = FeedDeliverySubmitted
	}
	if params.DeliveryStatus == FeedDeliveryDelivered && params.DeliveredAt == nil {
		params.DeliveredAt = &now
	}
	extractDisplayFields(request.Payload, &params)

	if err := worker.project(ctx, params); err != nil {
		_, markErr := worker.queue.MarkFeedProjectionUnknown(ctx, projection.ID, workerID, "feed_projection_ambiguous", now)
		if markErr != nil {
			return true, fmt.Errorf("project feed: %w; mark unknown: %v", err, markErr)
		}
		return true, nil
	}
	if _, err := worker.queue.MarkFeedProjectionProjected(ctx, projection.ID, workerID, now); err != nil {
		_, markErr := worker.queue.MarkFeedProjectionUnknown(ctx, projection.ID, workerID, "feed_projection_receipt_unavailable", now)
		if markErr != nil {
			return true, fmt.Errorf("mark feed projection projected: %w; mark unknown: %v", err, markErr)
		}
		return true, nil
	}
	return true, nil
}
