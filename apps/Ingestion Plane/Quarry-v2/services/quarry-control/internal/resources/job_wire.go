package resources

import (
	"time"

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
	JobID     string  `json:"job_id"`
	Kind      string  `json:"kind"`
	OrgID     string  `json:"org_id"`
	Status    string  `json:"status"`
	CreatedAt string  `json:"created_at"`
	RunID     *string `json:"run_id,omitempty"`
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
		JobID:     string(j.ID),
		Kind:      j.Kind,
		OrgID:     j.OrgID,
		Status:    j.Status,
		CreatedAt: time.UnixMilli(j.CreatedAt).UTC().Format(time.RFC3339Nano),
		RunID:     runID,
	}, true
}
