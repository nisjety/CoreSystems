//! Event publisher — drains the in-process event channel and POSTs batches
//! to the control-plane event ingest endpoint with exponential back-off.

use quarry_core::event::Event;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// Maximum number of events coalesced into a single HTTP POST.
const BATCH_CAP: usize = 64;

/// Initial back-off delay on a failed POST (doubles each retry up to `MAX_BACKOFF`).
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
/// Hard ceiling for back-off sleep.
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Number of consecutive failures before the publisher emits an error log
/// (it keeps retrying indefinitely — the channel is the back-pressure mechanism).
const ERROR_THRESHOLD: u32 = 3;

/// Consumes [`Event`]s produced by [`super::events::EventSink`] and forwards
/// them as JSON arrays to the control-plane ingest route:
///
/// ```text
/// POST {base_url}/v1/runs/{run_id}/events
/// Content-Type: application/json
/// Authorization: Bearer {api_key}
/// Body: [Event, ...]
/// ```
///
/// The publisher runs as a long-lived background task; spawn it with
/// [`tokio::spawn`] and hold the sender side of the channel until the job
/// is complete, then drop the sender so the receiver drains and the task
/// exits cleanly.
pub struct EventPublisher {
    rx: mpsc::Receiver<Event>,
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl EventPublisher {
    /// Create a new publisher.
    ///
    /// * `rx`       — receiving end of the event channel (producer holds `tx`).
    /// * `client`   — shared [`reqwest::Client`]; caller should enable `gzip`
    ///   and set a reasonable `timeout`.
    /// * `base_url` — control-plane origin, e.g. `https://control.example.com`.
    /// * `api_key`  — bearer token for the ingest endpoint.
    pub fn new(
        rx: mpsc::Receiver<Event>,
        client: reqwest::Client,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            rx,
            client,
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    /// Runs until the sender side of the channel is dropped *and* every
    /// queued event has been successfully delivered.
    pub async fn run(mut self) {
        loop {
            // --- 1. Collect up to BATCH_CAP events --------------------------------
            let mut batch: Vec<Event> = Vec::with_capacity(BATCH_CAP);

            // Block until at least one event arrives (or the channel closes).
            match self.rx.recv().await {
                Some(evt) => batch.push(evt),
                None => {
                    debug!("event channel closed — publisher exiting");
                    return;
                }
            }

            // Non-blocking drain for the rest of the batch.
            while batch.len() < BATCH_CAP {
                match self.rx.try_recv() {
                    Ok(evt) => batch.push(evt),
                    Err(_) => break,
                }
            }

            // --- 2. Determine target URL from the first event's run_id -----------
            // All events in a batch share the same run_id (the pipeline ensures
            // this), but we defend against mixed batches by grouping.
            let groups = group_by_run(batch);

            for (run_id_str, events) in groups {
                let url = format!(
                    "{}/v1/runs/{}/events",
                    self.base_url.trim_end_matches('/'),
                    run_id_str
                );
                self.post_with_backoff(&url, events).await;
            }
        }
    }

    /// POST `events` to `url` with exponential back-off until success.
    async fn post_with_backoff(&self, url: &str, events: Vec<Event>) {
        let body = match serde_json::to_vec(&events) {
            Ok(b) => b,
            Err(e) => {
                error!("failed to serialise event batch: {e}");
                return;
            }
        };

        let mut backoff = INITIAL_BACKOFF;
        let mut failures: u32 = 0;

        loop {
            let result = self
                .client
                .post(url)
                .bearer_auth(&self.api_key)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone())
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    debug!(url, events = events.len(), "event batch delivered");
                    return;
                }
                Ok(resp) => {
                    failures += 1;
                    let status = resp.status();
                    // 4xx (except 429) are non-retryable — discard the batch.
                    if status.is_client_error() && status.as_u16() != 429 {
                        error!(url, %status, "non-retryable error posting event batch — discarding");
                        return;
                    }
                    if failures >= ERROR_THRESHOLD {
                        error!(url, %status, failures, "repeated failures posting event batch");
                    } else {
                        warn!(url, %status, "transient error posting event batch, retrying");
                    }
                }
                Err(e) => {
                    failures += 1;
                    if failures >= ERROR_THRESHOLD {
                        error!(url, error = %e, failures, "network error posting event batch");
                    } else {
                        warn!(url, error = %e, "network error posting event batch, retrying");
                    }
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Group events by their `run_id` string representation so that one HTTP
/// request is made per run within a batch.
fn group_by_run(events: Vec<Event>) -> Vec<(String, Vec<Event>)> {
    let mut map: Vec<(String, Vec<Event>)> = Vec::new();

    for event in events {
        let key = event
            .run_id
            .as_ref()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if let Some(entry) = map.iter_mut().find(|(k, _)| k == &key) {
            entry.1.push(event);
        } else {
            map.push((key, vec![event]));
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use quarry_core::event::{Event, EventType};
    use quarry_core::ids::kinds::{EventKind, RunKind};

    fn make_event(run_id: &RunKind) -> Event {
        Event {
            event_id: EventKind::new(),
            run_id: Some(run_id.clone()),
            job_id: None,
            event_type: EventType::PageFetched,
            ts: Utc::now(),
            seq: 0,
            payload: serde_json::json!({}),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
        }
    }

    #[test]
    fn group_by_run_single_group() {
        let run = RunKind::new();
        let events: Vec<Event> = (0..4).map(|_| make_event(&run)).collect();
        let groups = group_by_run(events);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), 4);
    }

    #[test]
    fn group_by_run_two_groups() {
        let run_a = RunKind::new();
        let run_b = RunKind::new();
        let mut events: Vec<Event> = (0..3).map(|_| make_event(&run_a)).collect();
        events.extend((0..2).map(|_| make_event(&run_b)));
        let groups = group_by_run(events);
        assert_eq!(groups.len(), 2);
        let total: usize = groups.iter().map(|(_, v)| v.len()).sum();
        assert_eq!(total, 5);
    }
}
