package resources

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// TestToJobHistoryEvent_ForwardsOwnerOrgAndKind pins the exact fields
// quarry_core::job_history::JobHistoryEvent requires as non-optional
// (org_id, kind, stage, status) — omitting any of them is what produced
// the "missing field `org_id`" 500 on GET /v1/ingestions/evidence.
func TestToJobHistoryEvent_ForwardsOwnerOrgAndKind(t *testing.T) {
	t.Parallel()
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	owner := store.Job{OrgID: "org_a", Kind: "crawl"}
	ts := time.Date(2026, 7, 22, 12, 0, 0, 0, time.UTC)
	evt := quarrycontracts.Event{
		RunID:     &runID,
		Type:      quarrycontracts.EvtPageFetched,
		Seq:       3,
		Timestamp: ts,
		Payload:   map[string]any{"url": "https://coresystem.com"},
	}

	wire := toJobHistoryEvent(evt, owner)

	if wire.OrgID != "org_a" {
		t.Fatalf("org_id = %q, want org_a", wire.OrgID)
	}
	if wire.Kind != "crawl" {
		t.Fatalf("kind = %q, want crawl", wire.Kind)
	}
	if wire.RunID != string(runID) {
		t.Fatalf("run_id = %q, want %s", wire.RunID, runID)
	}
	if wire.Stage != "running" || wire.Status != "ok" {
		t.Fatalf("stage/status = %q/%q, want running/ok for page_fetched", wire.Stage, wire.Status)
	}
	if wire.Seq != 3 {
		t.Fatalf("seq = %d, want 3", wire.Seq)
	}
	if wire.Completed != 1 {
		t.Fatalf("completed = %d, want 1 for a fetched page", wire.Completed)
	}
	if wire.Timestamp != "2026-07-22T12:00:00Z" {
		t.Fatalf("timestamp = %q, want RFC3339Nano UTC", wire.Timestamp)
	}
}

// TestEventStageStatus_TerminalRunEvents pins the run-lifecycle mapping:
// a completed run must surface as (completed, ok) and a failed run as
// (failed, error) — these drive the terminal state a frontend renders.
func TestEventStageStatus_TerminalRunEvents(t *testing.T) {
	t.Parallel()
	stage, status := eventStageStatus(quarrycontracts.EvtRunCompleted)
	if stage != "completed" || status != "ok" {
		t.Fatalf("run_completed = %q/%q, want completed/ok", stage, status)
	}
	stage, status = eventStageStatus(quarrycontracts.EvtRunFailed)
	if stage != "failed" || status != "error" {
		t.Fatalf("run_failed = %q/%q, want failed/error", stage, status)
	}
}
