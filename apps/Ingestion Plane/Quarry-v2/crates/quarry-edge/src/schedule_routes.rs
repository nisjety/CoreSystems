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
//! | `GET /v1/schedules/:id/runs`           | The schedule's fires, keyset-paged        |
//! | `GET /v1/schedules/:id/requests/:rid`  | One trigger or backfill request row       |
//! | `DELETE /v1/schedules/:id`             | Soft-delete (Temporal stops; config kept) |
//!
//! Acceptance (gap-quarry §10.1 #5): "Operators never need direct
//! Temporal access for schedule work." This is the public surface
//! that closes that gap.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::resources::OverlapPolicy;

use crate::resource_routes::{
    optional_monitor_tag, saved_jobs_gate, tag_patch_body, TagPatchRequest,
};
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
///   token may activate it. With `source_id` a saved crawl definition supplies
///   the config instead.
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
    /// The operator label of a monitor (E3.5-6): trimmed, lowercased and checked against the
    /// monitor-tag grammar before the create is forwarded. Absent or blank is untagged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Per-kind payload: `url` for scrape and crawl, `urls` for batch, and
    /// optional `max_pages`, `max_depth`, `ingest`.
    #[serde(default)]
    pub config: serde_json::Value,
    /// Q4-E4.1-6d: the id of a saved crawl definition (`src_` and a ULID) that an executable
    /// schedule runs in place of `config`. The schedule's `target_ref` becomes this id and its
    /// config is built from the definition at create time. Only a crawl is schedulable (and a
    /// scrape with no options); a batch is refused. Answers 501 while `QUARRY_EDGE__SAVED_JOBS`
    /// is off, and 400 `config_conflicts_with_source_id` with a non-empty `config`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
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
    /// The operator label of the monitor (E3.5-6), normalised at the edge. Omitted when untagged.
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
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

/// `GET /v1/schedules/:id/runs` query (WS15.S3a). `limit` is clamped to the edge's page bounds
/// (default 25, at most 100). `cursor` is control's opaque `next_cursor` from the previous page.
#[derive(Debug, Default, Deserialize)]
pub struct ScheduleRunsQuery {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<String>,
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
    forward_json_status(state, method, path, org_id, body, None)
        .await
        .map(|(_, answer)| answer)
}

/// [`forward_json`] that also returns control's success status, so a queued request row answers
/// `202` as control does. `idempotency_key` (WS15.S3a: a caller's key, already validated) is
/// forwarded in place of a minted key, so a retry of the same request carries the same key.
async fn forward_json_status<TReq, TResp>(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    org_id: &str,
    body: Option<&TReq>,
    idempotency_key: Option<&str>,
) -> Result<(StatusCode, TResp), QuarryError>
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
    // WS15.S3a: a caller's key replaces the minted one, so a retry that repeats it reaches control
    // with the same key.
    let idem_key = match idempotency_key {
        Some(key) => key.to_owned(),
        None => {
            let minted: quarry_core::ids::kinds::RequestKind = quarry_core::ids::Id::new();
            minted.to_string()
        }
    };
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
            // WS15.S3a: control's conflict and unsupported answers keep their own codes (and reason).
            409 => ErrorCode::Conflict,
            501 => ErrorCode::Unsupported,
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
    let answer = serde_json::from_value::<TResp>(inner).map_err(|e| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("control-plane {method} {path} decode: {e}"),
        )
    })?;
    // The success status control answered (200, or 202 for a queued request row), passed on.
    let answered =
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Ok((answered, answer))
}

/// A tenant read of control (a GET), org from the verified claim. Inert when no control URL is
/// configured, as the lifecycle forwards are. The control view is returned as control wrote it.
async fn read_control(
    state: &AppState,
    path: &str,
    org_id: &str,
    query: &[(&str, String)],
) -> Result<serde_json::Value, QuarryError> {
    if state.control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "control plane URL not configured; schedule lifecycle routes are inert",
        ));
    }
    crate::control_client::get_json(state, path, org_id, query).await
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

