package schedules

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
	commonpb "go.temporal.io/api/common/v1"
	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/converter"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/controlauth"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/jobs"
)

// memoOrgKey is the Temporal schedule Memo field that carries the owning org.
// We stamp it at create time and read it back when listing existing schedules
// so the orphan-reaper can be ORG-SCOPED: a reconcile that only knows about
// org A's schedules must never delete org B's Temporal schedule as an "orphan".
const memoOrgKey = "org_id"

// Config configures the schedule reconciler.
type Config struct {
	ControlBaseURL    string
	ControlAuthToken  string
	ControlHMACSecret string
	TaskQueue         string
	HTTPClient        *http.Client
	Interval          time.Duration
	// CrawlLimits are the bounds a crawl schedule's fire runs with (WS15.S2).
	// The reconciler resolves them into the schedule args, so a fire uses the
	// same bounds the dispatcher would apply. Zero means the built-in defaults.
	CrawlLimits jobs.CrawlLimits
	// RuntimeSync turns on the schedule runtime sync (WS15.S4c, runtime.go). It is
	// read once from QUARRY_SCHEDULE_RUNTIME_SYNC by cmd/orchestrator, off by default.
	RuntimeSync bool
}

// Manager reconciles schedules from Control Plane into Temporal.
type Manager struct {
	cfg Config
	tc  client.Client
	log zerolog.Logger
	// runtime is the runtime sync's state per schedule (WS15.S4c). Only reconcile
	// passes touch it.
	runtime map[string]*runtimeTrack
}

// ScheduleSpec is the wire payload from Control Plane. OrgID carries the
// owning tenant (control's scheduleWire already serializes `org_id`); the
// reconciler stamps it into the Temporal schedule Memo so reaping can be
// org-scoped, and refuses to act on a schedule with no org.
type ScheduleSpec struct {
	ID       string        `json:"id"`
	Name     string        `json:"name"`
	OrgID    string        `json:"org_id"`
	Cron     string        `json:"cron"`
	Workflow string        `json:"workflow"`
	Args     []interface{} `json:"args"`
	Paused   bool          `json:"paused"`
	// Executable-schedule fields (WS15.S2). A zero value is the default.
	Overlap        string `json:"overlap,omitempty"`
	TimeZone       string `json:"timezone,omitempty"`
	JitterS        int64  `json:"jitter_s,omitempty"`
	CatchupWindowS int64  `json:"catchup_window_s,omitempty"`
	PauseOnFailure bool   `json:"pause_on_failure,omitempty"`
	// ExecutionTimeoutS is derived by prepareSpec; control never sends it.
	ExecutionTimeoutS int64 `json:"-"`
}

// existingSchedule is one Temporal schedule the reconciler discovered, paired
// with the org we decoded from its Memo. OrgID is "" when the schedule predates
// org-stamping (or carries no memo) — such schedules are NEVER reaped, because
// we can't prove which tenant they belong to (fail-closed).
type existingSchedule struct {
	OrgID string
	// LastRunAt and NextRunAt are the schedule's latest and next action times in
	// Temporal (WS15.S4c); zero when Temporal holds none.
	LastRunAt time.Time
	NextRunAt time.Time
}

// New builds a Manager.
func New(tc client.Client, cfg Config) *Manager {
	if cfg.CrawlLimits == (jobs.CrawlLimits{}) {
		cfg.CrawlLimits = jobs.DefaultCrawlLimits()
	}
	if cfg.Interval <= 0 {
		cfg.Interval = 30 * time.Second
	}
	if cfg.HTTPClient == nil {
		cfg.HTTPClient = &http.Client{Timeout: 30 * time.Second}
	}
	return &Manager{
		cfg: cfg,
		tc:  tc,
		log: log.With().Str("component", "schedules").Logger(),
	}
}

// Run starts the reconciliation loop; returns when ctx is cancelled.
func (m *Manager) Run(ctx context.Context) error {
	t := time.NewTicker(m.cfg.Interval)
	defer t.Stop()

	// Initial reconcile.
	if err := m.reconcile(ctx); err != nil {
		m.log.Warn().Err(err).Msg("initial reconcile failed")
	}

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-t.C:
			if err := m.reconcile(ctx); err != nil {
				m.log.Warn().Err(err).Msg("reconcile failed")
			}
		}
	}
}

func (m *Manager) reconcile(ctx context.Context) error {
	// Fetch the desired set FIRST. It needs no Temporal client, so a control
	// outage (or a nil client in tests) surfaces as a clean "fetch desired"
	// error without ever touching Temporal.
	desired, err := m.fetchDesired(ctx)
	if err != nil {
		return fmt.Errorf("fetch desired: %w", err)
	}
	return m.reconcileWith(ctx, m.tc.ScheduleClient(), desired)
}

