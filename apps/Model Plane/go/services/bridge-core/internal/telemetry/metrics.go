// Package telemetry exposes bridge-core OpenTelemetry metric instruments.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName is the OTEL meter scope used by bridge-core instruments.
const MeterName = "github.com/triodelab/model-plane/services/bridge-core"

var (
	// RequestsTotal counts HTTP requests handled, labelled by method and path.
	RequestsTotal metric.Int64Counter
	// SessionsCreatedTotal counts session registration attempts, labelled by channel and outcome.
	SessionsCreatedTotal metric.Int64Counter
	// SessionsClosedTotal counts session close attempts, labelled by outcome.
	SessionsClosedTotal metric.Int64Counter
	// IngestTotal counts ingest calls, labelled by channel and outcome.
	IngestTotal metric.Int64Counter
	// ActiveSessions is a gauge of currently active sessions.
	ActiveSessions metric.Int64UpDownCounter
)

func init() {
	meter := otel.Meter(MeterName)
	RequestsTotal, _ = meter.Int64Counter(
		"bridge_core_requests_total",
		metric.WithDescription("Total HTTP requests handled by bridge-core, labelled by method and path."),
	)
	SessionsCreatedTotal, _ = meter.Int64Counter(
		"bridge_core_sessions_created_total",
		metric.WithDescription("Total session registration attempts, labelled by channel and outcome."),
	)
	SessionsClosedTotal, _ = meter.Int64Counter(
		"bridge_core_sessions_closed_total",
		metric.WithDescription("Total session close attempts, labelled by outcome."),
	)
	IngestTotal, _ = meter.Int64Counter(
		"bridge_core_ingest_total",
		metric.WithDescription("Total ingest calls, labelled by channel and outcome."),
	)
	ActiveSessions, _ = meter.Int64UpDownCounter(
		"bridge_core_active_sessions",
		metric.WithDescription("Current number of active sessions."),
	)
}
