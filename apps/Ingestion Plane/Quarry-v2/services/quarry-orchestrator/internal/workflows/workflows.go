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

// runIDs bundles the Temporal run id and the originating control-plane
// job id so workflow-emitted events can be indexed under both keys.
// JobID is optional — left empty when the workflow was started outside
// the jobs dispatcher (e.g. scheduled cron runs that don't have a
// distinct job record yet).
type runIDs struct {
	RunID string
	JobID string
}

// ScrapeJobInput drives a single-URL run.
type ScrapeJobInput struct {
	// UserID is the verified-JWT initiator (private-by-default ownership).
	UserID string `json:"user_id,omitempty"`
	// Ingest (Phase 2 selective ingest): true = persist+embed; default NEVER.
	Ingest bool   `json:"ingest,omitempty"`
	RunID  string `json:"run_id"`
	JobID  string `json:"job_id,omitempty"`
	OrgID  string `json:"org_id,omitempty"`
	URL    string `json:"url"`
	Policy Policy `json:"policy,omitempty"`
}

// ChangeMonitorInput drives a single recurring change-monitor probe.
//
// RunID/JobID are empty for SCHEDULED fires (Temporal mints a fresh
// execution per cron tick, and there's no control-plane job record). The
// workflow derives a per-execution run_id from its own Temporal execution in
// that case — see ChangeMonitorWF. They're populated only for ad-hoc
// triggers that already carry a run/job id.
type ChangeMonitorInput struct {
	RunID string `json:"run_id,omitempty"`
	JobID string `json:"job_id,omitempty"`
	OrgID string `json:"org_id"`
	URL   string `json:"url"`
	// CreatedBy is the user_id of the schedule's creator. It rides into
	// the change_detected event payload so quarry-control can deliver the
	// one in-product notification to the right user. Empty = no notify.
	CreatedBy string `json:"created_by,omitempty"`
}

// BatchJobInput drives a fan-out over a bounded URL list.
type BatchJobInput struct {
	// UserID is the verified-JWT initiator (private-by-default ownership).
	UserID string `json:"user_id,omitempty"`
	// Ingest (Phase 2 selective ingest): true = persist+embed; default NEVER.
	Ingest bool     `json:"ingest,omitempty"`
	RunID  string   `json:"run_id"`
	JobID  string   `json:"job_id,omitempty"`
	OrgID  string   `json:"org_id,omitempty"`
	URLs   []string `json:"urls"`
	Policy Policy   `json:"policy,omitempty"`
}

