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

pub(super) async fn forward_seed_scrape(state: &AppState, url: &str) -> Result<Vec<Event>> {
    let response = state
        .client
        .post(format!("{}/v1/scrape/stream", state.quarry_edge_url))
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
    org_id: Option<&str>,
    url: &str,
    max_pages: u32,
) -> Result<String> {
    let body = json!({
        "kind": "crawl",
        "params": {
            "url": url,
            "max_pages": max_pages,
            "max_depth": 1,
            "auto_commit": org_id.is_some(),
            "org_id": org_id,
        }
    });
    let response = state
        .client
        .post(format!("{}/v1/jobs/", state.quarry_control_url))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let data = unwrap_data(&body);
    let job_id = data.get("id").and_then(Value::as_str).unwrap_or_default();
    if job_id.is_empty() {
        anyhow::bail!("crawl job missing id");
    }
    Ok(job_id.to_owned())
}

/// Poll quarry-control's job-event log and forward each normalized event to `tx`
/// the moment it is read, so the onboarding SSE streams live progress instead of
/// buffering the whole crawl and emitting it at the end. Returns when the run hits
/// a terminal event, the 18s budget elapses, or the receiver is dropped (client
/// disconnected). An upstream request error propagates as `Err` so the handler can
/// surface a "control unreachable" warning.
pub(super) async fn poll_crawl_events(
    state: &AppState,
    job_id: &str,
    tx: tokio::sync::mpsc::Sender<CrawlPayload>,
) -> Result<()> {
    let mut after_seq = 0u64;
    let started_at = Utc::now().timestamp_millis();

    loop {
        if Utc::now().timestamp_millis() - started_at > 18_000 {
            break;
        }
        let response = state
            .client
            .get(format!(
                "{}/v1/jobs/{}/events?after_seq={}&limit=100",
                state.quarry_control_url,
                urlencoding::encode(job_id),
                after_seq
            ))
            .send()
            .await?;
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        let items = unwrap_data(&body).as_array().cloned().unwrap_or_default();

        let mut terminal = false;
        for item in items {
            let seq = item.get("seq").and_then(Value::as_u64).unwrap_or(0);
            after_seq = after_seq.max(seq);
            let event_type = item
                .get("type")
                .or_else(|| item.get("event_type"))
                .and_then(Value::as_str)
                .unwrap_or("event");
            let payload = item.get("payload").cloned().unwrap_or_else(|| item.clone());
            if let Some(mapped) = normalize_quarry_payload(payload, event_type, "live") {
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
