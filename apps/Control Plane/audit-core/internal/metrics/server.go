// Package metrics exposes a Prometheus /metrics endpoint on a dedicated port,
// separate from the application HTTP server so Prometheus can scrape it without
// passing through the app's auth middleware (Phase 6 B13). Mirrors the proven
// org-core pattern.
package metrics

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
	"github.com/rs/zerolog/log"
)

var (
	natsConnected = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_nats_connected",
			Help: "Whether an audit-core NATS bus is currently connected (1/0).",
		},
		[]string{"bus"},
	)
	eventsTotal = prometheus.NewCounterVec(
		prometheus.CounterOpts{
			Name: "audit_core_events_total",
			Help: "Audit/usage messages handled by result.",
		},
		[]string{"bus", "kind", "result"},
	)
	eventProcessingDelay = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_event_processing_delay_seconds",
			Help: "Observed producer-to-persistence delay of the last successfully persisted event.",
		},
		[]string{"bus", "kind"},
	)
	eventLastProcessed = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_event_last_processed_timestamp_seconds",
			Help: "Unix timestamp of the last successfully persisted event.",
		},
		[]string{"bus", "kind"},
	)
	eventLastAck = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_event_last_ack_timestamp_seconds",
			Help: "Unix timestamp of the last successful JetStream ACK.",
		},
		[]string{"bus", "kind"},
	)
	consumerPending = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_consumer_pending",
			Help: "Messages pending delivery for each durable audit-core consumer.",
		},
		[]string{"bus", "kind"},
	)
	consumerAckPending = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_consumer_ack_pending",
			Help: "Delivered messages awaiting acknowledgement.",
		},
		[]string{"bus", "kind"},
	)
	consumerRedelivered = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_consumer_redelivered",
			Help: "Messages redelivered for each durable audit-core consumer.",
		},
		[]string{"bus", "kind"},
	)
	// The chain series carry no org label: an org id per series would grow the
	// label set with tenants, and the per-org picture is in the head table.
	chainAppends = prometheus.NewCounterVec(
		prometheus.CounterOpts{
			Name: "audit_core_chain_appends_total",
			Help: "Audit rows handled by the chained insert by result (appended, duplicate, catchup, error).",
		},
		[]string{"result"},
	)
	chainHeadLockWait = prometheus.NewHistogram(
		prometheus.HistogramOpts{
			Name:    "audit_core_chain_head_lock_wait_seconds",
			Help:    "Time a chained insert waited to take its org's chain head lock.",
			Buckets: []float64{0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10},
		},
	)
	chainBroken = prometheus.NewCounter(
		prometheus.CounterOpts{
			Name: "audit_core_chain_broken_total",
			Help: "Org chains found broken by the head spot-check at insert time (counted once per org transition).",
		},
	)
	// Purges refused for an org, by the incident kind written for it: the closed
	// vocabulary in store/attest.go. Like the other chain series it has no org
	// label; the incident row names the org.
	chainPurgeBlocked = prometheus.NewCounterVec(
		prometheus.CounterOpts{
			Name: "audit_core_chain_purge_blocked_total",
			Help: "Chained purges refused because a prefix or checkpoint did not verify, by incident kind. Counted once per org per refused sweep, in dry-run and enabled modes.",
		},
		[]string{"kind"},
	)
	detailsGuard = prometheus.NewCounterVec(
		prometheus.CounterOpts{
			Name: "audit_core_details_guard_total",
			Help: "Observe-only check of audit details at ingest by result (ok, oversize, denied_key).",
		},
		[]string{"result"},
	)
)

// chainPurgeKinds mirrors the incident kinds in store/attest.go. Each series is
// created at zero, so the first refusal after start reads as an increase:
// increase() cannot see a series whose first sample is already 1.
var chainPurgeKinds = []string{
	"chain_marked_broken",
	"checkpoint_key_unknown",
	"checkpoint_signature_invalid",
	"sequence_gap",
	"prev_hash_mismatch",
	"row_hash_mismatch",
	"unsupported_chain_version",
	"unhashable_row",
	"checkpoint_link_mismatch",
}

func init() {
	prometheus.MustRegister(
		natsConnected,
		eventsTotal,
		eventProcessingDelay,
		eventLastProcessed,
		eventLastAck,
		consumerPending,
		consumerAckPending,
		consumerRedelivered,
		chainAppends,
		chainHeadLockWait,
		chainBroken,
		chainPurgeBlocked,
		detailsGuard,
	)
	for _, kind := range chainPurgeKinds {
		chainPurgeBlocked.WithLabelValues(kind)
	}
}

// RecordChainAppend counts one chained-insert outcome.
func RecordChainAppend(result string) {
	chainAppends.WithLabelValues(result).Inc()
}

func ObserveChainHeadLockWait(wait time.Duration) {
	chainHeadLockWait.Observe(wait.Seconds())
}

func RecordChainBroken() {
	chainBroken.Inc()
}

// RecordChainPurgeBlocked counts one purge refused for an org, by the incident
// kind written for it.
func RecordChainPurgeBlocked(kind string) {
	chainPurgeBlocked.WithLabelValues(kind).Inc()
}

func RecordDetailsGuard(result string) {
	detailsGuard.WithLabelValues(result).Inc()
}

func SetConsumerState(bus, kind string, pending uint64, ackPending, redelivered int) {
	consumerPending.WithLabelValues(bus, kind).Set(float64(pending))
	consumerAckPending.WithLabelValues(bus, kind).Set(float64(ackPending))
	consumerRedelivered.WithLabelValues(bus, kind).Set(float64(redelivered))
}

func SetNATSConnected(bus string, connected bool) {
	value := 0.0
	if connected {
		value = 1
	}
	natsConnected.WithLabelValues(bus).Set(value)
}

func RecordEvent(bus, kind, result string, occurredAt time.Time) {
	eventsTotal.WithLabelValues(bus, kind, result).Inc()
	if result != "persisted" {
		return
	}
	now := time.Now().UTC()
	lag := now.Sub(occurredAt).Seconds()
	if lag < 0 {
		lag = 0
	}
	eventProcessingDelay.WithLabelValues(bus, kind).Set(lag)
	eventLastProcessed.WithLabelValues(bus, kind).Set(float64(now.Unix()))
}

func RecordAck(bus, kind string, acknowledgedAt time.Time) {
	eventLastAck.WithLabelValues(bus, kind).Set(float64(acknowledgedAt.Unix()))
}

type Server struct {
	address    string
	httpServer *http.Server
}

func NewServer(port int) *Server {
	mux := http.NewServeMux()
	mux.Handle("/metrics", promhttp.Handler())
	mux.HandleFunc("/health", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	return &Server{
		address: fmt.Sprintf(":%d", port),
		httpServer: &http.Server{
			Addr:              fmt.Sprintf(":%d", port),
			Handler:           mux,
			ReadHeaderTimeout: 5 * time.Second,
			ReadTimeout:       10 * time.Second,
			WriteTimeout:      10 * time.Second,
			IdleTimeout:       30 * time.Second,
		},
	}
}

func (s *Server) Start() error {
	listener, err := net.Listen("tcp", s.address)
	if err != nil {
		return fmt.Errorf("listen on %s: %w", s.address, err)
	}

	log.Info().Str("addr", s.address).Msg("audit-core metrics listening")
	if err := s.httpServer.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
		return fmt.Errorf("serve metrics on %s: %w", s.address, err)
	}

	return nil
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}