// CrawlJobInput drives a bounded BFS crawl.
type CrawlJobInput struct {
	// UserID is the verified-JWT initiator (private-by-default ownership).
	UserID string `json:"user_id,omitempty"`
	// Ingest (Phase 2 selective ingest): true = persist+embed; default NEVER.
	Ingest   bool     `json:"ingest,omitempty"`
	RunID    string   `json:"run_id"`
	JobID    string   `json:"job_id,omitempty"`
	OrgID    string   `json:"org_id,omitempty"`
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

// emitEvent builds an Event with a deterministic idempotency key and
// dispatches it as an activity. Sets both RunID and JobID on the event
// (when ids.JobID is non-empty) so control's `ForJob` index returns it
// for ad-hoc job consumers (e.g. verevon's onboarding wizard).
//
// CRITICAL: this runs inside a Temporal workflow, so every value it
// writes into the Event MUST be deterministic across replays.
// `quarrycontracts.NewID` (which is `ulid.Make` under the hood) uses
// `crypto/rand` and the wall clock — both forbidden in workflow code.
// Two replays would produce different EventIDs, the second
// `ExecuteActivity` call would have a different argument hash, and
// Temporal would log a non-determinism error and fail the run. We
// derive the EventID deterministically from the idempotency-key
// digest (which is itself derived from runID + url + event-type) so
// replays produce byte-identical Event records.
//
// Timestamp uses `workflow.Now(ctx)` which IS deterministic — Temporal
// records the wall clock at the original execution and replays it.
func emitEvent(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	evtType quarrycontracts.EventType,
	payload map[string]any,
	idemParts ...string,
) error {
	runID := quarrycontracts.ID(ids.RunID)
	sum := sha256.Sum256([]byte(strings.Join(idemParts, ":")))
	idemHex := hex.EncodeToString(sum[:])
	evt := quarrycontracts.Event{
		EventID:        deterministicEventID(idemHex),
		RunID:          &runID,
		Type:           evtType,
		Timestamp:      workflow.Now(ctx),
		Payload:        payload,
		IdempotencyKey: idemHex,
	}
	if ids.JobID != "" {
		jobID := quarrycontracts.ID(ids.JobID)
		evt.JobID = &jobID
	}
	return workflow.ExecuteActivity(ctx, a.EmitEvent, ids.RunID, evt).Get(ctx, nil)
}

// deterministicEventID builds an `evt_<hex>` ID from a stable digest
// so replays produce the same value. The shape matches what
// `quarrycontracts.NewID(KindEvent)` returns (`evt_` + 26-char ULID
// is the production shape, but consumers only require the `evt_`
// prefix per the contract — any unique suffix works).
func deterministicEventID(idemHex string) quarrycontracts.ID {
	// 26 chars after the prefix matches the existing ULID width so
	// downstream consumers that expect a fixed-width id keep working.
	const suffixLen = 26
	suffix := idemHex
	if len(suffix) > suffixLen {
		suffix = suffix[:suffixLen]
	}
	return quarrycontracts.ID("evt_" + suffix)
}

// runPage invokes the page activity and emits lifecycle events based on the
// returned error's classification.
//
// org is the originating tenant id. It rides into the RunPageInput so the
// activity can HMAC-bind org_id+url on the /v1/internal/run_page call (a
// leaked runtime bearer can't then forge another tenant's org). Empty org
// is allowed at this layer (the edge enforces a non-empty org_id itself);
// passing it through is deterministic — no new wall-clock/rand calls.
func runPage(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	org string,
	userID string,
	ingest bool,
	url string,
) (activities.RunPageResult, error) {
	var res activities.RunPageResult
	err := workflow.ExecuteActivity(ctx, a.RunPage, activities.RunPageInput{
		RunID:  ids.RunID,
		URL:    url,
		OrgID:  org,
		UserID: userID,
		Ingest: ingest,
	}).Get(ctx, &res)
	if err != nil {
		cat := errs.CategoryUnknown
		if e, ok := errs.Classify(err); ok {
			cat = e.Category
		}
		_ = emitEvent(ctx, a, ids, cat.Event(), map[string]any{
			"url":      url,
			"category": cat.String(),
			"error":    err.Error(),
		}, ids.RunID, url, string(cat.Event()))
		return res, err
	}
	_ = emitEvent(ctx, a, ids, quarrycontracts.EvtPageFetched, map[string]any{
		"url":          url,
		"status":       res.Status,
		"fingerprint":  res.Fingerprint,
		"links":        len(res.Links),
		"content_type": res.ContentType,
		"title":        res.Title,
	}, ids.RunID, url, string(quarrycontracts.EvtPageFetched))
	// Re-emit branding extracted from the seed page back through the
	// control event log so consumers polling /v1/jobs/{id}/events see it
	// alongside page_fetched. The runtime emits BrandingExtracted on its
	// local event sink (NATS); we also surface it here so it lands in
	// control without requiring NATS to be wired.
	if res.Branding != nil {
		_ = emitEvent(ctx, a, ids, quarrycontracts.EvtBrandingExtracted, map[string]any{
			"url":      url,
			"branding": res.Branding,
		}, ids.RunID, url, string(quarrycontracts.EvtBrandingExtracted))
	}
	// Re-emit the post-transform extraction as page_extracted so the
	// control event log — the only path the onboarding wizard's
	// /v1/jobs/{id}/events poll can see — carries the page title (with its
	// provenance), a plain-text excerpt, word count and the serving driver.
	// page_fetched above is pre-transform by design and cannot carry text.
	if payload := pageExtractedPayload(url, res); payload != nil {
		_ = emitEvent(ctx, a, ids, quarrycontracts.EvtPageExtracted, payload,
			ids.RunID, url, string(quarrycontracts.EvtPageExtracted))
	}
	return res, nil
}

// pageExtractedPayload builds the page_extracted payload from a runtime
// RunPageResult, mirroring quarry_runtime::page_extract::event_payload
// (Rust) field-for-field. Returns nil when the runtime supplied no
// extraction (older edge, non-HTML response) so no half-empty event is
// emitted. Optional fields (summary, lang, content_type) are omitted rather
// than sent as "" so the gateway's blank-vs-absent handling stays simple.
func pageExtractedPayload(url string, res activities.RunPageResult) map[string]any {
	if res.TitleSource == "" {
		return nil
	}
	title := res.DisplayTitle
	if title == "" {
		title = res.Title
	}
	payload := map[string]any{
		"url":          url,
		"title":        title,
		"title_source": res.TitleSource,
		"excerpt":      res.Excerpt,
		"word_count":   res.WordCount,
		"driver":       res.Driver,
		"fingerprint":  res.Fingerprint,
	}
	if res.Summary != "" {
		payload["summary"] = res.Summary
	}
	if res.Lang != "" {
		payload["lang"] = res.Lang
	}
	if res.ContentType != "" {
		payload["content_type"] = res.ContentType
	}
	return payload
}

// ScrapeJobWF runs a single URL and emits lifecycle events.
func ScrapeJobWF(ctx workflow.Context, in ScrapeJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())
	ids := runIDs{RunID: in.RunID, JobID: in.JobID}

	if err := emitEvent(ctx, a, ids, quarrycontracts.EvtRunStarted, map[string]any{
		"kind": "scrape",
		"url":  in.URL,
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	if _, err := runPage(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, in.URL); err != nil {
		_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, map[string]any{
			"url":   in.URL,
			"error": err.Error(),
		}, in.RunID, string(quarrycontracts.EvtRunFailed))
		return err
	}

	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": 1,
		"pages_failed":  0,
	}, in.RunID, string(quarrycontracts.EvtRunCompleted))
}

