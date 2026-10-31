package server

import (
	"context"
	"net/http"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// A capped org is admitted again once its period resets: spend before the
// window's start stops counting against the cap. An absent since keeps the
// lifetime totals.
func TestBudgetCheckSinceExcludesSpendBeforeTheWindow(t *testing.T) {
	srv, mux := newTestServer()
	ctx := context.Background()
	for _, e := range []ledger.Entry{
		{OrgID: "org1", UserID: "u1", Model: "gpt-4o", CostUSD: 4, CostBasis: ledger.CostBasisMetered,
			CreatedAt: time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)},
		{OrgID: "org1", UserID: "u2", Model: "gpt-4o", CostUSD: 3, CostBasis: ledger.CostBasisMetered,
			CreatedAt: time.Date(2026, 10, 3, 9, 0, 0, 0, time.UTC)},
	} {
		if err := srv.ledger.RecordEntry(ctx, e); err != nil {
			t.Fatalf("RecordEntry: %v", err)
		}
	}

	const october = `"since":"2026-10-01T00:00:00Z"`
	cases := []struct {
		name        string
		body        string
		wantAllowed bool
		wantCost    float64
	}{
		{"org lifetime", `{"org_id":"org1","max_cost_usd":5}`, false, 7},
		{"org window", `{"org_id":"org1","max_cost_usd":5,` + october + `}`, true, 3},
		{"user lifetime", `{"org_id":"org1","user_id":"u1","max_cost_usd":4}`, false, 4},
		{"user window", `{"org_id":"org1","user_id":"u1","max_cost_usd":4,` + october + `}`, true, 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := do(mux, http.MethodPost, "/api/v1/budget/check", tc.body)
			if rec.Code != http.StatusOK {
				t.Fatalf("status=%d body=%s", rec.Code, rec.Body.String())
			}
			var resp budgetCheckResponse
			mustJSON(t, rec.Body.Bytes(), &resp)
			if resp.Allowed != tc.wantAllowed || resp.CurrentCostUSD != tc.wantCost {
				t.Fatalf("got allowed=%v cost=%v, want allowed=%v cost=%v (%+v)",
					resp.Allowed, resp.CurrentCostUSD, tc.wantAllowed, tc.wantCost, resp)
			}
		})
	}
}

func TestBudgetCheckRejectsAMalformedSince(t *testing.T) {
	_, mux := newTestServer()
	rec := do(mux, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org1","max_cost_usd":5,"since":"2026-10-01"}`)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("status=%d want=400 body=%s", rec.Code, rec.Body.String())
	}
}