// reconcileWith runs one reconcile pass against the supplied ScheduleClient.
// Split out from reconcile() so tests can inject a fake client and drive the
// full create/update/REAP path — in particular the org-scoped reaper, which is
// the security-critical invariant here.
//
// ORG-SCOPED REAPING (the headline safety property):
//
//	A reconcile pass only ever reaps a Temporal schedule whose owning org is
//	present in the DESIRED set for this pass. Concretely, an existing schedule
//	is deleted as an orphan only when ALL of:
//	  1. it is not in the desired set (its control row is gone), AND
//	  2. its org (decoded from the schedule Memo) is non-empty, AND
//	  3. that org also owns at least one schedule in the desired set.
//
//	(3) is the guard that makes cross-org reaping impossible: if org A's
//	reconcile somehow observes org B's schedules (e.g. desired is a per-org
//	subset, or a list scoping bug), org B's schedules are NOT in (3)'s
//	desired-org set, so they are left untouched. An org with zero desired
//	schedules can never have any of its Temporal schedules reaped by another
//	org's pass. Unknown-org schedules (empty memo) are also never reaped
//	(fail-closed).
func (m *Manager) reconcileWith(ctx context.Context, sc client.ScheduleClient, desired []ScheduleSpec) error {
	existing, err := m.listExisting(ctx, sc)
	if err != nil {
		return fmt.Errorf("list existing: %w", err)
	}

	desiredSet := make(map[string]ScheduleSpec, len(desired))
	// desiredOrgs is the set of tenants represented in the desired set. Only
	// these orgs' schedules are eligible for orphan-reaping this pass.
	desiredOrgs := make(map[string]struct{}, len(desired))
	for _, s := range desired {
		desiredSet[s.ID] = s
		if s.OrgID != "" {
			desiredOrgs[s.OrgID] = struct{}{}
		}
	}

	// Create or update.
	for id, spec := range desiredSet {
		// Defensive: never materialize a workflowless Temporal schedule.
		// quarry-control only maps target kinds it can run as recurring
		// workflows (today: change_monitor); unmapped kinds serialize with
		// an empty Workflow. Skip them — but they STAY in desiredSet so the
		// orphan-reap loop below doesn't delete anything on their behalf.
		if spec.Workflow == "" {
			if _, ok := existing[id]; ok {
				m.pauseIfRunning(ctx, sc, id)
			}
			m.log.Debug().Str("id", id).Msg("skipping schedule with no mapped workflow")
			continue
		}
		spec = m.prepareSpec(spec)
		if _, ok := existing[id]; !ok {
			if err := m.create(ctx, sc, spec); err != nil {
				m.log.Error().Err(err).Str("id", id).Msg("create schedule failed")
			} else {
				m.log.Info().Str("id", id).Msg("schedule created")
			}
			continue
		}
		if err := m.update(ctx, sc, spec); err != nil {
			m.log.Error().Err(err).Str("id", id).Msg("update schedule failed")
		}
	}

	// Delete orphans — ORG-SCOPED. See the reconcileWith doc comment.
	for id, ex := range existing {
		if _, ok := desiredSet[id]; ok {
			continue // still desired — keep it.
		}
		if ex.OrgID == "" {
			// Unknown owner: we can't prove the tenant, so we refuse to reap
			// it. Fail closed rather than risk deleting another org's work.
			m.log.Warn().Str("id", id).Msg("orphan schedule has no org memo; NOT reaping (fail-closed)")
			continue
		}
		if _, ok := desiredOrgs[ex.OrgID]; !ok {
			// The orphan belongs to an org that has NO desired schedules in
			// this pass. Reaping it would be cross-org deletion — refuse.
			m.log.Debug().Str("id", id).Str("org_id", ex.OrgID).
				Msg("orphan schedule belongs to an org not in this reconcile's desired set; NOT reaping")
			continue
		}
		h := sc.GetHandle(ctx, id)
		if err := h.Delete(ctx); err != nil {
			m.log.Error().Err(err).Str("id", id).Str("org_id", ex.OrgID).Msg("delete schedule failed")
		} else {
			m.log.Info().Str("id", id).Str("org_id", ex.OrgID).Msg("schedule reaped (org-scoped)")
		}
	}

	// Runtime sync (WS15.S4c): off unless QUARRY_SCHEDULE_RUNTIME_SYNC is on. It never
	// fails the pass; see runtime.go.
	if m.cfg.RuntimeSync {
		m.syncRuntime(ctx, sc, existing, desiredSet, desiredOrgs)
	}
	return nil
}