/// Q4-E4.1-6d: the target and the config of an executable schedule that runs a saved crawl
/// definition. The source is read through control for the verified org (a missing or other-tenant
/// source is control's 404). Its definition is checked again with the live validators, because
/// the stored row is not trusted, and rebuilt into the fire config. The target is the source id,
/// and the URL the schedule fetches is gated as a plain create's is. Dark behind
/// `QUARRY_EDGE__SAVED_JOBS`.
async fn saved_source_schedule(
    state: &AppState,
    claims: &crate::auth::Claims,
    zdr: bool,
    request_id: &str,
    source_id: &str,
    kind: &str,
    config: &serde_json::Value,
) -> Result<(String, serde_json::Value), (StatusCode, Json<Envelope<()>>)> {
    saved_jobs_gate().map_err(|e| err_response(request_id, e))?;
    let id = source_id
        .parse::<quarry_core::ids::kinds::SourceKind>()
        .map_err(|_| {
            bad_schedule(
                request_id,
                "invalid_source_id",
                "source_id must be a source id (src_ and a ULID)",
            )
        })?;
    let config_given = match config {
        serde_json::Value::Null => false,
        serde_json::Value::Object(map) => !map.is_empty(),
        _ => true,
    };
    if config_given {
        return Err(bad_schedule(
            request_id,
            "config_conflicts_with_source_id",
            "send either config or source_id: the saved definition supplies the config",
        ));
    }
    // A batch names up to 200 URLs in its config and has no saved form.
    if kind == "batch" {
        return Err(bad_schedule(
            request_id,
            "saved_job_not_schedulable",
            "a batch schedule cannot run a saved job",
        ));
    }
    if state.control_base_url.is_empty() {
        return Err(err_response(
            request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "control plane URL not configured; schedule lifecycle routes are inert",
            ),
        ));
    }
    let source =
        crate::control_client::get_json(state, &format!("/v1/sources/{id}"), &claims.org_id, &[])
            .await
            .map_err(|e| err_response(request_id, e))?;
    let definition = source
        .get("config")
        .and_then(|source_config| source_config.get(crate::saved_job::DEFINITION_KEY))
        .filter(|definition| !definition.is_null())
        .ok_or_else(|| {
            bad_schedule(
                request_id,
                "source_has_no_definition",
                "the source has no saved job definition",
            )
        })?;
    let (Some(source_kind), Some(source_url)) = (
        source.get("kind").and_then(serde_json::Value::as_str),
        source.get("url").and_then(serde_json::Value::as_str),
    ) else {
        return Err(err_response(
            request_id,
            QuarryError::new(ErrorCode::Internal, "control source is missing kind or url"),
        ));
    };
    let canonical = crate::saved_job::check(
        source_kind,
        source_url,
        definition,
        crate::crawl_request::CrawlRequestSettings::current(),
    )
    .map_err(|fields| {
        err_response(
            request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "the source's saved job definition is no longer valid; save it again",
            )
            .with_details(serde_json::json!({
                "reason": "source_definition_invalid",
                "fields": crate::crawl_request::fields_json(&fields),
            })),
        )
    })?;
    // The saved job's kind must be the schedule's kind. Only a crawl is schedulable, and a scrape
    // only when it has no options: `saved_job::crawl_schedule_config` makes that call.
    let job_kind = canonical
        .get("job_kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if job_kind != kind {
        return Err(bad_schedule(
            request_id,
            "saved_job_kind_mismatch",
            format!("the saved job is a {job_kind}, not a {kind}"),
        ));
    }
    let mut fire_config =
        crate::saved_job::crawl_schedule_config(source_url, &canonical).map_err(|field| {
            err_response(
                request_id,
                QuarryError::new(ErrorCode::BadRequest, field.message.clone()).with_details(
                    serde_json::json!({
                        "reason": "saved_job_not_schedulable",
                        "fields": crate::crawl_request::fields_json(std::slice::from_ref(&field)),
                    }),
                ),
            )
        })?;
    if let Some(map) = fire_config.as_object_mut() {
        map.insert(
            "source_id".to_owned(),
            serde_json::Value::String(id.to_string()),
        );
    }
    // The same URL shape check as a plain create, then the same gate on the URL that is fetched.
    schedule_target(kind, &fire_config)
        .map_err(|(reason, message)| bad_schedule(request_id, reason, message))?;
    gate_schedule_urls(state, claims, zdr, [source_url], request_id)?;
    Ok((id.to_string(), fire_config))
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
    let enabled =
        crate::job_results_routes::flag_is_on(std::env::var(ENV_MONITOR_TAGS).ok().as_deref());
    monitor_tag_named_gate(req.tag.as_deref(), enabled)
        .map_err(|e| err_response(&request_id, e))?;
    let tag = optional_monitor_tag(req.tag.as_deref()).map_err(|e| err_response(&request_id, e))?;

    // W2 change-monitoring path: `preset` signals a recurring change monitor. The
    // org comes from the verified JWT (never the body), the cron from the preset on
    // the control side, and the monitor starts when created, as it always has.
    if let Some(preset) = req.preset.as_deref() {
        // Q4-E4.1-6d: a change monitor watches its target_ref; a source_id would be dropped here.
        if req.source_id.is_some() {
            return Err(bad_schedule(
                &request_id,
                "source_id_not_for_monitor",
                "source_id applies to an executable schedule, not a change monitor",
            ));
        }
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
            tag,
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
    // Q4-E4.1-6d: a saved crawl definition is the target and supplies the config. Otherwise the
    // config names its own URL, as it always has.
    let (target_ref, config) = match req.source_id.as_deref() {
        Some(source_id) => {
            saved_source_schedule(
                &state,
                &claims,
                zdr,
                &request_id,
                source_id,
                &target_kind,
                &req.config,
            )
            .await?
        }
        None => {
            let target_ref = schedule_target(&target_kind, &req.config)
                .map_err(|(reason, message)| bad_schedule(&request_id, reason, message))?;
            // Every URL the schedule will fetch is gated now: a batch names up to 200, the
            // others one.
            let urls = schedule_urls(&target_kind, &req.config, &target_ref);
            gate_schedule_urls(
                &state,
                &claims,
                zdr,
                urls.iter().map(String::as_str),
                &request_id,
            )?;
            (target_ref, req.config.clone())
        }
    };
    let body = ControlScheduleCreate {
        org_id: claims.org_id.clone(),
        created_by: claims.user_id.clone(),
        name: req.name.trim().to_owned(),
        cron: Some(cron.to_owned()),
        target_kind,
        target_ref,
        preset: None,
        config,
        timezone: req.timezone.clone(),
        overlap_policy: req.overlap_policy,
        catchup_window_s: req.catchup_window_s,
        pause_on_failure: req.pause_on_failure,
        activate: req.activate.unwrap_or(false) && !claims.is_service(),
        zdr: None,
        tag,
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
    let id = parse_schedule_id(&request_id, &id)?;
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
/// forward `pause_schedule` makes. The id is checked before the path is built, as the handlers
/// check theirs (a malformed id is a 400 and no control call). Unpause stays refused for a ZDR
/// caller.
pub(crate) async fn pause_for_posture(
    state: &AppState,
    schedule_id: &str,
    org_id: &str,
) -> Result<(), QuarryError> {
    let schedule_id = check_schedule_id(schedule_id)?;
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
    let id = parse_schedule_id(&request_id, &id)?;
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Envelope<serde_json::Value>>), (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "schedules",
        "trigger",
    )?;
    let id = parse_schedule_id(&request_id, &id)?;
    // WS15.S3a: a usable caller key is forwarded in place of a minted one (the public convention:
    // an invalid value is ignored, as if no key were sent). Control refuses a repeat of a key it holds.
    let idempotency_key = crate::handoff::idempotency_key_from_request(&headers);
    let path = format!("/v1/schedules/{id}/trigger");
    // Control answers 202 with the queued request row when its flag is on. That status is passed on.
    let (status, summary) = forward_json_status::<(), serde_json::Value>(
        &state,
        reqwest::Method::POST,
        &path,
        &claims.org_id,
        None,
        idempotency_key.as_deref(),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok((status, Json(Envelope::ok(request_id, summary))))
}

pub async fn backfill_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<BackfillRequest>,
) -> Result<(StatusCode, Json<Envelope<serde_json::Value>>), (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "schedules",
        "backfill",
    )?;
    let id = parse_schedule_id(&request_id, &id)?;
    if req.end_at <= req.start_at {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "backfill end_at must be strictly after start_at",
            ),
        ));
    }
    // WS15.S3a: the caller's key, as on trigger (see there).
    let idempotency_key = crate::handoff::idempotency_key_from_request(&headers);
    let path = format!("/v1/schedules/{id}/backfill");
    let (status, summary) = forward_json_status::<BackfillRequest, serde_json::Value>(
        &state,
        reqwest::Method::POST,
        &path,
        &claims.org_id,
        Some(&req),
        idempotency_key.as_deref(),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok((status, Json(Envelope::ok(request_id, summary))))
}