// ChangeMonitorWF fetches a URL, compares it to the org's stored baseline,
// and emits change_detected / change_unchanged. It is the workflow a
// change_monitor schedule fires on each cron tick.
//
// RUN ID: scheduled fires arrive with an empty RunID (the schedule's Args
// carry only org_id+url). We derive a fresh, replay-safe run_id from the
// Temporal workflow execution so every fire gets its own run timeline and
// its own deterministic event idempotency keys — otherwise repeated daily
// fires would all collapse onto one run id and the second fire's events
// would be deduped away. workflow.GetInfo is deterministic across replays.
func ChangeMonitorWF(ctx workflow.Context, in ChangeMonitorInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())

	runID := in.RunID
	if runID == "" {
		execRunID := workflow.GetInfo(ctx).WorkflowExecution.RunID
		runID = "run_" + strings.ReplaceAll(execRunID, "-", "")
	}
	ids := runIDs{RunID: runID, JobID: in.JobID}

	if err := emitEvent(ctx, a, ids, quarrycontracts.EvtRunStarted, map[string]any{
		"kind": "change_monitor",
		"url":  in.URL,
	}, runID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	var res activities.CheckChangeResult
	err := workflow.ExecuteActivity(ctx, a.CheckChange, activities.CheckChangeInput{
		RunID: runID,
		OrgID: in.OrgID,
		URL:   in.URL,
	}).Get(ctx, &res)
	if err != nil {
		_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, map[string]any{
			"url":   in.URL,
			"error": err.Error(),
		}, runID, string(quarrycontracts.EvtRunFailed))
		return err
	}

	evt := quarrycontracts.EvtChangeUnchanged
	if res.Changed {
		evt = quarrycontracts.EvtChangeDetected
	}
	_ = emitEvent(ctx, a, ids, evt, map[string]any{
		"url":         in.URL,
		"org_id":      in.OrgID,
		"created_by":  in.CreatedBy,
		"status":      res.Status,
		"fingerprint": res.Fingerprint,
		"diff_id":     res.DiffID,
		"baseline_id": res.BaselineID,
	}, runID, in.URL, string(evt))

	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": 1,
		"pages_failed":  0,
		"changed":       res.Changed,
	}, runID, string(quarrycontracts.EvtRunCompleted))
}

// BatchJobWF fans out page activities for each URL sequentially (bounded
// concurrency is enforced by the runtime) and aggregates the result.
func BatchJobWF(ctx workflow.Context, in BatchJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())
	ids := runIDs{RunID: in.RunID, JobID: in.JobID}

	if err := emitEvent(ctx, a, ids, quarrycontracts.EvtRunStarted, map[string]any{
		"kind":  "batch",
		"count": len(in.URLs),
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	var visited, failed uint32
	for _, url := range in.URLs {
		if _, err := runPage(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, url); err != nil {
			failed++
			continue
		}
		visited++
	}

	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, map[string]any{
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
	ids := runIDs{RunID: in.RunID, JobID: in.JobID}

	ctrl, err := installControlHandlers(ctx)
	if err != nil {
		return err
	}

	if err := emitEvent(ctx, a, ids, quarrycontracts.EvtRunStarted, map[string]any{
		"kind":      "crawl",
		"seeds":     len(in.Seeds),
		"max_depth": in.MaxDepth,
		"max_pages": in.MaxPages,
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}
	if a.DurableFrontier {
		return crawlJobDurableFrontier(ctx, in, a, ids, ctrl)
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
				_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunPaused, map[string]any{
					"pages_visited": pagesVisited,
					"pages_failed":  pagesFailed,
					"pending":       uint32(len(frontier)),
				}, in.RunID, string(quarrycontracts.EvtRunPaused))
				emitPaused = true
			}
			if !ctrl.waitWhilePaused(ctx) {
				break
			}
			_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunResumed, map[string]any{
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

		res, err := runPage(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, cur.URL)
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
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCancelled, map[string]any{
			"pages_visited": pagesVisited,
			"pages_failed":  pagesFailed,
			"pending":       uint32(len(frontier)),
		}, in.RunID, string(quarrycontracts.EvtRunCancelled))
	}

	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": pagesVisited,
		"pages_failed":  pagesFailed,
	}, in.RunID, string(quarrycontracts.EvtRunCompleted))
}

