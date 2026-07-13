// Package channel defines the ingress adapter interface and a registry of
// channel adapters used by bridge-core. Each ingress channel (CLI, VS Code,
// web, API) implements the Adapter interface.
package channel

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"time"
)

var ErrWebSocketUnavailable = errors.New("websocket transport is not configured")

// timeNow is a package-level variable for time.Now so tests can override it.
var timeNow = time.Now

// Adapter is the contract every ingress channel must satisfy. Ingest accepts a
// raw payload from the channel and returns a processed result. Deliver pushes a
// response back to the connected client.
type Adapter interface {
	// Ingest processes an incoming payload from the channel.
	Ingest(ctx context.Context, sessionID string, payload []byte) ([]byte, error)
	// Deliver sends a response back through the channel.
	Deliver(ctx context.Context, sessionID string, response []byte) error
}

// NoopAdapter is a pass-through adapter used as the default for channels that
// are not yet wired to a real implementation. Ingest echoes the payload;
// Deliver logs and discards.
type NoopAdapter struct{}

// Ingest returns the payload unchanged.
func (NoopAdapter) Ingest(_ context.Context, _ string, payload []byte) ([]byte, error) {
	return payload, nil
}

// Deliver logs the delivery attempt and returns nil.
func (NoopAdapter) Deliver(_ context.Context, sessionID string, response []byte) error {
	slog.Debug("noop deliver", "session_id", sessionID, "response_len", len(response))
	return nil
}

// AdapterRegistry maps channel names to their adapter implementations.
type AdapterRegistry struct {
	mu       sync.RWMutex
	adapters map[string]Adapter
	fallback Adapter
}

// NewAdapterRegistry creates a registry pre-populated with a NoopAdapter
// fallback for every known channel.
func NewAdapterRegistry() *AdapterRegistry {
	noop := NoopAdapter{}
	return &AdapterRegistry{
		adapters: map[string]Adapter{
			"cli":    noop,
			"vscode": noop,
			"web":    noop,
			"api":    noop,
		},
		fallback: noop,
	}
}

// Register replaces the adapter for the named channel. Passing nil reverts the
// channel to the fallback (noop) adapter.
func (r *AdapterRegistry) Register(channel string, a Adapter) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if a == nil {
		a = r.fallback
	}
	r.adapters[channel] = a
}

// Get returns the adapter for the named channel, falling back to the noop
// adapter if no adapter is registered.
func (r *AdapterRegistry) Get(channel string) (Adapter, error) {
	r.mu.RLock()
	a, ok := r.adapters[channel]
	r.mu.RUnlock()
	if !ok {
		return nil, fmt.Errorf("unknown channel %q", channel)
	}
	return a, nil
}

// WebSocketAdapter is a quarantined skeleton. No WebSocket upgrade protocol is
// exposed until it can authenticate an Auth Core bearer before session access.
type WebSocketAdapter struct {
	codec FrameCodec
}

// NewWebSocketAdapter constructs the fail-closed skeleton.
func NewWebSocketAdapter(codec FrameCodec) *WebSocketAdapter {
	return &WebSocketAdapter{codec: codec}
}

// Ingest intentionally rejects every payload because the current frame
// contract cannot carry a verifiable Auth Core identity.
func (ws *WebSocketAdapter) Ingest(_ context.Context, _ string, _ []byte) ([]byte, error) {
	// No HTTP upgrade path exists and MessageFrame has no bearer field. Keep the
	// skeleton fail-closed until Auth Core identity can be verified before a
	// connection is registered or a session is resolved.
	return nil, ErrWebSocketUnavailable
}

// Deliver wraps the response bytes in a MessageFrame and encodes it for
// transmission over the WebSocket. This is a skeleton — the actual write to
// the WebSocket connection will be added later.
func (ws *WebSocketAdapter) Deliver(_ context.Context, sessionID string, response []byte) error {
	frame := MessageFrame{
		Type:      "text",
		Payload:   response,
		SessionID: sessionID,
		Timestamp: timeNow(),
	}

	encoded, err := ws.codec.Encode(frame)
	if err != nil {
		return fmt.Errorf("websocket deliver: %w", err)
	}

	slog.Debug("websocket deliver", "session_id", sessionID, "encoded_len", len(encoded))
	return nil
}
