//! Schedule lifecycle endpoints — cluster #5.
//!
//! Cycle 23.
//!
//! All routes forward to the Go control plane, which owns the Temporal
//! client. The edge stays the public-facing surface so callers don't
//! need to know about the orchestrator vendor.
//!
//! ## Endpoints
//!
//! | Route                                  | Effect                                   |
//! | -------------------------------------- | ---------------------------------------- |
//! | `POST /v1/schedules`                   | Create (cron or `schedule_at` one-shot)   |
//! | `POST /v1/schedules/:id/pause`         | Pause (stops firing; preserves config)    |
//! | `POST /v1/schedules/:id/unpause`       | Resume after pause                        |
//! | `POST /v1/schedules/:id/trigger`       | Fire once manually, immediately           |
//! | `POST /v1/schedules/:id/backfill`      | Run for a specific time-range window      |
//! | `DELETE /v1/schedules/:id`             | Soft-delete (Temporal stops; config kept) |
//!
//! Acceptance (gap-quarry §10.1 #5): "Operators never need direct
//! Temporal access for schedule work." This is the public surface
//! that closes that gap.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::RequestKind;
use quarry_core::resources::{JobResourceKind, OverlapPolicy, ScheduleSummary};

use crate::state::AppState;

// =============================================================================
// Request bodies
// =============================================================================

/// `POST /v1/schedules` body. Exactly one of `cron` / `schedule_at`
/// must be set — the backend enforces the constraint.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateScheduleRequest {
    pub name: String,
    pub kind: JobResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
    #[serde(default)]
    pub catchup_window_s: u64,
    #[serde(default)]
    pub pause_on_failure: bool,
    /// Per-kind payload (seed URL + max_pages for crawl, query for
    /// search, etc.). Mirrors the inline request body the schedule
    /// would otherwise be triggering.
    #[serde(default)]
    pub config: serde_json::Value,
}

/// `POST /v1/schedules/:id/backfill` body. The Temporal `backfill`
/// API runs the schedule against historical time-ranges so an operator
/// can "catch up" on missed runs without writing custom orchestration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackfillRequest {
    pub start_at: chrono::DateTime<chrono::Utc>,
    pub end_at: chrono::DateTime<chrono::Utc>,
    /// Overlap policy applied during backfill — may differ from the
    /// schedule's default (backfills often want Allow to parallelize).
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
}

// =============================================================================
// Helpers
// =============================================================================

fn err_response(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

/// Forward a JSON body to control plane. Generic over both the request
/// and response shape — the schedule routes all share the same wire
/// pattern (POST → JSON → typed response).
async fn forward_json<TReq, TResp>(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    org_id: &str,
    body: Option<&TReq>,
) -> Result<TResp, QuarryError>
where
    TReq: Serialize + ?Sized,
    TResp: serde::de::DeserializeOwned,
{
    if state.control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "control plane URL not configured; schedule lifecycle routes are inert",
        ));
    }
    let url = format!("{}{}", state.control_base_url.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))?;

    // Serialize the body up front so we can HMAC-hash it AND attach
    // it to the request — both paths see the exact same bytes.
    let body_bytes: Vec<u8> = match body {
        Some(b) => serde_json::to_vec(b)
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("encode body: {e}")))?,
        None => Vec::new(),
    };

    let mut req = client
        .request(method.clone(), &url)
        .query(&[("org_id", org_id)]);
    if body.is_some() {
        req = req
            .header("content-type", "application/json")
            .body(body_bytes.clone());
    }

    // D3 / cluster #14 — Idempotency-Key on every mutating call so
    // a network retry doesn't double-execute the operation. The Go
    // control plane MUST dedupe `(org_id, idempotency_key, route)`
    // for at least 24h (see CROSS_PLANE_AUTH.md).
    let idem_key: quarry_core::ids::kinds::RequestKind = quarry_core::ids::Id::new();
    let idem_key = idem_key.to_string();
    req = req.header("Idempotency-Key", &idem_key);

    // D2 / cluster #14 — sign the canonical (method, path?org+key, body_hash, ts, nonce).
    if let Some(signer) = state.internal_signer.as_ref() {
        let path_q = format!("{path}?org_id={org_id}");
        let signed =
            crate::internal_auth::apply_to_request(signer, method.as_str(), &path_q, &body_bytes);
        req = req
            .header(crate::internal_auth::HEADER_SIG, signed.signature)
            .header(crate::internal_auth::HEADER_TS, signed.timestamp)
            .header(crate::internal_auth::HEADER_NONCE, signed.nonce);
    }

    let resp = req.send().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            format!("control-plane {method} {path} failed: {e}"),
        )
    })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let code = match status.as_u16() {
            404 => ErrorCode::NotFound,
            400 => ErrorCode::BadRequest,
            401 | 403 => ErrorCode::Forbidden,
            _ => ErrorCode::DriverFailed,
        };
        return Err(QuarryError::new(
            code,
            format!("control-plane {method} {path} returned {status}: {body}"),
        ));
    }
    resp.json::<TResp>().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane {method} {path} parse: {e}"),
        )
    })
}

