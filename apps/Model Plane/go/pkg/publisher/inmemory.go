package publisher

import (
	"context"
	"sync"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// PublishedRecord is a (subject, envelope) pair recorded by InMemoryPublisher.
type PublishedRecord struct {
	Subject  string
	Envelope envelope.Envelope
}

// InMemoryPublisher records published envelopes for testing and local
// development. Mirrors the Rust InMemoryPublisher.
type InMemoryPublisher struct {
	mu        sync.Mutex
	published []PublishedRecord
}

// NewInMemoryPublisher creates a new empty InMemoryPublisher.
func NewInMemoryPublisher() *InMemoryPublisher {
	return &InMemoryPublisher{}
}

// Publish records the (subject, envelope) pair. Always returns nil.
func (p *InMemoryPublisher) Publish(_ context.Context, subject string, env *envelope.Envelope) error {
	if env == nil {
		return NewSerializationError(errNilEnvelope)
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	p.published = append(p.published, PublishedRecord{
		Subject:  subject,
		Envelope: *env,
	})
	return nil
}

// Drain returns all recorded records and clears the internal buffer.
func (p *InMemoryPublisher) Drain() []PublishedRecord {
	p.mu.Lock()
	defer p.mu.Unlock()
	drained := p.published
	p.published = nil
	return drained
}
