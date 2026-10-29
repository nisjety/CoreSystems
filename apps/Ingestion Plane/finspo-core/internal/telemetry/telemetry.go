package telemetry

import (
	"context"
	"os"
	"strings"

	"github.com/rs/zerolog"
)

// Telemetry bundles the structured logger and any future OTEL providers.
// Phase 1 wires zerolog only; OTEL meter/tracer providers will land in a
// follow-up phase but the Shutdown hook is already in place so callers do
// not need to change when that lands.
type Telemetry struct {
	Logger  zerolog.Logger
	shutdown []func(context.Context) error
}

func init() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnixMs
}

// Init builds a Telemetry value configured for the given service.
// otelEndpoint is recorded for the future OTEL wiring but is otherwise
// unused in Phase 1 — emitting a "configured but not yet exported" log
// line so misconfigured environments are obvious.
func Init(service, environment, otelEndpoint string) Telemetry {
	level := zerolog.InfoLevel
	if strings.EqualFold(environment, "dev") || strings.EqualFold(environment, "local") {
		level = zerolog.DebugLevel
	}

	logger := zerolog.New(os.Stdout).
		Level(level).
		With().
		Timestamp().
		Str("service", service).
		Str("env", environment).
		Logger()

	if strings.TrimSpace(otelEndpoint) != "" {
		logger.Info().
			Str("otel_endpoint", otelEndpoint).
			Msg("OTEL endpoint configured; exporter wiring lands in a follow-up phase")
	}

	return Telemetry{Logger: logger}
}

// Shutdown flushes any registered exporters. Safe to call multiple times.
func (t *Telemetry) Shutdown(ctx context.Context) error {
	if t == nil {
		return nil
	}
	var firstErr error
	for _, fn := range t.shutdown {
		if err := fn(ctx); err != nil && firstErr == nil {
			firstErr = err
		}
	}
	t.shutdown = nil
	return firstErr
}