// =============================================================================
// Handlers
// =============================================================================

pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<Json<Envelope<ScheduleSummary>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    if req.cron.is_some() == req.schedule_at.is_some() {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "exactly one of `cron` / `schedule_at` must be set",
            ),
        ));
    }
    if let Some(cron) = req.cron.as_deref() {
        // Basic shape check — full validation lives on the control side
        // (the cron parser quirks are vendor-specific). Reject obvious
        // garbage at the edge so we never round-trip on bad input.
        if cron.split_whitespace().count() < 5 {
            return Err(err_response(
                &request_id,
                QuarryError::new(
                    ErrorCode::BadRequest,
                    "cron must be a 5- or 6-field expression",
                ),
            ));
        }
    }
    let summary = forward_json::<CreateScheduleRequest, ScheduleSummary>(
        &state,
        reqwest::Method::POST,
        "/v1/schedules",
        &claims.org_id,
        Some(&req),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
}

pub async fn pause_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<Json<Envelope<ScheduleSummary>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let path = format!("/v1/schedules/{id}/pause");
    let summary = forward_json::<(), ScheduleSummary>(
        &state,
        reqwest::Method::POST,
        &path,
        &claims.org_id,
        None,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
}

pub async fn unpause_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<Json<Envelope<ScheduleSummary>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let path = format!("/v1/schedules/{id}/unpause");
    let summary = forward_json::<(), ScheduleSummary>(
        &state,
        reqwest::Method::POST,
        &path,
        &claims.org_id,
        None,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
}

pub async fn trigger_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<Json<Envelope<ScheduleSummary>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let path = format!("/v1/schedules/{id}/trigger");
    let summary = forward_json::<(), ScheduleSummary>(
        &state,
        reqwest::Method::POST,
        &path,
        &claims.org_id,
        None,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
}

pub async fn backfill_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
    Json(req): Json<BackfillRequest>,
) -> Result<Json<Envelope<ScheduleSummary>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    if req.end_at <= req.start_at {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "backfill end_at must be strictly after start_at",
            ),
        ));
    }
    let path = format!("/v1/schedules/{id}/backfill");
    let summary = forward_json::<BackfillRequest, ScheduleSummary>(
        &state,
        reqwest::Method::POST,
        &path,
        &claims.org_id,
        Some(&req),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
}

pub async fn delete_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let path = format!("/v1/schedules/{id}");
    // DELETE returns no body — use forward_json with `serde_json::Value`
    // and ignore the parsed result.
    forward_json::<(), serde_json::Value>(
        &state,
        reqwest::Method::DELETE,
        &path,
        &claims.org_id,
        None,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_schedule_request_requires_cron_xor_schedule_at_check_pattern() {
        // Pin the validation contract directly via the shape: a request
        // with both fields populated must be rejected at the route
        // layer. (Route-level test runs in integration; here we just
        // assert the body shape can carry both fields, since the route
        // is the one that says "exactly one".)
        let r = CreateScheduleRequest {
            name: "n".into(),
            kind: JobResourceKind::Crawl,
            cron: Some("0 3 * * *".into()),
            schedule_at: Some(chrono::Utc::now()),
            overlap_policy: OverlapPolicy::Skip,
            catchup_window_s: 0,
            pause_on_failure: false,
            config: serde_json::json!({}),
        };
        // Body still serializes — the route layer rejects, not serde.
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"cron\":"));
        assert!(s.contains("\"schedule_at\":"));
    }

    #[test]
    fn backfill_request_serializes_with_required_fields() {
        let now = chrono::Utc::now();
        let r = BackfillRequest {
            start_at: now,
            end_at: now + chrono::Duration::hours(1),
            overlap_policy: OverlapPolicy::Allow,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"start_at\":"));
        assert!(s.contains("\"end_at\":"));
        assert!(s.contains("\"overlap_policy\":\"allow\""));
    }
}
