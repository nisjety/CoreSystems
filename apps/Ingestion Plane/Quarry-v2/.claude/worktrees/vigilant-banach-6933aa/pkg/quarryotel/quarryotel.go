// Package quarryotel provides OpenTelemetry tracing setup for Quarry Go services.
//
// Both quarry-control and quarry-orchestrator use Init to set up an OTLP gRPC
// exporter (or no-op exporter when OTEL_EXPORTER_OTLP_ENDPOINT is unset). The
// returned Shutdown func MUST be called from main() to flush spans on exit.
//
// Usage:
//
//	shutdown, err := quarryotel.Init(ctx, "quarry-orchestrator", "0.1.0")
//	if err != nil { return err }
//	defer shutdown(context.Background())
//
// Tracecontext propagation across the Go ↔ Rust boundary is handled at the
// HTTP edge layer via traceparent headers — both ends use the W3C
// TraceContext propagator below.
package quarryotel

import (
	"context"
	"fmt"
	"os"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	semconv "go.opentelemetry.io/otel/semconv/v1.26.0"
)

// ShutdownFunc flushes pending spans and closes the exporter. Always call it.
type ShutdownFunc func(ctx context.Context) error

// Init wires up the global TracerProvider + W3C TraceContext propagator.
// Returns a no-op shutdown func and nil error when OTEL_EXPORTER_OTLP_ENDPOINT
// is unset, so callers don't need to special-case "OTEL disabled".
func Init(ctx context.Context, serviceName, version string) (ShutdownFunc, error) {
	endpoint := os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT")
	if endpoint == "" {
		// Use a no-op TracerProvider but still set the propagator so
		// incoming traceparent headers are honored when present.
		otel.SetTextMapPropagator(propagation.TraceContext{})
		return func(context.Context) error { return nil }, nil
	}

	res, err := resource.New(ctx,
		resource.WithFromEnv(),
		resource.WithProcess(),
		resource.WithAttributes(
			semconv.ServiceName(serviceName),
			semconv.ServiceVersion(version),
			semconv.DeploymentEnvironment(envOr("ENVIRONMENT", "dev")),
		),
	)
	if err != nil {
		return nil, fmt.Errorf("quarryotel resource: %w", err)
	}

	exporterCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	insecure := os.Getenv("OTEL_EXPORTER_OTLP_INSECURE") == "true"
	opts := []otlptracegrpc.Option{otlptracegrpc.WithEndpoint(endpoint)}
	if insecure {
		opts = append(opts, otlptracegrpc.WithInsecure())
	}
	exporter, err := otlptrace.New(exporterCtx, otlptracegrpc.NewClient(opts...))
	if err != nil {
		return nil, fmt.Errorf("quarryotel exporter: %w", err)
	}

	tp := sdktrace.NewTracerProvider(
		sdktrace.WithBatcher(exporter,
			sdktrace.WithBatchTimeout(5*time.Second),
			sdktrace.WithMaxQueueSize(2048),
		),
		sdktrace.WithResource(res),
		sdktrace.WithSampler(sdktrace.ParentBased(sdktrace.AlwaysSample())),
	)
	otel.SetTracerProvider(tp)
	otel.SetTextMapPropagator(propagation.NewCompositeTextMapPropagator(
		propagation.TraceContext{},
		propagation.Baggage{},
	))

	return func(shutdownCtx context.Context) error {
		shutdownCtx, c := context.WithTimeout(shutdownCtx, 5*time.Second)
		defer c()
		return tp.Shutdown(shutdownCtx)
	}, nil
}

func envOr(key, fallback string) string {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	return v
}
