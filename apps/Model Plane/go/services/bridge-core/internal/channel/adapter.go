// Package channel defines the ingress adapter interface and a registry of
// channel adapters used by bridge-core. Each ingress channel (CLI, VS Code,
// web, API) implements the Adapter interface.
package channel

import (
	"context"
	"fmt"
	"log/slog"
	"sync"
	"time"
)

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

// WebSocketAdapter is a channel adapter for WebSocket connections. It
// validates incoming tokens via JWTValidator and encodes/decodes wire frames
// using a FrameCodec. This is a skeleton — the actual WebSocket upgrade and
// read/write loop will be implemented in a future iteration.
type WebSocketAdapter struct {
	jwt   *JWTValidator
	codec FrameCodec
}

// NewWebSocketAdapter constructs a WebSocketAdapter with the given JWT
// validator and frame codec.
func NewWebSocketAdapter(jwt *JWTValidator, codec FrameCodec) *WebSocketAdapter {
	return &WebSocketAdapter{jwt: jwt, codec: codec}
}

// Ingest validates the payload as a framed message, verifies the JWT token
// embedded in the session, and returns the decoded payload. This is a
// skeleton that decodes the frame and echoes the payload back.
func (ws *WebSocketAdapter) Ingest(_ context.Context, sessionID string, payload []byte) ([]byte, error) {
	frame, err := ws.codec.Decode(payload)
	if err != nil {
		return nil, fmt.Errorf("websocket ingest: %w", err)
	}

	if frame.SessionID != sessionID {
		return nil, fmt.Errorf("websocket ingest: session ID mismatch: frame=%q expected=%q", frame.SessionID, sessionID)
	}

	slog.Debug("websocket ingest", "session_id", sessionID, "frame_type", frame.Type, "payload_len", len(frame.Payload))
	return frame.Payload, nil
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
