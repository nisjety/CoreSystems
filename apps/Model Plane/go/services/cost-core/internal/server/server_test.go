package server

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// newTestServer wires a Server over a fresh in-memory ledger and a mux.
func newTestServer() (*Server, *http.ServeMux) {
	srv := NewServer(ledger.NewStore())
	mux := http.NewServeMux()
	srv.RegisterRoutes(mux)
	return srv, mux
}

func TestRecordThenUsage(t *testing.T) {
	_, mux := newTestServer()

	body := `{"org_id":"org1","user_id":"u1","run_id":"run1","model":"gpt","input_tokens":100,"output_tokens":40,"cost_usd":0.25,"idempotency_key":"k1"}`
	rec := do(mux, http.MethodPost, "/api/v1/cost/record", body)
	if rec.Code != http.StatusAccepted {
		t.Fatalf("record status = %d, body=%s", rec.Code, rec.Body.String())
	}

	// Duplicate idempotency key must not double-count.
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", body)

	rec = do(mux, http.MethodGet, "/api/v1/usage?org_id=org1&user_id=u1", "")
	if rec.Code != http.StatusOK {
		t.Fatalf("usage status = %d", rec.Code)
	}
	var usage usageResponse
	mustJSON(t, rec.Body.Bytes(), &usage)
	if usage.TotalInputTokens != 100 || usage.TotalOutputTokens != 40 {
		t.Fatalf("tokens = %d/%d", usage.TotalInputTokens, usage.TotalOutputTokens)
	}
	if usage.EntryCount != 1 {
		t.Fatalf("entry count = %d, want 1 (idempotent)", usage.EntryCount)
	}
}

func TestRunUsageEndpoint(t *testing.T) {
	_, mux := newTestServer()
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"a","run_id":"runX","cost_usd":1.0,"input_tokens":5}`)
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"b","run_id":"runX","cost_usd":2.0,"output_tokens":7}`)

	rec := do(mux, http.MethodGet, "/api/v1/cost/run?run_id=runX", "")
	if rec.Code != http.StatusOK {
		t.Fatalf("run usage status = %d", rec.Code)
	}
	var usage usageResponse
	mustJSON(t, rec.Body.Bytes(), &usage)
	if usage.RunID != "runX" || usage.TotalCostUSD != 3.0 || usage.EntryCount != 2 {
		t.Fatalf("run rollup wrong: %+v", usage)
	}
}

func TestRunUsageNotFound(t *testing.T) {
	_, mux := newTestServer()
	rec := do(mux, http.MethodGet, "/api/v1/cost/run?run_id=ghost", "")
	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", rec.Code)
	}
}

func TestAggregateEndpoint(t *testing.T) {
	_, mux := newTestServer()
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"a","cost_usd":1.0}`)
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"b","cost_usd":2.0}`)

	rec := do(mux, http.MethodGet, "/api/v1/cost/aggregate?org_id=org1", "")
	if rec.Code != http.StatusOK {
		t.Fatalf("aggregate status = %d", rec.Code)
	}
	var usage usageResponse
	mustJSON(t, rec.Body.Bytes(), &usage)
	if usage.TotalCostUSD != 3.0 || usage.EntryCount != 2 {
		t.Fatalf("aggregate wrong: %+v", usage)
	}
}

func TestBudgetCheckEndpoint(t *testing.T) {
	_, mux := newTestServer()
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"u1","cost_usd":5.0,"input_tokens":100}`)

	// Within budget.
	rec := do(mux, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org1","user_id":"u1","max_cost_usd":10}`)
	var resp budgetCheckResponse
	mustJSON(t, rec.Body.Bytes(), &resp)
	if !resp.Allowed || resp.CurrentCostUSD != 5.0 {
		t.Fatalf("expected allowed within budget: %+v", resp)
	}

	// Over budget.
	rec = do(mux, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org1","user_id":"u1","max_cost_usd":5}`)
	mustJSON(t, rec.Body.Bytes(), &resp)
	if resp.Allowed {
		t.Fatalf("expected over-budget rejection: %+v", resp)
	}
	if resp.Reason == "" {
		t.Fatalf("expected a rejection reason")
	}
}

func TestRecordRequiresOrgID(t *testing.T) {
	_, mux := newTestServer()
	rec := do(mux, http.MethodPost, "/api/v1/cost/record", `{"user_id":"u1"}`)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", rec.Code)
	}
}

func TestRecordUsageDirect(t *testing.T) {
	srv, _ := newTestServer()
	if err := srv.RecordUsage(context.Background(), ledger.Entry{OrgID: "o", UserID: "u", InputTokens: 3}); err != nil {
		t.Fatalf("RecordUsage: %v", err)
	}
}

func do(mux *http.ServeMux, method, target, body string) *httptest.ResponseRecorder {
	var r *http.Request
	if body == "" {
		r = httptest.NewRequest(method, target, nil)
	} else {
		r = httptest.NewRequest(method, target, strings.NewReader(body))
	}
	rec := httptest.NewRecorder()
	mux.ServeHTTP(rec, r)
	return rec
}

func mustJSON(t *testing.T, data []byte, v any) {
	t.Helper()
	if err := json.Unmarshal(data, v); err != nil {
		t.Fatalf("json unmarshal: %v (body=%s)", err, string(data))
	}
}
