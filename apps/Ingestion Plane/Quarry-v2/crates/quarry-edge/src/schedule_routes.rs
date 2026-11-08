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
use quarry_core::resources::OverlapPolicy;

use crate::state::AppState;

// =============================================================================
// Request bodies
// =============================================================================

/// `POST /v1/schedules` body (WS15.S1). Two shapes share it:
///
/// * a change monitor: `preset` (hourly | daily | weekly) and the URL to watch
///   (`target_ref`, or `config.url`). A monitor starts when created, as before;
///   its posture is stamped (WS4.H);
/// * an executable schedule: `kind` (scrape | crawl | batch), a 5-field `cron`,
///   and a `config` holding `url` (scrape, crawl) or `urls` (batch). It is
///   created disabled unless the caller sends `activate: true`, and only a user
///   token may activate it.
///
/// `schedule_at` is not supported yet and answers 400 `unsupported_schedule_at`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateScheduleRequest {
    #[serde(default)]
    pub name: String,
    /// `scrape` | `crawl` | `batch` for an executable schedule. Any other value
    /// answers 400 `unsupported_schedule_kind`. Absent for a change monitor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// One-shot timestamps are not supported yet (400 `unsupported_schedule_at`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_at: Option<chrono::DateTime<chrono::Utc>>,
    /// IANA zone the cron is read in. Control defaults to UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
    #[serde(default)]
    pub catchup_window_s: u64,
    #[serde(default)]
    pub pause_on_failure: bool,
    /// W2 recurring change-monitoring. When `preset` is set the request is a
    /// change-monitor create: `target_ref` (the URL to watch) is required and the
    /// cron is derived server-side from the preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    /// Per-kind payload: `url` for scrape and crawl, `urls` for batch, and
    /// optional `max_pages`, `max_depth`, `ingest`.
    #[serde(default)]
    pub config: serde_json::Value,
    /// Start the schedule when it is created. Honoured only for a user token; a
    /// service principal's value is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activate: Option<bool>,
    /// The caller may DECLARE zero data retention for a change-monitor create. A
    /// declaration only raises the stamp: [`Claims::effective_zdr`] never lets a body
    /// lower what a user token's floor requires. It is how a user of a standard
    /// organization says this monitor is ZDR. (A service principal cannot reach this
    /// route: `require_service_route_scope` answers 403 first.) Never forwarded as is
    /// (`skip_serializing`): only the stamped [`ControlScheduleCreate`] carries a
    /// posture to control.
    #[serde(default, skip_serializing)]
    pub zdr: Option<bool>,
}

/// Control-plane-facing schedule create body. Matches the Go `store.Schedule` json
/// tags. The org is carried on the signed query (control takes it from there and
/// ignores a body org_id); `org_id` here is the verified claim for older readers.
#[derive(Debug, Serialize)]
struct ControlScheduleCreate {
    org_id: String,
    /// user_id of the creator (verified JWT). Control rides this into the
    /// change-monitor workflow so the in-product notification on a detected change
    /// reaches the person who set up the monitor.
    #[serde(skip_serializing_if = "String::is_empty")]
    created_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cron: Option<String>,
    target_kind: String,
    target_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
    config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    timezone: Option<String>,
    overlap_policy: OverlapPolicy,
    catchup_window_s: u64,
    pause_on_failure: bool,
    /// Control starts the schedule only when this is true.
    activate: bool,
    /// The caller's Zero Data Retention posture, STAMPED AT CREATE (WS4.H,
    /// quarry-control migration 023). Set on a change-monitor body only; a legacy
    /// executable create never carries it. It is [`Claims::effective_zdr`] of the
    /// body's declared `zdr`: the body can raise the posture, never lower the
    /// token's floor.
    #[serde(skip_serializing_if = "Option::is_none")]
    zdr: Option<bool>,
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

/// The machine `reason` control attached to its error (`error.details.reason`), if any.
fn control_reason(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed["error"]["details"]["reason"].as_str().map(str::to_owned)
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
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {}", e.without_url())))?;

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
            format!("control-plane {method} {path} failed: {}", e.without_url()),
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
        let mut err = QuarryError::new(
            code,
            format!("control-plane {method} {path} returned {status}: {body}"),
        );
        if let Some(reason) = control_reason(&body) {
            err = err.with_details(serde_json::json!({ "reason": reason }));
        }
        return Err(err);
    }
    // Read the body as text first so we can tolerate three shapes:
    //   * empty body (204 No Content from DELETE / no-content POSTs),
    //   * a `{data: ...}` envelope (control's standard mutating response),
    //   * a bare object (older routes).
    // All current callers type TResp as `serde_json::Value`, so an empty
    // body decodes to `Value::Null` rather than erroring.
    let text = resp.text().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane {method} {path} read: {}", e.without_url()),
        )
    })?;
    let inner = if text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("control-plane {method} {path} parse: {e}"),
            )
        })?;
        raw.get("data").cloned().unwrap_or(raw)
    };
    serde_json::from_value::<TResp>(inner).map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane {method} {path} decode: {e}"),
        )
    })
}

