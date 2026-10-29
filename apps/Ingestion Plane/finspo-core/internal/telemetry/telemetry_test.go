package telemetry

import (
	"context"
	"testing"
)

func TestInitProducesLogger(t *testing.T) {
	t.Parallel()

	tel := Init("finspo-core", "dev", "")
	// zerolog loggers are value types; assert the logger is usable by
	// writing a debug-level event and checking it does not panic.
	tel.Logger.Debug().Str("k", "v").Msg("hello")
}

func TestShutdownNoopOnEmptyTelemetry(t *testing.T) {
	t.Parallel()

	tel := Init("finspo-core", "test", "")
	if err := tel.Shutdown(context.Background()); err != nil {
		t.Fatalf("Shutdown error = %v, want nil", err)
	}
}
