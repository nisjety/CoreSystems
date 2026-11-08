//! Live job-event tail (WS15.L3): `GET /v1/{jobs,crawl,batch}/:id/events?follow=true`.
//!
//! The legacy drain (no `follow`) is unchanged in shape: every event the job has,
//! then a terminal `done`. With `follow=true` and `QUARRY_EDGE__JOB_STREAM_FOLLOW=1`
//! the edge keeps the stream open and tails control's event log:
//!
//! - the first frame is `event: snapshot` with the job's status document and
//!   `last_seq`; its `id` is the cursor the stream starts from;
//! - every event frame carries `id: <seq>` and `event: <type>`, so a client that
//!   reconnects with `Last-Event-ID` gets exactly the events after it, once;
//! - the stream ends with `data: done` after a terminal event, or when the job
//!   record is terminal and the log has nothing newer (re-read every 5 s);
//! - after 30 minutes the stream ends with `event: reconnect` (graceful, the
//!   client resumes with `Last-Event-ID`);
//! - a failed control read ends the stream with `event: error` and a typed code.
//!
//! Each open stream holds a permit from a process-global limiter (per org and in
//! total). The limiter lives in this module, not in `AppState`, so the caps are
//! per edge instance: with N replicas the effective cap is N times the number.
//! Nothing here persists: the frames are read from control and forwarded.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::response::sse::Event;
use futures::Stream;
use serde_json::{json, Value};

use quarry_core::error::{ErrorCode, QuarryError};

use crate::control_client;
use crate::state::AppState;

/// Feature flag: `1` enables `follow=true`. Off (the default) makes `follow`
/// a no-op and keeps the legacy drain byte-compatible.
pub const ENV_FOLLOW: &str = "QUARRY_EDGE__JOB_STREAM_FOLLOW";
/// Open streams allowed per org on this instance.
pub const ENV_MAX_PER_ORG: &str = "QUARRY_EDGE__JOB_STREAM_MAX_PER_ORG";
/// Open streams allowed on this instance.
pub const ENV_MAX: &str = "QUARRY_EDGE__JOB_STREAM_MAX";
/// Age after which a stream ends with `event: reconnect`.
pub const ENV_MAX_AGE_S: &str = "QUARRY_EDGE__JOB_STREAM_MAX_AGE_S";

pub const DEFAULT_MAX_PER_ORG: usize = 20;
pub const DEFAULT_MAX: usize = 500;
pub const DEFAULT_MAX_AGE_S: u64 = 1800;

/// Events fetched from control per read.
pub const PAGE_LIMIT: u32 = 500;
/// How often the job record is re-read while the log is quiet.
const RECORD_RECHECK: Duration = Duration::from_secs(5);
/// SSE keep-alive comment interval (applied by the caller).
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// Reconnect delay the client is told to use.
const RETRY_MS: u64 = 3000;

/// Event types after which a job emits nothing more.
pub const TERMINAL_EVENTS: &[&str] = &["run_completed", "run_failed", "run_cancelled", "run_timed_out"];

/// Whether `follow=true` is honoured on this instance.
pub fn follow_enabled() -> bool {
    matches!(std::env::var(ENV_FOLLOW).ok().as_deref().map(str::trim), Some("1"))
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

/// The maximum age of one stream before it asks the client to reconnect.
pub fn max_age() -> Duration {
    let secs = std::env::var(ENV_MAX_AGE_S)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_AGE_S);
    Duration::from_secs(secs)
}

/// Whether a job status is one the job does not leave.
pub fn is_terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "timed_out" | "skipped")
}

// ---------------------------------------------------------------------------
// Zero Data Retention floor (D2-057)
// ---------------------------------------------------------------------------

/// `details.reason` of the 403 a caller under the token's ZDR floor gets for
/// `follow=true`: the events of a job carry page URLs, titles and excerpts.
pub const ZDR_FOLLOW_REASON: &str = "job_event_urls";

/// The refusal of a live tail for a ZDR caller. It is decided before control is
/// asked anything and before a stream permit is taken. No URL, host or title.
pub fn zdr_follow_refusal() -> QuarryError {
    QuarryError::new(
        ErrorCode::Forbidden,
        "the live job event stream is not available under zero data retention",
    )
    .with_details(json!({ "reason": ZDR_FOLLOW_REASON }))
}

/// Payload keys whose string value is a closed word or a short code, never page
/// content. Every other string in a ZDR caller's payload is dropped.
const ZDR_KEPT_STRING_KEYS: &[&str] = &[
    "status",
    "reason",
    "action",
    "actor_type",
    "timeout_type",
    "disposition",
    "content_type",
    "driver",
    "kind",
    "phase",
    "code",
    "dimension",
    "unit",
    "limit_source",
    "lang",
    "title_source",
];

