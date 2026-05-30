//! Forward durable-path requests (crawl, batch, scheduled) to control plane.
//!
//! Edge posts a job record to `quarry-control-go` so the operator can list/query
//! it. Orchestrator eventually picks up matching Temporal workflow via the
//! control → orchestrator channel (schedules reconcile loop + future dispatch).

use std::time::Duration;

use serde::Deserialize;

use quarry_core::error::{ErrorCode, QuarryError};

use crate::routes::HandoffAck;

/// Posts `POST {control}/v1/jobs` with `{kind, policy?}` plus an opaque
/// `params` blob carrying kind-specific payload (url, urls, max_pages…).
pub async fn forward_to_orchestrator(
    control_base_url: &str,
    _request_id: &str,
    payload: serde_json::Value,
) -> Result<HandoffAck, QuarryError> {
    if control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "control base url empty",
        ));
    }

    let kind = payload
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if kind.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "handoff payload missing 'kind'",
        ));
    }

    let body = serde_json::json!({ "kind": kind, "params": payload });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))?;

    let url = format!("{}/v1/jobs", control_base_url.trim_end_matches('/'));
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::UpstreamBlocked,
            format!("control unreachable: {e}"),
        )
    })?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(QuarryError::new(
            ErrorCode::UpstreamBlocked,
            format!("control {}: {}", status.as_u16(), truncate(&text, 500)),
        ));
    }

    let parsed: ControlJobEnvelope = resp.json().await.map_err(|e| {
        QuarryError::new(ErrorCode::Internal, format!("decode control response: {e}"))
    })?;

    let Some(job) = parsed.data else {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "control returned no job",
        ));
    };

    Ok(HandoffAck {
        job_id: job.id,
        accepted_at: chrono::Utc::now(),
    })
}

#[derive(Debug, Deserialize)]
struct ControlJobEnvelope {
    data: Option<ControlJob>,
}

#[derive(Debug, Deserialize)]
struct ControlJob {
    id: String,
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
