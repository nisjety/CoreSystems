package quarrycontracts

import "time"

type EventType string

const (
	EvtRunStarted         EventType = "run_started"
	EvtRunPaused          EventType = "run_paused"
	EvtRunResumed         EventType = "run_resumed"
	EvtRunCancelled       EventType = "run_cancelled"
	EvtRunCompleted       EventType = "run_completed"
	EvtRunFailed          EventType = "run_failed"
	EvtPageQueued         EventType = "page_queued"
	EvtPageFetched        EventType = "page_fetched"
	EvtPageFailed         EventType = "page_failed"
	EvtPageBlocked        EventType = "page_blocked"
	EvtPageRetried        EventType = "page_retried"
	EvtPageEscalated      EventType = "page_escalated"
	EvtArtifactWritten    EventType = "artifact_written"
	EvtSnapshotCreated    EventType = "snapshot_created"
	EvtStoreRecordWritten EventType = "store_record_written"
	EvtLeaseAcquired      EventType = "lease_acquired"
	EvtLeaseReleased      EventType = "lease_released"
	EvtProfileRestored    EventType = "profile_restored"
	EvtProfileCaptured    EventType = "profile_captured"
	EvtChangeDetected     EventType = "change_detected"
	EvtChangeUnchanged    EventType = "change_unchanged"
	EvtScheduleFired      EventType = "schedule_fired"
	// EvtBrandingExtracted carries the output of branding_rendered::extract
	// (favicon, theme color, palette, logo candidate, font family, site
	// name, og:image) for the page indicated by payload.url. Emitted by
	// the runtime page pipeline after each successful HTML fetch.
	EvtBrandingExtracted EventType = "branding_extracted"
)

type Event struct {
	EventID        ID             `json:"event_id"`
	RunID          *ID            `json:"run_id,omitempty"`
	JobID          *ID            `json:"job_id,omitempty"`
	Type           EventType      `json:"type"`
	Timestamp      time.Time      `json:"ts"`
	Seq            uint64         `json:"seq"`
	Payload        map[string]any `json:"payload"`
	IdempotencyKey string         `json:"idempotency_key"`
}
