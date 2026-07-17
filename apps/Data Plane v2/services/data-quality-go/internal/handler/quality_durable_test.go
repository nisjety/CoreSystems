package handler

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/eval"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

type fakeEvalRunner struct {
	run         *model.EvalRun
	created     bool
	getErr      error
	gotOrg      string
	gotEvalID   string
	gotInput    model.CreateEvalInput
	runRequests chan string
}

func (f *fakeEvalRunner) CreateEval(_ context.Context, input model.CreateEvalInput) (*model.EvalRun, bool, error) {
	f.gotInput = input
	return f.run, f.created, nil
}

func (f *fakeEvalRunner) RunEval(_ context.Context, orgID, evalID string) error {
	if f.runRequests != nil {
		f.runRequests <- orgID + "/" + evalID
	}
	return nil
}

func (f *fakeEvalRunner) GetEval(_ context.Context, orgID, evalID string) (*model.EvalRun, error) {
	f.gotOrg, f.gotEvalID = orgID, evalID
	return f.run, f.getErr
}

func (*fakeEvalRunner) RunCompare(context.Context, model.CompareEvalInput) (*model.CompareResult, error) {
	return &model.CompareResult{}, nil
}

func qualityTestRouter(runner EvalRunner) http.Handler {
	router := chi.NewRouter()
	h := NewQualityHandler(runner, nil, nil, nil, nil, nil)
	MountProtectedRoutes(router, authctx.Middleware(routeTestVerifier{}), h)
	return router
}

func TestGetEvalPinsClaimTenantAndReturnsPersistedResult(t *testing.T) {
	runner := &fakeEvalRunner{run: &model.EvalRun{EvalID: "eval-1", OrgID: "org_authorized", Status: model.EvalCompleted}}
	request := httptest.NewRequest(http.MethodGet, "/v1/evals/retrieval/eval-1", nil)
	request.Header.Set("Authorization", "Bearer valid-token")
	response := httptest.NewRecorder()

	qualityTestRouter(runner).ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if runner.gotOrg != "org_authorized" || runner.gotEvalID != "eval-1" {
		t.Fatalf("lookup scope = %q/%q", runner.gotOrg, runner.gotEvalID)
	}
	var got model.EvalRun
	if err := json.NewDecoder(response.Body).Decode(&got); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if got.Status != model.EvalCompleted {
		t.Fatalf("status = %q", got.Status)
	}
}

func TestGetEvalReturns404WithoutCrossTenantExistenceLeak(t *testing.T) {
	runner := &fakeEvalRunner{getErr: eval.ErrNotFound}
	request := httptest.NewRequest(http.MethodGet, "/v1/evals/retrieval/foreign-eval", nil)
	request.Header.Set("Authorization", "Bearer valid-token")
	response := httptest.NewRecorder()

	qualityTestRouter(runner).ServeHTTP(response, request)

	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404; body=%s", response.Code, response.Body.String())
	}
}

func TestRunEvalRequiresIdempotencyAndLaunchesOnlyNewRun(t *testing.T) {
	t.Run("missing key", func(t *testing.T) {
		runner := &fakeEvalRunner{}
		request := httptest.NewRequest(http.MethodPost, "/v1/evals/retrieval", strings.NewReader(`{"strategy":"hybrid"}`))
		request.Header.Set("Authorization", "Bearer valid-token")
		response := httptest.NewRecorder()
		qualityTestRouter(runner).ServeHTTP(response, request)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("status = %d, want 400", response.Code)
		}
	})

	t.Run("new key", func(t *testing.T) {
		runs := make(chan string, 1)
		runner := &fakeEvalRunner{
			run:     &model.EvalRun{EvalID: "eval-new", OrgID: "org_authorized", Status: model.EvalPending},
			created: true, runRequests: runs,
		}
		request := httptest.NewRequest(http.MethodPost, "/v1/evals/retrieval", strings.NewReader(`{"strategy":"hybrid"}`))
		request.Header.Set("Authorization", "Bearer valid-token")
		request.Header.Set("Idempotency-Key", "quality-eval-123")
		response := httptest.NewRecorder()
		qualityTestRouter(runner).ServeHTTP(response, request)
		if response.Code != http.StatusAccepted {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		if runner.gotInput.OrgID != "org_authorized" || runner.gotInput.IdempotencyKey != "quality-eval-123" {
			t.Fatalf("create input = %+v", runner.gotInput)
		}
		if got := <-runs; got != "org_authorized/eval-new" {
			t.Fatalf("run request = %q", got)
		}
	})

	t.Run("duplicate key", func(t *testing.T) {
		runs := make(chan string, 1)
		runner := &fakeEvalRunner{
			run:     &model.EvalRun{EvalID: "eval-existing", OrgID: "org_authorized", Status: model.EvalRunning},
			created: false, runRequests: runs,
		}
		request := httptest.NewRequest(http.MethodPost, "/v1/evals/retrieval", strings.NewReader(`{"strategy":"hybrid"}`))
		request.Header.Set("Authorization", "Bearer valid-token")
		request.Header.Set("Idempotency-Key", "quality-eval-123")
		response := httptest.NewRecorder()
		qualityTestRouter(runner).ServeHTTP(response, request)
		if response.Code != http.StatusAccepted {
			t.Fatalf("status = %d", response.Code)
		}
		select {
		case got := <-runs:
			t.Fatalf("duplicate launched run %q", got)
		default:
		}
	})
}

func TestGetEvalMapsUnexpectedStoreErrorTo500(t *testing.T) {
	runner := &fakeEvalRunner{getErr: errors.New("database unavailable")}
	request := httptest.NewRequest(http.MethodGet, "/v1/evals/retrieval/eval-1", nil)
	request.Header.Set("Authorization", "Bearer valid-token")
	response := httptest.NewRecorder()
	qualityTestRouter(runner).ServeHTTP(response, request)
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status = %d, want 500", response.Code)
	}
}
