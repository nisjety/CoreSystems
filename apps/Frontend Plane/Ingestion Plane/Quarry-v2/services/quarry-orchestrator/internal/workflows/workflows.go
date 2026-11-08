package workflows

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strconv"
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
	// Zdr is the ZDR posture stamped on the schedule at create (WS4.H). The
	// workflow only passes it to the CheckChange activity and branches on
	// nothing else; nil for a schedule that predates the stamp. Known gaps are
	// in RUNBOOKS: an org that turns ZDR on later keeps its stamp, and this
	// workflow still emits run_started/change_detected events with the URL.
	Zdr *bool `json:"zdr,omitempty"`
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
	// Robots is the robots mode the batch runs with (WS14.S5). Absent or unknown is respect.
	Robots string `json:"robots,omitempty"`
	// PolicySnapshot is the web policy the edge resolved at the handoff (WS7.G 6b), carried to
	// every page of the batch. Empty means the job was admitted without one.
	PolicySnapshot json.RawMessage `json:"policy_snapshot,omitempty"`
	// MaxParallel is the org's cap on how many pages of this batch run at a time
	// (W3 E3.7.2). Zero means no org cap: the QUARRY_BATCH_PARALLEL flag and the hard
	// ceiling still bound it. Omitted when zero, so an input built before it existed is
	// byte-identical.
	MaxParallel uint32 `json:"max_parallel,omitempty"`
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
	// Scope is the crawl scope the edge stamped on the job (WS14.S3). Nil or
	// Legacy runs the crawl as it did before scope existed.
	Scope *activities.CrawlScope `json:"scope,omitempty"`
	// ScopeMode is enforce or observe. Empty means observe.
	ScopeMode string `json:"scope_mode,omitempty"`
	// Robots is respect or ignore. Empty means respect.
	Robots string `json:"robots,omitempty"`
	// Sitemap is "include" or "only" when the job asked for sitemap seeding (INNH-6a). Empty
	// means skip: the crawl follows links only, as it did before.
	Sitemap string `json:"sitemap,omitempty"`
	// ScrapeOptions is the crawl's per-page scrape options (INNH-6b), the canonical JSON the
	// edge stamped on the job. Empty means the default cleaning.
	ScrapeOptions json.RawMessage `json:"scrape_options,omitempty"`
	// PolicySnapshot is the web policy the edge resolved at the handoff (WS7.G 6b), carried to
	// every page of the crawl. Empty means the job was admitted without one. Adding it changes
	// no command of the workflow (it only rides in the page activity's input), so histories
	// recorded before it replay unchanged and need no GetVersion marker.
	PolicySnapshot json.RawMessage `json:"policy_snapshot,omitempty"`
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

// runPage is runPageWith for a page that carries no crawl context.
func runPage(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	org string,
	userID string,
	ingest bool,
	kind string,
	url string,
) (activities.RunPageResult, error) {
	return runPageWith(ctx, a, ids, org, userID, ingest, kind, url, nil)
}

// runPageWith invokes the page activity and emits lifecycle events based on the
// returned error's classification.
//
// org is the originating tenant id. It rides into the RunPageInput so the
// activity can HMAC-bind org_id+url on the /v1/internal/run_page call (a
// leaked runtime bearer can't then forge another tenant's org). Empty org
// is allowed at this layer (the edge enforces a non-empty org_id itself);
// passing it through is deterministic — no new wall-clock/rand calls.
func runPageWith(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	org string,
	userID string,
	ingest bool,
	kind string,
	url string,
	crawl *activities.CrawlContext,
) (activities.RunPageResult, error) {
	return runPageScrape(ctx, a, ids, org, userID, ingest, kind, url, crawl, nil)
}

// runPageScrape is runPageWith for a crawl page that carries the crawl's scrape options
// (INNH-6b). Nil options leave the page call exactly what runPageWith made.
func runPageScrape(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	org string,
	userID string,
	ingest bool,
	kind string,
	url string,
	crawl *activities.CrawlContext,
	scrape json.RawMessage,
) (activities.RunPageResult, error) {
	return runPageSnapshot(ctx, a, ids, org, userID, ingest, kind, url, crawl, scrape, nil)
}

