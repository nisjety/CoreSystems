package streaming

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"
)

func newTestStreamer(t *testing.T) *Streamer {
	t.Helper()
	s, err := NewStreamer(BackpressureConfig{
		HighWatermark:     4,
		LowWatermark:      2,
		MaxBufferedTokens: 8,
	})
	if err != nil {
		t.Fatalf("NewStreamer: %v", err)
	}
	return s
}

func TestNewStreamer_ValidatesConfig(t *testing.T) {
	cases := []struct {
		name string
		cfg  BackpressureConfig
	}{
		{"zero max", BackpressureConfig{HighWatermark: 1, LowWatermark: 0, MaxBufferedTokens: 0}},
		{"high > max", BackpressureConfig{HighWatermark: 10, LowWatermark: 1, MaxBufferedTokens: 5}},
		{"low >= high", BackpressureConfig{HighWatermark: 3, LowWatermark: 3, MaxBufferedTokens: 5}},
		{"neg low", BackpressureConfig{HighWatermark: 3, LowWatermark: -1, MaxBufferedTokens: 5}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := NewStreamer(tc.cfg); !errors.Is(err, ErrInvalidConfig) {
				t.Fatalf("want ErrInvalidConfig, got %v", err)
			}
		})
	}
}

func TestStreamer_PublishConsumeAssignsSeq(t *testing.T) {
	s := newTestStreamer(t)
	ctx := context.Background()

	for i, data := range []string{"a", "b", "c"} {
		ev, err := s.Publish(Event{Type: EventToken, Data: data})
		if err != nil {
			t.Fatalf("publish %d: %v", i, err)
		}
		if ev.Seq != uint64(i+1) {
			t.Fatalf("seq: want %d got %d", i+1, ev.Seq)
		}
	}

	for i, want := range []string{"a", "b", "c"} {
		ev, err := s.Next(ctx)
		if err != nil {
			t.Fatalf("next %d: %v", i, err)
		}
		if ev.Data != want || ev.Seq != uint64(i+1) {
			t.Fatalf("event %d: %+v", i, ev)
		}
	}
}

func TestStreamer_RejectsInvalidEventType(t *testing.T) {
	s := newTestStreamer(t)
	if _, err := s.Publish(Event{Type: "bogus", Data: "x"}); !errors.Is(err, ErrInvalidEventTyp) {
		t.Fatalf("want ErrInvalidEventTyp, got %v", err)
	}
}

func TestStreamer_BackpressureHighLowWatermarks(t *testing.T) {
	s := newTestStreamer(t) // high=4, low=2, max=8
	ctx := context.Background()

	for i := 0; i < 3; i++ {
		if _, err := s.Publish(Event{Type: EventToken, Data: "x"}); err != nil {
			t.Fatalf("publish %d: %v", i, err)
		}
	}
	if s.Paused() {
		t.Fatalf("should not be paused at buffered=3 (high=4)")
	}

	if _, err := s.Publish(Event{Type: EventToken, Data: "x"}); err != nil {
		t.Fatalf("publish 4: %v", err)
	}
	if !s.Paused() {
		t.Fatalf("should be paused at buffered=4 (high=4)")
	}

	// Drain to low watermark.
	for s.Buffered() > 2 {
		if _, err := s.Next(ctx); err != nil {
			t.Fatalf("next: %v", err)
		}
	}
	if s.Paused() {
		t.Fatalf("should resume at buffered=2 (low=2)")
	}
}

func TestStreamer_MaxBufferedRejects(t *testing.T) {
	s, err := NewStreamer(BackpressureConfig{HighWatermark: 2, LowWatermark: 1, MaxBufferedTokens: 2})
	if err != nil {
		t.Fatalf("NewStreamer: %v", err)
	}
	if _, err := s.Publish(Event{Type: EventToken, Data: "a"}); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Publish(Event{Type: EventToken, Data: "b"}); err != nil {
		t.Fatal(err)
	}
	_, err = s.Publish(Event{Type: EventToken, Data: "c"})
	if !errors.Is(err, ErrBackpressure) {
		t.Fatalf("want ErrBackpressure, got %v", err)
	}
}

func TestStreamer_CloseUnblocksNext(t *testing.T) {
	s := newTestStreamer(t)
	done := make(chan error, 1)
	go func() {
		_, err := s.Next(context.Background())
		done <- err
	}()
	time.Sleep(10 * time.Millisecond)
	_ = s.Close()
	select {
	case err := <-done:
		if !errors.Is(err, ErrClosed) {
			t.Fatalf("want ErrClosed, got %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("Next did not unblock after Close")
	}

	if _, err := s.Publish(Event{Type: EventToken, Data: "x"}); !errors.Is(err, ErrClosed) {
		t.Fatalf("publish after close: want ErrClosed, got %v", err)
	}
}

func TestStreamer_NextHonorsContext(t *testing.T) {
	s := newTestStreamer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Millisecond)
	defer cancel()
	_, err := s.Next(ctx)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("want DeadlineExceeded, got %v", err)
	}
}

func TestStreamer_ConcurrentPublishConsume(t *testing.T) {
	s, err := NewStreamer(BackpressureConfig{HighWatermark: 50, LowWatermark: 10, MaxBufferedTokens: 200})
	if err != nil {
		t.Fatal(err)
	}
	const n = 500
	var wg sync.WaitGroup
	wg.Add(2)

	go func() {
		defer wg.Done()
		for i := 0; i < n; i++ {
			for {
				_, err := s.Publish(Event{Type: EventToken, Data: "x"})
				if err == nil {
					break
				}
				if errors.Is(err, ErrBackpressure) {
					time.Sleep(time.Millisecond)
					continue
				}
				t.Errorf("publish: %v", err)
				return
			}
		}
	}()

	go func() {
		defer wg.Done()
		ctx := context.Background()
		var last uint64
		for i := 0; i < n; i++ {
			ev, err := s.Next(ctx)
			if err != nil {
				t.Errorf("next: %v", err)
				return
			}
			if ev.Seq <= last {
				t.Errorf("seq regressed: %d after %d", ev.Seq, last)
				return
			}
			last = ev.Seq
		}
	}()

	wg.Wait()
}

func TestFormatSSE_EmitsCanonicalFrame(t *testing.T) {
	got := FormatSSE(Event{Type: EventToken, Data: "hello", Seq: 7})
	want := "event: token\ndata: hello\nid: 7\n\n"
	if got != want {
		t.Fatalf("SSE mismatch:\nwant %q\ngot  %q", want, got)
	}
}

func TestFormatSSE_SplitsMultilineData(t *testing.T) {
	got := FormatSSE(Event{Type: EventError, Data: "line1\nline2", Seq: 2})
	if !strings.Contains(got, "data: line1\ndata: line2\n") {
		t.Fatalf("multi-line SSE: %q", got)
	}
}
