// Package jobs implements the ad-hoc job dispatcher: a polling loop
// that picks up `accepted` jobs from quarry-control and starts the
// matching Temporal workflow. It's the wire that connects the
// "POST /v1/jobs" REST entrypoint (used by quarry-edge handoff and by
// verevon's onboarding crawl preview) to the actual execution path
// (Temporal workflows in this orchestrator).
//
// Without this loop, ad-hoc jobs sit forever in `accepted` because the
// only other place that starts workflows is the schedules manager,
// which only fires for cron schedules.
//
// The dispatcher is intentionally idempotent: the Temporal workflow ID
// is derived from the job ID (`quarry-job-<jobID>`) so re-dispatching
// the same job — across orchestrator restarts or polling overlaps —
// returns the existing workflow handle instead of starting a duplicate.
package jobs

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/rs/zerolog/log"
	"go.temporal.io/sdk/client"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/workflows"
)

// Config wires the dispatcher to control plane + Temporal.
type Config struct {
	ControlBaseURL   string
	ControlAuthToken string
	TaskQueue        string
	// Interval between control polls. Defaults to 2s when zero. Keep
	// short for dev (onboarding flow has a 12s deadline) and consider
	// raising it in production once a push notification (NATS event)
	// replaces the poll.
	Interval time.Duration
	// HTTPTimeout caps each control round-trip. Defaults to 5s.
	HTTPTimeout time.Duration
	// MaxAttempts caps how many times we retry a failing dispatch. After
	// this many consecutive failures the job is PUT to control with
	// status=failed so the user-facing flow doesn't spin forever, and
	// the orchestrator stops spamming the same broken job. Defaults
	// to 5.
	MaxAttempts int
	// EntryTTL is how long an entry stays in the in-memory dispatched
	// set before being pruned. Keeps memory bounded over months of
	// jobs. Defaults to 1h — well past any reasonable retry window
	// but short enough that the map doesn't grow unbounded across
	// long-running orchestrator processes.
	EntryTTL time.Duration
}

// dispatchEntry tracks state for one job ID inside the dispatcher's
// dedupe + retry-cap bookkeeping.
type dispatchEntry struct {
	// addedAt is when we first saw this job. Used for TTL pruning.
	addedAt time.Time
	// attempts counts failed dispatch attempts (Temporal start errors).
	// Successful starts mark the entry done and stop incrementing.
	attempts int
	// done is true once we've successfully started the workflow OR
	// marked the job as failed after exceeding MaxAttempts. Either way
	// we stop trying.
	done bool
}

// Manager polls control for `accepted` jobs and starts the matching
// Temporal workflow for each. Mirrors the shape of schedules.Manager.
type Manager struct {
	temporal client.Client
	cfg      Config
	http     *http.Client
	// entries tracks dispatch state per job ID: attempts, addedAt, done.
	// Bounded by EntryTTL via pruneOldEntries(). Across orchestrator
	// restarts the map is rebuilt — Temporal's workflow-id collision
	// check keeps us idempotent across process boundaries.
	entries map[string]*dispatchEntry
}

// New constructs a Manager. The Temporal client must already be
// connected to the orchestrator's namespace.
func New(c client.Client, cfg Config) *Manager {
	if cfg.Interval <= 0 {
		cfg.Interval = 2 * time.Second
	}
	if cfg.HTTPTimeout <= 0 {
		cfg.HTTPTimeout = 5 * time.Second
	}
	if cfg.MaxAttempts <= 0 {
		cfg.MaxAttempts = 5
	}
	if cfg.EntryTTL <= 0 {
		cfg.EntryTTL = time.Hour
	}
	return &Manager{
		temporal: c,
		cfg:      cfg,
		http:     &http.Client{Timeout: cfg.HTTPTimeout},
		entries:  make(map[string]*dispatchEntry),
	}
}

// pruneOldEntries drops entries older than EntryTTL. Called at the
// start of each tick — cheap O(n) over the dispatched set, and the set
// is bounded by control's job list size in any given interval anyway.
func (m *Manager) pruneOldEntries() {
	cutoff := time.Now().Add(-m.cfg.EntryTTL)
	for id, e := range m.entries {
		if e.addedAt.Before(cutoff) {
			delete(m.entries, id)
		}
	}
}