// runPageSnapshot is runPageScrape for a page of a job that carries the policy the edge
// resolved at the handoff (WS7.G 6b). A nil snapshot leaves the page call exactly what
// runPageScrape made.
func runPageSnapshot(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	org string,
	userID string,
	ingest bool,
	kind string,
	url string,
	crawl *activities.CrawlContext,
	scrape json.RawMessage,
	policySnapshot json.RawMessage,
) (activities.RunPageResult, error) {
	var res activities.RunPageResult
	err := workflow.ExecuteActivity(ctx, a.RunPage, activities.RunPageInput{
		RunID:          ids.RunID,
		URL:            url,
		OrgID:          org,
		UserID:         userID,
		Ingest:         ingest,
		Kind:           kind,
		Crawl:          crawl,
		Scrape:         scrape,
		PolicySnapshot: policySnapshot,
	}).Get(ctx, &res)
	if err != nil {
		cat := errs.CategoryUnknown
		if e, ok := errs.Classify(err); ok {
			cat = e.Category
		}
		// final: the activity's retries are exhausted, so this attempt is the last
		// one and the control errors view lists it. code is the closed category.
		payload := map[string]any{
			"url":      url,
			"category": cat.String(),
			"code":     cat.String(),
			"final":    true,
			"error":    err.Error(),
		}
		if crawl != nil {
			// WS14.S3: a page under scope reports the edge's denial (code, message, remedy)
			// and counts as blocked when the edge refused it.
			cat = errs.CategoryOf(err)
			payload["category"] = cat.String()
			payload["code"] = cat.String()
			payload["source"] = "fetch"
			if denial, ok := errs.DenialOf(err); ok {
				payload["code"] = denial["code"]
				payload["message"] = denial["message"]
				payload["remedy"] = denial["remedy"]
			}
		}
		_ = emitEvent(ctx, a, ids, cat.Event(), payload, ids.RunID, url, string(cat.Event()))
		return res, err
	}
	fetched := map[string]any{
		"url":          url,
		"status":       res.Status,
		"fingerprint":  res.Fingerprint,
		"links":        len(res.Links),
		"content_type": res.ContentType,
		"title":        res.Title,
	}
	// WS14.S4: the host's robots.txt Crawl-delay, when the edge reported one. Only a
	// page run under scope (the crawl-scope-v1 path) carries a crawl report.
	if crawl != nil && res.Crawl != nil && res.Crawl.CrawlDelayMS != nil {
		fetched["crawl_delay_ms"] = *res.Crawl.CrawlDelayMS
	}
	addFetchedResultFields(fetched, res)
	_ = emitEvent(ctx, a, ids, quarrycontracts.EvtPageFetched, fetched,
		ids.RunID, url, string(quarrycontracts.EvtPageFetched))
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

// addFetchedResultFields makes the page_fetched payload self-describing for the per-run
// results (INNH-12): the stored artifact ids, the markdown size, the word count and the
// serving driver. Every field is closed and content-free, and each is left out when the
// runtime did not report it (ZDR, an older runtime, a non-HTML response), so an event
// written without them keeps its old shape. Only the payload map changes: no workflow
// command is added or reordered, so a replayed history stays valid.
func addFetchedResultFields(fetched map[string]any, res activities.RunPageResult) {
	if res.MarkdownArtifactID != "" {
		fetched["markdown_artifact_id"] = res.MarkdownArtifactID
	}
	if res.SourceArtifactID != "" {
		fetched["source_artifact_id"] = res.SourceArtifactID
	}
	if res.Bytes > 0 {
		fetched["bytes"] = res.Bytes
	}
	if res.WordCount > 0 {
		fetched["word_count"] = res.WordCount
	}
	if res.Driver != "" {
		fetched["driver"] = res.Driver
	}
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

	if _, err := runPage(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, "", in.URL); err != nil {
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

// changeTextFPVersion guards the W3-E3.5-4 change to ChangeMonitorWF: the CheckChange input
// asks for the text fingerprint.
const changeTextFPVersion = "change-text-fp-v1"

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

	// W3-E3.5-4: from change-text-fp-v1 the probe forwards run_page's text fingerprint, which
	// the edge compares instead of the raw bytes when QUARRY_EDGE__CHANGE_TEXT_FP is on (off by
	// default, and then the edge ignores it). Histories recorded before the marker replay with
	// the activity input they recorded.
	textFP := workflow.GetVersion(ctx, changeTextFPVersion, workflow.DefaultVersion, 1) >= 1

	var res activities.CheckChangeResult
	err := workflow.ExecuteActivity(ctx, a.CheckChange, activities.CheckChangeInput{
		RunID: runID,
		OrgID: in.OrgID,
		URL:   in.URL,
		Zdr:   in.Zdr,

		TextFingerprint: textFP,
	}).Get(ctx, &res)
	if err != nil {
		_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, map[string]any{
			"url":   in.URL,
			"error": err.Error(),
		}, runID, string(quarrycontracts.EvtRunFailed))
		return err
	}

	// The edge refused to record the probe because the organization is under
	// Zero Data Retention (a baseline is durable). Terminal and clean: the run
	// is recorded as failed with reason zdr_refused, no change event is emitted
	// and the workflow completes without error, so nothing retries it. The
	// payload carries no URL.
	if res.ZDRRefused {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, map[string]any{
			"kind":         "change_monitor",
			"reason":       activities.StatusZDRRefused,
			"refusal_code": activities.ZDRRefusalCode,
		}, runID, string(quarrycontracts.EvtRunFailed))
	}

	evt := quarrycontracts.EvtChangeUnchanged
	if res.Changed {
		evt = quarrycontracts.EvtChangeDetected
	}
	payload := map[string]any{
		"url":         in.URL,
		"org_id":      in.OrgID,
		"created_by":  in.CreatedBy,
		"status":      res.Status,
		"fingerprint": res.Fingerprint,
		"diff_id":     res.DiffID,
		"baseline_id": res.BaselineID,
	}
	// A silent re-baseline (the first probe after the fingerprint scheme flipped) is an
	// unchanged event that says so; nothing else about the payload moves.
	if textFP && res.Rebaselined {
		payload["rebaselined"] = true
	}
	_ = emitEvent(ctx, a, ids, evt, payload, runID, in.URL, string(evt))

	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, map[string]any{
		"pages_visited": 1,
		"pages_failed":  0,
		"changed":       res.Changed,
	}, runID, string(quarrycontracts.EvtRunCompleted))
}

