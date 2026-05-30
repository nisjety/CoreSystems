// Package telemetry exposes OpenTelemetry metric instruments for
// orchestrator-core. Counters are created eagerly in init() so call sites
// can emit measurements without nil checks.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName is the OTEL meter identifier for orchestrator-core.
const MeterName = "github.com/triodelab/model-plane/services/orchestrator-core"

// RequestsTotal counts inbound handler invocations labelled by method.
var RequestsTotal metric.Int64Counter

// LegacyEventsTranslatedTotal counts legacy NATS messages processed by the
// compat subscriber, labelled by outcome (translated, unmapped, parse_error,
// publish_error).
var LegacyEventsTranslatedTotal metric.Int64Counter

// OrchestrationEventsPublishedTotal counts orchestration envelopes published
// to NATS, labelled by event type and outcome.
var OrchestrationEventsPublishedTotal metric.Int64Counter

// OrchestrationEventsConsumedTotal counts orchestration envelopes consumed
// from NATS, labelled by event type and outcome.
var OrchestrationEventsConsumedTotal metric.Int64Counter

func init() {
	m := otel.Meter(MeterName)

	var err error
	if RequestsTotal, err = m.Int64Counter(
		"orchestrator_core_requests_total",
		metric.WithDescription("Total orchestrator-core handler invocations."),
	); err != nil {
		panic(err)
	}
	if LegacyEventsTranslatedTotal, err = m.Int64Counter(
		"orchestrator_core_legacy_events_translated_total",
		metric.WithDescription("Total legacy NATS events processed by the compat subscriber."),
	); err != nil {
		panic(err)
	}
	if OrchestrationEventsPublishedTotal, err = m.Int64Counter(
		"orchestrator_core_orchestration_events_published_total",
		metric.WithDescription("Total orchestration events published to NATS by orchestrator-core."),
	); err != nil {
		panic(err)
	}
	if OrchestrationEventsConsumedTotal, err = m.Int64Counter(
		"orchestrator_core_orchestration_events_consumed_total",
		metric.WithDescription("Total orchestration events consumed from NATS by orchestrator-core."),
	); err != nil {
		panic(err)
	}
}
