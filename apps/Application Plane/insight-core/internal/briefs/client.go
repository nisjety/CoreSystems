package briefs

import (
	"context"
)

// NotificationRequest is an internal scheduler value. It is deliberately not a
// notification-core wire contract: RecipientID is unresolved until Control
// provides an authoritative organization subscription mapping.
type NotificationRequest struct {
	IdempotencyKey string         `json:"idempotency_key,omitempty"`
	RecipientID    string         `json:"recipient_id"`
	Type           string         `json:"type"`
	Payload        map[string]any `json:"payload"`
	Source         string         `json:"source,omitempty"`
}

// NotificationClient is the dormant scheduler's abstract output. No production
// notification-core adapter implements it: an organization id is not a user
// recipient, and wiring remains disabled until Control supplies a subscription
// mapping.
type NotificationClient interface {
	Send(ctx context.Context, req NotificationRequest) error
}
