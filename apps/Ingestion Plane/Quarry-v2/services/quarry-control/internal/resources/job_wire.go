package resources

import (
	"encoding/json"
	"strings"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// validJobResourceKinds mirrors quarry-core::resources::JobResourceKind's
// valid values exactly (crates/quarry-core/src/resources.rs). A Job.Kind
// outside this set has no enum variant on the Rust side and would fail
// forward_list's JobSummary decode with "unknown variant" — toJobWire
// skips those rows rather than let one bad job crash the entire
// GET /v1/{kind}/jobs response.
var validJobResourceKinds = map[string]bool{
	"crawl":    true,
	"search":   true,
	"extract":  true,
	"research": true,
	"agent":    true,
	"batch":    true,
	"scrape":   true,
}

// jobWire is the quarry_core::resources::JobSummary wire shape GET
// /v1/{kind}/jobs must emit — job_id (NOT id), org_id, and created_at as
// RFC3339 (mirroring store.Job.MarshalJSON's convention). started_at,
// completed_at, and stats are intentionally absent: store.Job has no
// equivalent fields, and all three are `#[serde(default, ...)]` on
// JobSummary, so omitting them is a valid empty value, not a lie.
//
// org_id is store.Job's real org_id column (migration 010_jobs_org_id.sql)
// — MountJobsByKind's ListByKind call already filters rows to the
// edge-verified `?org_id`, so every row here already belongs to that org.
//
// RunID DOES have a store.Job equivalent (RunID, populated once the
// orchestrator dispatches the job) and is required: GET
// /v1/runs/:id/events only accepts a run id, never a job id, so any
// consumer of this list that wants a run's event history needs it here.
// Its previous omission meant verevon's Ingestions evidence panel always
// fell back to the job id and 400'd against quarry-edge.
type jobWire struct {
	JobID        string         `json:"job_id"`
	Kind         string         `json:"kind"`
	OrgID        string         `json:"org_id"`
	Status       string         `json:"status"`
	CreatedAt    string         `json:"created_at"`
	RunID        *string        `json:"run_id,omitempty"`
	Origin       jobWireOrigin  `json:"origin"`
	Phase        string         `json:"phase"`
	DeadlineAt   *string        `json:"deadline_at,omitempty"`
	DesiredState string         `json:"desired_state,omitempty"`
	Stats        map[string]any `json:"stats"`
	// Credits is the derived per-job credit estimate (job_credits.go), present
	// only for page-priced kinds and only while QUARRY_CONTROL__JOB_CREDITS is on.
	Credits *jobCredits `json:"credits,omitempty"`
}

// jobWireOrigin is a job's origin as the list and status views show it.
type jobWireOrigin struct {
	Type  string `json:"type"`
	Actor string `json:"actor,omitempty"`
	Ref   string `json:"ref,omitempty"`
}

// toJobWire projects a store.Job into the JobSummary wire shape. ok is
// false when the job's Kind has no JobResourceKind variant on the Rust
// side (validJobResourceKinds) — callers should skip such rows rather
// than forward them.
func toJobWire(j store.Job) (jobWire, bool) {
	if !validJobResourceKinds[j.Kind] {
		return jobWire{}, false
	}
	var runID *string
	if j.RunID != nil {
		s := string(*j.RunID)
		runID = &s
	}
	return jobWire{
		JobID:        string(j.ID),
		Kind:         j.Kind,
		OrgID:        j.OrgID,
		Status:       j.Status,
		CreatedAt:    time.UnixMilli(j.CreatedAt).UTC().Format(time.RFC3339Nano),
		RunID:        runID,
		Origin:       jobWireOrigin{Type: j.Origin.Type, Actor: j.Origin.Actor, Ref: j.Origin.Ref},
		Phase:        jobPhase(j.Status),
		DeadlineAt:   optRFC3339Millis(j.DeadlineAt),
		DesiredState: j.DesiredState,
		Stats:        jobStats(j, nil),
	}, true
}

// jobStats are the counters a list row shows: pages completed and failed, and
// the job's total (see jobTotal). counts is the job's event counts by type, nil
// when none were read.
func jobStats(j store.Job, counts map[quarrycontracts.EventType]int64) map[string]any {
	total, _ := jobTotal(j)
	return map[string]any{
		"completed": counts[quarrycontracts.EvtPageFetched],
		"failed":    counts[quarrycontracts.EvtPageFailed],
		"total":     total,
	}
}

// jobWires projects jobs into the list wire shape, with stats from one grouped
// event count for the whole page. Jobs whose kind has no JobResourceKind are
// skipped, as toJobWire does.
func jobWires(db store.DB, jobs []store.Job) []jobWire {
	ids := make([]quarrycontracts.ID, 0, len(jobs))
	for _, j := range jobs {
		ids = append(ids, j.ID)
	}
	counts := db.Events().CountsForJobs(ids)
	withCredits := jobCreditsEnabled()
	out := make([]jobWire, 0, len(jobs))
	for _, j := range jobs {
		wire, ok := toJobWire(j)
		if !ok {
			continue
		}
		wire.Stats = jobStats(j, counts[j.ID])
		if withCredits {
			wire.Credits = creditsFor(j.Kind, j.Status, counts[j.ID][quarrycontracts.EvtPageFetched])
		}
		out = append(out, wire)
	}
	return out
}

// jobCreateResponse is the POST /v1/jobs response: the stored job as every read shows it, plus, when
// the job carries a per-job webhook and the control key is set, the one-time signing secret under
// `webhook.secret`. The secret is derived, never stored, and appears in no list or get response.
type jobCreateResponse struct {
	store.Job
	// Webhook is set only on the response that creates the job.
	Webhook *jobCreateWebhook `json:"-"`
}

// jobCreateWebhook is the per-job webhook block of the create response.
type jobCreateWebhook struct {
	// URL and Events are what was stored, after normalisation.
	URL    string   `json:"url"`
	Events []string `json:"events"`
	// Secret is the whjob_ signing secret, shown once.
	Secret string `json:"secret"`
}

// MarshalJSON renders the job exactly as store.Job does and adds the webhook block. Without it
// the promoted store.Job.MarshalJSON would drop the extra field.
func (c jobCreateResponse) MarshalJSON() ([]byte, error) {
	base, err := json.Marshal(c.Job)
	if err != nil {
		return nil, err
	}
	if c.Webhook == nil {
		return base, nil
	}
	fields := map[string]json.RawMessage{}
	if err := json.Unmarshal(base, &fields); err != nil {
		return nil, err
	}
	hook, err := json.Marshal(c.Webhook)
	if err != nil {
		return nil, err
	}
	fields[JobWebhookParamKey] = hook
	return json.Marshal(fields)
}

// withoutWebhookSecret returns j with any `secret` key removed from params.webhook. The secret
// is never stored, and NormalizeJobWebhook refuses a request that names one, so this is a second
// guard: a read of a job never echoes a signing secret. The params map is copied, not edited.
func withoutWebhookSecret(j store.Job) store.Job {
	hook, ok := j.Params[JobWebhookParamKey].(map[string]any)
	if !ok {
		return j
	}
	if _, has := hook["secret"]; !has {
		return j
	}
	params := make(map[string]any, len(j.Params))
	for k, v := range j.Params {
		params[k] = v
	}
	cleaned := make(map[string]any, len(hook))
	for k, v := range hook {
		if k != "secret" {
			cleaned[k] = v
		}
	}
	params[JobWebhookParamKey] = cleaned
	j.Params = params
	return j
}

// jobParamsZdr reports whether the job params declare a zero-data-retention posture. The edge
// refuses a ZDR request that carries a webhook, so this is control's own check of the stored
// params: a truthy `zdr` (true, or "true"/"on"/"1"/"yes") counts.
func jobParamsZdr(params map[string]any) bool {
	switch v := params["zdr"].(type) {
	case bool:
		return v
	case string:
		switch strings.ToLower(strings.TrimSpace(v)) {
		case "true", "on", "1", "yes":
			return true
		}
	}
	return false
}
