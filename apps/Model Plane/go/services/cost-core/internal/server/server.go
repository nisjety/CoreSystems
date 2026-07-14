// Package server implements the cost-core HTTP API for recording cost-bearing
// events and querying usage rollups and budget enforcement, backed by a
// durable (or in-memory fallback) ledger.
package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/pricing"
	"github.com/triodelab/model-plane/services/cost-core/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// Server is the cost-core HTTP server backed by a ledger implementation and a
// price catalogue (the resolver that turns tokens into a USD cost when a usage
// event arrives without one).
type Server struct {
	ledger          ledger.Ledger
	pricing         *pricing.Resolver
	requireIdentity bool
}

// NewServer constructs a Server with the provided ledger and the built-in
// default price catalogue. Call SetPricing to swap in a DB-backed catalogue.
func NewServer(l ledger.Ledger) *Server {
	return &Server{ledger: l, pricing: pricing.Default()}
}

// SetPricing replaces the price catalogue (e.g. with one loaded from Postgres).
// A nil resolver is ignored so the server always has a usable catalogue.
func (s *Server) SetPricing(p *pricing.Resolver) {
	if p != nil {
		s.pricing = p
	}
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
	ProducerID   string    `json:"producer_id,omitempty"`
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
	s.registerAPIRoutes(mux)
}

func (s *Server) registerAPIRoutes(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/v1/cost/record", s.handleRecord)
	mux.HandleFunc("GET /api/v1/usage", s.handleGetUsage)
	mux.HandleFunc("GET /api/v1/cost/run", s.handleGetRunUsage)
	mux.HandleFunc("GET /api/v1/cost/aggregate", s.handleAggregate)
	mux.HandleFunc("GET /api/v1/cost/entries", s.handleListEntries)
	mux.HandleFunc("GET /api/v1/pricing", s.handleListPricing)
	mux.HandleFunc("POST /api/v1/budget/check", s.handleBudgetCheck)
}

// Handler exposes public liveness and the global, non-tenant pricing catalogue.
// Every tenant-bearing API is wrapped in the authentication middleware. A nil
// middleware fails closed for those protected routes.
func (s *Server) Handler(protect func(http.Handler) http.Handler) http.Handler {
	root := http.NewServeMux()
	root.HandleFunc("GET /healthz", s.handleHealthz)
	root.HandleFunc("GET /readyz", s.handleReadyz)
	root.HandleFunc("GET /api/v1/pricing", s.handleListPricing)
	if protect == nil {
		root.Handle("/api/", http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			httpError(w, http.StatusServiceUnavailable, "authentication is unavailable")
		}))
		return root
	}
	protectedServer := *s
	protectedServer.requireIdentity = true
	api := http.NewServeMux()
	protectedServer.registerAPIRoutes(api)
	root.Handle("/api/", protect(api))
	return root
}

