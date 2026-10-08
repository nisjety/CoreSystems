package handler

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/eval"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// These tests run the REAL authentication middleware, the REAL scope check, the REAL route table
// (MountProtectedRoutes), the REAL operator rule and the REAL handler and validation (eval.ValidateRecord,
// over the goldens of harness-record-v1, which the producer pins too). The only stand-in is the store behind
// the handler: what it does with a row (row-level security, the immutable history, the table's CHECKs) is
// proved against Postgres in internal/eval (store_record_integration_test.go).

const resultsPath = "/v1/evals/results"

var recordGoldens = filepath.Join("..", "eval", "testdata", "harness-record-v1")

func goldenRecord(t *testing.T, name string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(recordGoldens, name))
	if err != nil {
		t.Fatalf("read golden %s: %v", name, err)
	}
	return string(raw)
}

// fakeRecorder keeps recorded runs per (organization, key) with the semantics the handler relies on:
// the same key with the same content is a replay, with other content a conflict.
type fakeRecorder struct {
	mu    sync.Mutex
	runs  map[string]model.EvalRun
	calls []model.EvalRun
	err   error
}

func newFakeRecorder() *fakeRecorder { return &fakeRecorder{runs: map[string]model.EvalRun{}} }

func (f *fakeRecorder) Record(_ context.Context, run model.EvalRun) (*model.EvalRun, bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, run)
	if f.err != nil {
		return nil, false, f.err
	}
	key := run.OrgID + "|" + run.IdempotencyKey
	if existing, ok := f.runs[key]; ok {
		if existing.Strategy != run.Strategy || existing.Corpus != run.Corpus || !bytes.Equal(existing.Scorecard, run.Scorecard) {
			return nil, false, eval.ErrIdempotencyConflict
		}
		return &existing, false, nil
	}
	f.runs[key] = run
	return &run, true, nil
}

func (f *fakeRecorder) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

func resultsRouter(recorder eval.ResultRecorder) http.Handler {
	router := chi.NewRouter()
	h := &QualityHandler{}
	if recorder != nil {
		h.WithResultRecorder(recorder)
	}
	MountProtectedRoutes(router, authctx.Middleware(testTokens), h)
	return router
}

func postResult(t *testing.T, router http.Handler, token, key, body string, headers ...string) reply {
	t.Helper()
	extra := append([]string{}, headers...)
	if key != "" {
		extra = append(extra, "Idempotency-Key", key)
	}
	return call(t, router, http.MethodPost, resultsPath, token, body, extra...)
}

func TestTheResultsRouteRequiresAuthenticationTheAdminScopeAndAnOperator(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	body := goldenRecord(t, "record.cell.golden.json")

	if got := postResult(t, router, "", "harness-run-1", body); got.Code != http.StatusUnauthorized {
		t.Fatalf("no token = %d, want 401", got.Code)
	}
	if got := postResult(t, router, "member-user-a", "harness-run-1", body); got.Code != http.StatusForbidden {
		t.Fatalf("a member without the admin scope = %d, want 403", got.Code)
	}
	if got := postResult(t, router, "operator-noscop", "harness-run-1", body); got.Code != http.StatusForbidden {
		t.Fatalf("an operator without the admin scope = %d, want 403", got.Code)
	}
	// An organization's own admin holds the scope too, and must not be able to write history that a
	// promotion decision reads: only an operator service principal may.
	got := postResult(t, router, "admin-user-a", "harness-run-1", body)
	if got.Code != http.StatusForbidden || errorCode(got) != "operator_only" {
		t.Fatalf("an organization admin = %d %s, want 403 operator_only", got.Code, got.Raw)
	}
	if got := postResult(t, router, "operator-a", "harness-run-1", body, "X-Org-ID", "org-b"); got.Code != http.StatusForbidden {
		t.Fatalf("a forged tenant header = %d, want 403", got.Code)
	}
	if n := recorder.callCount(); n != 0 {
		t.Fatalf("a refused request reached the store %d time(s)", n)
	}
}