// BatchJobWF fans out page activities for each URL and aggregates the result.
// Without the parallel batch (below) the URLs run one after the other.
//
// From batch-control-v1 the batch takes the same control requests as a crawl:
// pause holds it before the next URL (run_paused, then run_resumed), cancel ends
// it before the next URL with run_cancelled. Executions recorded before that
// version run to the end as they did.
//
// From batch-parallel-v1 (W3 E3.7.2) the batch reads QUARRY_BATCH_PARALLEL once,
// through a recorded side effect, and with a value above 1 runs up to that many
// pages at a time (an activity pool of workflow coroutines), capped by the org's
// MaxParallel. Pause and cancel gate each new page as before; pages already
// running finish. Results are aggregated in URL order, so the statistics and the
// terminal counts do not depend on which page finished first. With the flag off
// (the default) the batch runs the sequential loop it always ran.
func BatchJobWF(ctx workflow.Context, in BatchJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())
	ids := runIDs{RunID: in.RunID, JobID: in.JobID}

	var ctrl *crawlControl
	if workflow.GetVersion(ctx, "batch-control-v1", workflow.DefaultVersion, 1) >= 1 {
		var err error
		if ctrl, err = installControlHandlers(ctx); err != nil {
			return err
		}
	}

	if err := emitEvent(ctx, a, ids, quarrycontracts.EvtRunStarted, map[string]any{
		"kind":  "batch",
		"count": len(in.URLs),
	}, in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}

	stats := newJobStatistics(workflow.Now(ctx))
	// WS14.S5: the batch context on each page and the blocked count. A history recorded before
	// the marker runs as it did (no context, no blocked count).
	checked := workflow.GetVersion(ctx, batchCheckVersion, workflow.DefaultVersion, 1) >= 1

	// W3 E3.7.2: a history recorded before the marker never reads the flag and runs sequentially.
	parallel := 0
	if workflow.GetVersion(ctx, batchParallelVersion, workflow.DefaultVersion, 1) >= 1 {
		parallel = batchParallelism(ctx, in.MaxParallel, len(in.URLs))
	}
	if parallel > 1 {
		return batchJobParallel(ctx, a, in, ids, ctrl, stats, checked, parallel)
	}

	var visited, failed, blocked, blockedRobots uint32
	cancelled := false
	for i, url := range in.URLs {
		if ctrl != nil {
			remaining := uint32(len(in.URLs) - i)
			counts := map[string]any{"pages_visited": visited, "pages_failed": failed, "pending": remaining}
			if !controlGate(ctx, a, ids, ctrl, in.RunID, counts) {
				cancelled = true
				break
			}
		}
		var crawl *activities.CrawlContext
		if checked {
			crawl = batchCrawlContext(in.Robots)
		}
		res, err := runPageSnapshot(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, "batch", url, crawl, nil, in.PolicySnapshot)
		if err != nil {
			if checked && errs.CategoryOf(err) == errs.CategoryBlocklist {
				// WS14.S5: a page the edge refused (a denial) is also counted as blocked. It stays
				// in pages_failed and the error classes, so the statistics keep their shape.
				blocked++
				if robotsRefused(err) {
					blockedRobots++
				}
			}
			failed++
			stats.recordFailure(err)
			continue
		}
		visited++
		stats.recordPage(res)
	}

	return finishBatch(ctx, a, ids, in, stats, checked, cancelled, batchTally{
		visited: visited, failed: failed, blocked: blocked, blockedRobots: blockedRobots,
	})
}

// batchTally is the terminal count of a batch.
type batchTally struct {
	visited, failed, blocked, blockedRobots uint32
}

