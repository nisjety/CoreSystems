// Package telemetry exposes letta-bridge OpenTelemetry metric instruments.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName is the OTEL meter scope used by letta-bridge instruments.
const MeterName = "github.com/triodelab/model-plane/services/letta-bridge"

var (
	// RequestsTotal counts gRPC requests handled, labelled by method.
	RequestsTotal metric.Int64Counter
	// MemorySearchesTotal counts memory search attempts, labelled by outcome.
	MemorySearchesTotal metric.Int64Counter
	// MemoryIndexedTotal counts memory indexing attempts, labelled by outcome.
	MemoryIndexedTotal metric.Int64Counter
)

func init() {
	meter := otel.Meter(MeterName)
	RequestsTotal, _ = meter.Int64Counter(
		"letta_bridge_requests_total",
		metric.WithDescription("Total gRPC requests handled by letta-bridge, labelled by method."),
	)
	MemorySearchesTotal, _ = meter.Int64Counter(
		"letta_bridge_memory_searches_total",
		metric.WithDescription("Total memory search attempts, labelled by outcome."),
	)
	MemoryIndexedTotal, _ = meter.Int64Counter(
		"letta_bridge_memory_indexed_total",
		metric.WithDescription("Total memory indexing attempts, labelled by outcome."),
	)
}
