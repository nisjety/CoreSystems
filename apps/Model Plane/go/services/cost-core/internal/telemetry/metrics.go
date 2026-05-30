// Package telemetry exposes cost-core OpenTelemetry metric instruments.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName is the OTEL meter scope used by cost-core instruments.
const MeterName = "github.com/triodelab/model-plane/services/cost-core"

var (
	// TokensRecorded counts token-recording events, labelled by org_id.
	TokensRecorded metric.Int64Counter
	// BudgetChecks counts budget-check requests, labelled by org_id.
	BudgetChecks metric.Int64Counter
	// BudgetExceeded counts budget-check requests that exceeded a cap,
	// labelled by org_id and cap_type (cost | tokens).
	BudgetExceeded metric.Int64Counter
	// RequestsTotal counts HTTP requests handled, labelled by method and path.
	RequestsTotal metric.Int64Counter
)

func init() {
	meter := otel.Meter(MeterName)
	TokensRecorded, _ = meter.Int64Counter(
		"cost.tokens.recorded",
		metric.WithDescription("Total token-recording events processed by cost-core."),
	)
	BudgetChecks, _ = meter.Int64Counter(
		"cost.budget.checks",
		metric.WithDescription("Total budget-check requests handled by cost-core."),
	)
	BudgetExceeded, _ = meter.Int64Counter(
		"cost.budget.exceeded",
		metric.WithDescription("Total budget-check requests that exceeded a cap."),
	)
	RequestsTotal, _ = meter.Int64Counter(
		"cost_core_requests_total",
		metric.WithDescription("Total HTTP requests handled by cost-core, labelled by method and path."),
	)
}