// =============================================================================
// Handlers
// =============================================================================

/// A 400 with a stable machine reason in `error.details.reason`.
fn bad_schedule(request_id: &str, reason: &str, message: impl Into<String>) -> (StatusCode, Json<Envelope<()>>) {
    err_response(
        request_id,
        QuarryError::new(ErrorCode::BadRequest, message)
            .with_details(serde_json::json!({ "reason": reason })),
    )
}

/// Accepts an http or https URL with a host and no credentials, as control does.
fn checked_http_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    let scheme_ok = matches!(parsed.scheme(), "http" | "https");
    let plain = parsed.username().is_empty() && parsed.password().is_none();
    (scheme_ok && plain && parsed.host_str().is_some()).then(|| parsed.to_string())
}

/// Checks the per-kind config at the edge (control checks it again) and returns the
/// `target_ref` to store: the URL, or the first URL of a batch.
fn schedule_target(kind: &str, config: &serde_json::Value) -> Result<String, (&'static str, String)> {
    if kind == "batch" {
        let urls = config
            .get("urls")
            .and_then(serde_json::Value::as_array)
            .filter(|u| (1..=200).contains(&u.len()))
            .ok_or(("invalid_config", "config.urls must hold 1 to 200 URLs".to_owned()))?;
        let mut first = None;
        for entry in urls {
            let raw = entry.as_str().unwrap_or_default();
            let ok = checked_http_url(raw)
                .ok_or(("invalid_config", "config.urls entries must be http or https URLs without credentials".to_owned()))?;
            first.get_or_insert(ok);
        }
        return Ok(first.unwrap_or_default());
    }
    let raw = config
        .get("url")
        .and_then(serde_json::Value::as_str)
        .ok_or(("invalid_config", "config.url is required for this kind".to_owned()))?;
    checked_http_url(raw).ok_or((
        "invalid_config",
        "config.url must be an http or https URL without credentials".to_owned(),
    ))
}

/// The URLs a validated executable schedule fetches: all of a batch's `config.urls`, the one
/// `target_ref` otherwise.
fn schedule_urls(kind: &str, config: &serde_json::Value, target_ref: &str) -> Vec<String> {
    if kind == "batch" {
        if let Some(urls) = config.get("urls").and_then(|v| v.as_array()) {
            return urls
                .iter()
                .filter_map(|entry| entry.as_str())
                .map(|raw| raw.trim().to_owned())
                .collect();
        }
    }
    vec![target_ref.to_owned()]
}

/// Refuse a schedule create whose URL the org web policy denies. The schedule is a durable
/// resource that fetches later, so a denied URL is stopped here instead of failing on every fire.
fn gate_schedule_urls<'a>(
    state: &AppState,
    claims: &crate::auth::Claims,
    zdr: bool,
    urls: impl IntoIterator<Item = &'a str>,
    request_id: &str,
) -> Result<(), (StatusCode, Json<Envelope<()>>)> {
    crate::web_policy_edge::decide(&state.policy, claims, zdr, None)
        .and_then(|decision| crate::web_policy_edge::gate_urls(&decision, urls))
        .map_err(|error| crate::web_policy_edge::envelope_error(request_id, error))
}

pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    // One posture for both uses: the refusal below and the stamp on the forwarded
    // change-monitor schedule are the same value, so they can never disagree.
    let zdr = claims.effective_zdr(req.zdr.unwrap_or(false));
    crate::zdr_refusal::refuse(
        &state,
        &claims.org_id,
        zdr,
        &request_id,
        "schedules",
        "create",
    )?;

    // W2 change-monitoring path: `preset` signals a recurring change monitor. The
    // org comes from the verified JWT (never the body), the cron from the preset on
    // the control side, and the monitor starts when created, as it always has.
    if let Some(preset) = req.preset.as_deref() {
        const PRESETS: [&str; 3] = ["hourly", "daily", "weekly"];
        if !PRESETS.contains(&preset) {
            return Err(bad_schedule(
                &request_id,
                "invalid_preset",
                "preset must be one of hourly|daily|weekly",
            ));
        }
        let target_ref = req
            .target_ref
            .clone()
            .or_else(|| {
                req.config
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                bad_schedule(
                    &request_id,
                    "target_ref_required",
                    "change-monitor schedule requires target_ref (url)",
                )
            })?;
        if url::Url::parse(&target_ref).is_err() {
            return Err(bad_schedule(
                &request_id,
                "invalid_target_ref",
                "target_ref must be a valid url",
            ));
        }
        gate_schedule_urls(&state, &claims, zdr, [target_ref.as_str()], &request_id)?;
        let body = ControlScheduleCreate {
            org_id: claims.org_id.clone(),
            created_by: claims.user_id.clone(),
            name: req.name.trim().to_owned(),
            cron: None,
            target_kind: "change_monitor".to_string(),
            target_ref,
            preset: Some(preset.to_string()),
            config: serde_json::json!({}),
            timezone: None,
            overlap_policy: OverlapPolicy::default(),
            catchup_window_s: 0,
            pause_on_failure: false,
            activate: true,
            zdr: Some(zdr),
        };
        let summary = forward_json::<ControlScheduleCreate, serde_json::Value>(
            &state,
            reqwest::Method::POST,
            "/v1/schedules",
            &claims.org_id,
            Some(&body),
        )
        .await
        .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(Envelope::ok(request_id, summary)));
    }

    // WS15.S1 executable schedule: scrape, crawl or batch, with a 5-field cron.
    if req.schedule_at.is_some() {
        return Err(bad_schedule(
            &request_id,
            "unsupported_schedule_at",
            "schedule_at is not supported yet; use a 5-field cron",
        ));
    }
    let target_kind = match req.kind.as_deref().map(str::trim) {
        Some(kind @ ("scrape" | "crawl" | "batch")) => kind.to_owned(),
        _ => {
            return Err(bad_schedule(
                &request_id,
                "unsupported_schedule_kind",
                "kind must be scrape, crawl or batch",
            ))
        }
    };
    let cron = req
        .cron
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| bad_schedule(&request_id, "cron_required", "cron is required"))?;
    // Exactly five fields. Control accepts only five, so a six-field cron is refused here
    // rather than failing later.
    if cron.split_whitespace().count() != 5 {
        return Err(bad_schedule(
            &request_id,
            "invalid_cron",
            "cron must be a 5-field expression",
        ));
    }
    let target_ref = schedule_target(&target_kind, &req.config)
        .map_err(|(reason, message)| bad_schedule(&request_id, reason, message))?;
    // Every URL the schedule will fetch is gated now: a batch names up to 200, the others one.
    let urls = schedule_urls(&target_kind, &req.config, &target_ref);
    gate_schedule_urls(&state, &claims, zdr, urls.iter().map(String::as_str), &request_id)?;
    let body = ControlScheduleCreate {
        org_id: claims.org_id.clone(),
        created_by: claims.user_id.clone(),
        name: req.name.trim().to_owned(),
        cron: Some(cron.to_owned()),
        target_kind,
        target_ref,
        preset: None,
        config: req.config.clone(),
        timezone: req.timezone.clone(),
        overlap_policy: req.overlap_policy,
        catchup_window_s: req.catchup_window_s,
        pause_on_failure: req.pause_on_failure,
        activate: req.activate.unwrap_or(false) && !claims.is_service(),
        zdr: None,
    };
    let summary = forward_json::<ControlScheduleCreate, serde_json::Value>(
        &state,
        reqwest::Method::POST,
        "/v1/schedules",
        &claims.org_id,
        Some(&body),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
}

