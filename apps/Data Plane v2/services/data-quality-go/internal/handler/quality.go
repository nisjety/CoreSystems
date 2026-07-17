package handler

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/cost"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/eval"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/gates"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/lint"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/trust"
)

func orgIDFrom(ctx context.Context) string {
	claims, ok := authctx.FromContext(ctx)
	if !ok {
		return ""
	}
	return claims.OrgID
}

type QualityHandler struct {
	runner    EvalRunner
	golden    GoldenStore
	scorer    *trust.Scorer
	checker   *gates.Checker
	linter    *lint.Linter
	costQuery *cost.Query
}

type EvalRunner interface {
	CreateEval(context.Context, model.CreateEvalInput) (*model.EvalRun, bool, error)
	RunEval(context.Context, string, string) error
	GetEval(context.Context, string, string) (*model.EvalRun, error)
	RunCompare(context.Context, model.CompareEvalInput) (*model.CompareResult, error)
}

// GoldenStore manages the org's judged queries (golden sets) that upgrade eval
// metrics from candidate-count proxies to real recall/nDCG/MRR.
type GoldenStore interface {
	Upsert(ctx context.Context, orgID, query string, relevantIDs []string) error
	List(ctx context.Context, orgID string) (map[string][]string, error)
}

func NewQualityHandler(r EvalRunner, g GoldenStore, s *trust.Scorer, c *gates.Checker, l *lint.Linter, cq *cost.Query) *QualityHandler {
	return &QualityHandler{runner: r, golden: g, scorer: s, checker: c, linter: l, costQuery: cq}
}

type qualityRouteHandlers struct {
	runEval      http.HandlerFunc
	getEval      http.HandlerFunc
	compareEval  http.HandlerFunc
	upsertGolden http.HandlerFunc
	listGolden   http.HandlerFunc
	scoreTrust   http.HandlerFunc
	checkGates   http.HandlerFunc
	lint         http.HandlerFunc
	costSummary  http.HandlerFunc
}

// MountProtectedRoutes keeps the auth boundary and the complete sensitive
// route table together so adding a route cannot accidentally bypass it.
func MountProtectedRoutes(r chi.Router, auth func(http.Handler) http.Handler, h *QualityHandler) {
	mountProtectedRoutes(r, auth, qualityRouteHandlers{
		runEval:      h.RunEval,
		getEval:      h.GetEval,
		compareEval:  h.CompareEval,
		upsertGolden: h.UpsertGolden,
		listGolden:   h.ListGolden,
		scoreTrust:   h.ScoreTrust,
		checkGates:   h.CheckGates,
		lint:         h.Lint,
		costSummary:  h.CostSummary,
	})
}

func mountProtectedRoutes(r chi.Router, auth func(http.Handler) http.Handler, h qualityRouteHandlers) {
	r.Route("/v1/evals", func(r chi.Router) {
		r.Use(auth)
		r.Use(authctx.RequireScope("data:quality:admin"))
		r.Post("/retrieval", h.runEval)
		r.Get("/retrieval/{evalID}", h.getEval)
		r.Post("/compare", h.compareEval)
		r.Post("/golden", h.upsertGolden)
		r.Get("/golden", h.listGolden)
	})
	r.Route("/v1/quality", func(r chi.Router) {
		r.Use(auth)
		r.Use(authctx.RequireScope("data:quality:admin"))
		r.Post("/trust", h.scoreTrust)
		r.Get("/gates", h.checkGates)
		r.Get("/lint", h.lint)
	})
	r.Route("/v1/cost", func(r chi.Router) {
		r.Use(auth)
		r.Use(authctx.RequireScope("data:quality:admin"))
		r.Get("/summary", h.costSummary)
	})
}

// CostSummary returns aggregated cost events for an org over a time window.
// Query params: from (RFC3339), to (RFC3339). Defaults to last 30d.
func (h *QualityHandler) CostSummary(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var from, to time.Time
	if v := r.URL.Query().Get("from"); v != "" {
		if t, err := time.Parse(time.RFC3339, v); err == nil {
			from = t
		} else {
			writeError(w, http.StatusBadRequest, "from must be RFC3339")
			return
		}
	}
	if v := r.URL.Query().Get("to"); v != "" {
		if t, err := time.Parse(time.RFC3339, v); err == nil {
			to = t
		} else {
			writeError(w, http.StatusBadRequest, "to must be RFC3339")
			return
		}
	}

	summary, err := h.costQuery.Summary(r.Context(), orgID, from, to)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "cost query failed: "+err.Error())
		return
	}
	writeJSON(w, http.StatusOK, summary)
}

func (h *QualityHandler) Lint(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	staleAfter := 90
	if v := r.URL.Query().Get("stale_after_days"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			staleAfter = n
		}
	}

	report, err := h.linter.Run(r.Context(), orgID, staleAfter)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "lint failed: "+err.Error())
		return
	}
	writeJSON(w, http.StatusOK, report)
}