/// `GET /v1/schedules/:id/requests/:rid` (WS15.S3a): the tenant's read of one trigger or backfill
/// request row, control's view (status, attempts, params, result and a short error; never the
/// claiming worker or the lease). Both ids are parsed before they reach the path, and the org is the
/// verified claim. A row of another schedule or org is control's 404. No ZDR refusal: the view
/// carries ids, status and reasons, as the schedule list does. Dark behind
/// `QUARRY_EDGE__SCHEDULE_RUNS`, as the runs list is: the switch is read before the ids, so a dark
/// edge answers 501 whoever asks.
pub async fn get_schedule_request(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path((id, rid)): Path<(String, String)>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    let enabled =
        crate::job_results_routes::flag_is_on(std::env::var(ENV_SCHEDULE_RUNS).ok().as_deref());
    schedule_runs_gate(enabled).map_err(|e| err_response(&request_id, e))?;
    let id = parse_schedule_id(&request_id, &id)?;
    let rid = parse_request_id(&request_id, &rid)?;
    let path = format!("/v1/schedules/{id}/requests/{rid}");
    let view = read_control(&state, &path, &claims.org_id, &[])
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, view)))
}

/// Turns the schedule run list on. Off (the default), `GET /v1/schedules/:id/runs` answers 501
/// before control is called. Read per request from the edge's environment.
pub const ENV_SCHEDULE_RUNS: &str = "QUARRY_EDGE__SCHEDULE_RUNS";