// CostAuthorizer applies route-level authorization after token verification.
// User principals may read data only within their signed organization and may
// check their own budget. Ledger writes require cost:write; service org-wide
// reads and budget checks require cost:read.
func CostAuthorizer(principal authctx.Principal, r *http.Request) error {
	if principal.PrincipalType == "user" {
		if r.Method == http.MethodGet || (r.Method == http.MethodPost && r.URL.Path == "/api/v1/budget/check") {
			return nil
		}
		return errors.New("user principal is not authorized for this cost operation")
	}
	if principal.PrincipalType != "service" {
		return errors.New("unsupported principal type")
	}
	if r.Method == http.MethodPost && r.URL.Path == "/api/v1/cost/record" {
		if principal.HasScope("cost:write") {
			return nil
		}
		return errors.New("cost:write scope is required")
	}
	if principal.HasScope("cost:read") {
		return nil
	}
	return errors.New("cost:read scope is required")
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
	if err := decodeRequestJSON(w, r, &req); err != nil {
		httpError(w, http.StatusBadRequest, fmt.Sprintf("invalid request body: %v", err))
		return
	}
	principal, authenticated := s.principal(w, r)
	if s.requireIdentity && !authenticated {
		return
	}
	if authenticated {
		if !pinOrganization(w, req.OrgID, principal.OrganizationID) {
			return
		}
		req.OrgID = principal.OrganizationID
		if principal.PrincipalType == "user" {
			if req.UserID != "" && req.UserID != principal.ActorID {
				httpError(w, http.StatusForbidden, "user scope does not match verified token")
				return
			}
			req.UserID = principal.ActorID
		}
	}
	if req.OrgID == "" {
		httpError(w, http.StatusBadRequest, "org_id is required")
		return
	}

	entry := ledger.Entry{
		OrgID:          req.OrgID,
		UserID:         req.UserID,
		RunID:          req.RunID,
		RequestID:      req.RequestID,
		Model:          req.Model,
		InputTokens:    req.InputTokens,
		OutputTokens:   req.OutputTokens,
		CostUSD:        req.CostUSD,
		IdempotencyKey: req.IdempotencyKey,
	}
	if authenticated && principal.PrincipalType == "service" {
		entry.ProducerID = principal.ActorID
	}
	if err := s.RecordUsage(r.Context(), entry); err != nil {
		httpError(w, mapHTTPStatus(err), fmt.Sprintf("record failed: %v", err))
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
	principal, authenticated := s.principal(w, r)
	if s.requireIdentity && !authenticated {
		return
	}
	if authenticated {
		if !pinOrganization(w, orgID, principal.OrganizationID) {
			return
		}
		orgID = principal.OrganizationID
		if principal.PrincipalType == "user" && userID != principal.ActorID {
			httpError(w, http.StatusForbidden, "user scope does not match verified token")
			return
		}
	}
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
	orgID := r.URL.Query().Get("org_id")
	if runID == "" {
		httpError(w, http.StatusBadRequest, "run_id query param is required")
		return
	}

	principal, authenticated := s.principal(w, r)
	if s.requireIdentity && !authenticated {
		return
	}
	var usage *ledger.Usage
	var err error
	if authenticated {
		if !pinOrganization(w, orgID, principal.OrganizationID) {
			return
		}
		usage, err = s.ledger.Aggregate(r.Context(), ledger.AggregateFilter{
			OrgID: principal.OrganizationID,
			RunID: runID,
		})
		if err == nil && usage.EntryCount == 0 {
			err = ledger.ErrUsageNotFound
		}
	} else {
		usage, err = s.ledger.GetRunUsage(r.Context(), runID)
	}
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
	principal, authenticated := s.principal(w, r)
	if s.requireIdentity && !authenticated {
		return
	}
	if authenticated {
		if !pinOrganization(w, f.OrgID, principal.OrganizationID) {
			return
		}
		f.OrgID = principal.OrganizationID
		// Cross-user guard: a user token may only read its OWN cost rows within
		// the org (mirrors the record path). Service principals — e.g. the
		// budget checker — may still aggregate org-wide.
		if principal.PrincipalType == "user" {
			if f.UserID != "" && f.UserID != principal.ActorID {
				httpError(w, http.StatusForbidden, "user scope does not match verified token")
				return
			}
			f.UserID = principal.ActorID
		}
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
	principal, authenticated := s.principal(w, r)
	if s.requireIdentity && !authenticated {
		return
	}
	if authenticated {
		if !pinOrganization(w, f.OrgID, principal.OrganizationID) {
			return
		}
		f.OrgID = principal.OrganizationID
		// Cross-user guard: a user token may only read its OWN cost rows within
		// the org (mirrors the record path). Service principals — e.g. the
		// budget checker — may still aggregate org-wide.
		if principal.PrincipalType == "user" {
			if f.UserID != "" && f.UserID != principal.ActorID {
				httpError(w, http.StatusForbidden, "user scope does not match verified token")
				return
			}
			f.UserID = principal.ActorID
		}
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
			ProducerID:   e.ProducerID,
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
	if err := decodeRequestJSON(w, r, &req); err != nil {
		httpError(w, http.StatusBadRequest, fmt.Sprintf("invalid request body: %v", err))
		return
	}
	principal, authenticated := s.principal(w, r)
	if s.requireIdentity && !authenticated {
		return
	}
	if authenticated {
		if !pinOrganization(w, req.OrgID, principal.OrganizationID) {
			return
		}
		req.OrgID = principal.OrganizationID
		if principal.PrincipalType == "user" {
			if req.UserID != "" && req.UserID != principal.ActorID {
				httpError(w, http.StatusForbidden, "user scope does not match verified token")
				return
			}
			req.UserID = principal.ActorID
		}
	}
	if req.OrgID == "" {
		httpError(w, http.StatusBadRequest, "org_id is required")
		return
	}
	if err := ledger.ValidateScope(req.OrgID, req.UserID); err != nil {
		httpError(w, http.StatusBadRequest, err.Error())
		return
	}
	if err := ledger.ValidateBudget(req.MaxCostUSD, req.MaxTokens); err != nil {
		httpError(w, http.StatusBadRequest, err.Error())
		return
	}

	telemetry.BudgetChecks.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("org_id", req.OrgID),
	))

	// Budget scope: an empty user_id means an ORG-WIDE budget — the intent layer
	// (inference-core) sends no per-user scope, so aggregate across all of the
	// org's users. A specific user_id keeps the per-user rollup. The legacy
	// "__org__" sentinel is gone: it never matched stored rows (entries carry the
	// real user_id), so the org budget always read $0 and the posture never left
	// Healthy — the downgrade could not fire even with a fed ledger.
	var usage *ledger.Usage
	var usageErr error
	if req.UserID == "" {
		usage, usageErr = s.ledger.Aggregate(r.Context(), ledger.AggregateFilter{OrgID: req.OrgID})
	} else {
		usage, usageErr = s.ledger.GetUsage(r.Context(), req.OrgID, req.UserID)
	}

	if usageErr != nil && !errors.Is(usageErr, ledger.ErrUsageNotFound) {
		slog.Error("budget ledger read failed", "org_id", req.OrgID, "error", usageErr)
		httpError(w, http.StatusServiceUnavailable, "budget state is unavailable")
		return
	}

	var currentCost float64
	var currentTokens int64
	if usageErr == nil && usage != nil {
		currentCost = usage.TotalCostUSD
		currentTokens = usage.TotalInputTokens + usage.TotalOutputTokens
	}

	err := budgetDecision(req.MaxCostUSD, req.MaxTokens, currentCost, currentTokens)
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

func (s *Server) principal(w http.ResponseWriter, r *http.Request) (authctx.Principal, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok && s.requireIdentity {
		httpError(w, http.StatusUnauthorized, "verified identity is required")
	}
	return principal, ok
}

func pinOrganization(w http.ResponseWriter, requested, verified string) bool {
	requested = strings.TrimSpace(requested)
	if requested != "" && requested != verified {
		httpError(w, http.StatusForbidden, "organization scope does not match verified token")
		return false
	}
	return true
}

// handleListPricing returns the model price catalogue (USD per 1M tokens). The
// gateway reads this to compute the SSE display cost off the same source as the
// ledger; the cost dashboard renders it as the per-model rate card.
func (s *Server) handleListPricing(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/pricing"),
	))
	rates := s.pricing.Rates()
	writeJSON(w, map[string]any{"rates": rates, "count": len(rates)})
}

