// Package telemetry exposes browser-broker OpenTelemetry metric instruments.
package telemetry

import (
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

// MeterName is the OTEL meter scope used by browser-broker instruments.
const MeterName = "github.com/triodelab/model-plane/services/browser-broker"

var (
	// RequestsTotal counts gRPC requests handled, labelled by method.
	RequestsTotal metric.Int64Counter
	// GrantsIssuedTotal counts grant issuance attempts, labelled by outcome.
	GrantsIssuedTotal metric.Int64Counter
	// GrantsRevokedTotal counts revocation attempts, labelled by outcome.
	GrantsRevokedTotal metric.Int64Counter
	// GrantsValidatedTotal counts grant validation attempts, labelled by outcome.
	GrantsValidatedTotal metric.Int64Counter
	// GrantsRenewedTotal counts renewal attempts (WS8.C), labelled by outcome.
	GrantsRenewedTotal metric.Int64Counter
	// GatewayRateLimitedTotal counts requests rejected by the gateway rate limiter, labelled by method.
	GatewayRateLimitedTotal metric.Int64Counter
	// GrantStoreErrorsTotal counts grant store failures (the database, not a
	// refused grant), labelled by op only: never an org, owner or domain.
	GrantStoreErrorsTotal metric.Int64Counter
)

func init() {
	meter := otel.Meter(MeterName)
	RequestsTotal, _ = meter.Int64Counter(
		"browser_broker_requests_total",
		metric.WithDescription("Total gRPC requests handled by browser-broker, labelled by method."),
	)
	GrantsIssuedTotal, _ = meter.Int64Counter(
		"browser_broker_grants_issued_total",
		metric.WithDescription("Total browser grant issuance attempts, labelled by outcome."),
	)
	GrantsRevokedTotal, _ = meter.Int64Counter(
		"browser_broker_grants_revoked_total",
		metric.WithDescription("Total browser grant revocation attempts, labelled by outcome."),
	)
	GrantsValidatedTotal, _ = meter.Int64Counter(
		"browser_broker_grants_validated_total",
		metric.WithDescription("Total browser grant validation attempts, labelled by outcome."),
	)
	GrantsRenewedTotal, _ = meter.Int64Counter(
		"browser_broker_grants_renewed_total",
		metric.WithDescription("Total browser grant renewal attempts, labelled by outcome."),
	)
	GatewayRateLimitedTotal, _ = meter.Int64Counter(
		"browser_broker_gateway_rate_limited_total",
		metric.WithDescription("Total requests rejected by gateway rate limiter, labelled by method."),
	)
	GrantStoreErrorsTotal, _ = meter.Int64Counter(
		"browser_broker_grant_store_errors_total",
		metric.WithDescription("Total browser grant store failures, labelled by op."),
	)
}
