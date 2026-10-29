package handler

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/google/uuid"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/jobs"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

func orgIDFrom(ctx context.Context) string {
	claims, ok := authctx.FromContext(ctx)
	if !ok {
		return ""
	}
	return claims.OrgID
}

type OrchestratorHandler struct {
	executor      JobExecutor
	staleDetector *jobs.StaleDetector
}

type JobExecutor interface {
	CreateJob(context.Context, model.CreateJobInput, string) (*model.Job, bool, error)
	GetJob(context.Context, string, string) (*model.Job, error)
	Run(context.Context, model.Job) error
}

func NewOrchestratorHandler(e JobExecutor, sd *jobs.StaleDetector) *OrchestratorHandler {
	return &OrchestratorHandler{executor: e, staleDetector: sd}
}

type orchestratorRouteHandlers struct {
	createJob       http.HandlerFunc
	getJob          http.HandlerFunc
	reindex         http.HandlerFunc
	staleEmbeddings http.HandlerFunc
}

// MountProtectedRoutes keeps every orchestrator operation behind the same
// verified-identity boundary.
func MountProtectedRoutes(r chi.Router, auth func(http.Handler) http.Handler, h *OrchestratorHandler) {
	mountProtectedRoutes(r, auth, orchestratorRouteHandlers{
		createJob:       h.CreateJob,
		getJob:          h.GetJob,
		reindex:         h.Reindex,
		staleEmbeddings: h.StaleEmbeddings,
	})
}

func mountProtectedRoutes(r chi.Router, auth func(http.Handler) http.Handler, h orchestratorRouteHandlers) {
	r.Route("/v1/orchestrator", func(r chi.Router) {
		r.Use(auth)
		mutations := r.With(authctx.RequireAnyScope("data:orchestrate", "data:admin"))
		mutations.Post("/jobs", h.createJob)
		mutations.Get("/jobs/{jobID}", h.getJob)
		mutations.Post("/reindex", h.reindex)
		r.Get("/stale-embeddings", h.staleEmbeddings)
	})
}

func (h *OrchestratorHandler) CreateJob(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	idempotencyKey, ok := requestIdempotencyKey(r)
	if !ok {
		writeError(w, http.StatusBadRequest, "valid Idempotency-Key required")
		return
	}

	var input model.CreateJobInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID

	if !supportedJobType(input.JobType) {
		writeError(w, http.StatusBadRequest, "unsupported job_type")
		return
	}

	job, _, err := h.executor.CreateJob(r.Context(), input, idempotencyKey)
	if errors.Is(err, jobs.ErrExecutionUnavailable) {
		writeError(w, http.StatusServiceUnavailable, "durable signed job execution unavailable")
		return
	}
	if errors.Is(err, jobs.ErrIdempotencyConflict) {
		writeError(w, http.StatusConflict, "idempotency key is bound to another request")
		return
	}
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create job")
		return
	}

	// P2-4: the job is now performed by the durable worker (`jobs.Worker`),
	// which claims it from `data_orchestrator_jobs` under a lease. This handler
	// only records intent and returns 202.
	//
	// It deliberately no longer starts a goroutine. The previous
	// fire-and-forget pinned the work to whichever replica served this request
	// and stranded the row in `running` forever if the process restarted
	// mid-job, because nothing polled the table.

	writeJSON(w, http.StatusAccepted, job)
}

func (h *OrchestratorHandler) Reindex(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	idempotencyKey, ok := requestIdempotencyKey(r)
	if !ok {
		writeError(w, http.StatusBadRequest, "valid Idempotency-Key required")
		return
	}

	var req struct {
		DocumentIDs []string `json:"document_ids"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	job, _, err := h.executor.CreateJob(r.Context(), model.CreateJobInput{
		OrgID:       orgID,
		JobType:     model.JobReindex,
		DocumentIDs: req.DocumentIDs,
	}, idempotencyKey)
	if errors.Is(err, jobs.ErrExecutionUnavailable) {
		writeError(w, http.StatusServiceUnavailable, "durable signed job execution unavailable")
		return
	}
	if errors.Is(err, jobs.ErrIdempotencyConflict) {
		writeError(w, http.StatusConflict, "idempotency key is bound to another request")
		return
	}
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create reindex job")
		return
	}

	// P2-4: performed by the durable worker; see the note in CreateJob.
	writeJSON(w, http.StatusAccepted, job)
}

func (h *OrchestratorHandler) GetJob(w http.ResponseWriter, r *http.Request) {
	jobID := chi.URLParam(r, "jobID")
	if _, err := uuid.Parse(jobID); err != nil {
		writeError(w, http.StatusNotFound, "job not found")
		return
	}
	job, err := h.executor.GetJob(r.Context(), orgIDFrom(r.Context()), jobID)
	if errors.Is(err, jobs.ErrJobNotFound) {
		writeError(w, http.StatusNotFound, "job not found")
		return
	}
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to load job")
		return
	}
	writeJSON(w, http.StatusOK, job)
}

func supportedJobType(jobType model.JobType) bool {
	switch jobType {
	case model.JobReindex, model.JobGraphBuild, model.JobWikiRefresh:
		return true
	default:
		return false
	}
}

func requestIdempotencyKey(r *http.Request) (string, bool) {
	raw := r.Header.Get("Idempotency-Key")
	key := strings.TrimSpace(raw)
	if raw != key || len(key) < 8 || len(key) > 120 {
		return "", false
	}
	for _, character := range key {
		if character < 0x21 || character > 0x7e {
			return "", false
		}
	}
	return key, true
}

func (h *OrchestratorHandler) StaleEmbeddings(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	report, err := h.staleDetector.Detect(r.Context(), orgID, 0)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "stale detection failed")
		return
	}

	writeJSON(w, http.StatusOK, report)
}

func Health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok", "service": "data-orchestrator-go"})
}

func Readyz(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ready", "service": "data-orchestrator-go"})
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}
