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
    signer: Option<RequestSigner>,
}

/// Produces the extra headers (an HMAC signature, its timestamp and nonce) for one
/// outgoing request: `(method, path, body)` in, header name and value pairs out.
/// It runs on every attempt, because a nonce must not be reused when a batch is
/// retried. The runtime crate does not know the scheme; the edge supplies it.
pub type RequestSigner = std::sync::Arc<dyn Fn(&str, &str, &[u8]) -> Vec<(&'static str, String)> + Send + Sync>;

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
            signer: None,
        }
    }

    /// Sign every request (control rejects an unsigned one once
    /// `QUARRY_INTERNAL_HMAC_REQUIRED=1`).
    pub fn with_signer(mut self, signer: RequestSigner) -> Self {
        self.signer = Some(signer);
        self
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

    // zdr-log-ok-begin: the event publisher is a long-lived background task with no request span; `url` is the operator's control-plane origin plus a run id, and the error texts are transport errors for that endpoint, never an event's content
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
            let mut request = self
                .client
                .post(url)
                .bearer_auth(&self.api_key)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());
            if let Some(signer) = &self.signer {
                if let Ok(parsed) = reqwest::Url::parse(url) {
                    let path_q = match parsed.query() {
                        Some(query) => format!("{}?{}", parsed.path(), query),
                        None => parsed.path().to_string(),
                    };
                    for (name, value) in signer("POST", &path_q, &body) {
                        request = request.header(name, value);
                    }
                }
            }
            let result = request.send().await;

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
    // zdr-log-ok-end
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
            trace: None,
        }
    }

    #[tokio::test]
    async fn a_signer_adds_its_headers_over_the_path_and_body_actually_sent() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let run = RunKind::new();
        let (tx, rx) = mpsc::channel(8);
        tx.send(make_event(&run)).await.unwrap();
        drop(tx);
        let signer: RequestSigner = std::sync::Arc::new(|method, path, body| {
            vec![("x-test-sig", format!("{method} {path} {}", body.len()))]
        });
        let publisher =
            EventPublisher::new(rx, reqwest::Client::new(), server.uri(), "key").with_signer(signer);
        tokio::time::timeout(std::time::Duration::from_secs(10), publisher.run())
            .await
            .expect("publisher drains and exits");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let header = received[0].headers.get("x-test-sig").unwrap().to_str().unwrap();
        assert_eq!(
            header,
            format!("POST /v1/runs/{run}/events {}", received[0].body.len())
        );
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
