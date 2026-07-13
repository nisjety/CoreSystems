package server

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

type aggregateFailureLedger struct {
	*ledger.Store
}

func (l aggregateFailureLedger) Aggregate(context.Context, ledger.AggregateFilter) (*ledger.Usage, error) {
	return nil, errors.New("database unavailable")
}

type recordFailureLedger struct {
	*ledger.Store
}

func (l recordFailureLedger) RecordEntry(context.Context, ledger.Entry) error {
	return errors.New("write failed")
}

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

func TestRecordRejectsInvalidAccountingValues(t *testing.T) {
	_, mux := newTestServer()
	for _, body := range []string{
		`{"org_id":"org1","user_id":"u1","input_tokens":-1}`,
		`{"org_id":"org1","user_id":"u1","output_tokens":-1}`,
		`{"org_id":"org1","user_id":"u1","input_tokens":1000000000001}`,
		`{"org_id":"org1","user_id":"u1","cost_usd":-0.01}`,
		`{"org_id":"org1","user_id":"u1","cost_usd":1e1000}`,
		`{"org_id":"org1"}`,
	} {
		response := do(mux, http.MethodPost, "/api/v1/cost/record", body)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("body=%s status=%d want=400 response=%s", body, response.Code, response.Body.String())
		}
	}
}

func TestRecordUsageDirectRejectsNonFiniteCost(t *testing.T) {
	srv, _ := newTestServer()
	for _, cost := range []float64{math.NaN(), math.Inf(1), math.Inf(-1)} {
		if err := srv.RecordUsage(context.Background(), ledger.Entry{OrgID: "o", UserID: "u", CostUSD: cost}); err == nil {
			t.Fatalf("cost=%v should be rejected", cost)
		}
	}
}

func TestBudgetRejectsMalformedLimits(t *testing.T) {
	_, mux := newTestServer()
	for _, body := range []string{
		`{"org_id":"org1","max_cost_usd":-1}`,
		`{"org_id":"org1","max_tokens":-1}`,
		`{"org_id":"org1","max_cost_usd":1e1000}`,
		`{"org_id":"org1","max_tokens":1000000000001}`,
	} {
		response := do(mux, http.MethodPost, "/api/v1/budget/check", body)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("body=%s status=%d want=400 response=%s", body, response.Code, response.Body.String())
		}
	}
}

func TestRecordUsageDirect(t *testing.T) {
	srv, _ := newTestServer()
	if err := srv.RecordUsage(context.Background(), ledger.Entry{OrgID: "o", UserID: "u", InputTokens: 3}); err != nil {
		t.Fatalf("RecordUsage: %v", err)
	}
}

func TestBudgetCheckFailsClosedWhenLedgerIsUnavailable(t *testing.T) {
	srv := NewServer(aggregateFailureLedger{Store: ledger.NewStore()})
	mux := http.NewServeMux()
	srv.RegisterRoutes(mux)
	response := do(mux, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org1","max_cost_usd":10}`)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status=%d want=%d body=%s", response.Code, http.StatusServiceUnavailable, response.Body.String())
	}
}

func TestNilAuthenticationMiddlewareFailsClosedButHealthRemainsPublic(t *testing.T) {
	srv := NewServer(ledger.NewStore())
	handler := srv.Handler(nil)
	if response := costRequest(handler, http.MethodGet, "/api/v1/cost/entries?org_id=org1", "", "", "", ""); response.Code != http.StatusServiceUnavailable {
		t.Fatalf("api status=%d want=%d", response.Code, http.StatusServiceUnavailable)
	}
	if response := costRequest(handler, http.MethodGet, "/api/v1/pricing", "", "", "", ""); response.Code != http.StatusOK {
		t.Fatalf("global pricing status=%d want=%d", response.Code, http.StatusOK)
	}
	if response := costRequest(handler, http.MethodGet, "/readyz", "", "", "", ""); response.Code != http.StatusOK {
		t.Fatalf("ready status=%d", response.Code)
	}
}

