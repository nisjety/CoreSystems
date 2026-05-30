// Package server implements the bridge-core HTTP API for session lifecycle
// and payload ingestion.
package server

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"strings"

	"github.com/triodelab/model-plane/services/bridge-core/internal/channel"
	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
	"github.com/triodelab/model-plane/services/bridge-core/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

const maxPayloadBytes = 4 * 1024 * 1024 // 4 MiB

// Server holds the HTTP handler dependencies.
type Server struct {
	sessions *session.Registry
	adapters *channel.AdapterRegistry
}

// NewServer constructs a Server wired to the given session registry and
// channel adapter registry.
func NewServer(sessions *session.Registry, adapters *channel.AdapterRegistry) *Server {
	return &Server{sessions: sessions, adapters: adapters}
}

// registerSessionRequest is the JSON body for POST /api/v1/sessions.
type registerSessionRequest struct {
	OrgID   string `json:"org_id"`
	UserID  string `json:"user_id"`
	Channel string `json:"channel"`
}

// ingestRequest is the JSON body for POST /api/v1/sessions/:id/ingest.
type ingestRequest struct {
	Payload []byte `json:"payload"`
}

// Handler returns the top-level HTTP handler with all routes registered.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /healthz", s.handleHealthz)
	mux.HandleFunc("GET /readyz", s.handleReadyz)
	mux.HandleFunc("POST /api/v1/sessions", s.handleRegisterSession)
	mux.HandleFunc("GET /api/v1/sessions", s.handleListSessions)
	mux.HandleFunc("GET /api/v1/sessions/{id}", s.handleGetSession)
	mux.HandleFunc("POST /api/v1/sessions/{id}/ingest", s.handleIngest)
	mux.HandleFunc("DELETE /api/v1/sessions/{id}", s.handleCloseSession)

	return mux
}

func (s *Server) handleHealthz(w http.ResponseWriter, _ *http.Request) {
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

func (s *Server) handleReadyz(w http.ResponseWriter, _ *http.Request) {
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

func (s *Server) handleRegisterSession(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "POST"),
		attribute.String("path", "/api/v1/sessions"),
	))

	var req registerSessionRequest
	if err := json.NewDecoder(io.LimitReader(r.Body, maxPayloadBytes)).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	sess, err := s.sessions.Register(req.OrgID, req.UserID, req.Channel)
	if err != nil {
		telemetry.SessionsCreatedTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", req.Channel),
			attribute.String("outcome", "invalid_input"),
		))
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	telemetry.SessionsCreatedTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("channel", req.Channel),
		attribute.String("outcome", "created"),
	))
	telemetry.ActiveSessions.Add(r.Context(), 1)

	writeJSON(w, http.StatusCreated, sess)
}

func (s *Server) handleListSessions(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/sessions"),
	))

	orgID := r.URL.Query().Get("org_id")
	if orgID == "" {
		writeError(w, http.StatusBadRequest, "org_id query parameter is required")
		return
	}

	sessions := s.sessions.List(orgID)
	writeJSON(w, http.StatusOK, sessions)
}

func (s *Server) handleGetSession(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/sessions/{id}"),
	))

	id := r.PathValue("id")
	sess, err := s.sessions.Get(id)
	if err != nil {
		writeError(w, httpStatus(err), err.Error())
		return
	}

	writeJSON(w, http.StatusOK, sess)
}

func (s *Server) handleIngest(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "POST"),
		attribute.String("path", "/api/v1/sessions/{id}/ingest"),
	))

	id := r.PathValue("id")
	sess, err := s.sessions.Get(id)
	if err != nil {
		telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", "unknown"),
			attribute.String("outcome", errorOutcome(err)),
		))
		writeError(w, httpStatus(err), err.Error())
		return
	}

	var req ingestRequest
	if err := json.NewDecoder(io.LimitReader(r.Body, maxPayloadBytes)).Decode(&req); err != nil {
		telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", sess.Channel),
			attribute.String("outcome", "invalid_input"),
		))
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	adapter, err := s.adapters.Get(sess.Channel)
	if err != nil {
		telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", sess.Channel),
			attribute.String("outcome", "unknown_channel"),
		))
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	if err := s.sessions.UpdateActivity(id); err != nil {
		slog.Warn("failed to update session activity", "session_id", id, "error", err)
	}

	result, err := adapter.Ingest(r.Context(), id, req.Payload)
	if err != nil {
		telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", sess.Channel),
			attribute.String("outcome", "adapter_error"),
		))
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("channel", sess.Channel),
		attribute.String("outcome", "ok"),
	))

	writeJSON(w, http.StatusOK, map[string]any{
		"session_id": id,
		"result":     result,
	})
}

func (s *Server) handleCloseSession(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "DELETE"),
		attribute.String("path", "/api/v1/sessions/{id}"),
	))

	id := r.PathValue("id")
	if err := s.sessions.Close(id); err != nil {
		telemetry.SessionsClosedTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("outcome", errorOutcome(err)),
		))
		writeError(w, httpStatus(err), err.Error())
		return
	}

	telemetry.SessionsClosedTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("outcome", "closed"),
	))
	telemetry.ActiveSessions.Add(r.Context(), -1)

	writeJSON(w, http.StatusOK, map[string]any{"closed": true})
}

// writeJSON serializes v to JSON and writes it as the response body.
func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	if err := json.NewEncoder(w).Encode(v); err != nil {
		slog.Error("failed to encode response", "error", err)
	}
}

// writeError writes a standard JSON error response.
func writeError(w http.ResponseWriter, code int, msg string) {
	writeJSON(w, code, apiError{Code: code, Message: msg})
}

// extractSessionID parses the session ID from a path like
// /api/v1/sessions/{id} or /api/v1/sessions/{id}/ingest. This is a fallback
// helper for routers that do not support path parameters natively.
func extractSessionID(path, prefix string) string {
	rest := strings.TrimPrefix(path, prefix)
	rest = strings.TrimPrefix(rest, "/")
	if idx := strings.Index(rest, "/"); idx != -1 {
		return rest[:idx]
	}
	return rest
}

// Register is a convenience wrapper that attaches the HTTP handler to the
// given mux under the root path. It follows the same naming convention used
// by the browser-broker gRPC Register function but works with net/http.
func Register(mux *http.ServeMux, srv *Server) {
	handler := srv.Handler()
	mux.Handle("/", handler)
}

// GRPCPlaceholder is exported so the gRPC server import is justified in main.
// bridge-core will expose gRPC endpoints in a future iteration; until then
// the gRPC listener is kept alive for readiness probes and service mesh
// integration.
func GRPCPlaceholder(ctx context.Context) {
	_ = ctx
}