// Run blocks until ctx is cancelled, polling control and dispatching
// new jobs to Temporal each tick.
func (m *Manager) Run(ctx context.Context) {
	tick := time.NewTicker(m.cfg.Interval)
	defer tick.Stop()
	log.Info().
		Dur("interval", m.cfg.Interval).
		Str("control", m.cfg.ControlBaseURL).
		Msg("jobs dispatcher started")
	for {
		select {
		case <-ctx.Done():
			log.Info().Msg("jobs dispatcher stopped")
			return
		case <-tick.C:
			m.tick(ctx)
		}
	}
}

// tick is one poll cycle. Errors are logged and the loop continues —
// a flaky control plane shouldn't kill the dispatcher.
func (m *Manager) tick(ctx context.Context) {
	m.pruneOldEntries()
	jobs, err := m.listJobs(ctx)
	if err != nil {
		log.Warn().Err(err).Msg("jobs dispatcher: list failed")
		return
	}
	for _, j := range jobs {
		id := string(j.ID)
		entry := m.entries[id]
		if entry == nil {
			entry = &dispatchEntry{addedAt: time.Now()}
			m.entries[id] = entry
		}
		if entry.done {
			continue
		}
		if j.Status != "accepted" {
			// Already running, completed, failed externally — done.
			entry.done = true
			continue
		}
		if !isExecutableKind(j.Kind) {
			entry.done = true
			continue
		}
		if err := m.dispatch(ctx, j); err != nil {
			entry.attempts++
			log.Warn().
				Err(err).
				Str("job_id", id).
				Int("attempts", entry.attempts).
				Int("max_attempts", m.cfg.MaxAttempts).
				Msg("jobs dispatcher: dispatch failed")
			if entry.attempts >= m.cfg.MaxAttempts {
				// Give up: mark the job as failed via control so the
				// user-facing flow doesn't spin forever and the
				// dispatcher stops re-trying this entry.
				if perr := m.markFailed(ctx, j.ID, err); perr != nil {
					log.Warn().
						Err(perr).
						Str("job_id", id).
						Msg("mark failed: PUT control failed (will retry next tick)")
					continue
				}
				entry.done = true
				log.Info().
					Str("job_id", id).
					Int("attempts", entry.attempts).
					Msg("jobs dispatcher: gave up, job marked failed")
			}
			continue
		}
		entry.done = true
	}
}

// markFailed PUTs a job to control with status=failed after MaxAttempts
// dispatch errors. Mirrors markRunning but with a different status.
func (m *Manager) markFailed(
	ctx context.Context,
	jobID quarrycontracts.ID,
	cause error,
) error {
	patch := struct {
		Status string `json:"status"`
	}{Status: "failed"}
	body, _ := json.Marshal(patch)
	url := fmt.Sprintf("%s/v1/jobs/%s",
		strings.TrimRight(m.cfg.ControlBaseURL, "/"), string(jobID))
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, url, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	resp, err := m.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("control %d: %s — cause: %v",
			resp.StatusCode, truncate(string(bodyBytes), 200), cause)
	}
	return nil
}