// finishBatch emits the batch's terminal event: run_cancelled when a cancel ended it
// before every URL, run_completed otherwise. The sequential and the parallel batch
// share it, so both end with the same payload shape.
func finishBatch(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	in BatchJobInput,
	stats *jobStatistics,
	checked bool,
	cancelled bool,
	t batchTally,
) error {
	if cancelled {
		cancelPayload := map[string]any{
			"pages_visited": t.visited,
			"pages_failed":  t.failed,
			"pending":       uint32(len(in.URLs)) - t.visited - t.failed,
			"statistics":    stats.payload(t.visited, t.failed, workflow.Now(ctx)),
		}
		if checked {
			cancelPayload["pages_blocked"] = t.blocked
			cancelPayload["pages_blocked_robots"] = t.blockedRobots
		}
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCancelled, cancelPayload, in.RunID, string(quarrycontracts.EvtRunCancelled))
	}

	completed := map[string]any{
		"pages_visited": t.visited,
		"pages_failed":  t.failed,
		"statistics":    stats.payload(t.visited, t.failed, workflow.Now(ctx)),
	}
	if checked {
		completed["pages_blocked"] = t.blocked
		completed["pages_blocked_robots"] = t.blockedRobots
	}
	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, completed, in.RunID, string(quarrycontracts.EvtRunCompleted))
}

// The parallel batch (W3 E3.7.2).
const (
	// batchParallelVersion is the GetVersion id of the parallel batch. Its id is kept in
	// the order the markers were introduced, after batch-check-v1.
	batchParallelVersion = "batch-parallel-v1"
	// batchParallelFlag is the operator switch. Unset, 0, false or off is the sequential
	// batch; true or on is batchParallelDefault pages at a time; a positive integer is that
	// many, up to batchParallelHardMax.
	batchParallelFlag = "QUARRY_BATCH_PARALLEL"
	// batchParallelDefault is the concurrency when the flag is just switched on.
	batchParallelDefault = 4
	// batchParallelHardMax is the ceiling whatever the flag or the org says.
	batchParallelHardMax = 16
)

// batchParallelEnv reads the flag. It is only called inside workflow.SideEffect, so a
// replay reads the recorded value and never the worker's environment. A var so a test
// can set the flag without touching the process environment.
var batchParallelEnv = func() string { return os.Getenv(batchParallelFlag) }

// parseBatchParallel turns the flag's text into a concurrency: 0 (off), or 1 up to
// batchParallelHardMax. Anything it does not understand is off.
func parseBatchParallel(raw string) int {
	switch v := strings.ToLower(strings.TrimSpace(raw)); v {
	case "", "0", "false", "off", "no":
		return 0
	case "true", "on", "yes":
		return batchParallelDefault
	default:
		n, err := strconv.Atoi(v)
		if err != nil || n <= 0 {
			return 0
		}
		if n > batchParallelHardMax {
			return batchParallelHardMax
		}
		return n
	}
}

// effectiveBatchParallel caps the flag's concurrency by the org's limit (zero means no
// org limit) and by the number of URLs. A result of 0 or 1 is the sequential batch.
func effectiveBatchParallel(flag int, orgMax uint32, urls int) int {
	n := flag
	if orgMax > 0 && uint64(orgMax) < uint64(n) {
		n = int(orgMax)
	}
	if n > urls {
		n = urls
	}
	if n < 0 {
		return 0
	}
	return n
}

// batchParallelism reads the flag once per execution as a recorded side effect and
// returns the concurrency this batch runs with.
func batchParallelism(ctx workflow.Context, orgMax uint32, urls int) int {
	var flag int
	encoded := workflow.SideEffect(ctx, func(workflow.Context) any {
		return parseBatchParallel(batchParallelEnv())
	})
	if err := encoded.Get(&flag); err != nil {
		return 0
	}
	return effectiveBatchParallel(flag, orgMax, urls)
}

// batchOutcome is the result slot of one URL of a parallel batch.
type batchOutcome struct {
	res activities.RunPageResult
	err error
}

