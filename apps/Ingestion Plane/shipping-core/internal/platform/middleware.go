package platform

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"log/slog"
	"net/http"
	"time"
)

type contextKey string

const traceIDKey contextKey = "trace_id"

// TraceIDFromContext returns the request's trace ID, or "" if none is set.
// Every log line emitted while handling a request should include this so
// a request's full path (including into agent-service later) is one
// `jq 'select(.trace_id=="...")'` away.
func TraceIDFromContext(ctx context.Context) string {
	id, _ := ctx.Value(traceIDKey).(string)
	return id
}

func newTraceID() string {
	b := make([]byte, 8)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// RequestLogger returns middleware that assigns a trace ID to each request
// (reusing an inbound X-Trace-Id header if present, so a browser or
// agent-service call can supply its own), stores it on the request
// context, and logs one structured line per completed request.
func RequestLogger(logger *slog.Logger) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			traceID := r.Header.Get("X-Trace-Id")
			if traceID == "" {
				traceID = newTraceID()
			}
			ctx := context.WithValue(r.Context(), traceIDKey, traceID)
			w.Header().Set("X-Trace-Id", traceID)

			start := time.Now()
			rec := &statusRecorder{ResponseWriter: w, status: http.StatusOK}
			next.ServeHTTP(rec, r.WithContext(ctx))

			logger.Info("http_request",
				"trace_id", traceID,
				"method", r.Method,
				"path", r.URL.Path,
				"status", rec.status,
				"duration_ms", time.Since(start).Milliseconds(),
			)
		})
	}
}

type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (r *statusRecorder) WriteHeader(status int) {
	r.status = status
	r.ResponseWriter.WriteHeader(status)
}
