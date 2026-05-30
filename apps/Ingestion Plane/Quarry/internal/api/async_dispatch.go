package api

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/asyncjobs"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/sse"
)

func (h *Handler) enqueueAsyncJob(ctx context.Context, kind asyncjobs.Kind, jobID string, payload any, apiVersion string) error {
	msg, err := asyncjobs.NewMessage(kind, jobID, payload, apiVersion)
	if err != nil {
		return err
	}

	if h.asyncDispatcher != nil {
		return h.asyncDispatcher.Dispatch(ctx, msg)
	}

	go func() {
		if runErr := h.HandleAsyncJob(context.Background(), msg); runErr != nil {
			zlog.Error().
				Err(runErr).
				Str("job_id", jobID).
				Str("kind", string(kind)).
				Msg("async job execution failed")
		}
	}()
	return nil
}

func (h *Handler) publishAsyncCancel(ctx context.Context, kind asyncjobs.Kind, jobID string) {
	if h.asyncDispatcher == nil {
		return
	}
	_ = h.asyncDispatcher.Cancel(ctx, asyncjobs.CancelMessage{
		Kind:  kind,
		JobID: strings.TrimSpace(jobID),
	})
}

func (h *Handler) HandleAsyncJob(ctx context.Context, msg asyncjobs.Message) error {
	if h == nil {
		return fmt.Errorf("handler is nil")
	}
	if strings.TrimSpace(msg.JobID) == "" {
		return fmt.Errorf("job id is required")
	}
	if h.shouldIgnoreAsyncJob(msg.JobID) {
		return nil
	}

	switch msg.Kind {
	case asyncjobs.KindCrawl:
		var payload asyncjobs.CrawlPayload
		if err := msg.DecodePayload(&payload); err != nil {
			return err
		}
		runCtx, cancel := context.WithCancel(ctx)
		h.activeCrawlCancels.Store(msg.JobID, cancel)
		if payload.Spec.ScheduleAt != nil {
			waitFor := time.Until(payload.Spec.ScheduleAt.UTC())
			if waitFor > 0 {
				timer := time.NewTimer(waitFor)
				defer timer.Stop()
				select {
				case <-timer.C:
				case <-runCtx.Done():
					return nil
				}
			}
		}
		h.markAsyncJobRunning(msg.JobID)
		if h.streamManager != nil {
			h.streamManager.Broadcast(msg.JobID, sse.EventJobStarted, map[string]any{"progress": 0})
		}
		h.runV2CrawlJob(runCtx, msg.JobID, payload.Spec, payload.Webhook, payload.OrgID, payload.UserID)
		return nil
	case asyncjobs.KindSearch:
		var payload asyncjobs.SearchPayload
		if err := msg.DecodePayload(&payload); err != nil {
			return err
		}
		h.markAsyncJobRunning(msg.JobID)
		runCtx, cancel := context.WithCancel(ctx)
		h.activeSearchCancels.Store(msg.JobID, cancel)
		h.runV2SearchJob(runCtx, msg.JobID, v2SearchRunConfig{
			Preset:       payload.Preset,
			OrgID:        payload.OrgID,
			BlendMode:    payload.BlendMode,
			Query:        payload.Query,
			Limit:        payload.Limit,
			Sources:      fromAsyncSearchSources(payload.Sources),
			ScrapeOpts:   payload.ScrapeOpts,
			ShouldScrape: payload.ShouldScrape,
			TimeoutSec:   payload.TimeoutSec,
			Webhook:      payload.Webhook,
		})
		return nil
	case asyncjobs.KindExtract:
		var payload asyncjobs.ExtractPayload
		if err := msg.DecodePayload(&payload); err != nil {
			return err
		}
		h.markAsyncJobRunning(msg.JobID)
		runCtx, cancel := context.WithCancel(ctx)
		h.activeExtractCancels.Store(msg.JobID, cancel)
		h.runV2Extraction(runCtx, msg.JobID, v2ExtractRunConfig{
			Preset:       payload.Preset,
			OrgID:        payload.OrgID,
			Schema:       payload.Schema,
			Prompt:       payload.Prompt,
			SystemPrompt: payload.SystemPrompt,
			TimeoutSec:   payload.TimeoutSec,
			URLTrace:     append([]string(nil), payload.URLTrace...),
			Webhook:      payload.Webhook,
			ScrapeFormat: payload.ScrapeFormat,
		})
		return nil
	case asyncjobs.KindResearch:
		var payload asyncjobs.ResearchPayload
		if err := msg.DecodePayload(&payload); err != nil {
			return err
		}
		h.markAsyncJobRunning(msg.JobID)
		runCtx, cancel := context.WithCancel(ctx)
		h.activeResearchCancels.Store(msg.JobID, cancel)
		h.runV1ResearchJob(runCtx, msg.JobID, v1ResearchRunConfig{
			OrgID:         payload.OrgID,
			BlendMode:     payload.BlendMode,
			Query:         payload.Query,
			Prompt:        payload.Prompt,
			SystemPrompt:  payload.SystemPrompt,
			Preset:        payload.Preset,
			Limit:         payload.Limit,
			MaxIterations: payload.MaxIterations,
			Sources:       fromAsyncSearchSources(payload.Sources),
			ScrapeOpts:    payload.ScrapeOpts,
			Webhook:       payload.Webhook,
			TimeoutSec:    payload.TimeoutSec,
		})
		return nil
	case asyncjobs.KindAgent:
		var payload asyncjobs.AgentPayload
		if err := msg.DecodePayload(&payload); err != nil {
			return err
		}
		h.markAsyncJobRunning(msg.JobID)
		runCtx, cancel := context.WithCancel(ctx)
		h.activeAgentCancels.Store(msg.JobID, cancel)
		h.runAgentJob(runCtx, msg.JobID, payload)
		return nil
	case asyncjobs.KindLlmsTxt:
		var payload asyncjobs.LlmsTxtPayload
		if err := msg.DecodePayload(&payload); err != nil {
			return err
		}
		h.markAsyncJobRunning(msg.JobID)
		h.runLlmsTxtJob(ctx, msg.JobID, payload)
		return nil
	default:
		return fmt.Errorf("unsupported async job kind: %s", msg.Kind)
	}
}