/// The envelope fields of an event that are not caller content.
const ZDR_KEPT_ENVELOPE_KEYS: &[&str] = &["event_id", "run_id", "job_id", "type", "ts", "seq"];

/// One event as a ZDR caller may read it from the legacy drain: the envelope
/// without its idempotency key (a change event's key embeds the final URL), and a
/// payload cut down to numbers, booleans and the closed string fields of
/// [`ZDR_KEPT_STRING_KEYS`]. A kept string that still holds a URL is dropped too.
/// A URL, title, excerpt, summary, host, query or branding value never survives,
/// whatever event type or key carries it.
pub fn redact_event_for_zdr(event: Value) -> Value {
    let Value::Object(map) = event else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        if ZDR_KEPT_ENVELOPE_KEYS.contains(&key.as_str()) {
            out.insert(key, value);
        } else if key == "payload" {
            out.insert(key, redact_payload_for_zdr(value));
        }
    }
    Value::Object(out)
}

/// The payload half of [`redact_event_for_zdr`], for the run event route too
/// (`GET /v1/runs/:id/events` serves the same events with their payload). A null
/// payload stays null; any other non-object becomes `{}`.
pub fn redact_payload_for_zdr(payload: Value) -> Value {
    if payload.is_null() {
        return Value::Null;
    }
    let Value::Object(map) = payload else {
        return json!({});
    };
    let kept = map
        .into_iter()
        .filter(|(key, value)| match value {
            Value::Number(_) | Value::Bool(_) => true,
            Value::String(text) => {
                ZDR_KEPT_STRING_KEYS.contains(&key.as_str()) && !text.contains("://")
            }
            _ => false,
        })
        .collect::<serde_json::Map<String, Value>>();
    Value::Object(kept)
}

// ---------------------------------------------------------------------------
// Permits
// ---------------------------------------------------------------------------

#[derive(Default)]
struct OpenStreams {
    total: usize,
    per_org: HashMap<String, usize>,
}

static OPEN: LazyLock<Mutex<OpenStreams>> = LazyLock::new(|| Mutex::new(OpenStreams::default()));

/// One open stream of an org. Dropping it (the client went away, the stream
/// ended, or the handler failed) releases the slot.
#[derive(Debug)]
pub struct StreamPermit {
    org: String,
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        let mut open = OPEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        open.total = open.total.saturating_sub(1);
        let remove = match open.per_org.get_mut(&self.org) {
            Some(count) => {
                *count = count.saturating_sub(1);
                *count == 0
            }
            None => false,
        };
        if remove {
            open.per_org.remove(&self.org);
        }
    }
}

/// Takes a permit for one more stream of `org`, or `None` over either cap.
pub fn acquire(org: &str) -> Option<StreamPermit> {
    let per_org_cap = env_usize(ENV_MAX_PER_ORG, DEFAULT_MAX_PER_ORG);
    let total_cap = env_usize(ENV_MAX, DEFAULT_MAX);
    let mut open = OPEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mine = open.per_org.get(org).copied().unwrap_or(0);
    if open.total >= total_cap || mine >= per_org_cap {
        return None;
    }
    open.total += 1;
    *open.per_org.entry(org.to_string()).or_insert(0) += 1;
    Some(StreamPermit { org: org.to_string() })
}

/// The number of open streams of `org` on this instance.
pub fn open_for_org(org: &str) -> usize {
    let open = OPEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    open.per_org.get(org).copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Pure decisions (unit tested)
// ---------------------------------------------------------------------------

/// Which history a stream starts from when the client sends no cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replay {
    /// Every event the job has (the default).
    History,
    /// Only events after the snapshot (`replay=none`).
    Tail,
}

/// Parses `replay`: absent or `all` is History, `none` is Tail, anything else is refused.
pub fn parse_replay(raw: Option<&str>) -> Option<Replay> {
    match raw.map(str::trim) {
        None | Some("") | Some("all") => Some(Replay::History),
        Some("none") => Some(Replay::Tail),
        Some(_) => None,
    }
}

/// The seq the stream starts after. `Last-Event-ID` wins over `after_seq`. With
/// neither, `replay=none` starts at the job's current last seq.
pub fn start_cursor(last_event_id: Option<u64>, after_seq: Option<u64>, replay: Replay, last_seq: u64) -> u64 {
    match (last_event_id, after_seq) {
        (Some(id), _) => id,
        (None, Some(after)) => after,
        (None, None) => match replay {
            Replay::History => 0,
            Replay::Tail => last_seq,
        },
    }
}

/// Time between two reads of the event log: 500 ms while events flow, 2 s while
/// idle, and 5 s once the log has been idle for a minute.
pub fn poll_interval(got_events: bool, idle_for: Duration) -> Duration {
    if got_events {
        Duration::from_millis(500)
    } else if idle_for < Duration::from_secs(60) {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(5)
    }
}

