package temporal

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"
)

const (
	CrawlWorkflowName = "CrawlWorkflow"
	BatchWorkflowName = "BatchWorkflow"

	// Signal channels for workflow control.
	SignalPause  = "pause"
	SignalResume = "resume"
	SignalCancel = "cancel"
)

type CrawlWorkflowInput struct {
	JobID       string `json:"jobId"`
	URL         string `json:"url"`
	MaxDepth    int    `json:"maxDepth"`
	Module      string `json:"module,omitempty"`
	Enrich      bool   `json:"enrich,omitempty"`
	EnrichLimit int    `json:"enrichLimit,omitempty"`
	MaxAge      int64  `json:"maxAge,omitempty"`
	// Phase 3: Smart adaptive crawler fields
	Schema       string   `json:"schema,omitempty"`
	Prompt       string   `json:"prompt,omitempty"`
	IncludePaths []string `json:"includePaths,omitempty"`
	ExcludePaths []string `json:"excludePaths,omitempty"`
}

type BatchWorkflowInput struct {
	JobID       string   `json:"jobId"`
	URLs        []string `json:"urls"`
	MaxParallel int      `json:"maxParallel,omitempty"`
}

// CrawlResult is the typed result from a single URL crawl.
type CrawlResult struct {
	URL    string         `json:"url"`
	Data   map[string]any `json:"data,omitempty"`
	Error  string         `json:"error,omitempty"`
	Status string         `json:"status"` // "ok" or "failed"
}

func CrawlWorkflow(ctx workflow.Context, input CrawlWorkflowInput) (map[string]any, error) {
	if input.URL == "" {
		return nil, temporal.NewNonRetryableApplicationError("url is required", "InvalidArgument", nil)
	}

	// ── Signal handling: pause / cancel ──────────────────────────────────────
	paused := false
	cancelled := false

	pauseCh := workflow.GetSignalChannel(ctx, SignalPause)
	resumeCh := workflow.GetSignalChannel(ctx, SignalResume)
	cancelCh := workflow.GetSignalChannel(ctx, SignalCancel)

	// Drain signals in a non-blocking goroutine that runs throughout the workflow.
	workflow.Go(ctx, func(gCtx workflow.Context) {
		for {
			sel := workflow.NewSelector(gCtx)
			sel.AddReceive(pauseCh, func(ch workflow.ReceiveChannel, _ bool) {
				var msg string
				ch.Receive(gCtx, &msg)
				paused = true
			})
			sel.AddReceive(resumeCh, func(ch workflow.ReceiveChannel, _ bool) {
				var msg string
				ch.Receive(gCtx, &msg)
				paused = false
			})
			sel.AddReceive(cancelCh, func(ch workflow.ReceiveChannel, _ bool) {
				var msg string
				ch.Receive(gCtx, &msg)
				cancelled = true
			})
			sel.Select(gCtx)
		}
	})

	// Helper: block while paused, abort if cancelled.
	checkPause := func() error {
		for paused && !cancelled {
			_ = workflow.Sleep(ctx, 2*time.Second)
		}
		if cancelled {
			return temporal.NewApplicationError("workflow cancelled by signal", "CancelledByUser", nil)
		}
		return nil
	}

	// ── Activity options ────────────────────────────────────────────────────
	quickOpts := workflow.ActivityOptions{
		StartToCloseTimeout:    2 * time.Minute,
		HeartbeatTimeout:       30 * time.Second,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumInterval:    10 * time.Second,
			MaximumAttempts:    3,
		},
	}
	analyzeOpts := workflow.ActivityOptions{
		StartToCloseTimeout:    8 * time.Minute,
		HeartbeatTimeout:       60 * time.Second,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    5 * time.Second,
			BackoffCoefficient: 2.0,
			MaximumInterval:    30 * time.Second,
			MaximumAttempts:    2,
		},
	}

	// ── Step 1: Fetch ───────────────────────────────────────────────────────
	if err := checkPause(); err != nil {
		return nil, err
	}

	var html string
	fetchCtx := workflow.WithActivityOptions(ctx, quickOpts)
	if err := workflow.ExecuteActivity(fetchCtx, "FetchPageActivity", input.URL).Get(fetchCtx, &html); err != nil {
		return nil, err
	}

	// ── Step 2: Analyze ─────────────────────────────────────────────────────
	if err := checkPause(); err != nil {
		return nil, err
	}

	var analyzed map[string]any
	analyzeInput := map[string]any{
		"url":          input.URL,
		"html":         html,
		"module":       input.Module,
		"maxDepth":     input.MaxDepth,
		"enrich":       input.Enrich,
		"enrichLimit":  input.EnrichLimit,
		"maxAge":       input.MaxAge,
		"schema":       input.Schema,
		"prompt":       input.Prompt,
		"includePaths": input.IncludePaths,
		"excludePaths": input.ExcludePaths,
	}
	analyzeCtx := workflow.WithActivityOptions(ctx, analyzeOpts)
	if err := workflow.ExecuteActivity(analyzeCtx, "AnalyzePageActivity", analyzeInput).Get(analyzeCtx, &analyzed); err != nil {
		return nil, err
	}

	// ── Step 3: Store (runs pipeline) ───────────────────────────────────────
	if err := checkPause(); err != nil {
		return nil, err
	}

	analyzed["jobId"] = input.JobID
	analyzed["url"] = input.URL
	analyzed["module"] = input.Module
	storeCtx := workflow.WithActivityOptions(ctx, quickOpts)
	if err := workflow.ExecuteActivity(storeCtx, "StoreResultActivity", input.JobID, analyzed).Get(storeCtx, nil); err != nil {
		return nil, err
	}

	return analyzed, nil
}