// budgetDecision reports whether accumulated usage is within the caps, reusing
// the ledger's sentinel errors so budgetOutcome can classify the cap type. A
// cap <= 0 disables that check. Mirrors ledger.Store.CheckBudget but operates
// on already-aggregated totals, so an org-wide aggregate is checked without a
// per-user rollup.
func budgetDecision(maxCostUSD float64, maxTokens int64, cost float64, tokens int64) error {
	if maxCostUSD > 0 && cost >= maxCostUSD {
		return fmt.Errorf("%w: current %.6f >= limit %.6f", ledger.ErrBudgetExceededCost, cost, maxCostUSD)
	}
	if maxTokens > 0 && tokens >= maxTokens {
		return fmt.Errorf("%w: current %d >= limit %d", ledger.ErrBudgetExceededTokens, tokens, maxTokens)
	}
	return nil
}

// RecordUsage writes a cost event into the ledger and emits telemetry. It is
// used both by the HTTP record endpoint and the USAGE_ENVELOPE subscriber.
//
// When the event carries no pre-computed cost (the common case — the gateway
// publishes token counts only), cost-core prices it authoritatively from the
// catalogue so the dollar ledger and the budget posture are real, never $0.
func (s *Server) RecordUsage(ctx context.Context, e ledger.Entry) error {
	if err := ledger.ValidateEntry(e); err != nil {
		return err
	}
	if e.CostUSD == 0 && (e.InputTokens > 0 || e.OutputTokens > 0) && s.pricing != nil {
		e.CostUSD = s.pricing.Cost(e.Model, e.InputTokens, e.OutputTokens)
	}
	if err := ledger.ValidateEntry(e); err != nil {
		return err
	}
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

const maxRequestBodyBytes = 64 << 10

func decodeRequestJSON(w http.ResponseWriter, r *http.Request, target any) error {
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, maxRequestBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		if err == nil {
			return errors.New("request body must contain one JSON object")
		}
		return err
	}
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