// batchJobParallel runs the batch's pages up to `limit` at a time. Each page runs in its
// own workflow coroutine and writes its result into the slot of its URL; the terminal
// counts and the statistics are then taken in URL order. Before each new page the batch
// waits for a free slot and passes the control gate: a pause holds new pages (running
// ones finish), a cancel launches no more and ends with run_cancelled once the running
// pages are done.
func batchJobParallel(
	ctx workflow.Context,
	a *activities.Activities,
	in BatchJobInput,
	ids runIDs,
	ctrl *crawlControl,
	stats *jobStatistics,
	checked bool,
	limit int,
) error {
	outcomes := make([]batchOutcome, len(in.URLs))
	var visited, failed uint32
	inFlight, launched := 0, 0
	cancelled := false

	for i, url := range in.URLs {
		if err := workflow.Await(ctx, func() bool { return inFlight < limit }); err != nil {
			return err
		}
		if ctrl != nil {
			counts := map[string]any{
				"pages_visited": visited,
				"pages_failed":  failed,
				"pending":       uint32(len(in.URLs)) - visited - failed,
			}
			if !controlGate(ctx, a, ids, ctrl, in.RunID, counts) {
				cancelled = true
				break
			}
		}
		var crawl *activities.CrawlContext
		if checked {
			crawl = batchCrawlContext(in.Robots)
		}
		slot, pageURL := i, url
		inFlight++
		launched++
		workflow.Go(ctx, func(gctx workflow.Context) {
			res, err := runPageSnapshot(gctx, a, ids, in.OrgID, in.UserID, in.Ingest, "batch", pageURL, crawl, nil, in.PolicySnapshot)
			outcomes[slot] = batchOutcome{res: res, err: err}
			if err != nil {
				failed++
			} else {
				visited++
			}
			inFlight--
		})
	}
	if err := workflow.Await(ctx, func() bool { return inFlight == 0 }); err != nil {
		return err
	}

	var t batchTally
	for _, o := range outcomes[:launched] {
		if o.err != nil {
			if checked && errs.CategoryOf(o.err) == errs.CategoryBlocklist {
				t.blocked++
				if robotsRefused(o.err) {
					t.blockedRobots++
				}
			}
			t.failed++
			stats.recordFailure(o.err)
			continue
		}
		t.visited++
		stats.recordPage(o.res)
	}
	return finishBatch(ctx, a, ids, in, stats, checked, cancelled, t)
}

