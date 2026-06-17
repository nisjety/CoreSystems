// Package server implements the cost-core HTTP API for recording cost-bearing
// events and querying usage rollups and budget enforcement, backed by a
// durable (or in-memory fallback) ledger.
package server

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// Server is the cost-core HTTP server backed by a ledger implementation.
type Server struct {
	ledger ledger.Ledger
}

// NewServer constructs a Server with the provided ledger.
func NewServer(l ledger.Ledger) *Server {
	return &Server{ledger: l}
}

// recordRequest is the JSON body accepted by the record endpoint.
type recordRequest struct {
	OrgID          string  `json:"org_id"`
	UserID         string  `json:"user_id"`
	RunID          string  `json:"run_id"`
	RequestID      string  `json:"request_id"`
	Model          string  `json:"model"`
	InputTokens    int64   `json:"input_tokens"`
	OutputTokens   int64   `json:"output_tokens"`
	CostUSD        float64 `json:"cost_usd"`
	IdempotencyKey string  `json:"idempotency_key"`
}

// usageResponse is the JSON shape returned by the usage/run/aggregate endpoints.
type usageResponse struct {
	OrgID             string  `json:"org_id"`
	UserID            string  `json:"user_id,omitempty"`
	RunID             string  `json:"run_id,omitempty"`
	TotalInputTokens  int64   `json:"total_input_tokens"`
	TotalOutputTokens int64   `json:"total_output_tokens"`
	TotalCostUSD      float64 `json:"total_cost_usd"`
	EntryCount        int64   `json:"entry_count"`
}

// entryResponse is the JSON shape of a single ledger entry in a list.
type entryResponse struct {
	OrgID        string    `json:"org_id"`
	UserID       string    `json:"user_id"`
	RunID        string    `json:"run_id,omitempty"`
	RequestID    string    `json:"request_id,omitempty"`
	Model        string    `json:"model,omitempty"`
	InputTokens  int64     `json:"input_tokens"`
	OutputTokens int64     `json:"output_tokens"`
	CostUSD      float64   `json:"cost_usd"`
	CreatedAt    time.Time `json:"created_at"`
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
	mux.HandleFunc("POST /api/v1/cost/record", s.handleRecord)
	mux.HandleFunc("GET /api/v1/usage", s.handleGetUsage)
	mux.HandleFunc("GET /api/v1/cost/run", s.handleGetRunUsage)
	mux.HandleFunc("GET /api/v1/cost/aggregate", s.handleAggregate)
	mux.HandleFunc("GET /api/v1/cost/entries", s.handleListEntries)
	mux.HandleFunc("POST /api/v1/budget/check", s.handleBudgetCheck)
}

func (s *Server) handleHealthz(w http.ResponseWriter, _ *http.Request) {
	telemetry.RequestsTotal.Add(context.Background(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/healthz"),
	))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

func (s *Server) handleReadyz(w http.ResponseWriter, _ *http.Request) {
	telemetry.RequestsTotal.Add(context.Background(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/readyz"),
	))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

// handleRecord appends a cost-bearing event to the durable ledger.
func (s *Server) handleRecord(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "POST"),
		attribute.String("path", "/api/v1/cost/record"),
	))

	var req recordRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		httpError(w, http.StatusBadRequest, fmt.Sprintf("invalid request body: %v", err))
		return
	}
	if req.OrgID == "" {
		httpError(w, http.StatusBadRequest, "org_id is required")
		return
	}

	if err := s.RecordUsage(r.Context(), ledger.Entry{
		OrgID:          req.OrgID,
		UserID:         req.UserID,
		RunID:          req.RunID,
		RequestID:      req.RequestID,
		Model:          req.Model,
		InputTokens:    req.InputTokens,
		OutputTokens:   req.OutputTokens,
		CostUSD:        req.CostUSD,
		IdempotencyKey: req.IdempotencyKey,
	}); err != nil {
		httpError(w, http.StatusInternalServerError, fmt.Sprintf("record failed: %v", err))
		return
	}

	w.WriteHeader(http.StatusAccepted)
	_, _ = w.Write([]byte(`{"recorded":true}`))
}

