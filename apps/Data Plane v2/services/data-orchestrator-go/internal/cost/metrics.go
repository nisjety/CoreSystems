package cost

import (
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
)

// What the usage ledger does is counted here and never described: a label is a closed
// word, so nothing a producer sends (a model name, an organization) can reach a metric.
var (
	// consumedTotal counts what happened to each delivery of the durable stream.
	// result: persisted, duplicate, invalid, transient_failure, permanent_failure.
	consumedTotal = promauto.NewCounterVec(
		prometheus.CounterOpts{
			Name: "dpv2_data_orchestrator_go_usage_events_consumed_total",
			Help: "Deliveries of the usage ledger stream by outcome",
		},
		[]string{"result"},
	)

	// staleAcceptedTotal counts authentic envelopes accepted after their own lifetime had
	// passed: the backlog the durable stream exists to carry. A steady rate means the
	// consumer is behind, not that anything is wrong with an event.
	staleAcceptedTotal = promauto.NewCounter(
		prometheus.CounterOpts{
			Name: "dpv2_data_orchestrator_go_usage_events_stale_accepted_total",
			Help: "Authentic usage envelopes accepted after their own lifetime had passed",
		},
	)

	// forwardedTotal counts the forwarder's attempts by outcome.
	// result: forwarded, rejected, unauthorized, unavailable, token_unavailable.
	forwardedTotal = promauto.NewCounterVec(
		prometheus.CounterOpts{
			Name: "dpv2_data_orchestrator_go_usage_forward_total",
			Help: "Usage rows posted to cost-core by outcome",
		},
		[]string{"result"},
	)

	// forwardLagSeconds is the age of the oldest row waiting to be forwarded, 0 when none.
	// The alert on it is the one that says spend is not reaching cost-core.
	forwardLagSeconds = promauto.NewGauge(
		prometheus.GaugeOpts{
			Name: "dpv2_data_orchestrator_go_usage_forward_lag_seconds",
			Help: "Age of the oldest usage row not yet forwarded to cost-core",
		},
	)

	// forwardPending is the number of rows waiting to be forwarded.
	forwardPending = promauto.NewGauge(
		prometheus.GaugeOpts{
			Name: "dpv2_data_orchestrator_go_usage_forward_pending",
			Help: "Usage rows not yet forwarded to cost-core",
		},
	)
)