func (h *QualityHandler) RunEval(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	idempotencyKey, err := requestIdempotencyKey(r)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	var input model.CreateEvalInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	input.IdempotencyKey = idempotencyKey

	if input.Strategy == "" {
		writeError(w, http.StatusBadRequest, "strategy required")
		return
	}

	evalRun, created, err := h.runner.CreateEval(r.Context(), input)
	if errors.Is(err, eval.ErrIdempotencyConflict) {
		writeError(w, http.StatusConflict, "idempotency key is bound to another request")
		return
	}
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create eval")
		return
	}

	if created {
		evalCtx := context.WithoutCancel(r.Context())
		go func() {
			if err := h.runner.RunEval(evalCtx, orgID, evalRun.EvalID); err != nil {
				log.Error().Err(err).Str("eval_id", evalRun.EvalID).Str("org_id", orgID).Msg("quality evaluation failed")
			}
		}()
	}

	writeJSON(w, http.StatusAccepted, evalRun)
}

func (h *QualityHandler) GetEval(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	evalID := strings.TrimSpace(chi.URLParam(r, "evalID"))
	if evalID == "" {
		writeError(w, http.StatusBadRequest, "eval ID required")
		return
	}
	run, err := h.runner.GetEval(r.Context(), orgID, evalID)
	if errors.Is(err, eval.ErrNotFound) {
		writeError(w, http.StatusNotFound, "evaluation not found")
		return
	}
	if err != nil {
		writeError(w, http.StatusInternalServerError, "evaluation lookup failed")
		return
	}
	writeJSON(w, http.StatusOK, run)
}

func (h *QualityHandler) CompareEval(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	idempotencyKey, err := requestIdempotencyKey(r)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	var input model.CompareEvalInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	input.IdempotencyKey = idempotencyKey

	if input.StrategyA == "" || input.StrategyB == "" {
		writeError(w, http.StatusBadRequest, "strategy_a and strategy_b required")
		return
	}

	result, err := h.runner.RunCompare(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "comparison eval failed")
		return
	}

	writeJSON(w, http.StatusOK, result)
}

// UpsertGolden stores/replaces the judgment for one query: the ids (document
// and/or knowledge ids) a human/agent judged relevant. Evals score judged
// queries with real recall@10/nDCG@10/MRR from then on.
func (h *QualityHandler) UpsertGolden(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var req struct {
		Query       string   `json:"query"`
		RelevantIDs []string `json:"relevant_ids"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if strings.TrimSpace(req.Query) == "" {
		writeError(w, http.StatusBadRequest, "query required")
		return
	}
	if len(req.RelevantIDs) == 0 || len(req.RelevantIDs) > 500 {
		writeError(w, http.StatusBadRequest, "relevant_ids must contain 1-500 ids")
		return
	}
	for _, id := range req.RelevantIDs {
		if strings.TrimSpace(id) == "" {
			writeError(w, http.StatusBadRequest, "relevant_ids must not contain blank ids")
			return
		}
	}

	if err := h.golden.Upsert(r.Context(), orgID, req.Query, req.RelevantIDs); err != nil {
		log.Error().Err(err).Str("org_id", orgID).Msg("golden judgment upsert failed")
		writeError(w, http.StatusInternalServerError, "failed to store golden judgment")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"query_norm":   eval.NormalizeQuery(req.Query),
		"relevant_ids": len(req.RelevantIDs),
	})
}

// ListGolden returns the org's judged queries (normalized) with their ids.
func (h *QualityHandler) ListGolden(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	judgments, err := h.golden.List(r.Context(), orgID)
	if err != nil {
		log.Error().Err(err).Str("org_id", orgID).Msg("golden judgment list failed")
		writeError(w, http.StatusInternalServerError, "failed to list golden judgments")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"judgments": judgments})
}

func requestIdempotencyKey(r *http.Request) (string, error) {
	key := r.Header.Get("Idempotency-Key")
	if key != strings.TrimSpace(key) || len(key) < 8 || len(key) > 120 {
		return "", errors.New("Idempotency-Key must be 8-120 non-whitespace characters")
	}
	for _, char := range key {
		if char <= 0x20 || char >= 0x7f {
			return "", errors.New("Idempotency-Key must be 8-120 non-whitespace characters")
		}
	}
	return key, nil
}

func (h *QualityHandler) ScoreTrust(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var req struct {
		DocumentIDs []string `json:"document_ids"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	scores, err := h.scorer.ScoreDocuments(r.Context(), orgID, req.DocumentIDs)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "trust scoring failed")
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{"scores": scores})
}

func (h *QualityHandler) CheckGates(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	report, err := h.checker.CheckReleaseGates(r.Context(), orgID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "gate check failed")
		return
	}

	status := http.StatusOK
	if !report.AllPassed {
		status = http.StatusPreconditionFailed
	}
	writeJSON(w, status, report)
}

func Health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok", "service": "data-quality-go"})
}

func Readyz(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ready", "service": "data-quality-go"})
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}