// controlGate holds the workflow at a safe boundary while it is paused. It emits
// run_paused once for each pause (the pause count is part of the event id) and
// run_resumed when it goes on. It reports false when the run was cancelled, and
// the caller then stops before the next unit of work. The per-unit gate of the
// batch and the single hook between the waves of later slices (F-t2-040).
func controlGate(
	ctx workflow.Context,
	a *activities.Activities,
	ids runIDs,
	ctrl *crawlControl,
	runID string,
	counts map[string]any,
) bool {
	if ctrl.state != statePaused {
		return ctrl.state != stateCancelled
	}
	seq := strconv.Itoa(ctrl.pauseSeq)
	_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunPaused, counts,
		runID, string(quarrycontracts.EvtRunPaused), seq)
	_ = workflow.Await(ctx, func() bool { return ctrl.state != statePaused })
	if ctrl.state == stateCancelled {
		return false
	}
	_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunResumed, map[string]any{"pages_visited": counts["pages_visited"]},
		runID, string(quarrycontracts.EvtRunResumed), seq)
	return true
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

	// Bounded crawls. Before this version a MaxPages or MaxDepth of zero meant
	// "unlimited" and failed pages were never counted, so one request could
	// walk until the workflow timeout. Executions started from now on carry a
	// finite bound whatever the input says; histories recorded before the
	// version marker replay with the legacy behaviour. A workflow must not read
	// the environment, so the fallbacks are constants: the dispatcher normally
	// resolves the configured values into the input first.
	bounded := workflow.GetVersion(ctx, "crawl-bounded-v1", workflow.DefaultVersion, 1) >= 1
	if bounded {
		if in.MaxPages == 0 {
			in.MaxPages = workflowFallbackMaxPages
		}
		if in.MaxDepth == 0 {
			in.MaxDepth = workflowFallbackMaxDepth
		}
	}

	// WS14.S3: a crawl that carries a scope runs under the crawl-scope-v1 marker.
	// Executions recorded before it run as they did. A legacy crawl has no scope.
	var scope *crawlScopeRun
	if workflow.GetVersion(ctx, "crawl-scope-v1", workflow.DefaultVersion, 1) >= 1 {
		scope = newCrawlScopeRun(in)
		// W2-U64: a missing scope_applied (an edge older than this orchestrator) no longer
		// ends an observe-mode crawl after its first page. Histories recorded before the
		// marker keep the strict behaviour.
		if scope != nil && workflow.GetVersion(ctx, crawlScopeTolerantVersion, workflow.DefaultVersion, 1) >= 1 {
			scope.tolerateUnapplied = true
		}
	}

	ctrl, err := installControlHandlers(ctx)
	if err != nil {
		return err
	}

	started := map[string]any{
		"kind":      "crawl",
		"seeds":     len(in.Seeds),
		"max_depth": in.MaxDepth,
		"max_pages": in.MaxPages,
	}
	// INNH-6: named only when asked for, so the payload of a plain crawl is unchanged.
	if in.Sitemap == sitemapInclude || in.Sitemap == sitemapOnly {
		started["sitemap"] = in.Sitemap
	}
	if len(in.ScrapeOptions) > 0 {
		started["scrape_options"] = true
	}
	if err := emitEvent(ctx, a, ids, quarrycontracts.EvtRunStarted, started,
		in.RunID, string(quarrycontracts.EvtRunStarted)); err != nil {
		return err
	}
	if a.DurableFrontier {
		return crawlJobDurableFrontier(ctx, in, a, ids, ctrl, bounded, scope)
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

	// INNH-6a: the sitemap's URLs, queued one level below the seed. The in-memory frontier
	// keeps them in workflow state, so the activity is asked for a bounded number.
	sm := seedSitemap(ctx, in, a, scope, "")
	if sm.failedHard() {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, sitemapFailedEvent(sm),
			in.RunID, string(quarrycontracts.EvtRunFailed))
	}
	if sm != nil && sm.result != nil {
		for _, u := range sm.result.URLs {
			if u == "" {
				continue
			}
			if _, seen := visited[u]; seen {
				continue
			}
			visited[u] = struct{}{}
			frontier = append(frontier, frontierEntry{URL: u, Depth: sitemapSeedDepth})
		}
	}
	follow := sm.followLinks()

	var pagesVisited, pagesFailed, pagesBlocked, pagesBlockedRobots uint32
	stopReason := ""
	stats := newJobStatistics(workflow.Now(ctx))

	emitPaused := false
	for len(frontier) > 0 {
		// Honor pause/cancel signals before each page.
		if ctrl.state == statePaused {
			if !emitPaused {
				_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunPaused, map[string]any{
					"pages_visited": pagesVisited,
					"pages_failed":  pagesFailed,
					"pending":       uint32(len(frontier)),
				}, in.RunID, string(quarrycontracts.EvtRunPaused), strconv.Itoa(ctrl.pauseSeq))
				emitPaused = true
			}
			if !ctrl.waitWhilePaused(ctx) {
				break
			}
			_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunResumed, map[string]any{
				"pages_visited": pagesVisited,
			}, in.RunID, string(quarrycontracts.EvtRunResumed), strconv.Itoa(ctrl.pauseSeq))
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

		var crawl *activities.CrawlContext
		if scope != nil {
			crawl = scope.contextFor(cur.Depth)
		}
		res, err := runPageSnapshot(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, "crawl", cur.URL, crawl, in.ScrapeOptions, in.PolicySnapshot)
		if err != nil && scope != nil && errs.CategoryOf(err) == errs.CategoryBlocklist {
			// The edge refused the page (scope or robots). It is counted as blocked.
			pagesBlocked++
			if robotsRefused(err) {
				pagesBlockedRobots++
			}
			ctrl.progress.Blocked = pagesBlocked
			ctrl.progress.Denied = scope.linksDenied
			ctrl.progress.Pending = uint32(len(frontier))
			continue
		}
		if err != nil {
			pagesFailed++
			stats.recordFailure(err)
			ctrl.progress.Failed = pagesFailed
			ctrl.progress.Pending = uint32(len(frontier))
			// Failed pages count toward the same bound as visited ones, so a
			// site that fails every page cannot be walked indefinitely.
			if bounded && pagesFailed >= in.MaxPages {
				stopReason = crawlStopMaxFailures
				break
			}
			continue
		}
		if scope != nil {
			// An edge that did not apply the scope ends the run: nothing of this page is followed.
			switch scope.verdict(res.Crawl) {
			case verdictRefused:
				stopReason = edgeScopeUnsupported
			case verdictLegacyUnscoped:
				workflow.GetLogger(ctx).Warn("edge did not report scope_applied; crawl continues unscoped (observe)")
			default:
				scope.absorb(res.Crawl)
				ctrl.progress.Denied = scope.linksDenied
			}
			if stopReason == edgeScopeUnsupported {
				break
			}
		}
		// INNH-6b: an edge that did not apply the scrape options ends the run.
		if !scrapeAppliedBy(in, res) {
			stopReason = edgeScrapeUnsupported
			break
		}
		pagesVisited++
		stats.recordPage(res)
		ctrl.progress.Visited = pagesVisited

		// enqueue child links within depth bound (`sitemap: only` follows none)
		if follow && (in.MaxDepth == 0 || cur.Depth+1 <= in.MaxDepth) {
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
			"statistics":    stats.payload(pagesVisited, pagesFailed, workflow.Now(ctx)),
		}, in.RunID, string(quarrycontracts.EvtRunCancelled))
	}

	completed := map[string]any{
		"pages_visited": pagesVisited,
		"pages_failed":  pagesFailed,
		"statistics":    stats.payload(pagesVisited, pagesFailed, workflow.Now(ctx)),
	}
	if stopReason == edgeScopeUnsupported {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, map[string]any{
			"code":          edgeScopeUnsupported,
			"message":       "the edge did not apply the crawl scope; no link of the refused page was followed",
			"pages_visited": pagesVisited,
			"pages_failed":  pagesFailed,
			"pages_blocked": pagesBlocked,
		}, in.RunID, string(quarrycontracts.EvtRunFailed))
	}
	if stopReason == edgeScrapeUnsupported {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, scrapeFailedEvent(pagesVisited, pagesFailed, pagesBlocked),
			in.RunID, string(quarrycontracts.EvtRunFailed))
	}
	if scope != nil {
		for k, v := range scope.terminal(pagesBlocked, pagesBlockedRobots) {
			completed[k] = v
		}
	}
	if sm != nil {
		completed["sitemap"] = sm.payload()
	}
	if stopReason != "" {
		completed["reason"] = stopReason
	}
	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, completed,
		in.RunID, string(quarrycontracts.EvtRunCompleted))
}

