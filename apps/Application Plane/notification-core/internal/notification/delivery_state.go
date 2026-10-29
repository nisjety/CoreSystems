package notification

import (
	"context"
	"time"
)

// DeliveryAttemptStatus is the durable state of one provider delivery try.
// The state is intentionally separate from notification_requests.status:
// provider acceptance is not delivery confirmation, and an ambiguous provider
// response must remain visible instead of being retried blindly.
type DeliveryAttemptStatus string

const (
	DeliveryAttemptPending         DeliveryAttemptStatus = "pending"
	DeliveryAttemptClaimed         DeliveryAttemptStatus = "claimed"
	DeliveryAttemptSentUnconfirmed DeliveryAttemptStatus = "sent_unconfirmed"
	DeliveryAttemptAcknowledged    DeliveryAttemptStatus = "acknowledged"
	DeliveryAttemptFailed          DeliveryAttemptStatus = "failed"
	DeliveryAttemptUnknown         DeliveryAttemptStatus = "unknown"
)

// DeliveryAttempt contains only control metadata and provider correlation.
// Payload content stays on notification_requests and is deliberately absent
// here so ZDR attempts remain content-free.
type DeliveryAttempt struct {
	ID                    string                `json:"id"`
	NotificationID        string                `json:"notification_id"`
	AttemptNumber         int                   `json:"attempt_number"`
	Status                DeliveryAttemptStatus `json:"status"`
	WorkerID              string                `json:"worker_id,omitempty"`
	LeaseExpiresAt        *time.Time            `json:"lease_expires_at,omitempty"`
	ProviderRequestID     string                `json:"provider_request_id,omitempty"`
	ProviderReceiptDigest string                `json:"provider_receipt_digest,omitempty"`
	ErrorCode             string                `json:"error_code,omitempty"`
	CreatedAt             time.Time             `json:"created_at"`
	UpdatedAt             time.Time             `json:"updated_at"`
	SubmittedAt           *time.Time            `json:"submitted_at,omitempty"`
	AcknowledgedAt        *time.Time            `json:"acknowledged_at,omitempty"`
}

type DeliveryAttemptParams struct {
	ID             string
	NotificationID string
	AttemptNumber  int
	OccurredAt     time.Time
}

// DeliveryQueue is deliberately additive to Repository so existing callers
// can continue to use the synchronous compatibility path while the durable
// worker is rolled out behind an explicit wiring change.
type DeliveryQueue interface {
	EnqueueDeliveryAttempt(context.Context, DeliveryAttemptParams) (*DeliveryAttempt, error)
	ClaimDeliveryAttempt(context.Context, string, time.Time, time.Duration) (*DeliveryAttempt, error)
	MarkDeliverySubmitted(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error)
	MarkDeliveryUnknown(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error)
	MarkDeliveryAcknowledged(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error)
	MarkDeliveryFailed(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error)
}

// CanTransitionDeliveryAttempt is the only state transition policy. Claim
// expiry is handled by the repository's lease query, not by allowing an
// ambiguous provider result to jump back to claimed.
func CanTransitionDeliveryAttempt(from, to DeliveryAttemptStatus) bool {
	switch from {
	case DeliveryAttemptPending:
		return to == DeliveryAttemptClaimed || to == DeliveryAttemptFailed
	case DeliveryAttemptClaimed:
		return to == DeliveryAttemptSentUnconfirmed || to == DeliveryAttemptFailed || to == DeliveryAttemptUnknown
	case DeliveryAttemptSentUnconfirmed:
		return to == DeliveryAttemptAcknowledged || to == DeliveryAttemptFailed || to == DeliveryAttemptUnknown
	case DeliveryAttemptUnknown:
		return to == DeliveryAttemptAcknowledged || to == DeliveryAttemptFailed
	default:
		return false
	}
}

func (attempt DeliveryAttempt) LeaseExpired(now time.Time) bool {
	return attempt.Status == DeliveryAttemptClaimed &&
		attempt.LeaseExpiresAt != nil &&
		!attempt.LeaseExpiresAt.After(now.UTC())
}
