package notification

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"
)

type DeliveryRequestRepository interface {
	FindByID(context.Context, string) (*StoredRequest, error)
}

type DeliveryWorker struct {
	requestRepository DeliveryRequestRepository
	queue             DeliveryQueue
	runtimeClient     RuntimeClient
	recipientResolver RecipientResolver
	now               TimeSource
	lease             time.Duration
}

type DeliveryWorkerOption func(*DeliveryWorker)

func WithDeliveryWorkerClock(now TimeSource) DeliveryWorkerOption {
	return func(worker *DeliveryWorker) {
		if now != nil {
			worker.now = now
		}
	}
}

func WithDeliveryWorkerLease(lease time.Duration) DeliveryWorkerOption {
	return func(worker *DeliveryWorker) {
		if lease > 0 {
			worker.lease = lease
		}
	}
}

func NewDeliveryWorker(
	requestRepository DeliveryRequestRepository,
	queue DeliveryQueue,
	runtimeClient RuntimeClient,
	recipientResolver RecipientResolver,
	opts ...DeliveryWorkerOption,
) *DeliveryWorker {
	worker := &DeliveryWorker{
		requestRepository: requestRepository,
		queue:             queue,
		runtimeClient:     runtimeClient,
		recipientResolver: recipientResolver,
		now:               time.Now,
		lease:             30 * time.Second,
	}
	for _, opt := range opts {
		opt(worker)
	}
	return worker
}

// RunOnce claims and processes at most one attempt. Provider errors are
// intentionally recorded as unknown because the request may have reached the
// provider before the transport failed. Only a provider receipt or explicit
// operator/provider reconciliation may settle that state.
func (worker *DeliveryWorker) RunOnce(ctx context.Context, workerID string) (bool, error) {
	if worker == nil || worker.queue == nil || worker.requestRepository == nil || worker.runtimeClient == nil {
		return false, errors.New("delivery worker is not configured")
	}
	now := worker.now().UTC()
	attempt, err := worker.queue.ClaimDeliveryAttempt(ctx, workerID, now, worker.lease)
	if errors.Is(err, ErrNotFound) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("claim delivery attempt: %w", err)
	}
	if attempt == nil || strings.TrimSpace(attempt.ID) == "" || strings.TrimSpace(attempt.NotificationID) == "" {
		return false, errors.New("claimed delivery attempt is malformed")
	}

	request, err := worker.requestRepository.FindByID(ctx, attempt.NotificationID)
	if err != nil {
		_, markErr := worker.queue.MarkDeliveryFailed(ctx, attempt.ID, workerID, "notification_request_unavailable", now)
		if markErr != nil {
			return true, fmt.Errorf("load notification request: %w; mark failed: %v", err, markErr)
		}
		return true, nil
	}
	if request == nil || request.RetentionMode == RetentionModeZDR {
		_, markErr := worker.queue.MarkDeliveryFailed(ctx, attempt.ID, workerID, "zdr_async_delivery_unavailable", now)
		if markErr != nil {
			return true, fmt.Errorf("reject ZDR delivery attempt: %w", markErr)
		}
		return true, nil
	}
	if worker.recipientResolver == nil {
		_, markErr := worker.queue.MarkDeliveryFailed(ctx, attempt.ID, workerID, "recipient_resolver_unavailable", now)
		if markErr != nil {
			return true, fmt.Errorf("recipient resolver unavailable: %w", markErr)
		}
		return true, nil
	}
	resolvedRecipient, err := worker.recipientResolver.ResolveRecipient(ctx, request.OrganizationID, Recipient{
		Kind: request.RecipientKind,
		ID:   request.RecipientID,
	})
	if err != nil || resolvedRecipient == nil {
		_, markErr := worker.queue.MarkDeliveryFailed(ctx, attempt.ID, workerID, "recipient_not_authorized", now)
		if markErr != nil {
			return true, fmt.Errorf("resolve recipient: %w; mark failed: %v", err, markErr)
		}
		return true, nil
	}

	dispatchResult, err := worker.runtimeClient.Dispatch(ctx, DeliveryRequest{
		RequestID:           request.ID,
		OrganizationID:      request.OrganizationID,
		RecipientKind:       request.RecipientKind,
		RecipientID:         request.RecipientID,
		ProviderRecipientID: resolvedRecipient.ProviderSubscriberID,
		Type:                request.Type,
		Payload:             copyPayload(request.Payload),
		Source:              request.Source,
		RetentionMode:       request.RetentionMode,
	})
	if err != nil {
		_, markErr := worker.queue.MarkDeliveryUnknown(ctx, attempt.ID, workerID, "provider_dispatch_ambiguous", now)
		if markErr != nil {
			return true, fmt.Errorf("dispatch notification: %w; mark unknown: %v", err, markErr)
		}
		return true, nil
	}
	providerRequestID := ""
	if dispatchResult != nil {
		providerRequestID = strings.TrimSpace(dispatchResult.ProviderRequestID)
	}
	if providerRequestID == "" {
		_, markErr := worker.queue.MarkDeliveryUnknown(ctx, attempt.ID, workerID, "provider_request_id_missing", now)
		if markErr != nil {
			return true, fmt.Errorf("provider request id missing; mark unknown: %w", markErr)
		}
		return true, nil
	}
	if _, err := worker.queue.MarkDeliverySubmitted(ctx, attempt.ID, workerID, providerRequestID, now); err != nil {
		return true, fmt.Errorf("mark delivery sent_unconfirmed: %w", err)
	}
	return true, nil
}
