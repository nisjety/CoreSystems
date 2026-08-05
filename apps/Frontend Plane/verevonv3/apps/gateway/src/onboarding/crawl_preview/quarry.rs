use anyhow::Result;
use axum::response::sse::Event;
use chrono::Utc;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

use crate::{
    config::AppState,
    envelope::unwrap_data,
    onboarding::crawl_preview::{
        normalize::normalize_quarry_payload, sse::sse_json, types::CrawlPayload,
    },
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

pub(super) async fn forward_seed_scrape(
    state: &AppState,
    token: Option<&str>,
    url: &str,
) -> Result<Vec<Event>> {
    let response = bearer(
        state
            .client
            .post(format!("{}/v1/scrape/stream", state.quarry_edge_url)),
        token,
    )
    .json(&json!({ "url": url }))
    .send()
    .await?;

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
        if let Some(payload) = normalize_quarry_payload(json, event_name, "seed") {
            out.push(sse_json(
                &payload.kind,
                payload.value.unwrap_or_else(|| json!({})),
            ));
        }
    }
    Ok(out)
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
        if Utc::now().timestamp_millis() - started_at > 18_000 {
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
