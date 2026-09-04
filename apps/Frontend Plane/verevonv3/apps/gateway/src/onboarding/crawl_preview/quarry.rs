use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

use crate::{
    config::AppState,
    envelope::unwrap_data,
    onboarding::crawl_preview::{normalize::normalize_quarry_payload, types::CrawlPayload},
};

/// All onboarding crawl traffic goes through `quarry-edge` — the only
/// sanctioned cross-plane Ingestion entrypoint. Edge verifies the Bearer JWT
/// against auth-core and derives the org from the verified claims, so no
/// org/tenant fields travel in request bodies.
fn bearer(request: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

/// Returns the seed page's normalized payloads (snippets, branding, …) in
/// stream order. The handler — not this function — serializes them to SSE,
/// so it can run the same snippet ledger over seed and live events and let
/// the richer `page_extracted` snippet update the `page_fetched` card.
pub(super) async fn forward_seed_scrape(
    state: &AppState,
    token: Option<&str>,
    url: &str,
) -> Result<Vec<CrawlPayload>> {
    let response = bearer(
        state
            .client
            .post(format!("{}/v1/scrape/stream", state.quarry_edge_url)),
        token,
    )
    .json(&json!({ "url": url }))
    .send()
    .await?;

    // A non-2xx here is an error envelope, not an SSE stream — parsing it
    // below would silently yield zero packets and hide the real failure.
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let body: String = body.chars().take(300).collect();
        anyhow::bail!("seed scrape upstream status {status}: {body}");
    }

    let mut out = Vec::new();
    let text = response.text().await.unwrap_or_default();
    for packet in text.split("\n\n") {
        let mut event_name = "message";
        let mut data = None::<String>;
        for line in packet.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_name = value.trim();
            }
            if let Some(value) = line.strip_prefix("data:") {
                data = Some(value.trim().to_owned());
            }
        }
        let Some(data) = data else { continue };
        let json = serde_json::from_str::<Value>(&data).unwrap_or_else(|_| json!({}));
        let (event_type, payload) = seed_event(json, event_name);
        if let Some(payload) = normalize_quarry_payload(payload, &event_type, "seed") {
            out.push(payload);
        }
    }
    Ok(out)
}