// Fallback bounds used inside the workflow when the input carries none. The
// dispatcher resolves the operator-configured values (QUARRY_CRAWL_*) into the
// input before the workflow starts; these constants only cover an input that
// arrives with a zero, and match the built-in defaults of the jobs package.
const (
	workflowFallbackMaxPages uint32 = 1000
	workflowFallbackMaxDepth uint32 = 5
	// crawlStopMaxFailures is the terminal reason when failed pages alone
	// reached the page bound.
	crawlStopMaxFailures = "max_failures"
)

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
	bounded bool,
	scope *crawlScopeRun,
) error {
	queueName := durableFrontierQueueName(in.RunID)
	// W3 E3.7.6: the canonical frontier identity, behind QUARRY_ORCHESTRATOR__CANONICAL_FRONTIER
	// (default off). A history recorded before the marker keeps the raw ids and URLs.
	canonical := false
	if workflow.GetVersion(ctx, canonicalFrontierVersion, workflow.DefaultVersion, 1) >= 1 {
		canonical = canonicalFrontierEnabled(ctx)
	}
	for _, seed := range in.Seeds {
		if strings.TrimSpace(seed) == "" {
			continue
		}
		seedID, seedURL := frontierTarget(in.RunID, seed, canonical)
		if err := workflow.ExecuteActivity(ctx, a.FrontierEnqueue, activities.FrontierEnqueueInput{
			Queue: queueName, OrgID: in.OrgID, RequestID: seedID, URL: seedURL,
		}).Get(ctx, nil); err != nil {
			return err
		}
	}

	// INNH-6a: the sitemap's URLs go into the queue from the activity, one level below the
	// seed, so none of them passes through the workflow history.
	sm := seedSitemap(ctx, in, a, scope, queueName)
	if sm.failedHard() {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, sitemapFailedEvent(sm),
			in.RunID, string(quarrycontracts.EvtRunFailed))
	}
	follow := sm.followLinks()

	var pagesVisited, pagesFailed, pagesBlocked, pagesBlockedRobots uint32
	stopReason := ""
	stats := newJobStatistics(workflow.Now(ctx))
	emitPaused := false
	for {
		if ctrl.state == statePaused {
			if !emitPaused {
				_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunPaused, map[string]any{
					"pages_visited": pagesVisited, "pages_failed": pagesFailed, "pending": 0,
				}, in.RunID, string(quarrycontracts.EvtRunPaused), strconv.Itoa(ctrl.pauseSeq))
				emitPaused = true
			}
			if !ctrl.waitWhilePaused(ctx) {
				break
			}
			_ = emitEvent(ctx, a, ids, quarrycontracts.EvtRunResumed, map[string]any{
				"pages_visited": pagesVisited,
			}, in.RunID, string(quarrycontracts.EvtRunResumed), strconv.Itoa(ctrl.pauseSeq))
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
		var crawl *activities.CrawlContext
		if scope != nil {
			crawl = scope.contextFor(depth)
		}
		res, pageErr := runPageSnapshot(ctx, a, ids, in.OrgID, in.UserID, in.Ingest, "crawl", item.URL, crawl, in.ScrapeOptions, in.PolicySnapshot)
		if pageErr != nil && scope != nil && errs.CategoryOf(pageErr) == errs.CategoryBlocklist {
			pagesBlocked++
			if robotsRefused(pageErr) {
				pagesBlockedRobots++
			}
		} else if pageErr != nil {
			pagesFailed++
			stats.recordFailure(pageErr)
			if bounded && pagesFailed >= in.MaxPages {
				stopReason = crawlStopMaxFailures
			}
		} else if scope != nil && scope.verdict(res.Crawl) == verdictRefused {
			stopReason = edgeScopeUnsupported
		} else if !scrapeAppliedBy(in, res) {
			stopReason = edgeScrapeUnsupported
		} else {
			if scope != nil && appliedBy(res.Crawl) {
				scope.absorb(res.Crawl)
			}
			pagesVisited++
			stats.recordPage(res)
			// `sitemap: only` follows no page link.
			links := res.Links
			if !follow {
				links = nil
			}
			if scope != nil && (in.MaxDepth == 0 || depth+1 <= in.MaxDepth) {
				// WS14.S3: the children of a page under scope go in one batch call.
				if err := enqueueChildrenBatch(ctx, a, queueName, in.OrgID, in.RunID, depth+1, links, canonical); err != nil {
					return err
				}
			} else if in.MaxDepth == 0 || depth+1 <= in.MaxDepth {
				for _, link := range links {
					if strings.TrimSpace(link) == "" {
						continue
					}
					linkID, linkURL := frontierTarget(in.RunID, link, canonical)
					if err := workflow.ExecuteActivity(ctx, a.FrontierEnqueue, activities.FrontierEnqueueInput{
						Queue: queueName, OrgID: in.OrgID, RequestID: linkID, URL: linkURL, Depth: depth + 1,
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
		ctrl.progress.Blocked = pagesBlocked
		ctrl.progress.Pending = 0 // queue depth is read from the durable queue view
		if pagesVisited > 0 && pagesVisited%50 == 0 {
			_ = workflow.ExecuteActivity(ctx, a.Checkpoint, activities.CheckpointInput{
				RunID: in.RunID, Visited: pagesVisited, Frontier: 0,
			}).Get(ctx, nil)
		}
		if stopReason != "" {
			break
		}
	}

	if ctrl.state == stateCancelled {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCancelled, map[string]any{
			"pages_visited": pagesVisited, "pages_failed": pagesFailed,
			"statistics": stats.payload(pagesVisited, pagesFailed, workflow.Now(ctx)),
		}, in.RunID, string(quarrycontracts.EvtRunCancelled))
	}
	completed := map[string]any{
		"pages_visited": pagesVisited, "pages_failed": pagesFailed,
		"statistics": stats.payload(pagesVisited, pagesFailed, workflow.Now(ctx)),
	}
	if stopReason == edgeScopeUnsupported {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, map[string]any{
			"code":          edgeScopeUnsupported,
			"message":       "the edge did not apply the crawl scope; no link of the refused page was followed",
			"pages_visited": pagesVisited,
			"pages_failed":  pagesFailed,
			"pages_blocked": pagesBlocked,
		}, in.RunID, string(quarrycontracts.EvtRunFailed))
	}
	if stopReason == edgeScrapeUnsupported {
		return emitEvent(ctx, a, ids, quarrycontracts.EvtRunFailed, scrapeFailedEvent(pagesVisited, pagesFailed, pagesBlocked),
			in.RunID, string(quarrycontracts.EvtRunFailed))
	}
	if scope != nil {
		for k, v := range scope.terminal(pagesBlocked, pagesBlockedRobots) {
			completed[k] = v
		}
	}
	if sm != nil {
		completed["sitemap"] = sm.payload()
	}
	if stopReason != "" {
		completed["reason"] = stopReason
	}
	return emitEvent(ctx, a, ids, quarrycontracts.EvtRunCompleted, completed,
		in.RunID, string(quarrycontracts.EvtRunCompleted))
}

func durableFrontierQueueName(runID string) string {
	sum := sha256.Sum256([]byte(runID))
	return "crawl-" + hex.EncodeToString(sum[:])[:24]
}

func durableFrontierRequestID(runID, url string) string {
	return activities.FrontierRequestID(runID, url)
}

// The canonical frontier identity (W3 E3.7.6).
const (
	// canonicalFrontierVersion is the GetVersion id of the canonical frontier.
	canonicalFrontierVersion = "crawl-canonical-frontier-v1"
	// canonicalFrontierFlag is the operator switch, default off. On, a crawl URL the Go
	// mirror of the canonical helper covers is enqueued in its canonical form under the id
	// of that form, so the spellings of one page (fragment, host case, default port, dot
	// segments, escapes) are one queue entry, and a seed matches its sitemap entry.
	canonicalFrontierFlag = "QUARRY_ORCHESTRATOR__CANONICAL_FRONTIER"
)

// canonicalFrontierEnv reads the flag. It is only called inside workflow.SideEffect, so a
// replay reads the recorded value. A var so a test can set the flag.
var canonicalFrontierEnv = func() string { return os.Getenv(canonicalFrontierFlag) }

// parseCanonicalFrontier is true for 1, true, on or yes (any case); anything else is off.
func parseCanonicalFrontier(raw string) bool {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "1", "true", "on", "yes":
		return true
	}
	return false
}

// canonicalFrontierEnabled reads the flag once per execution as a recorded side effect.
func canonicalFrontierEnabled(ctx workflow.Context) bool {
	var on bool
	encoded := workflow.SideEffect(ctx, func(workflow.Context) any {
		return parseCanonicalFrontier(canonicalFrontierEnv())
	})
	if err := encoded.Get(&on); err != nil {
		return false
	}
	return on
}

// frontierTarget is the request id and the URL a crawl URL is enqueued with. Canonical on
// and covered by the mirror: the canonical URL and its id. Otherwise the URL as given and
// its raw id, as before (activities.FrontierCanonicalRequestID agrees on both branches).
func frontierTarget(runID, raw string, canonical bool) (string, string) {
	if canonical {
		if canonicalURL, ok := activities.CanonicalFrontierURL(raw); ok {
			return activities.FrontierRequestID(runID, canonicalURL), canonicalURL
		}
	}
	return durableFrontierRequestID(runID, raw), raw
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