func (h *Handler) HandleAsyncCancel(msg asyncjobs.CancelMessage) {
	if h == nil {
		return
	}

	switch msg.Kind {
	case asyncjobs.KindCrawl:
		h.invokeCancelMap(&h.activeCrawlCancels, msg.JobID)
	case asyncjobs.KindSearch:
		h.invokeCancelMap(&h.activeSearchCancels, msg.JobID)
	case asyncjobs.KindExtract:
		h.invokeCancelMap(&h.activeExtractCancels, msg.JobID)
	case asyncjobs.KindResearch:
		h.invokeCancelMap(&h.activeResearchCancels, msg.JobID)
	case asyncjobs.KindAgent:
		h.invokeCancelMap(&h.activeAgentCancels, msg.JobID)
	case asyncjobs.KindLlmsTxt:
		// LLMs.txt jobs use context cancellation inherited from runLlmsTxtJob;
		// no dedicated cancel map needed — the job context is scoped per-run.
	}
}

func (h *Handler) shouldIgnoreAsyncJob(jobID string) bool {
	if h.jobStore == nil {
		return false
	}
	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil {
		return false
	}
	return job.Status == jobs.StatusCancelled || job.Status == jobs.StatusReady || job.Status == jobs.StatusFailed
}

func (h *Handler) markAsyncJobRunning(jobID string) {
	if h.jobStore == nil {
		return
	}
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		if current.Status == jobs.StatusCancelled {
			return
		}
		current.Status = jobs.StatusRunning
	})
}

func (h *Handler) invokeCancelMap(cancelMap *sync.Map, jobID string) {
	if cancelMap == nil || strings.TrimSpace(jobID) == "" {
		return
	}
	if cancelValue, ok := cancelMap.Load(jobID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
}

func toAsyncSearchSources(sources []v2SearchSource) []asyncjobs.SearchSource {
	if len(sources) == 0 {
		return nil
	}
	converted := make([]asyncjobs.SearchSource, 0, len(sources))
	for _, source := range sources {
		converted = append(converted, asyncjobs.SearchSource{
			Type:          source.Type,
			Site:          source.Site,
			Weight:        source.Weight,
			Limit:         source.Limit,
			Country:       source.Country,
			SearchLang:    source.SearchLang,
			UILang:        source.UILang,
			Freshness:     source.Freshness,
			SafeSearch:    source.SafeSearch,
			ExtraSnippets: source.ExtraSnippets,
		})
	}
	return converted
}

func fromAsyncSearchSources(sources []asyncjobs.SearchSource) []v2SearchSource {
	if len(sources) == 0 {
		return nil
	}
	converted := make([]v2SearchSource, 0, len(sources))
	for _, source := range sources {
		converted = append(converted, v2SearchSource{
			Type:          source.Type,
			Site:          source.Site,
			Weight:        source.Weight,
			Limit:         source.Limit,
			Country:       source.Country,
			SearchLang:    source.SearchLang,
			UILang:        source.UILang,
			Freshness:     source.Freshness,
			SafeSearch:    source.SafeSearch,
			ExtraSnippets: source.ExtraSnippets,
		})
	}
	return converted
}

func markJobDispatchFailure(store *jobs.Store, jobID string, err error) {
	if store == nil || strings.TrimSpace(jobID) == "" || err == nil {
		return
	}
	_, _ = store.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusFailed
		current.Error = err.Error()
	})
}

func wrapDispatchError(err error, action string) error {
	if err == nil {
		return nil
	}
	if errors.Is(err, context.Canceled) {
		return err
	}
	return fmt.Errorf("%s: %w", action, err)
}