// The route's own boundary, proved with a stub where the handler would be: the operator rule is the ROUTE's, not only the
// handler's (which checks again, as defence in depth), so a handler that forgot to check would still not be reached.
func TestTheRouteItselfRefusesAUserPrincipalBeforeTheHandlerIsReached(t *testing.T) {
	reached := 0
	stub := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		reached++
		w.WriteHeader(http.StatusNoContent)
	})
	router := chi.NewRouter()
	mountProtectedRoutes(router, authctx.Middleware(testTokens), qualityRouteHandlers{recordEvalResult: stub})

	if got := postResult(t, router, "admin-user-a", "harness-run-1", "{}"); got.Code != http.StatusForbidden || errorCode(got) != "operator_only" {
		t.Fatalf("a user principal with the admin scope = %d %s, want 403 operator_only from the route", got.Code, got.Raw)
	}
	if reached != 0 {
		t.Fatal("the handler was reached by a user principal")
	}
	if got := postResult(t, router, "operator-a", "harness-run-1", "{}"); got.Code != http.StatusNoContent || reached != 1 {
		t.Fatalf("an operator = %d (handler reached %d times), want 204 and 1", got.Code, reached)
	}
}

func TestAnOperatorRecordsARunForTheOrganizationOfItsTokenOnly(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	body := goldenRecord(t, "record.cell.golden.json")

	got := postResult(t, router, "operator-a", "harness-20261007-contextual", body)
	if got.Code != http.StatusCreated {
		t.Fatalf("POST = %d %s, want 201", got.Code, got.Raw)
	}
	if recorder.callCount() != 1 {
		t.Fatalf("the store was called %d times", recorder.callCount())
	}
	run := recorder.calls[0]
	if run.OrgID != "org-a" {
		t.Fatalf("recorded for %q, want the token's org-a", run.OrgID)
	}
	if run.Status != model.EvalCompleted || run.Strategy != "harness:contextual" || run.IdempotencyKey != "harness-20261007-contextual" {
		t.Fatalf("recorded run = %+v", run)
	}
	if run.EvalID == "" || run.CreatedAt.IsZero() {
		t.Fatalf("the handler must stamp the run's id and time: %+v", run)
	}
	var stored map[string]any
	if err := json.Unmarshal(run.Scorecard, &stored); err != nil || stored["queries_run"] != float64(10) {
		t.Fatalf("stored scorecard = %v (%v)", stored, err)
	}
	if got.Body["status"] != "completed" || got.Body["strategy"] != "harness:contextual" || got.Body["eval_id"] == nil {
		t.Fatalf("response = %s", got.Raw)
	}
}

func TestAReplayIsOKAndAChangedReplayIsAConflict(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	body := goldenRecord(t, "record.cell.golden.json")
	if got := postResult(t, router, "operator-a", "harness-run-2", body); got.Code != http.StatusCreated {
		t.Fatalf("first = %d %s", got.Code, got.Raw)
	}
	if got := postResult(t, router, "operator-a", "harness-run-2", body); got.Code != http.StatusOK {
		t.Fatalf("a replay = %d %s, want 200", got.Code, got.Raw)
	}
	changed := strings.Replace(body, `"mean_mrr": 0.733333,
    "mean_ndcg_at_10"`, `"mean_mrr": 0.9,
    "mean_ndcg_at_10"`, 1)
	if changed == body {
		t.Fatal("the replacement did not apply: the golden's layout changed, fix this test")
	}
	got := postResult(t, router, "operator-a", "harness-run-2", changed)
	if got.Code != http.StatusConflict || errorCode(got) != "idempotency_conflict" {
		t.Fatalf("a changed replay = %d %s, want 409 idempotency_conflict", got.Code, got.Raw)
	}
}

func TestTheIdempotencyKeyIsRequired(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	body := goldenRecord(t, "record.cell.golden.json")
	// 121 is past the handler's own bound (requestIdempotencyKey: 8 to 120), which is tighter than the table's CHECK of 128:
	// the producer (scripts/eval-gate.py, IDEMPOTENCY_KEY_MAX) must stop at the handler's bound, not the table's.
	for _, key := range []string{"", "short", strings.Repeat("k", 121), strings.Repeat("k", 129), " padded-key-1 "} {
		if got := postResult(t, router, "operator-a", key, body); got.Code != http.StatusBadRequest {
			t.Fatalf("key %q = %d %s, want 400", key, got.Code, got.Raw)
		}
	}
	if recorder.callCount() != 0 {
		t.Fatal("a request without a usable key reached the store")
	}
}