// handleGetUsage returns the rolled-up totals for an org+user pair.
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

	usage, err := s.ledger.GetUsage(r.Context(), orgID, userID)
	if err != nil {
		httpError(w, mapHTTPStatus(err), err.Error())
		return
	}
	writeJSON(w, usageFromLedger(usage))
}

// handleGetRunUsage returns the rolled-up totals for a single run.
func (s *Server) handleGetRunUsage(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/cost/run"),
	))

	runID := r.URL.Query().Get("run_id")
	if runID == "" {
		httpError(w, http.StatusBadRequest, "run_id query param is required")
		return
	}

	usage, err := s.ledger.GetRunUsage(r.Context(), runID)
	if err != nil {
		httpError(w, mapHTTPStatus(err), err.Error())
		return
	}
	writeJSON(w, usageFromLedger(usage))
}

// handleAggregate returns rolled-up totals across entries matching the filter.
func (s *Server) handleAggregate(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/cost/aggregate"),
	))

	f, err := filterFromQuery(r)
	if err != nil {
		httpError(w, http.StatusBadRequest, err.Error())
		return
	}

	usage, err := s.ledger.Aggregate(r.Context(), f)
	if err != nil {
		httpError(w, mapHTTPStatus(err), err.Error())
		return
	}
	writeJSON(w, usageFromLedger(usage))
}

// handleListEntries returns the most recent entries matching the filter.
func (s *Server) handleListEntries(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/cost/entries"),
	))

	f, err := filterFromQuery(r)
	if err != nil {
		httpError(w, http.StatusBadRequest, err.Error())
		return
	}
	limit := parseLimit(r.URL.Query().Get("limit"))

	entries, err := s.ledger.ListEntries(r.Context(), f, limit)
	if err != nil {
		httpError(w, mapHTTPStatus(err), err.Error())
		return
	}

	out := make([]entryResponse, 0, len(entries))
	for i := range entries {
		e := entries[i]
		out = append(out, entryResponse{
			OrgID:        e.OrgID,
			UserID:       e.UserID,
			RunID:        e.RunID,
			RequestID:    e.RequestID,
			Model:        e.Model,
			InputTokens:  e.InputTokens,
			OutputTokens: e.OutputTokens,
			CostUSD:      e.CostUSD,
			CreatedAt:    e.CreatedAt,
		})
	}
	writeJSON(w, map[string]any{"entries": out, "count": len(out)})
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

	var currentCost float64
	var currentTokens int64
	if usage, usageErr := s.ledger.GetUsage(r.Context(), req.OrgID, userID); usageErr == nil {
		currentCost = usage.TotalCostUSD
		currentTokens = usage.TotalInputTokens + usage.TotalOutputTokens
	}

	err := s.ledger.CheckBudget(r.Context(), req.OrgID, userID, req.MaxCostUSD, req.MaxTokens)
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
	writeJSON(w, resp)
}

// RecordUsage writes a cost event into the ledger and emits telemetry. It is
// used both by the HTTP record endpoint and the USAGE_ENVELOPE subscriber.
func (s *Server) RecordUsage(ctx context.Context, e ledger.Entry) error {
	if err := s.ledger.RecordEntry(ctx, e); err != nil {
		slog.Error("failed to record usage", "org_id", e.OrgID, "error", err)
		return err
	}

	telemetry.TokensRecorded.Add(ctx, e.InputTokens+e.OutputTokens,
		metric.WithAttributes(attribute.String("org_id", e.OrgID)),
	)

	slog.Debug("usage recorded",
		"org_id", e.OrgID,
		"user_id", e.UserID,
		"run_id", e.RunID,
		"input_tokens", e.InputTokens,
		"output_tokens", e.OutputTokens,
		"cost_usd", fmt.Sprintf("%.8f", e.CostUSD),
	)
	return nil
}

func usageFromLedger(u *ledger.Usage) usageResponse {
	return usageResponse{
		OrgID:             u.OrgID,
		UserID:            u.UserID,
		RunID:             u.RunID,
		TotalInputTokens:  u.TotalInputTokens,
		TotalOutputTokens: u.TotalOutputTokens,
		TotalCostUSD:      u.TotalCostUSD,
		EntryCount:        u.EntryCount,
	}
}

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(v); err != nil {
		slog.Error("failed to encode response", "error", err)
	}
}
