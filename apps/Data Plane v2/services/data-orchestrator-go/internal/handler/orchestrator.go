package handler

import (
	"context"
	"encoding/json"
	"net/http"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/jobs"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

type contextKey string

const orgIDKey contextKey = "org_id"

func OrgIDMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		orgID := r.Header.Get("X-Org-ID")
		if orgID == "" {
			writeError(w, http.StatusBadRequest, "X-Org-ID header required")
			return
		}
		ctx := context.WithValue(r.Context(), orgIDKey, orgID)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func orgIDFrom(ctx context.Context) string {
	v, _ := ctx.Value(orgIDKey).(string)
	return v
}

type OrchestratorHandler struct {
	executor      *jobs.Executor
	staleDetector *jobs.StaleDetector
}

func NewOrchestratorHandler(e *jobs.Executor, sd *jobs.StaleDetector) *OrchestratorHandler {
	return &OrchestratorHandler{executor: e, staleDetector: sd}
}

func (h *OrchestratorHandler) CreateJob(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var input model.CreateJobInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID

	if input.JobType == "" {
		writeError(w, http.StatusBadRequest, "job_type required")
		return
	}

	job, err := h.executor.CreateJob(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create job")
		return
	}

	go func() {
		if err := h.executor.Run(context.Background(), job); err != nil {
			errMsg := err.Error()
			job.Status = model.StatusFailed
			job.ErrorMessage = &errMsg
		}
	}()

	writeJSON(w, http.StatusAccepted, job)
}

func (h *OrchestratorHandler) Reindex(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var req struct {
		DocumentIDs []string `json:"document_ids"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	job, err := h.executor.CreateJob(r.Context(), model.CreateJobInput{
		OrgID:       orgID,
		JobType:     model.JobReindex,
		DocumentIDs: req.DocumentIDs,
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create reindex job")
		return
	}

	go func() {
		if err := h.executor.Run(context.Background(), job); err != nil {
			errMsg := err.Error()
			job.Status = model.StatusFailed
			job.ErrorMessage = &errMsg
		}
	}()

	writeJSON(w, http.StatusAccepted, job)
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
