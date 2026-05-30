package natsx

import (
	"fmt"
	"testing"
	"time"
)

func TestPublishThroughput(t *testing.T) {
	f := &fakeRaw{}
	p := NewPublisher(f, ModeV1Only)
	const n = 10_000
	start := time.Now()
	for i := 0; i < n; i++ {
		if err := p.Publish(fmt.Sprintf("mp.v1.run.run-%d.event", i), []byte("payload")); err != nil {
			t.Fatalf("publish error at %d: %v", i, err)
		}
	}
	elapsed := time.Since(start)
	if len(f.calls) != n {
		t.Errorf("expected %d calls, got %d", n, len(f.calls))
	}
	t.Logf("published %d messages in %v (%.0f msg/s)", n, elapsed, float64(n)/elapsed.Seconds())
}

func TestPublishRecoveryAfterError(t *testing.T) {
	f := &fakeRaw{failAt: 3}
	p := NewPublisher(f, ModeV1Only)
	var firstErr error
	for i := 0; i < 5; i++ {
		err := p.Publish("mp.v1.run.run-1.event", []byte("x"))
		if err != nil && firstErr == nil {
			firstErr = err
		}
	}
	if firstErr == nil {
		t.Error("expected at least one error from failAt publisher, got none")
	}
	if f.callNum < 5 {
		t.Errorf("expected at least 5 call attempts, got %d", f.callNum)
	}
}

func TestTranslationPerformance(t *testing.T) {
	subjects := []string{
		"velion.agent.run.run-1.event",
		"velion.session.sess-1.command",
		"aqencia.reasoning.reasoning.started",
		"aqencia.reasoning.reasoning.completed",
		"aqencia.reasoning.usage.recorded",
	}
	const iterations = 10_000
	start := time.Now()
	for i := 0; i < iterations; i++ {
		for _, s := range subjects {
			_ = TranslateLegacySubject(s)
		}
	}
	elapsed := time.Since(start)
	total := iterations * len(subjects)
	t.Logf("translated %d subjects in %v (%.0f/s)", total, elapsed, float64(total)/elapsed.Seconds())
	if elapsed > 2*time.Second {
		t.Errorf("translation too slow: %v for %d ops", elapsed, total)
	}
}