// dispatch starts the Temporal workflow for one accepted job and then
// best-effort PUTs the job back as status=running with the run_id.
func (m *Manager) dispatch(ctx context.Context, j job) error {
	runID := string(quarrycontracts.NewID(quarrycontracts.KindRun))
	workflowID := "quarry-job-" + string(j.ID)
	// Workflow ID is deterministic from the job ID so a re-poll of the
	// same accepted job produces a `WorkflowExecutionAlreadyStarted`
	// error rather than firing a duplicate workflow. We let Temporal's
	// default reuse policy (AllowDuplicateFailedOnly) handle the rest.
	startOpts := client.StartWorkflowOptions{
		ID:                       workflowID,
		TaskQueue:                m.cfg.TaskQueue,
		WorkflowExecutionTimeout: 30 * time.Minute,
	}

	var (
		we  client.WorkflowRun
		err error
	)
	// Pass the workflow type by name (not by function reference). The
	// workflow functions in `workflows.go` take an extra
	// `*activities.Activities` argument that can't be serialized over
	// the wire — we register thin wrappers in orchestrator main.go
	// that capture `acts` in closure scope and re-expose the workflow
	// under its plain name ("ScrapeJobWF" etc.). Using the name string
	// here makes the client send 1 data arg (the input struct) and the
	// worker dispatches to the wrapper, which calls the real workflow
	// with `acts` injected.
	switch j.Kind {
	case "scrape":
		in, ierr := scrapeInputFromJob(j, runID)
		if ierr != nil {
			return ierr
		}
		we, err = m.temporal.ExecuteWorkflow(ctx, startOpts, "ScrapeJobWF", in)
	case "crawl":
		in, ierr := crawlInputFromJob(j, runID)
		if ierr != nil {
			return ierr
		}
		we, err = m.temporal.ExecuteWorkflow(ctx, startOpts, "CrawlJobWF", in)
	case "batch":
		in, ierr := batchInputFromJob(j, runID)
		if ierr != nil {
			return ierr
		}
		we, err = m.temporal.ExecuteWorkflow(ctx, startOpts, "BatchJobWF", in)
	default:
		return fmt.Errorf("unsupported kind: %s", j.Kind)
	}
	if err != nil {
		return fmt.Errorf("execute workflow: %w", err)
	}
	log.Info().
		Str("job_id", string(j.ID)).
		Str("workflow_id", we.GetID()).
		Str("run_id", we.GetRunID()).
		Str("kind", j.Kind).
		Msg("workflow started")

	// Best-effort: mark the job as running. If this fails we still
	// dispatched the workflow — the job record just won't reflect the
	// status change. Events will still flow via run_id/job_id fan-out.
	if err := m.markRunning(ctx, j.ID, runID); err != nil {
		log.Warn().Err(err).Str("job_id", string(j.ID)).Msg("mark running failed")
	}
	return nil
}

// markRunning PUTs to control. Idempotent — control's updateJob handler
// only overwrites the supplied fields.
func (m *Manager) markRunning(ctx context.Context, jobID quarrycontracts.ID, runID string) error {
	patch := struct {
		Status string `json:"status"`
		RunID  string `json:"run_id"`
	}{Status: "running", RunID: runID}
	body, _ := json.Marshal(patch)
	url := fmt.Sprintf("%s/v1/jobs/%s",
		strings.TrimRight(m.cfg.ControlBaseURL, "/"), string(jobID))
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, url, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	resp, err := m.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("control %d: %s", resp.StatusCode, truncate(string(bodyBytes), 200))
	}
	return nil
}

// listJobs fetches the first page of jobs from control. We don't
// paginate because the goal is to catch new `accepted` jobs quickly —
// stale historic jobs are filtered client-side and ignored.
func (m *Manager) listJobs(ctx context.Context) ([]job, error) {
	url := strings.TrimRight(m.cfg.ControlBaseURL, "/") + "/v1/jobs/?limit=50"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	resp, err := m.http.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("control %d: %s", resp.StatusCode, truncate(string(bodyBytes), 200))
	}
	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	// The list endpoint returns a bare JSON array per resources.go
	// `listJobs` handler. Accept `{data: [...]}` too in case the
	// contract gets wrapped later.
	var out []job
	if err := json.Unmarshal(bodyBytes, &out); err == nil {
		return out, nil
	}
	var enveloped struct {
		Data []job `json:"data"`
	}
	if err := json.Unmarshal(bodyBytes, &enveloped); err != nil {
		return nil, fmt.Errorf("decode jobs list: %w", err)
	}
	return enveloped.Data, nil
}

// job mirrors the subset of control's `store.Job` we care about. Kept
// local so the orchestrator doesn't take a runtime dependency on the
// control plane's internal package layout.
//
// CreatedAt is an RFC3339 string, not a Unix-millis int64: control's
// `store.Job` (services/quarry-control/internal/store/store.go) renders
// `created_at` on the wire via a custom MarshalJSON as RFC3339 — the
// convention every other timestamp on this API already follows (Source,
// Snapshot, and the Rust `quarry_core::resources::JobSummary` all use
// RFC3339 / `DateTime<Utc>`). This field is unused by the dispatch logic
// below; it's kept only so decoding a control job into this struct
// doesn't reject the whole response on a type mismatch.
type job struct {
	ID        quarrycontracts.ID `json:"id"`
	Kind      string             `json:"kind"`
	Status    string             `json:"status"`
	Params    map[string]any     `json:"params,omitempty"`
	CreatedAt string             `json:"created_at"`
}

