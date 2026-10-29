// Package delivery provides a durable outbox for outbound channel deliveries.
//
// bridge-core bridges operator sessions to external channels. Delivering a
// response to an external channel (an HTTP webhook, a chat-platform incoming
// webhook, etc.) is a network operation that can fail transiently. To give the
// delivery durable, at-least-once semantics we enqueue every delivery into an
// outbox and a background worker drains it with bounded exponential backoff.
//
// The Store is an interface so the backing storage can evolve. bridge-core has
// no database wiring today, so the default implementation is in-memory
// (MemoryStore). A Postgres-backed implementation can be added later by
// satisfying the same interface — the worker and adapter code do not change.
package delivery

import (
	"context"
	"errors"
	"time"
)

// Status is the lifecycle state of an outbox record.
type Status string

const (
	// StatusPending means the record is queued and eligible for delivery once
	// NextAttemptAt has passed.
	StatusPending Status = "pending"
	// StatusDelivered means the record was delivered successfully.
	StatusDelivered Status = "delivered"
	// StatusDead means the record exhausted its retry budget and was moved to
	// the dead-letter state. It will not be retried automatically.
	StatusDead Status = "dead"
)

// ErrNotFound is returned when a record ID is not present in the store.
var ErrNotFound = errors.New("delivery record not found")

// Record is a single durable outbound-delivery attempt. It carries the target
// channel, the session it belongs to, the destination, and the payload along
// with retry bookkeeping.
type Record struct {
	ID            string    `json:"id"`
	SessionID     string    `json:"session_id"`
	Channel       string    `json:"channel"`
	Destination   string    `json:"destination"`
	Payload       []byte    `json:"payload"`
	Status        Status    `json:"status"`
	Attempts      int       `json:"attempts"`
	MaxAttempts   int       `json:"max_attempts"`
	LastError     string    `json:"last_error,omitempty"`
	CreatedAt     time.Time `json:"created_at"`
	UpdatedAt     time.Time `json:"updated_at"`
	NextAttemptAt time.Time `json:"next_attempt_at"`
}

// Store is the durable persistence contract for the delivery outbox.
// Implementations must be safe for concurrent use.
type Store interface {
	// Enqueue inserts a new pending record and returns it (with ID populated).
	Enqueue(ctx context.Context, rec Record) (Record, error)
	// ClaimDue atomically claims up to limit pending records whose
	// NextAttemptAt is at or before now, returning the claimed records. A
	// claimed record must not be returned to another concurrent ClaimDue call
	// until it is released back to pending (MarkRetry) or finalised.
	ClaimDue(ctx context.Context, now time.Time, limit int) ([]Record, error)
	// MarkDelivered finalises a record as delivered.
	MarkDelivered(ctx context.Context, id string) error
	// MarkRetry records a failed attempt, stores the error, and re-arms the
	// record for a future attempt at nextAttemptAt (status returns to pending).
	MarkRetry(ctx context.Context, id string, attemptErr string, nextAttemptAt time.Time) error
	// MarkDead moves a record to the dead-letter state after exhausting retries.
	MarkDead(ctx context.Context, id string, attemptErr string) error
	// Get returns a record by ID, or ErrNotFound.
	Get(ctx context.Context, id string) (Record, error)
	// PendingCount returns the number of records not yet delivered or dead.
	// Used for readiness/health reporting and tests.
	PendingCount(ctx context.Context) (int, error)
}
