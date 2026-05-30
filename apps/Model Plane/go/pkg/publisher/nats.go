package publisher

import (
	"context"
	"encoding/json"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// RawPublisher is the minimal byte-level publish surface that NATSPublisher
// delegates to after JSON-serializing an Envelope. This is intentionally a
// duck-typed interface so callers can inject *natsx.Publisher, *nats.Conn,
// or any test double without pkg/publisher depending on nats.go.
type RawPublisher interface {
	Publish(subject string, data []byte) error
}

// NATSPublisher is an envelope-aware EventPublisher that serializes the
// envelope to JSON and delegates transport to a RawPublisher.
type NATSPublisher struct {
	raw RawPublisher
}

// NewNATSPublisher constructs a NATSPublisher that delegates to raw.
func NewNATSPublisher(raw RawPublisher) *NATSPublisher {
	return &NATSPublisher{raw: raw}
}

// Publish serializes env as JSON and delegates to the underlying raw
// publisher. A nil envelope yields a KindSerialization PublishError.
// Marshal failures yield KindSerialization; transport failures yield
// KindTransport with the underlying error wrapped.
func (p *NATSPublisher) Publish(_ context.Context, subject string, env *envelope.Envelope) error {
	if env == nil {
		return NewSerializationError(errNilEnvelope)
	}
	data, err := json.Marshal(env)
	if err != nil {
		return NewSerializationError(err)
	}
	if err := p.raw.Publish(subject, data); err != nil {
		return NewTransportError(err)
	}
	return nil
}
