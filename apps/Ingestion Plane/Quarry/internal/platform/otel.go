package platform

import (
	"context"
	"fmt"

	"github.com/rs/zerolog/log"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/exporters/prometheus"
	"go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/resource"
	semconv "go.opentelemetry.io/otel/semconv/v1.26.0"
)

// OTelProvider wraps the MeterProvider lifecycle.
type OTelProvider struct {
	meterProvider *metric.MeterProvider
}

// InitOTel initialises a Prometheus-backed OpenTelemetry MeterProvider and
// registers it as the global OTel meter provider.
// Call provider.Shutdown(ctx) during graceful shutdown.
func InitOTel(ctx context.Context, serviceName, serviceVersion string) (*OTelProvider, error) {
	res, err := resource.New(ctx,
		resource.WithAttributes(
			semconv.ServiceName(serviceName),
			semconv.ServiceVersion(serviceVersion),
		),
	)
	if err != nil {
		return nil, fmt.Errorf("otel resource: %w", err)
	}

	promExporter, err := prometheus.New()
	if err != nil {
		return nil, fmt.Errorf("prometheus exporter: %w", err)
	}

	mp := metric.NewMeterProvider(
		metric.WithResource(res),
		metric.WithReader(promExporter),
	)

	otel.SetMeterProvider(mp)
	log.Info().
		Str("service", serviceName).
		Str("version", serviceVersion).
		Msg("otel meter provider initialised (prometheus exporter)")

	return &OTelProvider{meterProvider: mp}, nil
}

// Shutdown flushes pending metrics and releases resources.
func (p *OTelProvider) Shutdown(ctx context.Context) {
	if p == nil || p.meterProvider == nil {
		return
	}
	if err := p.meterProvider.Shutdown(ctx); err != nil {
		log.Warn().Err(err).Msg("otel shutdown error")
	}
}
