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
	// EvtStoreRecordFailed mirrors quarry_core::event::EventType::StoreRecordFailed
	// (Rust) — emitted when a Data Plane ingest attempt for a page fails (e.g. the
	// service-token mint for the run's org_id was refused, or documents-api-go
	// rejected the write), so job-history consumers can tell a real ingest
	// failure apart from a page that was never asked to ingest at all.
	EvtStoreRecordFailed EventType = "store_record_failed"
	EvtLeaseAcquired     EventType = "lease_acquired"
	EvtLeaseReleased     EventType = "lease_released"
	EvtProfileRestored   EventType = "profile_restored"
	EvtProfileCaptured   EventType = "profile_captured"
	EvtChangeDetected    EventType = "change_detected"
	EvtChangeUnchanged   EventType = "change_unchanged"
	EvtScheduleFired     EventType = "schedule_fired"
	// EvtBrandingExtracted carries the output of branding_rendered::extract
	// (favicon, theme color, palette, logo candidate, font family, site
	// name, og:image) for the page indicated by payload.url. Emitted by
	// the runtime page pipeline after each successful HTML fetch.
	EvtBrandingExtracted EventType = "branding_extracted"
	// EvtPageExtracted mirrors quarry_core::event::EventType::PageExtracted
	// (Rust). Emitted AFTER transform (page_fetched fires before it, with
	// only url/status/duration/content_type) and carries the page-level
	// extraction: title, title_source (html|model|host), a ~300-char plain
	// excerpt, summary (optional), word_count, lang, driver, content_type,
	// fingerprint. The orchestrator re-emits it from RunPageResult so
	// consumers polling /v1/jobs/{id}/events (verevon onboarding) see real
	// page names and text. Additive: consumers that only know page_fetched
	// keep working.
	EvtPageExtracted EventType = "page_extracted"
	// WS15.L2 (job control): a caller asked for a pause, resume or cancel. Recorded
	// by control when the request is written; the workflow acts on it later.
	EvtControlRequested EventType = "control_requested"
	// EvtRunTimedOut ends a job whose workflow ran past its execution timeout. Written
	// by the orchestrator's sweep with a deterministic event id.
	EvtRunTimedOut EventType = "run_timed_out"
	// EvtScheduleSkipped records a scheduled fire that did not start a job (overlap
	// policy or posture). Mirrors quarry_core::event::EventType (WS15.S4).
	EvtScheduleSkipped EventType = "schedule_skipped"
	// WS10.S4 browser trace events. Mirror quarry_core::event::EventType. All are
	// agent class: Quarry never publishes them on the Ingestion bus, and the
	// orchestrator does not persist them; they are listed so a Go reader can name
	// them. Payloads are allowlisted projections (no URL, path, query or typed text).
	// EvtBrowserSessionOpened: a browser session opened for an agent run.
	EvtBrowserSessionOpened EventType = "browser_session_opened"
	// EvtBrowserSessionClosed: a browser session closed.
	EvtBrowserSessionClosed EventType = "browser_session_closed"
	// EvtBrowserControlChanged: control of a session changed hands.
	EvtBrowserControlChanged EventType = "browser_control_changed"
	// EvtBrowserTabChanged: a tab was opened, focused or closed.
	EvtBrowserTabChanged EventType = "browser_tab_changed"
	// EvtStepReceiptRecorded: a step receipt was recorded.
	EvtStepReceiptRecorded EventType = "step_receipt_recorded"
	// EvtEgressDecided: an egress decision summary or block for a step.
	EvtEgressDecided EventType = "egress_decided"
	// EvtArtifactRecorded: an artifact recorded for a step, by id.
	EvtArtifactRecorded EventType = "artifact_recorded"
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