// crawlJobDurableFrontier keeps only counters in Temporal workflow state. All
// URLs/depths/deduplication/in-flight visibility live in the Rust-owned
// PostgresRequestQueue, so a worker restart resumes from the queue rather than
// replaying a potentially massive frontier slice.
func crawlJobDurableFrontier(
	ctx workflow.Context,
	in CrawlJobInput,
	a *activities.Activities,
	ids runIDs,
	ctrl *crawlControl,
) error {
	queueName := durableFrontierQueueName(in.RunID)
	for _, seed := range in.Seeds {
		if strings.TrimSpace(seed) == "" {
			continue
		}
		if err := workflow.ExecuteActivity(ctx, a.FrontierEnqueue, activities.FrontierEnqueueInput{
			Queue: queueName, OrgID: in.OrgID, RequestID: durableFrontierRequestID(in.RunID, seed), URL: seed,
		}).Get(ctx, nil); err != nil {
			return err
		}
	}

	var pagesVisited, pagesFailed uint32
	emitPaused := false
	for {
		if ctrl.state == statePaused {
			if !emitPaused {
				_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunPaused, map[string]any{
					"pages_visited": pagesVisited, "pages_failed": pagesFailed, "pending": 0,
				}, in.RunID, string(quarrycontracts.EvtRunPaused))
				emitPaused = true
			}
			if !ctrl.waitWhilePaused(ctx) {
				break
			}
			_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunResumed, map[string]any{
				"pages_visited": pagesVisited,
			}, in.RunID, string(quarrycontracts.EvtRunResumed))
			emitPaused = false
		}
		if ctrl.state == stateCancelled || (in.MaxPages > 0 && pagesVisited >= in.MaxPages) {
			break
		}

		var item *activities.FrontierQueueItem
		if err := workflow.ExecuteActivity(ctx, a.FrontierPop, activities.FrontierPopInput{
			Queue: queueName, OrgID: in.OrgID,
		}).Get(ctx, &item); err != nil {
			return err
		}
		if item == nil {
			break
		}
		depth := frontierPayloadDepth(item.Payload)
		res, pageErr := runPage(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, item.URL)
		if pageErr != nil {
			pagesFailed++
		} else {
			pagesVisited++
			if in.MaxDepth == 0 || depth+1 <= in.MaxDepth {
				for _, link := range res.Links {
					if strings.TrimSpace(link) == "" {
						continue
					}
					if err := workflow.ExecuteActivity(ctx, a.FrontierEnqueue, activities.FrontierEnqueueInput{
						Queue: queueName, OrgID: in.OrgID, RequestID: durableFrontierRequestID(in.RunID, link), URL: link, Depth: depth + 1,
					}).Get(ctx, nil); err != nil {
						return err
					}
				}
			}
		}
		if err := workflow.ExecuteActivity(ctx, a.FrontierAck, activities.FrontierAckInput{
			Queue: queueName, OrgID: in.OrgID, RequestID: item.RequestID,
		}).Get(ctx, nil); err != nil {
			return err
		}
		ctrl.progress.Visited = pagesVisited
		ctrl.progress.Failed = pagesFailed
		ctrl.progress.Pending = 0 // queue depth is read from the durable queue view
		if pagesVisited > 0 && pagesVisited%50 == 0 {
			_ = workflow.ExecuteActivity(ctx, a.Checkpoint, activities.CheckpointInput{
				RunID: in.RunID, Visited: pagesVisited, Frontier: 0,
			}).Get(ctx, nil)
		}
	}

	if ctrl.state == stateCancelled {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCancelled, map[string]any{
			"pages_visited": pagesVisited, "pages_failed": pagesFailed,
		}, in.RunID, string(quarrycontracts.EvtRunCancelled))
	}
	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": pagesVisited, "pages_failed": pagesFailed,
	}, in.RunID, string(quarrycontracts.EvtRunCompleted))
}

func durableFrontierQueueName(runID string) string {
	sum := sha256.Sum256([]byte(runID))
	return "crawl-" + hex.EncodeToString(sum[:])[:24]
}

func durableFrontierRequestID(runID, url string) string {
	sum := sha256.Sum256([]byte(runID + "\n" + url))
	return "frontier-" + hex.EncodeToString(sum[:])
}

func frontierPayloadDepth(payload map[string]any) uint32 {
	value, ok := payload["depth"]
	if !ok {
		return 0
	}
	switch depth := value.(type) {
	case float64:
		if depth >= 0 {
			return uint32(depth)
		}
	case int:
		if depth >= 0 {
			return uint32(depth)
		}
	case uint32:
		return depth
	}
	return 0
}
