package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName identifies the sandbox-manager meter.
const MeterName = "github.com/triodelab/model-plane/services/sandbox-manager"

// Counters exported for use by handlers.
var (
	RequestsTotal          metric.Int64Counter
	LeaseDecisionsTotal    metric.Int64Counter
	SnapshotDecisionsTotal metric.Int64Counter
	ProcessDecisionsTotal  metric.Int64Counter
)

func init() {
	meter := otel.Meter(MeterName)
	RequestsTotal, _ = meter.Int64Counter(
		"sandbox_manager_requests_total",
		metric.WithDescription("Total gRPC requests handled by sandbox-manager, labelled by method."),
	)
	LeaseDecisionsTotal, _ = meter.Int64Counter(
		"sandbox_manager_lease_decisions_total",
		metric.WithDescription("Total lease lifecycle decisions emitted by sandbox-manager, labelled by outcome."),
	)
	SnapshotDecisionsTotal, _ = meter.Int64Counter(
		"sandbox_manager_snapshot_decisions_total",
		metric.WithDescription("Total snapshot decisions emitted by sandbox-manager, labelled by outcome."),
	)
	ProcessDecisionsTotal, _ = meter.Int64Counter(
		"sandbox_manager_process_decisions_total",
		metric.WithDescription("Total background-process registry decisions emitted by sandbox-manager, labelled by outcome."),
	)
}