/// The check the run list makes first, as `job_results_routes::gate` makes its own: a dark route
/// answers 501 whoever asks, and the message names the switch.
fn schedule_runs_gate(enabled: bool) -> Result<(), QuarryError> {
    if enabled {
        return Ok(());
    }
    Err(QuarryError::new(
        ErrorCode::Unsupported,
        format!("schedule runs are not enabled on this edge: set {ENV_SCHEDULE_RUNS}=1"),
    ))
}

/// `GET /v1/schedules/:id/runs` (WS15.S3a): the fires of one schedule, newest first, as control's
/// page of job rows (`items`, `next_cursor`). `limit` is clamped to the page bounds and `cursor`
/// passes through. Dark behind `QUARRY_EDGE__SCHEDULE_RUNS`: the switch is read before the id, so a
/// dark edge answers 501 whoever asks. The id is parsed before it reaches the path, and the org is
/// the verified claim. No ZDR refusal, as for the schedule list.
pub async fn list_schedule_runs(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
    Query(q): Query<ScheduleRunsQuery>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    let enabled =
        crate::job_results_routes::flag_is_on(std::env::var(ENV_SCHEDULE_RUNS).ok().as_deref());
    schedule_runs_gate(enabled).map_err(|e| err_response(&request_id, e))?;
    let id = parse_schedule_id(&request_id, &id)?;
    let path = format!("/v1/schedules/{id}/runs");
    let limit = q
        .limit
        .unwrap_or(quarry_core::pagination::DEFAULT_PAGE_LIMIT)
        .clamp(1, quarry_core::pagination::MAX_PAGE_LIMIT);
    let mut query = vec![("limit", limit.to_string())];
    if let Some(cursor) = q.cursor {
        query.push(("cursor", cursor));
    }
    let page = read_control(&state, &path, &claims.org_id, &query)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, page)))
}

pub async fn delete_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    let id = parse_schedule_id(&request_id, &id)?;
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

/// Parses a schedule id from a path segment (`sch_` and a ULID). A value that is not one is a 400
/// before any control call, so a path built from the parsed id's Display cannot carry a `/` or `..`.
fn parse_schedule_id(
    request_id: &str,
    raw: &str,
) -> Result<quarry_core::ids::kinds::ScheduleKind, (StatusCode, Json<Envelope<()>>)> {
    check_schedule_id(raw).map_err(|e| err_response(request_id, e))
}

/// The check [`parse_schedule_id`] makes, with the bare error, for a caller that is not a handler
/// and answers with a [`QuarryError`] (the posture pause, which the preflight spawns).
fn check_schedule_id(raw: &str) -> Result<quarry_core::ids::kinds::ScheduleKind, QuarryError> {
    raw.parse::<quarry_core::ids::kinds::ScheduleKind>()
        .map_err(|_| QuarryError::new(ErrorCode::BadRequest, "invalid schedule id"))
}

/// Parses a request id from a path segment (`req_` and a ULID), with the same 400 shape as a
/// schedule id.
fn parse_request_id(
    request_id: &str,
    raw: &str,
) -> Result<quarry_core::ids::kinds::RequestKind, (StatusCode, Json<Envelope<()>>)> {
    raw.parse::<quarry_core::ids::kinds::RequestKind>()
        .map_err(|_| {
            err_response(
                request_id,
                QuarryError::new(ErrorCode::BadRequest, "invalid request id"),
            )
        })
}

/// Turns the monitor tags on. Off (the default), the tag PATCH answers 501 before control is
/// called, and so does a create or a monitor list that names a tag. Read per request from the
/// edge's environment.
pub const ENV_MONITOR_TAGS: &str = "QUARRY_EDGE__MONITOR_TAGS";