func isExecutableKind(k string) bool {
	switch k {
	case "scrape", "crawl", "batch":
		return true
	}
	return false
}

func scrapeInputFromJob(j job, runID string) (workflows.ScrapeJobInput, error) {
	url, _ := j.Params["url"].(string)
	if url == "" {
		return workflows.ScrapeJobInput{}, errors.New("scrape job missing params.url")
	}
	return workflows.ScrapeJobInput{
		UserID: userFromParams(j.Params),
		Ingest: ingestFromParams(j.Params),
		RunID:  runID,
		JobID: string(j.ID),
		OrgID: orgFromParams(j.Params),
		URL:   url,
	}, nil
}

func crawlInputFromJob(j job, runID string) (workflows.CrawlJobInput, error) {
	// Accept either a single `url` (verevon onboarding) or an array of
	// `seeds` (richer callers).
	seeds := stringsFrom(j.Params["seeds"])
	if len(seeds) == 0 {
		if single, ok := j.Params["url"].(string); ok && single != "" {
			seeds = []string{single}
		}
	}
	if len(seeds) == 0 {
		return workflows.CrawlJobInput{}, errors.New("crawl job missing params.url or params.seeds")
	}
	return workflows.CrawlJobInput{
		UserID:   userFromParams(j.Params),
		Ingest:   ingestFromParams(j.Params),
		RunID:    runID,
		JobID:    string(j.ID),
		OrgID:    orgFromParams(j.Params),
		Seeds:    seeds,
		MaxDepth: uintFrom(j.Params["max_depth"]),
		MaxPages: uintFrom(j.Params["max_pages"]),
	}, nil
}

func batchInputFromJob(j job, runID string) (workflows.BatchJobInput, error) {
	urls := stringsFrom(j.Params["urls"])
	if len(urls) == 0 {
		return workflows.BatchJobInput{}, errors.New("batch job missing params.urls")
	}
	return workflows.BatchJobInput{
		UserID: userFromParams(j.Params),
		Ingest: ingestFromParams(j.Params),
		RunID:  runID,
		JobID: string(j.ID),
		OrgID: orgFromParams(j.Params),
		URLs:  urls,
	}, nil
}

// ingestFromParams reads the selective-ingest flag (Phase 2) from a job's
// params. The edge handoff stamps `ingest` (set by the gateway from the user's
// crawl_ingest_mode). false/absent = working-set only (default NEVER); true =
// persist+embed each crawled page into the Data Plane.
func ingestFromParams(params map[string]any) bool {
	if b, ok := params["ingest"].(bool); ok {
		return b
	}
	return false
}

// userFromParams extracts the initiating user id from a job's params. The
// edge handoff stamps `user_id` (verified from the JWT claim) alongside
// `org_id`; the orchestrator forwards it into the workflow → run_page body,
// and the edge forwards it as x-user-id on ingest so crawled docs are
// owner-stamped private (private-by-default). "" when absent → system/legacy
// job → org-visible.
func userFromParams(params map[string]any) string {
	if s, ok := params["user_id"].(string); ok {
		return s
	}
	return ""
}

// orgFromParams extracts the originating tenant id from a job's params.
// The edge handoff stamps `org_id` (verified from the JWT claim) into the
// job params, so the orchestrator forwards it into the workflow → the
// run_page HMAC org-binding. Returns "" when absent (legacy jobs); the
// edge enforces a non-empty org_id itself.
func orgFromParams(params map[string]any) string {
	if s, ok := params["org_id"].(string); ok {
		return s
	}
	return ""
}

func stringsFrom(v any) []string {
	arr, ok := v.([]any)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(arr))
	for _, x := range arr {
		if s, ok := x.(string); ok && s != "" {
			out = append(out, s)
		}
	}
	return out
}

func uintFrom(v any) uint32 {
	switch n := v.(type) {
	case float64:
		if n < 0 {
			return 0
		}
		return uint32(n)
	case int:
		if n < 0 {
			return 0
		}
		return uint32(n)
	case int64:
		if n < 0 {
			return 0
		}
		return uint32(n)
	case uint32:
		return n
	case uint64:
		return uint32(n)
	}
	return 0
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "…"
}
