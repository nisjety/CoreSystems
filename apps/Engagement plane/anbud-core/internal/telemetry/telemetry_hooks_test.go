package telemetry

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"testing"
)

func TestInitBuildsALoggerWithoutTouchingTheNetwork(t *testing.T) {
	t.Parallel()

	tel := Init("anbud-core", "production", "")
	if tel.Logger.GetLevel().String() != "info" {
		t.Fatalf("production level = %s, want info", tel.Logger.GetLevel())
	}
}

func TestOTELEndpointIsAnnounced(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	New(&buf, "anbud-core", "dev", "http://otel-collector:4318")
	if !strings.Contains(buf.String(), "otel-collector:4318") {
		t.Fatalf("expected the configured endpoint to be logged, got %q", buf.String())
	}
}

func TestShutdownRunsEveryHookAndReturnsTheFirstError(t *testing.T) {
	t.Parallel()

	first, second := errors.New("first failure"), errors.New("second failure")
	ran := 0
	tel := New(&bytes.Buffer{}, "anbud-core", "dev", "")
	tel.shutdown = []func(context.Context) error{
		func(context.Context) error { ran++; return first },
		func(context.Context) error { ran++; return second },
		func(context.Context) error { ran++; return nil },
	}

	if err := tel.Shutdown(context.Background()); !errors.Is(err, first) {
		t.Fatalf("Shutdown error = %v, want the first failure", err)
	}
	if ran != 3 {
		t.Fatalf("ran %d hooks, want all 3", ran)
	}
	if err := tel.Shutdown(context.Background()); err != nil || ran != 3 {
		t.Fatalf("a second Shutdown must be a no-op (err=%v, ran=%d)", err, ran)
	}
}
