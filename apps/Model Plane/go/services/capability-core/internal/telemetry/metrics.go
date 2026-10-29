// Package telemetry exposes OpenTelemetry metric instruments shared across
// the capability-core service. Instruments are created once at package init
// against the global MeterProvider so callers can record without plumbing.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName identifies the capability-core meter in the global provider.
const MeterName = "github.com/triodelab/model-plane/services/capability-core"

var (
	// RequestsTotal counts gRPC requests handled by the service, labelled by method.
	RequestsTotal metric.Int64Counter

	// PolicyDecisionsTotal counts policy evaluation outcomes, labelled by
	// decision ("allow"|"deny") and risk level.
	PolicyDecisionsTotal metric.Int64Counter

	// CapabilityRankingTotal counts capability-discovery ranking sources and
	// stable fallback reasons. It never records user query text or tool names.
	CapabilityRankingTotal metric.Int64Counter
)

func init() {
	meter := otel.Meter(MeterName)

	RequestsTotal, _ = meter.Int64Counter(
		"capability_core_requests_total",
		metric.WithDescription("Total gRPC requests handled by capability-core, labelled by method."),
	)

	PolicyDecisionsTotal, _ = meter.Int64Counter(
		"capability_core_policy_decisions_total",
		metric.WithDescription("Total policy decisions emitted by capability-core, labelled by decision and risk level."),
	)

	CapabilityRankingTotal, _ = meter.Int64Counter(
		"capability_core_discovery_ranking_total",
		metric.WithDescription("Capability discovery ranking outcomes labelled by source and safe reason code."),
	)
}
