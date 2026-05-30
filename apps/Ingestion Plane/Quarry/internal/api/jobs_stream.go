package api

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/jobs"
)

func (h *Handler) streamJob(c *fiber.Ctx) error {
	if h.jobStore == nil || h.streamManager == nil {
		return writeError(c, http.StatusServiceUnavailable, "streaming is not initialized", nil)
	}

	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	// Evict stale cache before the initial existence check so reconnects always
	// read the latest status from the persistent backend.
	h.jobStore.Evict(jobID)
	if existing, exists := h.jobStore.Get(jobID); !exists {
		// Fall back to crawlStore: async-worker crawl jobs may exist there even if
		// the api process in-memory cache missed the creation (e.g. after a restart).
		crawlFound := false
		if h.crawlStore != nil {
			if _, err := h.crawlStore.GetRun(c.Context(), jobID); err == nil {
				crawlFound = true
			}
		}
		if !crawlFound {
			if h.scheduledExec != nil && h.scheduledExec.Enabled() {
				ctx, cancel := context.WithTimeout(c.Context(), 3*time.Second)
				defer cancel()
				if _, err := h.scheduledExec.RehydrateJob(ctx, h.jobStore, jobID); err != nil {
					return writeError(c, http.StatusNotFound, "job not found", nil)
				}
			} else {
				return writeError(c, http.StatusNotFound, "job not found", nil)
			}
		}
	} else if existing.Status == jobs.StatusRunning && h.scheduledExec != nil && h.scheduledExec.Enabled() {
		// On reconnect: refresh from Temporal immediately so we don't miss a completed workflow
		ctx, cancel := context.WithTimeout(c.Context(), 3*time.Second)
		_, _ = h.scheduledExec.RefreshJob(ctx, h.jobStore, jobID)
		cancel()
	}

	eventCh, cleanup := h.streamManager.Subscribe(c.Context(), jobID)

	c.Set("Content-Type", "text/event-stream")
	c.Set("Cache-Control", "no-cache")
	c.Set("Connection", "keep-alive")
	c.Set("X-Accel-Buffering", "no")

	c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
		defer cleanup()
		ticker := time.NewTicker(2 * time.Second)
		defer ticker.Stop()
		// Extend timeout to 15 minutes to cover long crawls (5+ min)
		timeout := time.NewTimer(15 * time.Minute)
		defer timeout.Stop()

		for {
			select {
			case ev, ok := <-eventCh:
				if !ok {
					return
				}
				payload, err := json.Marshal(ev.Data)
				if err != nil {
					continue
				}
				_, _ = w.WriteString(fmt.Sprintf("event: %s\n", ev.Type))
				_, _ = w.WriteString(fmt.Sprintf("data: %s\n\n", payload))
				_ = w.Flush()
			case <-ticker.C:
				// For scheduled (Temporal) jobs: poll workflow completion on every tick
				// so the stream detects "ready" without waiting for GET /jobs/:id
				if h.scheduledExec != nil && h.scheduledExec.Enabled() {
					refreshCtx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
					_, _ = h.scheduledExec.RefreshJob(refreshCtx, h.jobStore, jobID)
					cancel()
				}

				// For crawl jobs processed by the async worker the quarry-api in-memory
				// jobStore cache is stale: the worker updates postgres but the api process
				// never sees those writes via its own in-memory map.  Reading crawlStore
				// (pure postgres, no in-memory layer) gives us the authoritative status
				// and the real completed-page count, and lets us sync jobStore so the
				// next read reflects reality.
				if h.crawlStore != nil {
					if run, err := h.crawlStore.GetRun(context.Background(), jobID); err == nil {
						switch run.Status {
						case quarrycrawl.StatusCompleted:
							_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
								current.Status = jobs.StatusReady
								current.Progress = 100
								current.Result = map[string]any{
									"completed": run.Completed,
									"total":     run.Total,
								}
							})
						case quarrycrawl.StatusFailed:
							_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
								if current.Status != jobs.StatusFailed {
									current.Status = jobs.StatusFailed
								}
							})
						case quarrycrawl.StatusCancelled:
							_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
								if current.Status != jobs.StatusCancelled {
									current.Status = jobs.StatusCancelled
								}
							})
						}
					}
				}

				// Evict stale in-memory cache entry so Get() reads fresh from postgres.
				if h.jobStore != nil {
					h.jobStore.Evict(jobID)
				}
				job, exists := h.jobStore.Get(jobID)
				if !exists {
					_, _ = w.WriteString("event: job:deleted\ndata: {}\n\n")
					_ = w.Flush()
					return
				}
				// Extract page count: crawl jobs store {"completed": N} in Result;
				// product scraping jobs store {"products": [...]} — support both.
				pages := 0
				if job.Result != nil {
					if n, ok := job.Result["completed"].(float64); ok {
						pages = int(n)
					} else if prods, ok := job.Result["products"].([]interface{}); ok {
						pages = len(prods)
					}
				}
				statusPayload := map[string]any{
					"jobId":    job.ID,
					"status":   job.Status,
					"progress": job.Progress,
					"pages":    pages,
				}
				payload, _ := json.Marshal(statusPayload)
				// If job completed, send "completed" event (frontend listens for this exact name)
				if job.Status == jobs.StatusReady {
					_, _ = w.WriteString("event: completed\n")
					_, _ = w.WriteString(fmt.Sprintf("data: %s\n\n", payload))
					_ = w.Flush()
					return
				}
				// If job failed or was cancelled, send as heartbeat with final status and stop.
				if job.Status == jobs.StatusFailed || job.Status == jobs.StatusCancelled {
					_, _ = w.WriteString("event: heartbeat\n")
					_, _ = w.WriteString(fmt.Sprintf("data: %s\n\n", payload))
					_ = w.Flush()
					return
				}
				_, _ = w.WriteString("event: heartbeat\n")
				_, _ = w.WriteString(fmt.Sprintf("data: %s\n\n", payload))
				_ = w.Flush()
			case <-timeout.C:
				return
			}
		}
	})

	return nil
}
