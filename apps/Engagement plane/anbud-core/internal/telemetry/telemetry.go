// Package telemetry builds the structured logger. OTEL exporters are not wired
// yet; the Shutdown hook exists so callers will not change when they are.
package telemetry

import (
	"context"
	"io"
	"os"
	"strings"

	"github.com/rs/zerolog"
)

// Telemetry bundles the logger and any future exporters.
type Telemetry struct {
	Logger   zerolog.Logger
	shutdown []func(context.Context) error
}

func init() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnixMs
}

// Init builds Telemetry that logs JSON to stdout.
func Init(service, environment, otelEndpoint string) Telemetry {
	return New(os.Stdout, service, environment, otelEndpoint)
}

// New builds Telemetry that logs JSON to w. Debug logging is on only for the
// dev and local environments.
func New(w io.Writer, service, environment, otelEndpoint string) Telemetry {
	level := zerolog.InfoLevel
	if strings.EqualFold(environment, "dev") || strings.EqualFold(environment, "local") {
		level = zerolog.DebugLevel
	}

	logger := zerolog.New(w).
		Level(level).
		With().
		Timestamp().
		Str("service", service).
		Str("env", environment).
		Logger()

	if strings.TrimSpace(otelEndpoint) != "" {
		logger.Info().
			Str("otel_endpoint", otelEndpoint).
			Msg("OTEL endpoint configured; exporter wiring is not implemented yet")
	}

	return Telemetry{Logger: logger}
}

// Shutdown flushes registered exporters. It is safe to call repeatedly and on
// a nil receiver.
func (t *Telemetry) Shutdown(ctx context.Context) error {
	if t == nil {
		return nil
	}
	var first error
	for _, fn := range t.shutdown {
		if err := fn(ctx); err != nil && first == nil {
			first = err
		}
	}
	t.shutdown = nil
	return first
}
