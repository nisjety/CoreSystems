// Package server implements the cost-core HTTP API for usage queries and
// budget enforcement.
package server

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"strconv"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// Server is the cost-core HTTP server backed by an in-memory ledger.
type Server struct {
	ledger *ledger.Store
}

// NewServer constructs a Server with the provided ledger store.
func NewServer(store *ledger.Store) *Server {
	return &Server{ledger: store}
}

// usageResponse is the JSON shape returned by the usage endpoint.
type usageResponse struct {
	OrgID             string  `json:"org_id"`
	UserID            string  `json:"user_id"`
	TotalInputTokens  int64   `json:"total_input_tokens"`
	TotalOutputTokens int64   `json:"total_output_tokens"`
	TotalCostUSD      float64 `json:"total_cost_usd"`
}

// budgetCheckRequest is the JSON body accepted by the budget-check endpoint.
type budgetCheckRequest struct {
	OrgID      string  `json:"org_id"`
	UserID     string  `json:"user_id"`
	MaxCostUSD float64 `json:"max_cost_usd"`
	MaxTokens  int64   `json:"max_tokens"`
}

// budgetCheckResponse is the JSON shape returned by the budget-check endpoint.
type budgetCheckResponse struct {
	Allowed        bool    `json:"allowed"`
	Reason         string  `json:"reason,omitempty"`
	CurrentCostUSD float64 `json:"current_cost_usd"`
	CurrentTokens  int64   `json:"current_tokens"`
}

// RegisterRoutes attaches cost-core API handlers to the given mux.
func (s *Server) RegisterRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /healthz", s.handleHealthz)
	mux.HandleFunc("GET /readyz", s.handleReadyz)
	mux.HandleFunc("GET /api/v1/usage", s.handleGetUsage)
	mux.HandleFunc("POST /api/v1/budget/check", s.handleBudgetCheck)
}

// handleHealthz reports liveness.
func (s *Server) handleHealthz(w http.ResponseWriter, _ *http.Request) {
	telemetry.RequestsTotal.Add(context.TODO(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/healthz"),
	))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

// handleReadyz reports readiness.
func (s *Server) handleReadyz(w http.ResponseWriter, _ *http.Request) {
	telemetry.RequestsTotal.Add(context.TODO(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/readyz"),
	))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

// handleGetUsage returns the current token/cost totals for an org+user pair.
func (s *Server) handleGetUsage(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/usage"),
	))

	orgID := r.URL.Query().Get("org_id")
	userID := r.URL.Query().Get("user_id")
	if orgID == "" || userID == "" {
		httpError(w, http.StatusBadRequest, "org_id and user_id query params are required")
		return
	}

	usage, err := s.ledger.GetUsage(orgID, userID)
	if err != nil {
		code := mapHTTPStatus(err)
		httpError(w, code, err.Error())
		return
	}

	resp := usageResponse{
		OrgID:             usage.OrgID,
		UserID:            usage.UserID,
		TotalInputTokens:  usage.TotalInputTokens,
		TotalOutputTokens: usage.TotalOutputTokens,
		TotalCostUSD:      usage.TotalCostUSD,
	}

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(resp); err != nil {
		slog.Error("failed to encode usage response", "error", err)
	}
}

// handleBudgetCheck verifies whether the accumulated usage for an org+user
// pair would exceed the provided budget caps.
func (s *Server) handleBudgetCheck(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "POST"),
		attribute.String("path", "/api/v1/budget/check"),
	))

	var req budgetCheckRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		httpError(w, http.StatusBadRequest, fmt.Sprintf("invalid request body: %v", err))
		return
	}

	if req.OrgID == "" {
		httpError(w, http.StatusBadRequest, "org_id is required")
		return
	}

	telemetry.BudgetChecks.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("org_id", req.OrgID),
	))

	userID := req.UserID
	if userID == "" {
		userID = "__org__"
	}

	usage, usageErr := s.ledger.GetUsage(req.OrgID, userID)
	var currentCost float64
	var currentTokens int64
	if usageErr == nil {
		currentCost = usage.TotalCostUSD
		currentTokens = usage.TotalInputTokens + usage.TotalOutputTokens
	}

	err := s.ledger.CheckBudget(req.OrgID, userID, req.MaxCostUSD, req.MaxTokens)

	resp := budgetCheckResponse{
		Allowed:        err == nil,
		CurrentCostUSD: currentCost,
		CurrentTokens:  currentTokens,
	}
	if err != nil {
		resp.Reason = err.Error()
		telemetry.BudgetExceeded.Add(r.Context(), 1, metric.WithAttributes(
			attribute.String("org_id", req.OrgID),
			attribute.String("cap_type", budgetOutcome(err)),
		))
	}

	w.Header().Set("Content-Type", "application/json")
	if encErr := json.NewEncoder(w).Encode(resp); encErr != nil {
		slog.Error("failed to encode budget check response", "error", encErr)
	}
}

// RecordUsage is called by the NATS subscriber to write a usage event into
// the ledger and emit telemetry.
func (s *Server) RecordUsage(orgID, userID string, inputTokens, outputTokens int32, costUSD float64) {
	s.ledger.Record(orgID, userID, inputTokens, outputTokens, costUSD)

	telemetry.TokensRecorded.Add(context.TODO(), int64(inputTokens)+int64(outputTokens),
		metric.WithAttributes(attribute.String("org_id", orgID)),
	)

	slog.Debug("usage recorded",
		"org_id", orgID,
		"user_id", userID,
		"input_tokens", strconv.FormatInt(int64(inputTokens), 10),
		"output_tokens", strconv.FormatInt(int64(outputTokens), 10),
		"cost_usd", fmt.Sprintf("%.8f", costUSD),
	)
}
