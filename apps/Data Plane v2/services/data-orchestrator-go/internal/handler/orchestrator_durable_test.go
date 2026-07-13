package handler

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/jobs"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

const durableJobID = "91b88438-4190-45f3-8674-58da542d8faf"

type fakeJobExecutor struct {
	mu            sync.Mutex
	job           *model.Job
	created       bool
	createErr     error
	getErr        error
	createdInput  model.CreateJobInput
	idempotency   string
	getOrgID      string
	getJobID      string
	runInvoked    chan struct{}
	runInvocation int
}

func (f *fakeJobExecutor) CreateJob(_ context.Context, input model.CreateJobInput, key string) (*model.Job, bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.createdInput = input
	f.idempotency = key
	return f.job, f.created, f.createErr
}

func (f *fakeJobExecutor) GetJob(_ context.Context, orgID, jobID string) (*model.Job, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.getOrgID, f.getJobID = orgID, jobID
	return f.job, f.getErr
}

func (f *fakeJobExecutor) Run(_ context.Context, _ model.Job) error {
	f.mu.Lock()
	f.runInvocation++
	ch := f.runInvoked
	f.mu.Unlock()
	if ch != nil {
		select {
		case ch <- struct{}{}:
		default:
		}
	}
	return nil
}

func durableTestRouter(executor *fakeJobExecutor) http.Handler {
	router := chi.NewRouter()
	h := NewOrchestratorHandler(executor, nil)
	MountProtectedRoutes(router, authctx.Middleware(routeTestVerifier{}), h)
	return router
}

func TestCreateJobRequiresIdempotencyAndPinsVerifiedTenant(t *testing.T) {
	job := &model.Job{JobID: durableJobID, OrgID: "org_authorized", Status: model.StatusPending}
	fake := &fakeJobExecutor{job: job, created: true, runInvoked: make(chan struct{}, 1)}
	router := durableTestRouter(fake)

	withoutKey := httptest.NewRequest(http.MethodPost, "/v1/orchestrator/jobs", bytes.NewBufferString(`{"org_id":"org_victim","job_type":"reindex"}`))
	withoutKey.Header.Set("Authorization", "Bearer valid-token")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, withoutKey)
	if response.Code != http.StatusBadRequest {
		t.Fatalf("missing idempotency status = %d, want 400; body=%s", response.Code, response.Body.String())
	}

	request := httptest.NewRequest(http.MethodPost, "/v1/orchestrator/jobs", bytes.NewBufferString(`{"org_id":"org_victim","job_type":"reindex","document_ids":["doc-1"]}`))
	request.Header.Set("Authorization", "Bearer valid-token")
	request.Header.Set("Idempotency-Key", "job-request-123")
	response = httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusAccepted {
		t.Fatalf("status = %d, want 202; body=%s", response.Code, response.Body.String())
	}
	select {
	case <-fake.runInvoked:
	case <-time.After(time.Second):
		t.Fatal("new durable job was not launched")
	}
	fake.mu.Lock()
	defer fake.mu.Unlock()
	if fake.createdInput.OrgID != "org_authorized" || fake.idempotency != "job-request-123" {
		t.Fatalf("create input = %+v key=%q", fake.createdInput, fake.idempotency)
	}
}

func TestIdempotentReplayReturnsSnapshotWithoutRelaunch(t *testing.T) {
	job := &model.Job{JobID: durableJobID, OrgID: "org_authorized", Status: model.StatusRunning}
	fake := &fakeJobExecutor{job: job, created: false, runInvoked: make(chan struct{}, 1)}
	request := httptest.NewRequest(http.MethodPost, "/v1/orchestrator/reindex", bytes.NewBufferString(`{"document_ids":["doc-1"]}`))
	request.Header.Set("Authorization", "Bearer valid-token")
	request.Header.Set("Idempotency-Key", "same-reindex-request")
	response := httptest.NewRecorder()

	durableTestRouter(fake).ServeHTTP(response, request)

	if response.Code != http.StatusAccepted {
		t.Fatalf("status = %d, want 202; body=%s", response.Code, response.Body.String())
	}
	select {
	case <-fake.runInvoked:
		t.Fatal("idempotent replay relaunched the job")
	case <-time.After(50 * time.Millisecond):
	}
}

func TestUnavailableSignedWorkerFailsBeforeAcceptingJob(t *testing.T) {
	fake := &fakeJobExecutor{createErr: jobs.ErrExecutionUnavailable}
	request := httptest.NewRequest(http.MethodPost, "/v1/orchestrator/reindex", bytes.NewBufferString(`{"document_ids":["doc-1"]}`))
	request.Header.Set("Authorization", "Bearer valid-token")
	request.Header.Set("Idempotency-Key", "contained-reindex-request")
	response := httptest.NewRecorder()

	durableTestRouter(fake).ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503; body=%s", response.Code, response.Body.String())
	}
	if fake.runInvocation != 0 {
		t.Fatal("unavailable mutation launched background work")
	}
}

func TestGetJobUsesVerifiedTenantAndHidesOtherTenants(t *testing.T) {
	result, _ := json.Marshal(map[string]int{"published": 2})
	job := &model.Job{JobID: durableJobID, OrgID: "org_authorized", Status: model.StatusCompleted, Result: result}
	fake := &fakeJobExecutor{job: job}
	request := httptest.NewRequest(http.MethodGet, "/v1/orchestrator/jobs/"+durableJobID, nil)
	request.Header.Set("Authorization", "Bearer valid-token")
	response := httptest.NewRecorder()

	durableTestRouter(fake).ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200; body=%s", response.Code, response.Body.String())
	}
	fake.mu.Lock()
	if fake.getOrgID != "org_authorized" || fake.getJobID != durableJobID {
		t.Fatalf("get scope = %q/%q", fake.getOrgID, fake.getJobID)
	}
	fake.mu.Unlock()

	fake.getErr = jobs.ErrJobNotFound
	response = httptest.NewRecorder()
	durableTestRouter(fake).ServeHTTP(response, request)
	if response.Code != http.StatusNotFound {
		t.Fatalf("not found status = %d, want 404; body=%s", response.Code, response.Body.String())
	}

	fake.getErr = errors.New("database unavailable")
	response = httptest.NewRecorder()
	durableTestRouter(fake).ServeHTTP(response, request)
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("storage error status = %d, want 500; body=%s", response.Code, response.Body.String())
	}
}