func (m *Manager) fetchDesired(ctx context.Context) ([]ScheduleSpec, error) {
	url := m.cfg.ControlBaseURL + "/v1/internal/schedules/desired"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	if err := controlauth.Sign(req, nil, m.cfg.ControlHMACSecret); err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")

	resp, err := m.cfg.HTTPClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("control plane returned %d: %s", resp.StatusCode, string(body))
	}

	// quarry-control wraps list responses in the project-standard envelope
	// `{"data": [...], "meta": {...}, "error": null}`. Decode the envelope
	// and return the inner array. We also tolerate a bare-array response
	// (older fixtures + the unit tests use that shape) by inspecting the
	// first non-whitespace byte before choosing a decoder.
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read body: %w", err)
	}

	trimmed := bytes.TrimLeft(body, " \t\r\n")
	if len(trimmed) > 0 && trimmed[0] == '[' {
		var bare []ScheduleSpec
		if err := json.Unmarshal(body, &bare); err != nil {
			return nil, fmt.Errorf("decode bare array: %w", err)
		}
		return bare, nil
	}

	var envelope struct {
		Data  []ScheduleSpec `json:"data"`
		Error *string        `json:"error"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return nil, fmt.Errorf("decode: %w", err)
	}
	if envelope.Error != nil && *envelope.Error != "" {
		return nil, fmt.Errorf("control plane error: %s", *envelope.Error)
	}
	return envelope.Data, nil
}

// listExisting enumerates the Temporal schedules and pairs each with the org
// decoded from its Memo. The org is what makes reaping org-scoped; a schedule
// whose Memo lacks an org_id (or can't be decoded) maps to OrgID:"" and is
// therefore never reaped (fail-closed).
func (m *Manager) listExisting(ctx context.Context, sc client.ScheduleClient) (map[string]existingSchedule, error) {
	it, err := sc.List(ctx, client.ScheduleListOptions{})
	if err != nil {
		return nil, err
	}
	out := make(map[string]existingSchedule)
	for it.HasNext() {
		entry, err := it.Next()
		if err != nil {
			return nil, err
		}
		out[entry.ID] = existingSchedule{
			OrgID:     orgFromMemo(entry.Memo),
			LastRunAt: latestScheduleTime(entry.RecentActions),
			NextRunAt: nextScheduleTime(entry.NextActionTimes),
		}
	}
	return out, nil
}

// orgFromMemo decodes the org_id field from a schedule's Temporal Memo using
// the default data converter (the same converter the SDK encodes ScheduleOptions
// .Memo with). Returns "" when the memo is absent, lacks the field, or fails to
// decode — callers treat "" as "unknown owner, do not reap".
func orgFromMemo(memo *commonpb.Memo) string {
	if memo == nil {
		return ""
	}
	payload, ok := memo.GetFields()[memoOrgKey]
	if !ok || payload == nil {
		return ""
	}
	var org string
	if err := converter.GetDefaultDataConverter().FromPayload(payload, &org); err != nil {
		return ""
	}
	return org
}

func (m *Manager) create(ctx context.Context, sc client.ScheduleClient, spec ScheduleSpec) error {
	// Stamp the owning org into the schedule Memo so a future reconcile can
	// reap orphans org-scoped (see reconcileWith). The SDK encodes this map
	// with the default data converter; listExisting decodes it symmetrically.
	var memo map[string]interface{}
	if spec.OrgID != "" {
		memo = map[string]interface{}{memoOrgKey: spec.OrgID}
	}
	opts := client.ScheduleOptions{
		ID:             spec.ID,
		Spec:           m.scheduleSpecFor(spec),
		Action:         m.scheduleActionFor(spec),
		Overlap:        overlapPolicy(spec.Overlap),
		PauseOnFailure: spec.PauseOnFailure,
		Paused:         spec.Paused,
		Note:           spec.Name,
		Memo:           memo,
	}
	// A zero catch-up window keeps the server default, so it is set only when asked for.
	if spec.CatchupWindowS > 0 {
		opts.CatchupWindow = time.Duration(spec.CatchupWindowS) * time.Second
	}
	_, err := sc.Create(ctx, opts)
	return err
}

func (m *Manager) update(ctx context.Context, sc client.ScheduleClient, spec ScheduleSpec) error {
	h := sc.GetHandle(ctx, spec.ID)

	// Compare what Temporal holds with the desired spec. Update is called only
	// when they differ, so an unchanged schedule costs one Describe per pass.
	desc, err := h.Describe(ctx)
	if err != nil {
		return fmt.Errorf("describe: %w", err)
	}
	want, err := m.canonicalFromSpec(spec)
	if err != nil {
		return fmt.Errorf("canonical spec: %w", err)
	}
	have, err := m.canonicalFromDescription(desc, spec)
	if err != nil {
		return fmt.Errorf("canonical description: %w", err)
	}
	if want != have {
		err := h.Update(ctx, client.ScheduleUpdateOptions{
			DoUpdate: func(in client.ScheduleUpdateInput) (*client.ScheduleUpdate, error) {
				sched := in.Description.Schedule
				spc := m.scheduleSpecFor(spec)
				sched.Spec = &spc
				sched.Action = m.scheduleActionFor(spec)
				sched.Policy = m.policyFor(spec, sched.Policy)
				return &client.ScheduleUpdate{Schedule: &sched}, nil
			},
		})
		if err != nil {
			return fmt.Errorf("update: %w", err)
		}
	}

	// Pause state is reconciled only when it differs. A description without a
	// state (an unknown state) is acted on, as before.
	if desc.Schedule.State != nil && desc.Schedule.State.Paused == spec.Paused {
		return nil
	}
	if spec.Paused {
		if err := h.Pause(ctx, client.SchedulePauseOptions{Note: "reconciled: paused"}); err != nil {
			return fmt.Errorf("pause: %w", err)
		}
	} else {
		if err := h.Unpause(ctx, client.ScheduleUnpauseOptions{Note: "reconciled: unpaused"}); err != nil {
			return fmt.Errorf("unpause: %w", err)
		}
	}
	return nil
}

// Trigger fires a recurring schedule immediately ("run now") via the real
// Temporal schedule client — the materialization of control's
// `/v1/schedules/{id}/trigger` once a schedule exists in Temporal. Overlap
// defaults to SKIP so a manual trigger never collides with an in-flight run.
func (m *Manager) Trigger(ctx context.Context, scheduleID string) error {
	return m.triggerWith(ctx, m.tc.ScheduleClient(), scheduleID)
}

func (m *Manager) triggerWith(ctx context.Context, sc client.ScheduleClient, scheduleID string) error {
	h := sc.GetHandle(ctx, scheduleID)
	if err := h.Trigger(ctx, client.ScheduleTriggerOptions{
		Overlap: enumspb.SCHEDULE_OVERLAP_POLICY_SKIP,
	}); err != nil {
		return fmt.Errorf("trigger %s: %w", scheduleID, err)
	}
	return nil
}

// Backfill replays a schedule's actions across [start, end] via the real
// Temporal schedule client — the materialization of control's
// `/v1/schedules/{id}/backfill`. overlap is the policy the caller requested
// (WS15.S3b, stored by control in the request row's params.overlap_policy);
// empty keeps the old default ALLOW_ALL so the historical fires can run
// concurrently and the window fills quickly.
func (m *Manager) Backfill(ctx context.Context, scheduleID string, start, end time.Time, overlap string) error {
	return m.backfillWith(ctx, m.tc.ScheduleClient(), scheduleID, start, end, overlap)
}

// backfillOverlapPolicy maps a requested backfill overlap policy to Temporal.
// Empty is ALLOW_ALL (the backfill default); skip, allow and cancel use the same
// mapping as create/update (overlapPolicy), and the Temporal-style spellings
// buffer_one and cancel_other are accepted too. Anything else falls through to
// overlapPolicy, i.e. skip. The caller passes the value already trimmed and
// lower-cased (requests.go backfillOverlap).
func backfillOverlapPolicy(s string) enumspb.ScheduleOverlapPolicy {
	switch s {
	case "":
		return enumspb.SCHEDULE_OVERLAP_POLICY_ALLOW_ALL
	case "buffer_one":
		return enumspb.SCHEDULE_OVERLAP_POLICY_BUFFER_ONE
	case "cancel_other":
		return enumspb.SCHEDULE_OVERLAP_POLICY_CANCEL_OTHER
	default:
		return overlapPolicy(s)
	}
}

func (m *Manager) backfillWith(ctx context.Context, sc client.ScheduleClient, scheduleID string, start, end time.Time, overlap string) error {
	if !end.After(start) {
		return fmt.Errorf("backfill %s: end (%s) must be after start (%s)", scheduleID, end, start)
	}
	h := sc.GetHandle(ctx, scheduleID)
	if err := h.Backfill(ctx, client.ScheduleBackfillOptions{
		Backfill: []client.ScheduleBackfill{{
			Start:   start,
			End:     end,
			Overlap: backfillOverlapPolicy(overlap),
		}},
	}); err != nil {
		return fmt.Errorf("backfill %s: %w", scheduleID, err)
	}
	return nil
}
