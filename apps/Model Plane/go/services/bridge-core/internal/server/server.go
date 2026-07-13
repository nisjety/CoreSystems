// Package server implements the bridge-core HTTP API for session lifecycle
// and payload ingestion.
package server

import (
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"strings"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/bridge-core/internal/authz"
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

// registerSessionRequest keeps legacy identity fields for compatibility, but
// they may only match the verified claims and never define authority.
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
func (s *Server) Handler(verifier *authctx.Verifier) http.Handler {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /healthz", s.handleHealthz)
	mux.HandleFunc("GET /readyz", s.handleReadyz)
	protected := http.NewServeMux()
	protected.HandleFunc("POST /api/v1/sessions", s.handleRegisterSession)
	protected.HandleFunc("GET /api/v1/sessions", s.handleListSessions)
	protected.HandleFunc("GET /api/v1/sessions/{id}", s.handleGetSession)
	protected.HandleFunc("POST /api/v1/sessions/{id}/ingest", s.handleIngest)
	protected.HandleFunc("DELETE /api/v1/sessions/{id}", s.handleCloseSession)
	mux.Handle("/api/", verifier.HTTPMiddleware(authz.Authorize)(protected))

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
	principal, _ := authctx.PrincipalFromContext(r.Context())
	if (strings.TrimSpace(req.OrgID) != "" && req.OrgID != principal.OrganizationID) ||
		(strings.TrimSpace(req.UserID) != "" && req.UserID != principal.ActorID) {
		writeError(w, http.StatusForbidden, "request identity does not match verified identity")
		return
	}

	sess, err := s.sessions.Register(principal.OrganizationID, principal.ActorID, req.Channel)
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

	principal, _ := authctx.PrincipalFromContext(r.Context())

	sessions := s.sessions.ListScoped(principal.OrganizationID, authz.OwnerFilter(principal))
	writeJSON(w, http.StatusOK, sessions)
}

func (s *Server) handleGetSession(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/sessions/{id}"),
	))

	principal, _ := authctx.PrincipalFromContext(r.Context())
	id := r.PathValue("id")
	sess, err := s.sessions.GetScoped(id, principal.OrganizationID, authz.OwnerFilter(principal))
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

	principal, _ := authctx.PrincipalFromContext(r.Context())
	id := r.PathValue("id")
	sess, err := s.sessions.GetScoped(id, principal.OrganizationID, authz.OwnerFilter(principal))
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

	if err := s.sessions.UpdateActivityScoped(id, principal.OrganizationID, authz.OwnerFilter(principal)); err != nil {
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

	// Deliver the processed result back through the channel. For the noop
	// adapter this is a logged no-op; for the webhook adapter this enqueues a
	// durable, retried delivery to the external channel. Enqueue failures are
	// surfaced as a delivery_error outcome but the ingest still returns the
	// processed result so the synchronous caller is not blocked on transport.
	delivered := true
	if derr := adapter.Deliver(r.Context(), id, result); derr != nil {
		delivered = false
		slog.Warn("channel deliver failed", "session_id", id, "channel", sess.Channel, "error", derr)
		telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", sess.Channel),
			attribute.String("outcome", "delivery_error"),
		))
	} else {
		telemetry.IngestTotal.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("channel", sess.Channel),
			attribute.String("outcome", "ok"),
		))
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"session_id": id,
		"result":     result,
		"delivered":  delivered,
	})
}

func (s *Server) handleCloseSession(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "DELETE"),
		attribute.String("path", "/api/v1/sessions/{id}"),
	))

	principal, _ := authctx.PrincipalFromContext(r.Context())
	id := r.PathValue("id")
	if err := s.sessions.CloseScoped(id, principal.OrganizationID, authz.OwnerFilter(principal)); err != nil {
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
