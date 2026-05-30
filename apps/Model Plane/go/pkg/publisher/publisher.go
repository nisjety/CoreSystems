// Package publisher defines the EventPublisher interface and error types
// for publishing envelopes to a message bus. Mirrors the Rust mp-events
// publisher module for cross-language parity.
package publisher

import (
	"context"
	"fmt"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// EventPublisher publishes envelopes to a named subject.
//
// Implementations must be safe for concurrent use.
type EventPublisher interface {
	Publish(ctx context.Context, subject string, env *envelope.Envelope) error
}

// ErrorKind classifies a PublishError.
type ErrorKind int

const (
	// KindSerialization indicates the envelope could not be serialized.
	KindSerialization ErrorKind = iota
	// KindTransport indicates the underlying transport failed.
	KindTransport
)

func (k ErrorKind) String() string {
	switch k {
	case KindSerialization:
		return "serialization"
	case KindTransport:
		return "transport"
	default:
		return "unknown"
	}
}

// PublishError is returned by EventPublisher.Publish on failure. Mirrors
// the Rust PublishError enum: Serialization wraps a JSON error, Transport
// wraps an underlying transport error.
type PublishError struct {
	Kind ErrorKind
	Err  error
}

// Error implements the error interface.
func (e *PublishError) Error() string {
	return fmt.Sprintf("%s error: %s", e.Kind, e.Err)
}

// Unwrap returns the wrapped error for errors.Is / errors.As.
func (e *PublishError) Unwrap() error {
	return e.Err
}

// NewSerializationError wraps err as a serialization PublishError.
func NewSerializationError(err error) *PublishError {
	return &PublishError{Kind: KindSerialization, Err: err}
}

// NewTransportError wraps err as a transport PublishError.
func NewTransportError(err error) *PublishError {
	return &PublishError{Kind: KindTransport, Err: err}
}
