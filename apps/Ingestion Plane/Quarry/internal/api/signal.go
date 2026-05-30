package api

import (
	"net/http"
	"strings"

	"github.com/gofiber/fiber/v2"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/jobs"
	quarrytemporal "github.com/triodelab/quarry/internal/temporal"
)

// signalJob sends a Temporal signal (pause / resume / cancel) to a running workflow.
// POST /v1/jobs/:id/signal/:action
func (h *Handler) signalJob(c *fiber.Ctx) error {
	if h.temporalClient == nil {
		return writeError(c, http.StatusServiceUnavailable, "temporal is not enabled", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}

	action := strings.ToLower(strings.TrimSpace(c.Params("action")))
	var signalName string
	switch action {
	case "pause":
		signalName = quarrytemporal.SignalPause
	case "resume":
		signalName = quarrytemporal.SignalResume
	case "cancel":
		signalName = quarrytemporal.SignalCancel
	default:
		return writeError(c, http.StatusBadRequest, "action must be pause, resume, or cancel", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	workflowID := ""
	runID := ""
	if job.Meta != nil {
		workflowID = strings.TrimSpace(job.Meta["workflowId"])
		runID = strings.TrimSpace(job.Meta["runId"])
	}
	if workflowID == "" {
		return writeError(c, http.StatusBadRequest, "job has no associated workflow (immediate-mode jobs cannot be signalled)", nil)
	}

	if err := h.temporalClient.SignalWorkflow(c.UserContext(), workflowID, runID, signalName, action); err != nil {
		zlog.Error().Err(err).Str("job_id", jobID).Str("action", action).Msg("signal workflow failed")
		return writeError(c, http.StatusInternalServerError, "failed to signal workflow: "+err.Error(), nil)
	}

	// Update job meta with signal info.
	if action == "cancel" {
		_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = "cancelled by user"
		})
	}

	zlog.Info().Str("job_id", jobID).Str("workflow_id", workflowID).Str("action", action).Msg("workflow signalled")
	return c.JSON(fiber.Map{
		"success":     true,
		"action":      action,
		"job_id":      jobID,
		"workflow_id": workflowID,
	})
}
