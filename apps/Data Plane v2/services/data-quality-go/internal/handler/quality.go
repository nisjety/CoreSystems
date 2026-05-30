package handler

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"

	"time"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/cost"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/eval"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/gates"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/lint"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/trust"
)

type contextKey string

const orgIDKey contextKey = "org_id"

func OrgIDMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		orgID := r.Header.Get("X-Org-ID")
		if orgID == "" {
			writeError(w, http.StatusBadRequest, "X-Org-ID header required")
			return
		}
		ctx := context.WithValue(r.Context(), orgIDKey, orgID)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func orgIDFrom(ctx context.Context) string {
	v, _ := ctx.Value(orgIDKey).(string)
	return v
}

type QualityHandler struct {
	runner    *eval.Runner
	scorer    *trust.Scorer
	checker   *gates.Checker
	linter    *lint.Linter
	costQuery *cost.Query
}

func NewQualityHandler(r *eval.Runner, s *trust.Scorer, c *gates.Checker, l *lint.Linter, cq *cost.Query) *QualityHandler {
	return &QualityHandler{runner: r, scorer: s, checker: c, linter: l, costQuery: cq}
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

	var input model.CreateEvalInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID

	if input.Strategy == "" {
		writeError(w, http.StatusBadRequest, "strategy required")
		return
	}

	evalRun, err := h.runner.CreateEval(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create eval")
		return
	}

	go func() {
		h.runner.RunEval(context.Background(), evalRun)
	}()

	writeJSON(w, http.StatusAccepted, evalRun)
}

func (h *QualityHandler) GetEval(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{
		"message": "eval lookup by ID — requires persistent storage (future)",
	})
}

func (h *QualityHandler) CompareEval(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var input model.CompareEvalInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID

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