/// The check the tag PATCH makes first, as `schedule_runs_gate` does: a dark route answers 501
/// whoever asks, and the message names the switch.
pub(crate) fn monitor_tags_gate(enabled: bool) -> Result<(), QuarryError> {
    if enabled {
        return Ok(());
    }
    Err(QuarryError::new(
        ErrorCode::Unsupported,
        format!("monitor tags are not enabled on this edge: set {ENV_MONITOR_TAGS}=1"),
    ))
}

/// The check a create or a monitor list makes before control when it names a tag (E3.5-6), so a
/// dark edge never forwards one. A blank or absent tag is untagged and passes; a named tag answers
/// the same 501 as `monitor_tags_gate` while the switch is off.
pub(crate) fn monitor_tag_named_gate(raw: Option<&str>, enabled: bool) -> Result<(), QuarryError> {
    let names_a_tag = raw.is_some_and(|tag| !tag.trim().is_empty());
    if names_a_tag {
        return monitor_tags_gate(enabled);
    }
    Ok(())
}

/// `PATCH /v1/schedules/{id}` (E3.5-6): set the monitor's tag, or clear it with `""`. The id must
/// parse as a schedule id, so the path segment cannot reach another control route; the org is the
/// verified JWT org. The answer is control's updated schedule view, passed through unchanged. This
/// route does not call the ZDR refusal: it only relabels a row that already exists, as pause and
/// delete do, and starts no run and creates no resource. Dark behind `QUARRY_EDGE__MONITOR_TAGS`:
/// the switch is read before the id, so a dark edge answers 501 whoever asks.
pub async fn patch_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
    Json(req): Json<TagPatchRequest>,
) -> Result<Json<Envelope<serde_json::Value>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    let enabled =
        crate::job_results_routes::flag_is_on(std::env::var(ENV_MONITOR_TAGS).ok().as_deref());
    monitor_tags_gate(enabled).map_err(|e| err_response(&request_id, e))?;
    let id = id
        .parse::<quarry_core::ids::kinds::ScheduleKind>()
        .map_err(|_| {
            err_response(
                &request_id,
                QuarryError::new(ErrorCode::BadRequest, "invalid schedule id"),
            )
        })?;
    let body = tag_patch_body(&req).map_err(|e| err_response(&request_id, e))?;
    let path = format!("/v1/schedules/{id}");
    let summary = forward_json::<TagPatchRequest, serde_json::Value>(
        &state,
        reqwest::Method::PATCH,
        &path,
        &claims.org_id,
        Some(&body),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, summary)))
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
            tag: None,
            config: serde_json::json!({}),
            source_id: None,
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
            tag: None,
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

    #[test]
    fn schedule_runs_is_dark_until_its_switch_is_on() {
        let dark = schedule_runs_gate(false).expect_err("a dark run list answers 501");
        assert_eq!(dark.code, ErrorCode::Unsupported);
        assert!(dark.message.contains(ENV_SCHEDULE_RUNS));
        assert!(schedule_runs_gate(true).is_ok());
        assert!(
            !crate::job_results_routes::flag_is_on(None),
            "the switch is off when unset"
        );
    }

    #[test]
    fn monitor_tag_patch_is_dark_until_its_switch_is_on() {
        let dark = monitor_tags_gate(false).expect_err("a dark tag patch answers 501");
        assert_eq!(dark.code, ErrorCode::Unsupported);
        assert!(dark.message.contains(ENV_MONITOR_TAGS));
        assert!(monitor_tags_gate(true).is_ok());
        assert!(
            !crate::job_results_routes::flag_is_on(None),
            "the switch is off when unset"
        );
    }

    #[test]
    fn monitor_tag_create_and_filter_are_dark_until_the_switch_is_on() {
        // A create body or a monitor list that names a tag answers the dark 501 before control.
        for named in ["pricing", " Pricing ", "a"] {
            let dark = monitor_tag_named_gate(Some(named), false)
                .expect_err("a named tag on a create or a filter answers 501 while dark");
            assert_eq!(dark.code, ErrorCode::Unsupported);
            assert!(dark.message.contains(ENV_MONITOR_TAGS));
            assert!(monitor_tag_named_gate(Some(named), true).is_ok());
        }
        // An absent or blank tag names no monitor tag: it is untagged, so it passes while dark.
        for untagged in [None, Some(""), Some("   ")] {
            assert!(
                monitor_tag_named_gate(untagged, false).is_ok(),
                "{untagged:?}"
            );
        }
    }
}
