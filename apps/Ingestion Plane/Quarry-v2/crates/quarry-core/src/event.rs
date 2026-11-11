//! Event envelope (SSE + webhook + durable history). Mirrors CONTRACTS §3.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::contracts::ElementRect;
use crate::ids::kinds;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    RunStarted,
    RunPaused,
    RunResumed,
    RunCancelled,
    RunCompleted,
    RunFailed,
    PageQueued,
    PageFetched,
    PageFailed,
    PageBlocked,
    PageRetried,
    PageEscalated,
    ArtifactWritten,
    SnapshotCreated,
    StoreRecordWritten,
    /// Emitted when a Data Plane ingest attempt (`DataPlaneIngest::ingest`)
    /// returns an error — e.g. the service-token mint for the run's `org_id`
    /// was refused by Auth Core, or documents-api-go rejected the write.
    /// Previously this failure was ONLY `tracing::warn!`'d as "non-fatal" and
    /// never surfaced on the job's event stream, so a crawl could show
    /// "completed" while zero pages were ever durably persisted. Consumers
    /// (the Home dashboard's crawl-status widget) must treat a job as
    /// actually indexed only when ingest was requested AND this event was
    /// never seen (or `StoreRecordWritten` was seen instead).
    StoreRecordFailed,
    LeaseAcquired,
    LeaseReleased,
    ProfileRestored,
    ProfileCaptured,
    ChangeDetected,
    ChangeUnchanged,
    ScheduleFired,
    AgentStarted,
    ActionStarted,
    ActionCompleted,
    ActionFailed,
    ObservationReady,
    AgentCompleted,
    AgentFailed,
    /// A live agent run was released by the sweeper (idle, over its lifetime, or a
    /// takeover nobody came back to). Payload: `reason`, `idle_s`, `age_s`; no URL.
    /// A non-ZDR run's checkpoint stays `active`, so it can be resumed.
    AgentExpired,
    /// Cycle 19 / cluster #19: emitted by `/v1/search` route on every
    /// query. Reserved for future `autocomplete-core` Sonic ingest into
    /// the `queries` bucket. Payload includes `query` + `provider` +
    /// `result_count`.
    SearchIssued,
    /// Cycle 19 / cluster #19: emitted by `CrawlFrontier::seed` when a
    /// new host is added to scope. Reserved for `autocomplete-core`
    /// Sonic ingest into the `hosts` bucket. Payload includes `host` +
    /// `seed_url`.
    HostDiscovered,
    /// Emitted by the page pipeline after each successful HTML fetch
    /// with the output of `quarry_transform::branding_rendered::extract`
    /// (favicon, theme color, palette, logo candidate, font family,
    /// site name, og:image). Lets downstream consumers — most notably
    /// the verevon onboarding wizard — render real brand colors and the
    /// detected logo immediately, instead of waiting for downstream
    /// model-plane interpretation.
    BrandingExtracted,
    /// Emitted by the page pipeline once per successfully transformed HTML
    /// page, AFTER readability/markdown/metadata have run. Carries the
    /// page-level extraction the earlier `page_fetched` (pre-transform, only
    /// `{url,status,duration_ms,content_type}`) and `artifact_written`
    /// (artifact ids + byte counts) never could: `title`, `title_source`
    /// (`html` | `model` | `host`), a whitespace-collapsed ~300-char
    /// `excerpt` of the markdown, `word_count`, `lang`, the `driver` that
    /// served the fetch, and an optional one-sentence model `summary`.
    /// Additive and backward compatible — consumers that only know
    /// `page_fetched` keep working; the verevon onboarding wizard maps this
    /// into its snippet cards so a crawl shows real titles and text instead
    /// of `N utdrag samlet` over empty excerpts.
    PageExtracted,
    /// WS6.B: an agent run reached a budget bound while enforcement is `on`
    /// (once per segment of the run). Payload: `dimension`, `limit`, `spent`,
    /// `unit`, `estimated`, `limit_source`. User-facing (X-12): the run's SSE
    /// stream sends it as `agent.budget_exhausted`; the run trace still needs a
    /// projection row (WS10).
    AgentBudgetExhausted,
    /// WS15.L2: a caller asked for a pause, resume or cancel of a job. Written by
    /// control when the request is recorded; the workflow acts on it later.
    /// Payload: `action`, `actor`, `actor_type`.
    ControlRequested,
    /// WS15.L2: a job's workflow ran past its execution timeout. Payload:
    /// `timeout_type`, `limit_s`.
    RunTimedOut,
    /// WS15.S4 mirror: a scheduled fire that did not start a job. Payload: `reason`.
    ScheduleSkipped,
    /// WS8.F: an agent attached to a live run and was given a tab. Payload: `attach_id`,
    /// `tab_id`. No URL and no title.
    AgentAttached,
    /// WS8.F: an agent attachment ended (released, aborted, revoked, idle or session ended).
    /// Payload: `attach_id`, `disposition`. No URL and no title.
    AgentDetached,
    /// WS10.S4: a browser session opened for an agent run. Agent class (never on the
    /// Ingestion bus); SSE name `browser.session.opened`. Emitted from Q2-WS10.S4d.
    BrowserSessionOpened,
    /// WS10.S4: a browser session closed. Agent class; SSE `browser.session.closed`.
    BrowserSessionClosed,
    /// WS10.S4: control of the session changed hands (agent, user, paused). Agent
    /// class; SSE `browser.control`.
    BrowserControlChanged,
    /// WS10.S4: a tab was opened, focused or closed. Agent class; SSE `browser.tab`.
    BrowserTabChanged,
    /// WS10.S4: a step receipt was recorded (action carried as its serde tag only).
    /// Agent class; SSE `browser.receipt`.
    StepReceiptRecorded,
    /// WS10.S4: an egress decision summary or an individual block for a step. Agent
    /// class; SSE `browser.egress`.
    EgressDecided,
    /// WS10.S4: an artifact (screenshot, visual observation, evidence delta) recorded
    /// for a step, by id. Agent class; SSE `browser.artifact`.
    ArtifactRecorded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: kinds::EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<kinds::RunKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<kinds::JobKind>,
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub ts: DateTime<Utc>,
    pub seq: u64,
    pub payload: serde_json::Value,
    pub idempotency_key: String,
    /// WS10.S4: run trace stamp (origin, agent, request id, causation). Labels only,
    /// never authority. Left out of the JSON when absent, so events without a trace
    /// keep their old shape; an event without the key reads back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<crate::trace::EventTrace>,
}

