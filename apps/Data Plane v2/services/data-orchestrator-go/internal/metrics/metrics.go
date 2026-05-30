package metrics

import (
	"net/http"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

const service = "data_orchestrator_go"

var (
	httpRequestsTotal = promauto.NewCounterVec(
		prometheus.CounterOpts{
			Name: "dpv2_" + service + "_http_requests_total",
			Help: "Total HTTP requests by method, path, and status",
		},
		[]string{"method", "path", "status"},
	)

	httpRequestDuration = promauto.NewHistogramVec(
		prometheus.HistogramOpts{
			Name:    "dpv2_" + service + "_http_request_duration_seconds",
			Help:    "HTTP request latency in seconds",
			Buckets: prometheus.ExponentialBuckets(0.005, 2, 12),
		},
		[]string{"method", "path"},
	)

	BusinessOpsTotal = promauto.NewCounterVec(
		prometheus.CounterOpts{
			Name: "dpv2_" + service + "_business_ops_total",
			Help: "Business operations by type and outcome",
		},
		[]string{"op", "outcome"},
	)
)

// Middleware records request duration and status for every HTTP request.
func Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		ww := &statusRecorder{ResponseWriter: w, status: http.StatusOK}

		next.ServeHTTP(ww, r)

		path := normalizedPath(r)
		method := r.Method
		status := strconv.Itoa(ww.status)

		httpRequestsTotal.WithLabelValues(method, path, status).Inc()
		httpRequestDuration.WithLabelValues(method, path).Observe(time.Since(start).Seconds())
	})
}

// Handler returns the /metrics endpoint handler.
func Handler() http.Handler {
	return promhttp.Handler()
}

type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (s *statusRecorder) WriteHeader(code int) {
	s.status = code
	s.ResponseWriter.WriteHeader(code)
}

// normalizedPath returns the route pattern (e.g. "/v1/documents/{id}") rather
// than the concrete URL path, to keep label cardinality bounded.
func normalizedPath(r *http.Request) string {
	if rctx := chi.RouteContext(r.Context()); rctx != nil {
		if pattern := rctx.RoutePattern(); pattern != "" {
			return pattern
		}
	}
	return "unknown"
}
