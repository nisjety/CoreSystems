package server

import (
	"errors"
	"net/http"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// httpError writes a JSON error body with the appropriate status code.
func httpError(w http.ResponseWriter, code int, msg string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_, _ = w.Write([]byte(`{"error":"` + msg + `"}`))
}

// mapHTTPStatus translates internal ledger errors into HTTP status codes.
func mapHTTPStatus(err error) int {
	switch {
	case err == nil:
		return http.StatusOK
	case errors.Is(err, ledger.ErrUsageNotFound):
		return http.StatusNotFound
	case errors.Is(err, ledger.ErrBudgetExceededCost),
		errors.Is(err, ledger.ErrBudgetExceededTokens):
		return http.StatusForbidden
	default:
		return http.StatusInternalServerError
	}
}

// budgetOutcome classifies a budget-check error for telemetry labels.
func budgetOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, ledger.ErrBudgetExceededCost):
		return "cost_exceeded"
	case errors.Is(err, ledger.ErrBudgetExceededTokens):
		return "tokens_exceeded"
	default:
		return "internal_error"
	}
}
