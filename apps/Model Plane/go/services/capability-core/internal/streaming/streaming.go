// Package streaming provides the Go-side contract and runtime primitives for
// streaming-inference events produced by the Rust model-gateway. It defines
// the canonical SSE event schema, a bounded-buffer token streamer with
// high/low watermark backpressure, and SSE wire-format helpers used by any
// capability-core HTTP handler that re-broadcasts gateway tokens.
package streaming

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
)

// EventType enumerates the SSE event types emitted by a Streamer.
type EventType string

const (
	EventStart EventType = "start"
	EventToken EventType = "token"
	EventEnd   EventType = "end"
	EventError EventType = "error"
)

// Event is a single streaming-inference event. Seq is assigned monotonically
// by the Streamer on Publish and used as the SSE "id:" field.
type Event struct {
	Type EventType
	Data string
	Seq  uint64
}

// BackpressureConfig configures the bounded-buffer backpressure policy.
//
//   - HighWatermark: when buffered events reach this count, Paused() returns
//     true, signalling upstream producers to throttle.
//   - LowWatermark: once the buffer drains to this count, Paused() returns
//     false again.
//   - MaxBufferedTokens: hard cap. Publish returns ErrBackpressure when the
//     buffer already holds this many events.
type BackpressureConfig struct {
	HighWatermark     int
	LowWatermark      int
	MaxBufferedTokens int
}

// Sentinel errors returned by Streamer operations.
var (
	ErrBackpressure    = errors.New("streaming: backpressure limit exceeded")
	ErrClosed          = errors.New("streaming: stream closed")
	ErrInvalidConfig   = errors.New("streaming: invalid backpressure config")
	ErrInvalidEventTyp = errors.New("streaming: invalid event type")
)

// Streamer is a concurrent-safe bounded-buffer event streamer.
type Streamer struct {
	cfg    BackpressureConfig
	mu     sync.Mutex
	cond   *sync.Cond
	buf    []Event
	seq    uint64
	closed bool
}

// NewStreamer constructs a Streamer, validating the backpressure config.
func NewStreamer(cfg BackpressureConfig) (*Streamer, error) {
	if cfg.MaxBufferedTokens <= 0 {
		return nil, fmt.Errorf("%w: MaxBufferedTokens must be > 0", ErrInvalidConfig)
	}
	if cfg.HighWatermark <= 0 || cfg.HighWatermark > cfg.MaxBufferedTokens {
		return nil, fmt.Errorf("%w: HighWatermark must be in (0, MaxBufferedTokens]", ErrInvalidConfig)
	}
	if cfg.LowWatermark < 0 || cfg.LowWatermark >= cfg.HighWatermark {
		return nil, fmt.Errorf("%w: LowWatermark must be in [0, HighWatermark)", ErrInvalidConfig)
	}
	s := &Streamer{cfg: cfg}
	s.cond = sync.NewCond(&s.mu)
	return s, nil
}

func validEventType(t EventType) bool {
	switch t {
	case EventStart, EventToken, EventEnd, EventError:
		return true
	}
	return false
}

// Publish appends ev to the buffer and assigns a monotonic Seq. Returns
// ErrBackpressure when the buffer is full, ErrClosed after Close.
func (s *Streamer) Publish(ev Event) (Event, error) {
	if !validEventType(ev.Type) {
		return Event{}, ErrInvalidEventTyp
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return Event{}, ErrClosed
	}
	if len(s.buf) >= s.cfg.MaxBufferedTokens {
		return Event{}, ErrBackpressure
	}
	s.seq++
	ev.Seq = s.seq
	s.buf = append(s.buf, ev)
	s.cond.Broadcast()
	return ev, nil
}

// Next blocks until an event is available, the context is cancelled, or the
// stream is closed.
func (s *Streamer) Next(ctx context.Context) (Event, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	// Wake waiters on ctx cancellation.
	stop := context.AfterFunc(ctx, func() {
		s.mu.Lock()
		s.cond.Broadcast()
		s.mu.Unlock()
	})
	defer stop()

	for len(s.buf) == 0 {
		if s.closed {
			return Event{}, ErrClosed
		}
		if err := ctx.Err(); err != nil {
			return Event{}, err
		}
		s.cond.Wait()
	}
	ev := s.buf[0]
	s.buf = s.buf[1:]
	s.cond.Broadcast()
	return ev, nil
}

// Paused reports whether producers should throttle per high/low watermarks.
func (s *Streamer) Paused() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.buf) >= s.cfg.HighWatermark
}

// Buffered returns the current buffered event count.
func (s *Streamer) Buffered() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.buf)
}

// Close permanently closes the Streamer; pending Next callers return ErrClosed.
func (s *Streamer) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	s.closed = true
	s.cond.Broadcast()
	return nil
}

// FormatSSE renders ev in SSE wire format: "event: <type>\ndata: <data>\nid: <seq>\n\n".
// Multi-line Data is prefixed with "data: " per SSE spec.
func FormatSSE(ev Event) string {
	var b strings.Builder
	b.WriteString("event: ")
	b.WriteString(string(ev.Type))
	b.WriteByte('\n')
	for _, line := range strings.Split(ev.Data, "\n") {
		b.WriteString("data: ")
		b.WriteString(line)
		b.WriteByte('\n')
	}
	fmt.Fprintf(&b, "id: %d\n\n", ev.Seq)
	return b.String()
}