/// The first frame's data: the job's status document as the stream sees it.
/// Built from control's job summary; the caller has already checked the org,
/// the id and the kind.
pub fn snapshot(job_id: &str, summary: &Value, cursor: u64) -> Value {
    let job = summary.get("job").unwrap_or(&Value::Null);
    let status = job.get("status").cloned().unwrap_or(Value::Null);
    let terminal = status.as_str().map(is_terminal_status).unwrap_or(false);
    let desired = job
        .get("desired_state")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let desired_seq = job.get("desired_seq").and_then(Value::as_i64).unwrap_or(0);
    let applied_seq = job.get("applied_seq").and_then(Value::as_i64).unwrap_or(0);
    json!({
        "job_id": job_id,
        "kind": job.get("kind").cloned().unwrap_or(Value::Null),
        "status": status,
        "phase": summary.get("phase").cloned().unwrap_or(Value::Null),
        "terminal": terminal,
        "counts": summary.get("counts").cloned().unwrap_or_else(|| json!({})),
        "last_seq": summary.get("last_seq").and_then(Value::as_u64).unwrap_or(0),
        "cursor": cursor,
        "deadline_at": summary
            .get("deadline_at")
            .or_else(|| job.get("deadline_at"))
            .cloned()
            .unwrap_or(Value::Null),
        "origin": job.get("origin").cloned().unwrap_or(Value::Null),
        "control": { "desired": desired, "pending": desired_seq > applied_seq },
    })
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

/// One page of events after `after`, in seq order, from control's job log.
async fn fetch_page(state: &AppState, org_id: &str, job_id: &str, after: u64) -> Result<Vec<Value>, QuarryError> {
    let raw = control_client::get_json(
        state,
        &format!("/v1/jobs/{job_id}/events"),
        org_id,
        &[
            ("after_seq", after.to_string()),
            ("limit", PAGE_LIMIT.to_string()),
        ],
    )
    .await?;
    Ok(match raw {
        Value::Array(items) => items,
        _ => Vec::new(),
    })
}

/// Control's job summary, re-read to learn whether the job is over.
async fn load_summary(state: &AppState, org_id: &str, job_id: &str) -> Result<Value, QuarryError> {
    control_client::get_json(state, &format!("/v1/jobs/{job_id}/summary"), org_id, &[]).await
}

/// Tails one job's events from `cursor`. The first frame is `snapshot`, built by
/// the caller with [`snapshot`]. The permit is held for the life of the stream.
pub fn follow(
    state: AppState,
    org_id: String,
    job_id: String,
    cursor: u64,
    snapshot: Value,
    permit: StreamPermit,
) -> impl Stream<Item = Result<Event, Infallible>> + Send + 'static {
    let max_age = max_age();
    async_stream::stream! {
        // Held for the life of the stream; dropping the stream releases it.
        let _permit = permit;
        let started = Instant::now();
        let mut cursor = cursor;
        let mut idle_since = Instant::now();
        let mut last_recheck: Option<Instant> = None;
        let mut close_with_done = true;

        yield Ok(Event::default().retry(Duration::from_millis(RETRY_MS)));
        if let Ok(frame) = Event::default()
            .id(cursor.to_string())
            .event("snapshot")
            .json_data(&snapshot)
        {
            yield Ok(frame);
        }

        loop {
            if started.elapsed() >= max_age {
                if let Ok(frame) = Event::default()
                    .event("reconnect")
                    .json_data(json!({ "reason": "max_age", "cursor": cursor }))
                {
                    yield Ok(frame);
                }
                close_with_done = false;
                break;
            }

            let page = match fetch_page(&state, &org_id, &job_id, cursor).await {
                Ok(page) => page,
                Err(err) => {
                    if let Ok(frame) = Event::default()
                        .event("error")
                        .json_data(json!({ "code": err.code.wire_name() }))
                    {
                        yield Ok(frame);
                    }
                    // A read failure is not the end of the job: no `done`.
                    close_with_done = false;
                    break;
                }
            };

            let got = !page.is_empty();
            let mut terminal_seen = false;
            for event in page {
                let Some(seq) = event.get("seq").and_then(Value::as_u64) else {
                    continue;
                };
                if seq <= cursor {
                    continue;
                }
                cursor = seq;
                let name = event
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("event")
                    .to_owned();
                if TERMINAL_EVENTS.contains(&name.as_str()) {
                    terminal_seen = true;
                }
                if let Ok(frame) = Event::default().id(seq.to_string()).event(name).json_data(&event) {
                    yield Ok(frame);
                }
            }

            if got {
                idle_since = Instant::now();
            }
            if terminal_seen {
                break;
            }
            if !got && last_recheck.map_or(true, |at| at.elapsed() >= RECORD_RECHECK) {
                last_recheck = Some(Instant::now());
                match load_summary(&state, &org_id, &job_id).await {
                    Ok(summary) => {
                        let status = summary
                            .get("job")
                            .and_then(|job| job.get("status"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if is_terminal_status(status) {
                            // Caught up (the last page was empty) and the job is over.
                            break;
                        }
                    }
                    Err(err) => {
                        if let Ok(frame) = Event::default()
                            .event("error")
                            .json_data(json!({ "code": err.code.wire_name() }))
                        {
                            yield Ok(frame);
                        }
                        close_with_done = false;
                        break;
                    }
                }
            }

            tokio::time::sleep(poll_interval(got, idle_since.elapsed())).await;
        }

        if close_with_done {
            // Legacy terminator, so a client written for the drain still sees its end.
            yield Ok(Event::default().data("done"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_parses_the_two_values_and_refuses_the_rest() {
        assert_eq!(parse_replay(None), Some(Replay::History));
        assert_eq!(parse_replay(Some("all")), Some(Replay::History));
        assert_eq!(parse_replay(Some("none")), Some(Replay::Tail));
        assert_eq!(parse_replay(Some("some")), None);
    }

    #[test]
    fn last_event_id_wins_over_after_seq() {
        assert_eq!(start_cursor(Some(7), Some(2), Replay::History, 9), 7);
        assert_eq!(start_cursor(None, Some(2), Replay::History, 9), 2);
        assert_eq!(start_cursor(None, None, Replay::History, 9), 0);
        assert_eq!(start_cursor(None, None, Replay::Tail, 9), 9);
    }

    #[test]
    fn poll_interval_backs_off_from_flowing_to_idle() {
        assert_eq!(poll_interval(true, Duration::ZERO), Duration::from_millis(500));
        assert_eq!(poll_interval(false, Duration::from_secs(1)), Duration::from_secs(2));
        assert_eq!(poll_interval(false, Duration::from_secs(61)), Duration::from_secs(5));
    }

    #[test]
    fn terminal_statuses_are_the_five_the_job_does_not_leave() {
        for status in ["completed", "failed", "cancelled", "timed_out", "skipped"] {
            assert!(is_terminal_status(status), "{status}");
        }
        assert!(!is_terminal_status("running"));
        assert!(!is_terminal_status("paused"));
    }

    #[test]
    fn permits_are_capped_per_org_and_released_on_drop() {
        let org = "org_job_stream_unit_cap";
        let mut held: Vec<StreamPermit> = Vec::new();
        for _ in 0..DEFAULT_MAX_PER_ORG {
            held.push(acquire(org).expect("under the per-org cap"));
        }
        assert!(acquire(org).is_none(), "the next stream of the org is refused");
        assert_eq!(open_for_org(org), DEFAULT_MAX_PER_ORG);
        held.pop();
        assert!(acquire(org).is_some(), "a dropped permit frees its slot");
    }

    #[test]
    fn zdr_redaction_keeps_the_envelope_and_counts_and_drops_every_page_string() {
        let event = json!({
            "event_id": "evt_1",
            "type": "page_extracted",
            "seq": 7,
            "ts": "2026-10-23T10:00:00Z",
            "idempotency_key": "run_x:https://secret.test/a:changed",
            "payload": {
                "url": "https://secret.test/a",
                "title": "Secret title",
                "excerpt": "Secret words",
                "summary": "Secret summary",
                "host": "secret.test",
                "seed_url": "https://secret.test/",
                "query": "secret query",
                "branding": { "logo": "https://secret.test/logo.png" },
                "urls": ["https://secret.test/b"],
                "status": 200,
                "word_count": 120,
                "pages_visited": 4,
                "ok": true,
                "driver": "static",
                "reason": "https://secret.test/leaked-in-a-kept-key"
            }
        });
        let out = redact_event_for_zdr(event);
        let text = out.to_string();
        assert!(!text.contains("secret"), "{text}");
        assert!(out.get("idempotency_key").is_none());
        assert_eq!(out["event_id"], "evt_1");
        assert_eq!(out["type"], "page_extracted");
        assert_eq!(out["seq"], 7);
        assert_eq!(out["payload"]["status"], 200);
        assert_eq!(out["payload"]["word_count"], 120);
        assert_eq!(out["payload"]["pages_visited"], 4);
        assert_eq!(out["payload"]["ok"], true);
        assert_eq!(out["payload"]["driver"], "static");
    }

    #[test]
    fn zdr_follow_refusal_is_a_typed_403_without_caller_data() {
        let err = zdr_follow_refusal();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert_eq!(err.details.as_ref().unwrap()["reason"], ZDR_FOLLOW_REASON);
        assert_eq!(ZDR_FOLLOW_REASON, "job_event_urls");
    }
}