func TestPricingEntriesAggregateRunAndBudgetBranches(t *testing.T) {
	_, mux := newTestServer()
	if response := do(mux, http.MethodGet, "/api/v1/pricing", ""); response.Code != http.StatusOK {
		t.Fatalf("pricing status=%d", response.Code)
	}
	_ = do(mux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"u1","run_id":"run1","model":"m1","input_tokens":7,"output_tokens":5,"cost_usd":1.25}`)

	if response := do(mux, http.MethodGet, "/api/v1/cost/run?run_id=run1", ""); response.Code != http.StatusOK {
		t.Fatalf("run status=%d body=%s", response.Code, response.Body.String())
	}
	if response := do(mux, http.MethodGet, "/api/v1/cost/entries?org_id=org1&model=m1&limit=1", ""); response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"count":1`) {
		t.Fatalf("entries status=%d body=%s", response.Code, response.Body.String())
	}
	if response := do(mux, http.MethodGet, "/api/v1/cost/aggregate?org_id=org1&run_id=run1", ""); response.Code != http.StatusOK {
		t.Fatalf("aggregate status=%d body=%s", response.Code, response.Body.String())
	}
	response := do(mux, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org1","user_id":"u1","max_tokens":12}`)
	var budget budgetCheckResponse
	mustJSON(t, response.Body.Bytes(), &budget)
	if budget.Allowed || budget.Reason == "" {
		t.Fatalf("token budget should reject: %+v", budget)
	}
}

func TestValidationAndInternalFailureBranches(t *testing.T) {
	_, mux := newTestServer()
	for _, test := range []struct {
		method string
		path   string
		body   string
	}{
		{http.MethodPost, "/api/v1/cost/record", "{"},
		{http.MethodGet, "/api/v1/usage?org_id=org1", ""},
		{http.MethodGet, "/api/v1/cost/run", ""},
		{http.MethodGet, "/api/v1/cost/aggregate?since=not-time", ""},
		{http.MethodGet, "/api/v1/cost/entries?until=not-time", ""},
		{http.MethodPost, "/api/v1/budget/check", "{"},
	} {
		if response := do(mux, test.method, test.path, test.body); response.Code != http.StatusBadRequest {
			t.Fatalf("%s %s status=%d body=%s", test.method, test.path, response.Code, response.Body.String())
		}
	}

	failing := NewServer(recordFailureLedger{Store: ledger.NewStore()})
	failingMux := http.NewServeMux()
	failing.RegisterRoutes(failingMux)
	response := do(failingMux, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org1","user_id":"u1"}`)
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("record failure status=%d", response.Code)
	}
	if err := failing.RecordUsage(context.Background(), ledger.Entry{OrgID: "org1"}); err == nil {
		t.Fatal("direct record should report storage failure")
	}
}

func TestErrorMappingFilteringAndLimits(t *testing.T) {
	if mapHTTPStatus(nil) != http.StatusOK ||
		mapHTTPStatus(ledger.ErrUsageNotFound) != http.StatusNotFound ||
		mapHTTPStatus(ledger.ErrBudgetExceededCost) != http.StatusForbidden ||
		mapHTTPStatus(errors.New("other")) != http.StatusInternalServerError {
		t.Fatal("unexpected HTTP error mapping")
	}
	if budgetOutcome(nil) != "ok" ||
		budgetOutcome(ledger.ErrBudgetExceededCost) != "cost_exceeded" ||
		budgetOutcome(ledger.ErrBudgetExceededTokens) != "tokens_exceeded" ||
		budgetOutcome(errors.New("other")) != "internal_error" {
		t.Fatal("unexpected budget outcome mapping")
	}

	request := httptest.NewRequest(http.MethodGet, "/?org_id=o&user_id=u&run_id=r&model=m&since=2026-01-01T00:00:00Z&until=2026-01-02T00:00:00Z", nil)
	filter, err := filterFromQuery(request)
	if err != nil || filter.OrgID != "o" || filter.UserID != "u" || filter.RunID != "r" || filter.Model != "m" || filter.Since.IsZero() || filter.Until.IsZero() {
		t.Fatalf("unexpected filter: %+v err=%v", filter, err)
	}
	for input, expected := range map[string]int{"": 0, "bad": 0, "-1": 0, "10": 10, "5000": maxListLimit} {
		if actual := parseLimit(input); actual != expected {
			t.Fatalf("parseLimit(%q)=%d want=%d", input, actual, expected)
		}
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
