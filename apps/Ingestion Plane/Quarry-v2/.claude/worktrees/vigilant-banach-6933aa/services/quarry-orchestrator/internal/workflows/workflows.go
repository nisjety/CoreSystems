package workflows

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// Policy is an opaque per-run policy bundle forwarded to the runtime.
type Policy map[string]any

// ScrapeJobInput drives a single-URL run.
type ScrapeJobInput struct {
	RunID  string `json:"run_id"`
	URL    string `json:"url"`
	Policy Policy `json:"policy,omitempty"`
}

// BatchJobInput drives a fan-out over a bounded URL list.
type BatchJobInput struct {
	RunID  string   `json:"run_id"`
	URLs   []string `json:"urls"`
	Policy Policy   `json:"policy,omitempty"`
}

// CrawlJobInput drives a bounded BFS crawl.
type CrawlJobInput struct {
	RunID    string   `json:"run_id"`
	Seeds    []string `json:"seeds"`
	MaxDepth uint32   `json:"max_depth"`
	MaxPages uint32   `json:"max_pages"`
	Policy   Policy   `json:"policy,omitempty"`
}

type frontierEntry struct {
	URL   string
	Depth uint32
}

func defaultActivityOpts() workflow.ActivityOptions {
	return workflow.ActivityOptions{
		StartToCloseTimeout: 5 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumInterval:    time.Minute,
			MaximumAttempts:    5,
		},
	}
}

// emitEvent builds an Event with a deterministic idempotency key and dispatches
// it as an activity. All workflow-emitted lifecycle events go through here.
func emitEvent(
	ctx workflow.Context,
	a *activities.Activities,
	runIDStr string,
	evtType quarrycontracts.EventType,
	payload map[string]any,
	idemParts ...string,
) error {
	runID := quarrycontracts.ID(runIDStr)
	sum := sha256.Sum256([]byte(strings.Join(idemParts, ":")))
	evt := quarrycontracts.Event{
		EventID:        quarrycontracts.NewID(quarrycontracts.KindEvent),
		RunID:          &runID,
		Type:           evtType,
		Timestamp:      workflow.Now(ctx),
		Payload:        payload,
		IdempotencyKey: hex.EncodeToString(sum[:]),
	}
	return workflow.ExecuteActivity(ctx, a.EmitEvent, runIDStr, evt).Get(ctx, nil)
}

// runPage invokes the page activity and emits lifecycle events based on the
// returned error's classification.
func runPage(
	ctx workflow.Context,
	a *activities.Activities,
	runIDStr, url string,
) (activities.RunPageResult, error) {
	var res activities.RunPageResult
	err := workflow.ExecuteActivity(ctx, a.RunPage, activities.RunPageInput{
		RunID: runIDStr,
		URL:   url,
	}).Get(ctx, &res)
	if err != nil {
		cat := errs.CategoryUnknown
		if e, ok := errs.Classify(err); ok {
			cat = e.Category
		}
		_ = emitEvent(ctx, a, runIDStr, cat.Event(), map[string]any{
			"url":      url,
			"category": cat.String(),
			"error":    err.Error(),
		}, runIDStr, url, string(cat.Event()))
		return res, err
	}
	_ = emitEvent(ctx, a, runIDStr, quarrycontracts.EvtPageFetched, map[string]any{
		"url":         url,
		"status":      res.Status,
		"fingerprint": res.Fingerprint,
		"links":       len(res.Links),
	}, runIDStr, url, string(quarrycontracts.EvtPageFetched))
	return res, nil
}

// ScrapeJobWF runs a single URL and emits lifecycle events.
func ScrapeJobWF(ctx workflow.Context, in ScrapeJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())

	if err := emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunStarted, map[string]any{
		"kind": "scrape",
		"url":  in.URL,
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	if _, err := runPage(ctx, a, in.RunID, in.URL); err != nil {
		_ = emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunFailed, map[string]any{
			"url":   in.URL,
			"error": err.Error(),
		}, in.RunID, string(quarrycontracts.EvtRunFailed))
		return err
	}

	return emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": 1,
		"pages_failed":  0,
	}, in.RunID, string(quarrycontracts.EvtRunCompleted))
}