pub async fn pause_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    let path = format!("/v1/schedules/{id}/pause");
    let summary = forward_json::<(), serde_json::Value>(
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

/// WS15.S4a: pause a schedule because its organization's fire-time posture is
/// Zero Data Retention (`skipped_zdr`). Called by the schedule preflight under
/// `QUARRY_EDGE__SCHEDULE_POSTURE=enforce`, with no user token: the org comes
/// from the preflight's run-bound body, and the call is the same signed control
/// forward `pause_schedule` makes. Unpause stays refused for a ZDR caller.
pub(crate) async fn pause_for_posture(
    state: &AppState,
    schedule_id: &str,
    org_id: &str,
) -> Result<(), QuarryError> {
    let path = format!("/v1/schedules/{schedule_id}/pause");
    forward_json::<(), serde_json::Value>(state, reqwest::Method::POST, &path, org_id, None)
        .await
        .map(|_| ())
}

pub async fn unpause_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "schedules",
        "unpause",
    )?;
    let path = format!("/v1/schedules/{id}/unpause");
    let summary = forward_json::<(), serde_json::Value>(
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
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "schedules",
        "trigger",
    )?;
    let path = format!("/v1/schedules/{id}/trigger");
    let summary = forward_json::<(), serde_json::Value>(
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
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "schedules",
        "backfill",
    )?;
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
    let summary = forward_json::<BackfillRequest, serde_json::Value>(
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
    let request_id = crate::request_id::current();
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
    fn create_request_serializes_cron_and_schedule_at_when_both_are_set() {
        // Pin the validation contract directly via the shape: a request
        // with both fields populated must be rejected at the route
        // layer. (Route-level test runs in integration; here we just
        // assert the body shape can carry both fields, since the route
        // is the one that says "exactly one".)
        let r = CreateScheduleRequest {
            name: "n".into(),
            kind: Some("crawl".into()),
            cron: Some("0 3 * * *".into()),
            schedule_at: Some(chrono::Utc::now()),
            timezone: None,
            overlap_policy: OverlapPolicy::Skip,
            catchup_window_s: 0,
            pause_on_failure: false,
            preset: None,
            target_ref: None,
            config: serde_json::json!({}),
            activate: None,
            zdr: None,
        };
        // Body still serializes — the route layer rejects, not serde.
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"cron\":"));
        assert!(s.contains("\"schedule_at\":"));
    }

    #[test]
    fn change_monitor_request_decodes_preset_and_target_ref() {
        // W2 shape: a change-monitor create carries preset + target_ref and
        // omits kind/cron/schedule_at.
        let r: CreateScheduleRequest = serde_json::from_value(serde_json::json!({
            "preset": "daily",
            "target_ref": "https://example.com/pricing"
        }))
        .unwrap();
        assert_eq!(r.preset.as_deref(), Some("daily"));
        assert_eq!(r.target_ref.as_deref(), Some("https://example.com/pricing"));
        assert!(r.kind.is_none());
        assert!(r.cron.is_none());
    }

    #[test]
    fn control_schedule_create_serializes_store_schedule_shape() {
        let body = ControlScheduleCreate {
            org_id: "org_verevon".into(),
            created_by: "user_42".into(),
            cron: None,
            target_kind: "change_monitor".into(),
            target_ref: "https://example.com".into(),
            name: String::new(),
            config: serde_json::json!({}),
            timezone: None,
            overlap_policy: OverlapPolicy::Skip,
            catchup_window_s: 0,
            pause_on_failure: false,
            activate: true,
            preset: Some("daily".into()),
            zdr: Some(true),
        };
        let s = serde_json::to_string(&body).unwrap();
        assert!(s.contains("\"org_id\":\"org_verevon\""));
        assert!(s.contains("\"created_by\":\"user_42\""));
        assert!(s.contains("\"target_kind\":\"change_monitor\""));
        assert!(s.contains("\"target_ref\":\"https://example.com\""));
        assert!(s.contains("\"preset\":\"daily\""));
        // The stamp is always serialised, true and false alike (false is
        // "stamped standard", which control keeps distinct from unstamped).
        assert!(s.contains("\"zdr\":true"));
        let standard = ControlScheduleCreate { zdr: Some(false), ..body };
        assert!(serde_json::to_string(&standard).unwrap().contains("\"zdr\":false"));
        // cron omitted (derived server-side from preset).
        assert!(!s.contains("\"cron\":"));
    }

    #[test]
    fn schedule_target_checks_the_url_shape_per_kind() {
        let crawl = serde_json::json!({ "url": "https://example.com/docs", "max_pages": 20 });
        assert_eq!(schedule_target("crawl", &crawl).unwrap(), "https://example.com/docs");
        for bad in [
            serde_json::json!({}),
            serde_json::json!({ "url": "ftp://example.com" }),
            serde_json::json!({ "url": "https://user:pw@example.com" }),
            serde_json::json!({ "url": "not a url" }),
        ] {
            let (reason, _) = schedule_target("scrape", &bad).unwrap_err();
            assert_eq!(reason, "invalid_config", "{bad}");
        }
    }

    #[test]
    fn batch_target_is_the_first_url_and_bounds_the_list() {
        let batch = serde_json::json!({ "urls": ["https://a.example.com", "https://b.example.com"] });
        assert_eq!(schedule_target("batch", &batch).unwrap(), "https://a.example.com/");
        assert!(schedule_target("batch", &serde_json::json!({ "urls": [] })).is_err());
        let too_many: Vec<String> = (0..201).map(|i| format!("https://x{i}.example.com")).collect();
        assert!(schedule_target("batch", &serde_json::json!({ "urls": too_many })).is_err());
    }

    #[test]
    fn control_reason_reads_the_details_token_only() {
        let body = r#"{"error":{"code":"BAD_REQUEST","message":"x","details":{"reason":"invalid_timezone"}}}"#;
        assert_eq!(control_reason(body).as_deref(), Some("invalid_timezone"));
        assert_eq!(control_reason(r#"{"error":{"code":"BAD_REQUEST"}}"#), None);
        assert_eq!(control_reason("not json"), None);
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
