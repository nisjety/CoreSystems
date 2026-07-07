package platform

import (
	"bytes"
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestRequestLogger_AssignsAndReturnsTraceID(t *testing.T) {
	var logBuf bytes.Buffer
	logger := slog.New(slog.NewJSONHandler(&logBuf, nil))

	var gotTraceID string
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotTraceID = TraceIDFromContext(r.Context())
		w.WriteHeader(http.StatusTeapot)
	})

	req := httptest.NewRequest(http.MethodGet, "/some/path", nil)
	rec := httptest.NewRecorder()

	RequestLogger(logger)(next).ServeHTTP(rec, req)

	if gotTraceID == "" {
		t.Error("expected a trace ID to be set on the request context")
	}
	if rec.Header().Get("X-Trace-Id") != gotTraceID {
		t.Errorf("response header X-Trace-Id=%q, want %q", rec.Header().Get("X-Trace-Id"), gotTraceID)
	}

	var logLine map[string]any
	if err := json.Unmarshal(bytes.TrimSpace(logBuf.Bytes()), &logLine); err != nil {
		t.Fatalf("expected one JSON log line, got: %s (%v)", logBuf.String(), err)
	}
	if logLine["trace_id"] != gotTraceID {
		t.Errorf("logged trace_id=%v, want %v", logLine["trace_id"], gotTraceID)
	}
	if int(logLine["status"].(float64)) != http.StatusTeapot {
		t.Errorf("logged status=%v, want %d", logLine["status"], http.StatusTeapot)
	}
}

func TestRequestLogger_ReusesInboundTraceID(t *testing.T) {
	logger := slog.New(slog.NewJSONHandler(&bytes.Buffer{}, nil))
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {})

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Header.Set("X-Trace-Id", "caller-supplied-id")
	rec := httptest.NewRecorder()

	RequestLogger(logger)(next).ServeHTTP(rec, req)

	if got := rec.Header().Get("X-Trace-Id"); got != "caller-supplied-id" {
		t.Errorf("got X-Trace-Id=%q, want the inbound value to be reused", got)
	}
}

func TestTraceIDFromContext_EmptyWhenUnset(t *testing.T) {
	if got := TraceIDFromContext(context.Background()); got != "" {
		t.Errorf("got %q, want empty string for a context with no trace ID", got)
	}
}

func TestNewLogger_ProducesJSONLines(t *testing.T) {
	logger := NewLogger()
	if logger == nil {
		t.Fatal("NewLogger returned nil")
	}
	// Smoke-test that logging doesn't panic; NewLogger writes to stdout by
	// design (structured logs are meant to go to the process's own
	// stdout for platform log aggregation), so we only verify usability
	// here, not captured output.
	logger.Info("smoke test", "component", strings.ToLower("Platform"))
}
