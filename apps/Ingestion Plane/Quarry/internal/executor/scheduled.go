package executor

import (
	"context"
	"fmt"
	"strings"
	"time"

	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/sdk/client"

	"github.com/triodelab/quarry/internal/config"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	quarrytemporal "github.com/triodelab/quarry/internal/temporal"
)

type DispatchInfo struct {
	WorkflowID string `json:"workflowId"`
	RunID      string `json:"runId"`
}

type ScheduledExecutor struct {
	cfg    *config.Config
	client client.Client
}

func NewScheduledExecutor(cfg *config.Config, temporalClient client.Client) *ScheduledExecutor {
	return &ScheduledExecutor{cfg: cfg, client: temporalClient}
}

func (e *ScheduledExecutor) Enabled() bool {
	return e != nil && e.client != nil && e.cfg != nil && e.cfg.TemporalEnabled
}

func (e *ScheduledExecutor) DispatchCrawl(ctx context.Context, jobID string, req *models.CrawlAPIRequest) (*DispatchInfo, error) {
	if !e.Enabled() {
		return nil, fmt.Errorf("scheduled executor is disabled")
	}
	if strings.TrimSpace(jobID) == "" {
		return nil, fmt.Errorf("job id is required")
	}
	if req == nil || strings.TrimSpace(req.URL) == "" {
		return nil, fmt.Errorf("url is required")
	}

	workflowID := "crawl-" + jobID
	startOpts := client.StartWorkflowOptions{
		ID:        workflowID,
		TaskQueue: e.cfg.TemporalTaskQueue,
	}

	exec, err := e.client.ExecuteWorkflow(ctx, startOpts, quarrytemporal.CrawlWorkflow, quarrytemporal.CrawlWorkflowInput{
		JobID:        jobID,
		URL:          req.URL,
		MaxDepth:     req.MaxDepth,
		Module:       req.Module,
		Enrich:       req.Enrich,
		EnrichLimit:  req.EnrichLimit,
		MaxAge:       req.MaxAge,
		Schema:       req.Schema,
		Prompt:       req.Prompt,
		IncludePaths: req.IncludePaths,
		ExcludePaths: req.ExcludePaths,
	})
	if err != nil {
		return nil, err
	}

	return &DispatchInfo{WorkflowID: exec.GetID(), RunID: exec.GetRunID()}, nil
}

func (e *ScheduledExecutor) RefreshJob(ctx context.Context, store *jobs.Store, jobID string) (*jobs.Job, error) {
	if store == nil {
		return nil, fmt.Errorf("job store is nil")
	}
	job, ok := store.Get(jobID)
	if !ok {
		return nil, fmt.Errorf("job not found")
	}
	if !e.Enabled() {
		return job, nil
	}

	workflowID := strings.TrimSpace(job.Meta["workflowId"])
	runID := strings.TrimSpace(job.Meta["runId"])
	if workflowID == "" {
		return job, nil
	}

	desc, err := e.client.DescribeWorkflowExecution(ctx, workflowID, runID)
	if err != nil {
		return job, nil
	}

	status := desc.WorkflowExecutionInfo.GetStatus()
	switch status {
	case enumspb.WORKFLOW_EXECUTION_STATUS_RUNNING:
		updated, _ := store.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusRunning
		})
		if updated != nil {
			return updated, nil
		}
		return job, nil
	case enumspb.WORKFLOW_EXECUTION_STATUS_COMPLETED:
		var payload map[string]any
		wfRun := e.client.GetWorkflow(ctx, workflowID, runID)
		if getErr := wfRun.Get(ctx, &payload); getErr != nil {
			updated, _ := store.Update(job.ID, func(current *jobs.Job) {
				current.Status = jobs.StatusFailed
				current.Error = getErr.Error()
			})
			if updated != nil {
				return updated, nil
			}
			return job, nil
		}
		updated, _ := store.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusReady
			current.Progress = 100
			current.Result = payload
		})
		if updated != nil {
			return updated, nil
		}
		return job, nil
	case enumspb.WORKFLOW_EXECUTION_STATUS_FAILED,
		enumspb.WORKFLOW_EXECUTION_STATUS_CANCELED,
		enumspb.WORKFLOW_EXECUTION_STATUS_TERMINATED,
		enumspb.WORKFLOW_EXECUTION_STATUS_TIMED_OUT:
		updated, _ := store.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = status.String()
		})
		if updated != nil {
			return updated, nil
		}
		return job, nil
	default:
		return job, nil
	}
}

func (e *ScheduledExecutor) RehydrateJob(ctx context.Context, store *jobs.Store, jobID string) (*jobs.Job, error) {
	if store == nil {
		return nil, fmt.Errorf("job store is nil")
	}
	if !e.Enabled() {
		return nil, fmt.Errorf("scheduled executor is disabled")
	}
	if strings.TrimSpace(jobID) == "" {
		return nil, fmt.Errorf("job id is required")
	}

	workflowID := "crawl-" + jobID
	desc, err := e.client.DescribeWorkflowExecution(ctx, workflowID, "")
	if err != nil || desc == nil || desc.WorkflowExecutionInfo == nil {
		if err != nil {
			return nil, err
		}
		return nil, fmt.Errorf("workflow not found")
	}

	info := desc.WorkflowExecutionInfo
	createdAt := time.Now()
	if started := info.GetStartTime(); started != nil {
		createdAt = started.AsTime()
	}

	job := &jobs.Job{
		ID:        jobID,
		Status:    jobs.StatusRunning,
		CreatedAt: createdAt,
		ExpiresAt: time.Now().Add(24 * time.Hour),
		Progress:  10,
		Result:    map[string]any{},
		Meta: map[string]string{
			"workflowId": workflowID,
			"runId":      info.GetExecution().GetRunId(),
			"mode":       string(ModeScheduled),
		},
	}

	status := info.GetStatus()
	switch status {
	case enumspb.WORKFLOW_EXECUTION_STATUS_RUNNING:
		job.Status = jobs.StatusRunning
		job.Progress = 25
	case enumspb.WORKFLOW_EXECUTION_STATUS_COMPLETED:
		job.Status = jobs.StatusReady
		job.Progress = 100
		wfRun := e.client.GetWorkflow(ctx, workflowID, info.GetExecution().GetRunId())
		var payload map[string]any
		if getErr := wfRun.Get(ctx, &payload); getErr != nil {
			job.Status = jobs.StatusFailed
			job.Error = getErr.Error()
		} else {
			job.Result = payload
			if payload != nil {
				if rawURL, ok := payload["url"].(string); ok && strings.TrimSpace(rawURL) != "" {
					job.Meta["url"] = rawURL
				}
				if rawModule, ok := payload["module"].(string); ok && strings.TrimSpace(rawModule) != "" {
					job.Meta["module"] = rawModule
				}
			}
		}
	case enumspb.WORKFLOW_EXECUTION_STATUS_FAILED,
		enumspb.WORKFLOW_EXECUTION_STATUS_CANCELED,
		enumspb.WORKFLOW_EXECUTION_STATUS_TERMINATED,
		enumspb.WORKFLOW_EXECUTION_STATUS_TIMED_OUT:
		job.Status = jobs.StatusFailed
		job.Error = status.String()
	default:
		job.Status = jobs.StatusRunning
	}

	return store.Upsert(job), nil
}