// BatchWorkflow processes multiple URLs in parallel with a configurable concurrency limit.
// Uses fan-out/fan-in pattern: launches child workflows up to maxParallel at a time.
func BatchWorkflow(ctx workflow.Context, input BatchWorkflowInput) ([]CrawlResult, error) {
	if len(input.URLs) == 0 {
		return nil, temporal.NewNonRetryableApplicationError("urls are required", "InvalidArgument", nil)
	}

	maxParallel := input.MaxParallel
	if maxParallel <= 0 {
		maxParallel = 5
	}
	if maxParallel > 20 {
		maxParallel = 20
	}

	results := make([]CrawlResult, len(input.URLs))
	inflight := 0

	type indexedResult struct {
		index int
		data  map[string]any
		err   error
	}

	resultCh := workflow.NewBufferedChannel(ctx, len(input.URLs))

	for i, currentURL := range input.URLs {
		// Respect concurrency limit
		for inflight >= maxParallel {
			// Wait for one result to come back before launching more.
			var ir indexedResult
			resultCh.Receive(ctx, &ir)
			inflight--
			if ir.err != nil {
				results[ir.index] = CrawlResult{
					URL:    input.URLs[ir.index],
					Error:  ir.err.Error(),
					Status: "failed",
				}
			} else {
				results[ir.index] = CrawlResult{
					URL:    input.URLs[ir.index],
					Data:   ir.data,
					Status: "ok",
				}
			}
		}

		// Launch child workflow for this URL.
		idx := i
		url := currentURL
		childID := fmt.Sprintf("batch-%s-url-%d", input.JobID, idx)

		workflow.Go(ctx, func(gCtx workflow.Context) {
			childOpts := workflow.ChildWorkflowOptions{
				WorkflowID: childID,
			}
			childCtx := workflow.WithChildOptions(gCtx, childOpts)

			var data map[string]any
			err := workflow.ExecuteChildWorkflow(childCtx, CrawlWorkflow, CrawlWorkflowInput{
				JobID:    fmt.Sprintf("%s-%d", input.JobID, idx),
				URL:      url,
				MaxDepth: 1,
			}).Get(childCtx, &data)

			resultCh.Send(gCtx, indexedResult{index: idx, data: data, err: err})
		})
		inflight++
	}

	// Drain remaining in-flight results.
	for inflight > 0 {
		var ir indexedResult
		resultCh.Receive(ctx, &ir)
		inflight--
		if ir.err != nil {
			results[ir.index] = CrawlResult{
				URL:    input.URLs[ir.index],
				Error:  ir.err.Error(),
				Status: "failed",
			}
		} else {
			results[ir.index] = CrawlResult{
				URL:    input.URLs[ir.index],
				Data:   ir.data,
				Status: "ok",
			}
		}
	}

	return results, nil
}
