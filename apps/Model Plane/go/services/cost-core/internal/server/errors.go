package server

import (
	"errors"
	"net/http"
	"strconv"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// maxListLimit caps the number of entries the list endpoint will return.
const maxListLimit = 1000

// httpError writes a JSON error body with the appropriate status code.
func httpError(w http.ResponseWriter, code int, msg string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	// Encode via strconv.Quote to safely escape quotes/control chars in msg.
	_, _ = w.Write([]byte(`{"error":` + strconv.Quote(msg) + `}`))
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

// filterFromQuery builds an AggregateFilter from request query params. The
// since/until params accept RFC3339 timestamps; an unparseable value is a
// 400-worthy error.
func filterFromQuery(r interface{ FormValue(string) string }) (ledger.AggregateFilter, error) {
	f := ledger.AggregateFilter{
		OrgID:  r.FormValue("org_id"),
		UserID: r.FormValue("user_id"),
		RunID:  r.FormValue("run_id"),
		Model:  r.FormValue("model"),
	}
	if v := r.FormValue("since"); v != "" {
		t, err := time.Parse(time.RFC3339, v)
		if err != nil {
			return f, errors.New("since must be an RFC3339 timestamp")
		}
		f.Since = t
	}
	if v := r.FormValue("until"); v != "" {
		t, err := time.Parse(time.RFC3339, v)
		if err != nil {
			return f, errors.New("until must be an RFC3339 timestamp")
		}
		f.Until = t
	}
	return f, nil
}

// parseLimit parses the list limit, clamping to [0, maxListLimit]. A blank or
// invalid value yields 0, which the ledger interprets as its default.
func parseLimit(s string) int {
	if s == "" {
		return 0
	}
	n, err := strconv.Atoi(s)
	if err != nil || n < 0 {
		return 0
	}
	if n > maxListLimit {
		return maxListLimit
	}
	return n
}