/// Payload of an `action.started` event (`EventType::ActionStarted`). `step` and
/// `action` are the shape every consumer reads. `rect` is additive: the target
/// element's box in CSS pixels, when the browser reported one before the action
/// ran. It is for presentation only; no resolver, action or receipt reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionStartedPayload {
    pub step: u32,
    /// The action as the event may carry it. `None` serializes as `null`.
    #[serde(default)]
    pub action: Option<serde_json::Value>,
    /// Left out of the JSON when absent, so an event without a box has the same
    /// payload as before this field existed. A payload without the key reads
    /// back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<ElementRect>,
    /// Correlation for an admitted attached-agent step, omitted for other lanes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

impl ActionStartedPayload {
    /// The JSON payload for the event. Every field is a plain value, so this
    /// cannot fail; should a derive ever break, the event carries `null` rather
    /// than panicking on the emit path.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod action_started_payload_tests {
    use super::*;
    use serde_json::json;

    fn payload(rect: Option<ElementRect>) -> ActionStartedPayload {
        ActionStartedPayload {
            step: 3,
            action: Some(json!({ "kind": "click_ref" })),
            rect,
            attach_id: None,
            tab_id: None,
        }
    }

    #[test]
    fn without_a_box_the_payload_is_the_same_as_before() {
        let value = payload(None).to_value();
        assert_eq!(
            value,
            json!({ "step": 3, "action": { "kind": "click_ref" } })
        );
        assert!(value.get("rect").is_none());
    }

    #[test]
    fn a_box_is_added_beside_step_and_action() {
        let rect = ElementRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 30.0,
        };
        let value = payload(Some(rect)).to_value();
        assert_eq!(value["step"], json!(3));
        assert_eq!(
            value["rect"],
            json!({ "x": 10.0, "y": 20.0, "width": 100.0, "height": 30.0 })
        );
    }

    #[test]
    fn a_missing_action_stays_null_on_the_wire() {
        let value = ActionStartedPayload {
            step: 1,
            action: None,
            rect: None,
            attach_id: None,
            tab_id: None,
        }
        .to_value();
        assert_eq!(value, json!({ "step": 1, "action": null }));
    }

    #[test]
    fn a_payload_written_before_the_rect_reads_back_with_none() {
        let legacy = json!({ "step": 2, "action": { "kind": "press" } });
        let read: ActionStartedPayload = serde_json::from_value(legacy).unwrap();
        assert_eq!(read.step, 2);
        assert_eq!(read.rect, None);
        assert_eq!(read.attach_id, None);
        assert_eq!(read.tab_id, None);
    }

    #[test]
    fn a_payload_with_a_box_round_trips() {
        let original = payload(Some(ElementRect {
            x: 0.5,
            y: 1.5,
            width: 2.0,
            height: 3.0,
        }));
        let read: ActionStartedPayload = serde_json::from_value(original.to_value()).unwrap();
        assert_eq!(read, original);
    }

    #[test]
    fn attached_agent_identity_round_trips_with_target() {
        let mut original = payload(Some(ElementRect { x: 1.0, y: 2.0, width: 4.0, height: 6.0 }));
        original.attach_id = Some("attach-1".into());
        original.tab_id = Some("tab-1".into());
        let value = original.to_value();
        assert_eq!(value["attach_id"], "attach-1");
        assert_eq!(value["tab_id"], "tab-1");
        assert_eq!(serde_json::from_value::<ActionStartedPayload>(value).unwrap(), original);
    }

    #[test]
    fn attached_event_envelope_exposes_session_run_and_time_for_live_correlation() {
        let session_run = crate::ids::Id::new();
        let mut started = payload(Some(ElementRect { x: 1.0, y: 2.0, width: 4.0, height: 6.0 }));
        started.attach_id = Some("attach-1".into());
        started.tab_id = Some("tab-1".into());
        let event = Event { event_id: crate::ids::Id::new(), run_id: Some(session_run.clone()), job_id: None,
            event_type: EventType::ActionStarted, ts: chrono::Utc::now(), seq: 1,
            payload: started.to_value(), idempotency_key: "event-1".into(), trace: None };
        let wire = serde_json::to_value(event).unwrap();
        assert_eq!(wire["run_id"], session_run.to_string());
        assert!(wire["ts"].as_str().is_some());
        assert_eq!(wire["payload"]["attach_id"], "attach-1");
        assert_eq!(wire["payload"]["tab_id"], "tab-1");
    }
}
