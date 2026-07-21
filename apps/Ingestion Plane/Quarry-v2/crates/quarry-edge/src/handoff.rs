//! Forward durable-path requests (crawl, batch, scheduled) to control plane.
//!
//! Edge posts a job record to `quarry-control-go` so the operator can list/query
//! it. Orchestrator eventually picks up matching Temporal workflow via the
//! control → orchestrator channel (schedules reconcile loop + future dispatch).

use std::time::Duration;

use serde::Deserialize;

use quarry_core::error::{ErrorCode, QuarryError};

use crate::routes::HandoffAck;

/// Posts `POST {control}/v1/jobs?org_id=<org>` with `{kind, policy?}` plus an
/// opaque `params` blob carrying kind-specific payload (url, urls,
/// max_pages…).
///
/// `org_id` rides as a query param — mirroring `resource_routes.rs`'s
/// `forward_mutation` convention for `/v1/sources` — so control's
/// `createJob` handler can read the edge-verified org the same way
/// `createSourceHandler` does, and reject/stamp it without trusting
/// anything from the JSON body. `params.org_id` (embedded in `payload` by
/// the caller) is untouched: the orchestrator dispatcher still reads org
/// from there to build the Temporal workflow input.
pub async fn forward_to_orchestrator(
    control_base_url: &str,
    _request_id: &str,
    org_id: &str,
    payload: serde_json::Value,
) -> Result<HandoffAck, QuarryError> {
    if control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "control base url empty",
        ));
    }
    if org_id.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "handoff missing org_id",
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
    let resp = client
        .post(&url)
        .query(&[("org_id", org_id)])
        .json(&body)
        .send()
        .await
        .map_err(|e| {
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
        run_id: job.run_id,
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
    /// Populated by the orchestrator once the job is running; `None` at
    /// initial handoff. Threaded into `HandoffAck.run_id` for consumers
    /// that prefer the run-keyed event stream.
    #[serde(default)]
    run_id: Option<String>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
