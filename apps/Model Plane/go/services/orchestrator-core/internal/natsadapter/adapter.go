// Package natsadapter adapts a *nats.Conn to the natsx RawPublisher/RawSubscriber
// interfaces so the orchestrator can participate in dual-write/dual-read compat.
package natsadapter

import (
	"github.com/nats-io/nats.go"

	"github.com/triodelab/model-plane/pkg/natsx"
)

// ConnAdapter wraps a *nats.Conn to satisfy natsx.RawPublisher and natsx.RawSubscriber.
type ConnAdapter struct {
	nc *nats.Conn
}

// New constructs a ConnAdapter around the provided NATS connection.
func New(nc *nats.Conn) *ConnAdapter {
	return &ConnAdapter{nc: nc}
}

// Publish sends raw bytes to the given subject.
func (a *ConnAdapter) Publish(subject string, data []byte) error {
	return a.nc.Publish(subject, data)
}

// Subscribe registers a handler for the given subject. The returned
// *nats.Subscription natively satisfies natsx.Subscription via Unsubscribe() error.
func (a *ConnAdapter) Subscribe(subject string, h natsx.RawMsgHandler) (natsx.Subscription, error) {
	return a.nc.Subscribe(subject, func(m *nats.Msg) {
		h(m.Subject, m.Data)
	})
}
