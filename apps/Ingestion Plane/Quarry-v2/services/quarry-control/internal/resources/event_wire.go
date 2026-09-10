package resources

import (
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// jobHistoryEventWire is the quarry_core::job_history::JobHistoryEvent wire
// shape GET /v1/runs/:id/events must emit. quarry-edge's fallback path
// (crates/quarry-edge/src/resource_routes.rs::list_run_events) deserializes
// every item strictly into that struct — org_id, kind, stage, and status
// are all required fields with no #[serde(default)], so omitting any of
// them 500s the whole page ("missing field `org_id`") rather than losing
// just one row. quarrycontracts.Event (event.go), which this endpoint used
// to serialize directly, predates JobHistoryEvent's cycle-24 canonicalization
// and has neither org_id nor kind/stage/status — only a flat EventType.
type jobHistoryEventWire struct {
	RunID      string         `json:"run_id"`
	OrgID      string         `json:"org_id"`
	Kind       string         `json:"kind"`
	Stage      string         `json:"stage"`
	Status     string         `json:"status"`
	Seq        uint64         `json:"seq"`
	Completed  uint32         `json:"completed"`
	Discovered uint32         `json:"discovered"`
	Queued     uint32         `json:"queued"`
	Retries    uint32         `json:"retries"`
	Blocks     uint32         `json:"blocks"`
	Timestamp  string         `json:"timestamp"`
	Payload    map[string]any `json:"payload"`
}

// eventStageStatus maps quarrycontracts.EventType (the flat, per-occurrence
// event log quarry-control actually stores) onto JobHistoryEvent's
// (stage, status) pair. There's no stored per-run accumulator here — each
// event carries only its own occurrence, so completed/discovered/queued/
// retries/blocks below reflect this ONE event, not a running total; a
// consumer wanting cumulative counts must sum across the page itself.
func eventStageStatus(t quarrycontracts.EventType) (stage string, status string) {
	switch t {
	case quarrycontracts.EvtRunStarted:
		return "starting", "ok"
	case quarrycontracts.EvtRunPaused, quarrycontracts.EvtRunResumed:
		return "running", "ok"
	case quarrycontracts.EvtRunCancelled:
		return "cancelled", "ok"
	case quarrycontracts.EvtRunCompleted:
		return "completed", "ok"
	case quarrycontracts.EvtRunFailed:
		return "failed", "error"
	case quarrycontracts.EvtPageQueued:
		return "running", "ok"
	case quarrycontracts.EvtPageFetched:
		return "running", "ok"
	case quarrycontracts.EvtPageFailed:
		return "running", "error"
	case quarrycontracts.EvtPageBlocked, quarrycontracts.EvtPageRetried, quarrycontracts.EvtPageEscalated:
		return "running", "warn"
	case quarrycontracts.EvtArtifactWritten, quarrycontracts.EvtStoreRecordWritten:
		return "finalizing", "ok"
	case quarrycontracts.EvtStoreRecordFailed:
		return "finalizing", "error"
	case quarrycontracts.EvtLeaseAcquired, quarrycontracts.EvtProfileRestored, quarrycontracts.EvtProfileCaptured:
		return "starting", "ok"
	case quarrycontracts.EvtLeaseReleased:
		return "running", "ok"
	case quarrycontracts.EvtSnapshotCreated, quarrycontracts.EvtBrandingExtracted, quarrycontracts.EvtPageExtracted:
		return "running", "ok"
	case quarrycontracts.EvtChangeDetected, quarrycontracts.EvtChangeUnchanged, quarrycontracts.EvtScheduleFired:
		return "running", "ok"
	default:
		return "running", "ok"
	}
}

// eventCounters returns the single-event delta this EventType represents,
// so a frontend summing a page's events gets a sensible progress readout
// even though nothing here is a running total (see eventStageStatus).
func eventCounters(t quarrycontracts.EventType) (completed, discovered, queued, retries, blocks uint32) {
	switch t {
	case quarrycontracts.EvtPageFetched, quarrycontracts.EvtArtifactWritten, quarrycontracts.EvtStoreRecordWritten:
		return 1, 0, 0, 0, 0
	case quarrycontracts.EvtPageQueued:
		return 0, 1, 1, 0, 0
	case quarrycontracts.EvtPageRetried, quarrycontracts.EvtPageEscalated:
		return 0, 0, 0, 1, 0
	case quarrycontracts.EvtPageBlocked:
		return 0, 0, 0, 0, 1
	default:
		return 0, 0, 0, 0, 0
	}
}

// toJobHistoryEvent projects a durable quarrycontracts.Event onto the
// JobHistoryEvent wire shape, using owner (the job GetByRunID resolved) for
// the org_id/kind every event in the run shares.
func toJobHistoryEvent(evt quarrycontracts.Event, owner store.Job) jobHistoryEventWire {
	stage, status := eventStageStatus(evt.Type)
	completed, discovered, queued, retries, blocks := eventCounters(evt.Type)
	runID := ""
	if evt.RunID != nil {
		runID = string(*evt.RunID)
	}
	return jobHistoryEventWire{
		RunID:      runID,
		OrgID:      owner.OrgID,
		Kind:       owner.Kind,
		Stage:      stage,
		Status:     status,
		Seq:        evt.Seq,
		Completed:  completed,
		Discovered: discovered,
		Queued:     queued,
		Retries:    retries,
		Blocks:     blocks,
		Timestamp:  evt.Timestamp.UTC().Format(time.RFC3339Nano),
		Payload:    evt.Payload,
	}
}