/// `/v1/scrape/stream` frames carry quarry-core's full `Event` envelope as
/// their data (`{ event_id, run_id, type, ts, seq, payload, .. }`) -- the same
/// shape `/v1/jobs/{id}/events` returns and `poll_crawl_events` unwraps. This
/// path used to hand the envelope itself to `normalize_quarry_payload`, which
/// reads `url` / `branding` at the top level, so every seed-scrape event (the
/// seed page's `page_fetched` and its `branding_extracted`) was silently
/// dropped and only the crawl job's own events ever reached the wizard.
/// Prefer the envelope's `type` when the SSE `event:` line is absent or the
/// default `message`; a frame without a `payload` object is used as-is.
fn seed_event(frame: Value, event_name: &str) -> (String, Value) {
    let event_type = match event_name {
        "" | "message" => frame
            .get("type")
            .or_else(|| frame.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or(event_name)
            .to_owned(),
        named => named.to_owned(),
    };
    let payload = match frame.get("payload") {
        Some(payload) if payload.is_object() => payload.clone(),
        _ => frame,
    };
    (event_type, payload)
}

pub(super) async fn create_crawl_job(
    state: &AppState,
    token: Option<&str>,
    url: &str,
    max_pages: u32,
) -> Result<String> {
    // Edge crawl handoff. The old direct-control body also carried `org_id`
    // and `auto_commit`; org now comes from the verified JWT claims and
    // `auto_commit` was a dead param the orchestrator never read. `ingest` is
    // deliberately absent: a preview stays working-set only.
    let body = json!({
        "url": url,
        "max_pages": max_pages,
        "max_depth": 1,
    });
    let response = bearer(
        state
            .client
            .post(format!("{}/v1/crawl", state.quarry_edge_url)),
        token,
    )
    .header("content-type", "application/json")
    .json(&body)
    .send()
    .await?;
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let data = unwrap_data(&body);
    // Edge returns a control job record; prefer explicit job-id fields, then
    // the generic `id` the HandoffAck envelope uses.
    let job_id = ["job_id", "jobId", "id"]
        .iter()
        .find_map(|key| data.get(key).and_then(Value::as_str))
        .unwrap_or_default();
    if job_id.is_empty() {
        anyhow::bail!("crawl job missing id");
    }
    Ok(job_id.to_owned())
}

/// Ceiling on how long the preview follows a crawl job's event log.
///
/// This is a CEILING, not a dwell time: the loop returns the moment a
/// terminal event arrives, and the wizard lets the user move on while the
/// stream is still open. The previous 18s routinely expired before the crawl
/// emitted anything — the orchestrator's dispatcher polls for accepted jobs,
/// Temporal then has to schedule `CrawlJobWF`, and only then does the first
/// page get fetched and transformed. Measured live against aquatiq.com, the
/// first `page_extracted` landed ~21s after handoff, so the stream closed
/// three seconds early and the wizard showed nothing but the seed page.
const CRAWL_POLL_BUDGET_MS: i64 = 45_000;

/// Poll quarry-edge's job-event log and forward each normalized event to `tx`
/// the moment it is read, so the onboarding SSE streams live progress instead
/// of buffering the whole crawl and emitting it at the end. Edge serves the
/// log as a drain-once SSE stream (`event: <type>` / `data: <event json>`
/// frames plus a terminal `data: done` marker), so each poll parses frames and
/// skips those at or below the local `seq` watermark (edges that honor
/// `after_seq` filter server-side; older ones re-send the whole log). Returns
/// when the run hits a terminal event, the 18s budget elapses, or the receiver
/// is dropped (client disconnected). An upstream request error propagates as
/// `Err` so the handler can surface an "ingestion unreachable" warning.
pub(super) async fn poll_crawl_events(
    state: &AppState,
    token: Option<&str>,
    job_id: &str,
    tx: tokio::sync::mpsc::Sender<CrawlPayload>,
) -> Result<()> {
    let mut after_seq = 0u64;
    let started_at = Utc::now().timestamp_millis();

    loop {
        if Utc::now().timestamp_millis() - started_at > CRAWL_POLL_BUDGET_MS {
            break;
        }
        let response = bearer(
            state.client.get(format!(
                "{}/v1/jobs/{}/events?after_seq={}&limit=100",
                state.quarry_edge_url,
                urlencoding::encode(job_id),
                after_seq
            )),
            token,
        )
        .send()
        .await?;
        let text = response.text().await.unwrap_or_default();

        let mut terminal = false;
        for packet in text.split("\n\n") {
            let mut data = None::<String>;
            for line in packet.lines() {
                if let Some(value) = line.strip_prefix("data:") {
                    data = Some(value.trim().to_owned());
                }
            }
            let Some(data) = data else { continue };
            if data == "done" {
                // Edge's end-of-drain marker, not a crawl event.
                continue;
            }
            let Ok(item) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            let seq = item.get("seq").and_then(Value::as_u64).unwrap_or(0);
            if seq != 0 && seq <= after_seq {
                continue;
            }
            after_seq = after_seq.max(seq);
            let event_type = item
                .get("type")
                .or_else(|| item.get("event_type"))
                .and_then(Value::as_str)
                .unwrap_or("event")
                .to_owned();
            let payload = item.get("payload").cloned().unwrap_or_else(|| item.clone());
            if let Some(mapped) = normalize_quarry_payload(payload, &event_type, "live") {
                terminal = terminal
                    || event_type == "run_completed"
                    || event_type == "run_failed"
                    || event_type == "run_cancelled";
                // Receiver dropped → client disconnected; stop polling.
                if tx.send(mapped).await.is_err() {
                    return Ok(());
                }
            }
        }

        if terminal {
            break;
        }

        sleep(Duration::from_millis(750)).await;
    }

    Ok(())
}

#[cfg(test)]
mod seed_event_tests {
    use super::seed_event;
    use serde_json::json;

    // Regression: the seed scrape's SSE data is the whole quarry-core Event
    // envelope. Passing it through unchanged made the normalizer look for
    // `url` on the envelope, so the seed page never produced a snippet or
    // branding for the onboarding wizard.
    #[test]
    fn unwraps_the_event_envelope_payload() {
        let frame = json!({
            "event_id": "evt_1", "run_id": "run_1", "type": "page_fetched", "seq": 3,
            "payload": { "url": "https://aquatiq.com/", "status": 200 },
        });
        let (event_type, payload) = seed_event(frame, "page_fetched");
        assert_eq!(event_type, "page_fetched");
        assert_eq!(payload["url"], "https://aquatiq.com/");
        assert!(payload.get("event_id").is_none());
    }

    #[test]
    fn falls_back_to_the_envelope_type_for_unnamed_frames() {
        let frame = json!({ "type": "branding_extracted", "payload": { "branding": {} } });
        let (event_type, _) = seed_event(frame, "message");
        assert_eq!(event_type, "branding_extracted");
    }

    #[test]
    fn keeps_flat_frames_and_named_events_as_is() {
        let frame = json!({ "url": "https://aquatiq.com/", "title": "Aquatiq" });
        let (event_type, payload) = seed_event(frame.clone(), "page_fetched");
        assert_eq!(event_type, "page_fetched");
        assert_eq!(payload, frame);
    }
}
