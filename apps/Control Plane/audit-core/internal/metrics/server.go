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
	eventProcessingLag = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "audit_core_event_processing_lag_seconds",
			Help: "Wall-clock age of the last successfully persisted event.",
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
)

func init() {
	prometheus.MustRegister(natsConnected, eventsTotal, eventProcessingLag, eventLastProcessed)
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
	eventProcessingLag.WithLabelValues(bus, kind).Set(lag)
	eventLastProcessed.WithLabelValues(bus, kind).Set(float64(now.Unix()))
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
