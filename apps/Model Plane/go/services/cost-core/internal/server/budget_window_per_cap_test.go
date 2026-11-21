package server

import (
	"context"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// seedPerCapLedger records one September spend and one October spend for org1, each with metered
// tokens: September is 700 tokens (u1), October is 350 (u2), 1050 in all.
func seedPerCapLedger(t *testing.T, srv *Server) {
	t.Helper()
	ctx := context.Background()
	for _, e := range []ledger.Entry{
		{OrgID: "org1", UserID: "u1", Model: "gpt-4o", InputTokens: 600, OutputTokens: 100, CostUSD: 4,
			CostBasis: ledger.CostBasisMetered, CreatedAt: time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)},
		{OrgID: "org1", UserID: "u2", Model: "gpt-4o", InputTokens: 300, OutputTokens: 50, CostUSD: 3,
			CostBasis: ledger.CostBasisMetered, CreatedAt: time.Date(2026, 10, 3, 9, 0, 0, 0, time.UTC)},
	} {
		if err := srv.ledger.RecordEntry(ctx, e); err != nil {
			t.Fatalf("RecordEntry: %v", err)
		}
	}
}

// Each cap is measured over the window that names it: the cost cap over since_cost, the token cap over
// since_tokens, and a cap whose own window is absent over since (lifetime when that is absent too).
func TestBudgetCheckMeasuresEachCapOverItsOwnWindow(t *testing.T) {
	srv, mux := newTestServer()
	seedPerCapLedger(t, srv)

	const october = `"2026-10-01T00:00:00Z"`
	const september = `"2026-09-01T00:00:00Z"`
	cases := []struct {
		name        string
		body        string
		wantAllowed bool
		wantCost    float64
		wantTokens  int64
	}{
		// Cost from October (3 < 5), tokens lifetime (1050 >= 1000): refused on tokens.
		{"cost window only, tokens lifetime",
			`{"org_id":"org1","max_cost_usd":5,"max_tokens":1000,"since_cost":` + october + `}`, false, 3, 1050},
		// Tokens from October (350 < 1000), cost lifetime (7 >= 5): refused on cost.
		{"token window only, cost lifetime",
			`{"org_id":"org1","max_cost_usd":5,"max_tokens":1000,"since_tokens":` + october + `}`, false, 7, 350},
		// Each cap over its own window: cost from October, tokens from September (1050 < 2000).
		{"each cap over its own window",
			`{"org_id":"org1","max_cost_usd":5,"max_tokens":2000,"since_cost":` + october + `,"since_tokens":` + september + `}`, true, 3, 1050},
		// Equal windows are one read: both from October.
		{"equal windows are one read",
			`{"org_id":"org1","max_cost_usd":5,"max_tokens":1000,"since_cost":` + october + `,"since_tokens":` + october + `}`, true, 3, 350},
		// A per-cap window overrides since for its own cap only: tokens keep since (September).
		{"since is the fallback for the cap without a window",
			`{"org_id":"org1","max_cost_usd":5,"max_tokens":2000,"since":` + september + `,"since_cost":` + october + `}`, true, 3, 1050},
		// Per user: the token window from October holds no spend of u1, the cost lifetime holds 4.
		{"per user, the windows apply to that user",
			`{"org_id":"org1","user_id":"u1","max_cost_usd":5,"max_tokens":500,"since_tokens":` + october + `}`, true, 4, 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := do(mux, http.MethodPost, "/api/v1/budget/check", tc.body)
			if rec.Code != http.StatusOK {
				t.Fatalf("status=%d body=%s", rec.Code, rec.Body.String())
			}
			var resp budgetCheckResponse
			mustJSON(t, rec.Body.Bytes(), &resp)
			if resp.Allowed != tc.wantAllowed || resp.CurrentCostUSD != tc.wantCost || resp.CurrentTokens != tc.wantTokens {
				t.Fatalf("got allowed=%v cost=%v tokens=%d, want allowed=%v cost=%v tokens=%d (%+v)",
					resp.Allowed, resp.CurrentCostUSD, resp.CurrentTokens, tc.wantAllowed, tc.wantCost, tc.wantTokens, resp)
			}
		})
	}
}

// A request without the per-cap fields is answered exactly as before they existed: lifetime totals,
// the same response bytes.
func TestBudgetCheckWithoutPerCapWindowsAnswersAsBefore(t *testing.T) {
	srv, mux := newTestServer()
	seedPerCapLedger(t, srv)

	rec := do(mux, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org1","max_cost_usd":100,"max_tokens":5000}`)
	if rec.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", rec.Code, rec.Body.String())
	}
	const want = `{"allowed":true,"current_cost_usd":7,"current_tokens":1050}`
	if got := strings.TrimSpace(rec.Body.String()); got != want {
		t.Fatalf("body=%s, want %s", got, want)
	}
}

// A failed read of either windowed total fails the check closed, as the single read does: the two reads are
// answered together, so a ledger that cannot answer one of them answers 503 for both.
func TestBudgetCheckFailsClosedWhenAPerCapWindowedReadFails(t *testing.T) {
	srv := NewServer(aggregateFailureLedger{Store: ledger.NewStore()})
	mux := http.NewServeMux()
	srv.RegisterRoutes(mux)
	body := `{"org_id":"org1","max_cost_usd":5,"max_tokens":1000,"since_cost":"2026-10-01T00:00:00Z","since_tokens":"2026-09-01T00:00:00Z"}`
	rec := do(mux, http.MethodPost, "/api/v1/budget/check", body)
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status=%d want=%d body=%s", rec.Code, http.StatusServiceUnavailable, rec.Body.String())
	}
}

func TestBudgetCheckRejectsAMalformedPerCapWindow(t *testing.T) {
	_, mux := newTestServer()
	for _, field := range []string{"since_cost", "since_tokens"} {
		body := `{"org_id":"org1","max_cost_usd":5,"` + field + `":"2026-10-01"}`
		rec := do(mux, http.MethodPost, "/api/v1/budget/check", body)
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d want=400 body=%s", field, rec.Code, rec.Body.String())
		}
	}
}