// BatchJobWF fans out page activities for each URL sequentially (bounded
// concurrency is enforced by the runtime) and aggregates the result.
func BatchJobWF(ctx workflow.Context, in BatchJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())

	if err := emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunStarted, map[string]any{
		"kind":  "batch",
		"count": len(in.URLs),
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	var visited, failed uint32
	for _, url := range in.URLs {
		if _, err := runPage(ctx, a, in.RunID, url); err != nil {
			failed++
			continue
		}
		visited++
	}

	return emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": visited,
		"pages_failed":  failed,
	}, in.RunID, string(quarrycontracts.EvtRunCompleted))
}

// CrawlJobWF runs a BFS crawl bounded by MaxDepth and MaxPages. Checkpoints
// are emitted every 50 pages so progress is durable across worker restarts.
//
// Operators can send signals at runtime:
//   - "pause"  → suspend the BFS loop after the current page
//   - "resume" → continue from where it paused
//   - "cancel" → exit cleanly with run_cancelled event
//
// Queries:
//   - "state"    → "running"/"paused"/"cancelled"
//   - "progress" → {visited, failed, pending}
func CrawlJobWF(ctx workflow.Context, in CrawlJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())

	ctrl, err := installControlHandlers(ctx)
	if err != nil {
		return err
	}

	if err := emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunStarted, map[string]any{
		"kind":      "crawl",
		"seeds":     len(in.Seeds),
		"max_depth": in.MaxDepth,
		"max_pages": in.MaxPages,
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	visited := map[string]struct{}{}
	frontier := make([]frontierEntry, 0, len(in.Seeds))
	for _, seed := range in.Seeds {
		if seed == "" {
			continue
		}
		if _, seen := visited[seed]; seen {
			continue
		}
		frontier = append(frontier, frontierEntry{URL: seed, Depth: 0})
		visited[seed] = struct{}{}
	}

	var pagesVisited, pagesFailed uint32

	emitPaused := false
	for len(frontier) > 0 {
		// Honor pause/cancel signals before each page.
		if ctrl.state == statePaused {
			if !emitPaused {
				_ = emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunPaused, map[string]any{
					"pages_visited": pagesVisited,
					"pages_failed":  pagesFailed,
					"pending":       uint32(len(frontier)),
				}, in.RunID, string(quarrycontracts.EvtRunPaused))
				emitPaused = true
			}
			if !ctrl.waitWhilePaused(ctx) {
				break
			}
			_ = emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunResumed, map[string]any{
				"pages_visited": pagesVisited,
			}, in.RunID, string(quarrycontracts.EvtRunResumed))
			emitPaused = false
		}
		if ctrl.state == stateCancelled {
			break
		}

		if in.MaxPages > 0 && pagesVisited >= in.MaxPages {
			break
		}

		cur := frontier[0]
		frontier = frontier[1:]

		res, err := runPage(ctx, a, in.RunID, cur.URL)
		if err != nil {
			pagesFailed++
			ctrl.progress.Failed = pagesFailed
			ctrl.progress.Pending = uint32(len(frontier))
			continue
		}
		pagesVisited++
		ctrl.progress.Visited = pagesVisited

		// enqueue child links within depth bound
		if in.MaxDepth == 0 || cur.Depth+1 <= in.MaxDepth {
			for _, link := range res.Links {
				if link == "" {
					continue
				}
				if _, seen := visited[link]; seen {
					continue
				}
				visited[link] = struct{}{}
				frontier = append(frontier, frontierEntry{URL: link, Depth: cur.Depth + 1})
			}
		}
		ctrl.progress.Pending = uint32(len(frontier))

		// durable checkpoint every 50 pages
		if pagesVisited%50 == 0 {
			_ = workflow.ExecuteActivity(ctx, a.Checkpoint, activities.CheckpointInput{
				RunID:    in.RunID,
				Visited:  pagesVisited,
				Frontier: uint32(len(frontier)),
			}).Get(ctx, nil)
		}
	}

	// If we exited because of cancellation, emit run_cancelled. Otherwise
	// emit run_completed as usual.
	if ctrl.state == stateCancelled {
		return emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunCancelled, map[string]any{
			"pages_visited": pagesVisited,
			"pages_failed":  pagesFailed,
			"pending":       uint32(len(frontier)),
		}, in.RunID, string(quarrycontracts.EvtRunCancelled))
	}

	return emitEvent(ctx, a, in.RunID, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": pagesVisited,
		"pages_failed":  pagesFailed,
	}, in.RunID, string(quarrycontracts.EvtRunCompleted))
}