func TestEveryInvalidGoldenIsRefusedAt400WithoutTouchingTheStore(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	entries, err := os.ReadDir(recordGoldens)
	if err != nil {
		t.Fatal(err)
	}
	seen := 0
	for _, entry := range entries {
		if !strings.HasPrefix(entry.Name(), "invalid.") {
			continue
		}
		seen++
		got := postResult(t, router, "operator-a", "harness-run-"+entry.Name(), goldenRecord(t, entry.Name()))
		if got.Code != http.StatusBadRequest || errorCode(got) != "invalid_record" {
			t.Fatalf("%s = %d %s, want 400 invalid_record", entry.Name(), got.Code, got.Raw)
		}
		if problems, _ := got.Body["problems"].([]any); len(problems) == 0 {
			t.Fatalf("%s: the refusal does not say why: %s", entry.Name(), got.Raw)
		}
	}
	if seen == 0 {
		t.Fatal("no invalid goldens were found")
	}
	if recorder.callCount() != 0 {
		t.Fatal("an invalid record reached the store")
	}
}

func TestTheOrganizationCannotBeChosenByTheBody(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	var body map[string]any
	if err := json.Unmarshal([]byte(goldenRecord(t, "record.cell.golden.json")), &body); err != nil {
		t.Fatal(err)
	}
	body["org_id"] = "org-b"
	raw, _ := json.Marshal(body)
	got := postResult(t, router, "operator-a", "harness-run-3", string(raw))
	if got.Code != http.StatusBadRequest {
		t.Fatalf("a body naming another organization = %d %s, want 400", got.Code, got.Raw)
	}
	if recorder.callCount() != 0 {
		t.Fatal("it reached the store")
	}
}

func TestAnOversizedRecordIsRefused(t *testing.T) {
	recorder := newFakeRecorder()
	router := resultsRouter(recorder)
	huge := `{"strategy":"harness:x","corpus":"c","scorecard":{"pad":"` + strings.Repeat("x", eval.MaxRecordBytes+10) + `"}}`
	got := postResult(t, router, "operator-a", "harness-run-4", huge)
	if got.Code != http.StatusRequestEntityTooLarge || errorCode(got) != "record_too_large" {
		t.Fatalf("an oversized record = %d %s, want 413 record_too_large", got.Code, got.Raw)
	}
	if recorder.callCount() != 0 {
		t.Fatal("it reached the store")
	}
}

func TestWithoutAStoreTheRouteSaysSoAndNeverPretendsToHaveRecorded(t *testing.T) {
	router := resultsRouter(nil)
	got := postResult(t, router, "operator-a", "harness-run-5", goldenRecord(t, "record.cell.golden.json"))
	if got.Code != http.StatusServiceUnavailable || errorCode(got) != "results_store_unavailable" {
		t.Fatalf("no store = %d %s, want 503 results_store_unavailable", got.Code, got.Raw)
	}
}

func TestAStoreFailureIsA500ThatDoesNotLeakTheError(t *testing.T) {
	recorder := newFakeRecorder()
	recorder.err = errors.New(`password authentication failed for user "dataplane" at 10.0.0.5`)
	router := resultsRouter(recorder)
	got := postResult(t, router, "operator-a", "harness-run-6", goldenRecord(t, "record.cell.golden.json"))
	if got.Code != http.StatusInternalServerError || errorCode(got) != "record_failed" {
		t.Fatalf("a store failure = %d %s, want 500 record_failed", got.Code, got.Raw)
	}
	if strings.Contains(got.Raw, "password") || strings.Contains(got.Raw, "10.0.0.5") {
		t.Fatalf("the response leaks the store's error: %s", got.Raw)
	}
}

func TestTheResultsRouteAcceptsOnlyPost(t *testing.T) {
	router := resultsRouter(newFakeRecorder())
	for _, method := range []string{http.MethodGet, http.MethodPut, http.MethodDelete} {
		req := httptest.NewRequest(method, resultsPath, nil)
		req.Header.Set("Authorization", "Bearer operator-a")
		rec := httptest.NewRecorder()
		router.ServeHTTP(rec, req)
		if rec.Code != http.StatusMethodNotAllowed && rec.Code != http.StatusNotFound {
			t.Fatalf("%s %s = %d, want 405 or 404", method, resultsPath, rec.Code)
		}
	}
}
