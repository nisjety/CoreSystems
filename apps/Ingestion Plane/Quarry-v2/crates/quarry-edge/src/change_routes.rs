//! `/v1/change/*` endpoints — versioned change history.
//!
//! Cycle 30 / cluster #9.
//!
//! Three routes:
//!
//! | Method | Path                | Effect                                        |
//! | ------ | ------------------- | --------------------------------------------- |
//! | POST   | `/v1/change/check`  | Compare a URL's latest baseline; classify     |
//! | GET    | `/v1/change/latest` | Most-recent baseline for a URL                |
//! | GET    | `/v1/change/history`| Paginated walk of the baseline chain          |
//!
//! When the edge has a `PostgresBaselineStore` wired (postgres-queue
//! feature + DATABASE_URL), the routes serve from local DB.
//! Otherwise they forward to control plane via the existing
//! HMAC-signed forward path.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use quarry_core::change_history::{BaselineSnapshot, ChangeRecord};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
#[cfg(feature = "postgres-queue")]
use quarry_core::ids::kinds::SnapshotKind;
#[cfg(feature = "postgres-queue")]
use quarry_core::resources::Snapshot;
#[cfg(feature = "postgres-queue")]
use quarry_core::tracked_snapshot::baseline_chain_to_snapshot;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub url: String,
    /// Optional fresh fingerprint. When provided, the route compares
    /// directly without re-fetching. When omitted, the route returns
    /// just the latest-baseline-status (caller fetches separately).
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue`; wired in follow-up
    pub fresh_fingerprint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UrlQuery {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub url: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn err_response(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

fn validate_url(s: &str) -> Result<(), QuarryError> {
    if s.trim().is_empty() {
        return Err(QuarryError::new(ErrorCode::BadRequest, "url required"));
    }
    url::Url::parse(s)
        .map(|_| ())
        .map_err(|e| QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}")))
}

pub async fn check(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CheckRequest>,
) -> Result<Json<Envelope<ChangeRecord>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "change_history",
        "check",
    )?;
    validate_url(&req.url).map_err(|e| err_response(&request_id, e))?;

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let fresh = req.fresh_fingerprint.as_deref().ok_or_else(|| {
            err_response(
                &request_id,
                QuarryError::new(
                    ErrorCode::BadRequest,
                    "fresh_fingerprint required when running against local store",
                ),
            )
        })?;
        let record = store
            .compare_snapshot(&claims.org_id, &req.url, fresh)
            .await
            .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(Envelope::ok(request_id, record)));
    }

    // Without a local store, the operation requires control-plane
    // round-trip — return 501 with a hint until cycle 31 wires the
    // forward path (Go side handler pending).
    let _ = state;
    let _ = claims;
    Err(err_response(
        &request_id,
        QuarryError::new(
            ErrorCode::Unsupported,
            "change tracking requires postgres-queue feature + DATABASE_URL on the edge \
             OR a control-plane handler — see docs/CHANGE_TRACKING.md",
        ),
    ))
}

pub async fn latest(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<UrlQuery>,
) -> Result<Json<Envelope<BaselineSnapshot>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    validate_url(&q.url).map_err(|e| err_response(&request_id, e))?;

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let latest = store
            .load_latest(&claims.org_id, &q.url)
            .await
            .map_err(|e| err_response(&request_id, e))?
            .ok_or_else(|| {
                err_response(
                    &request_id,
                    QuarryError::new(ErrorCode::NotFound, format!("no baseline for {}", q.url)),
                )
            })?;
        return Ok(Json(Envelope::ok(request_id, latest)));
    }
    let _ = state;
    let _ = claims;
    Err(err_response(
        &request_id,
        QuarryError::new(ErrorCode::Unsupported, "postgres-queue feature required"),
    ))
}

pub async fn history(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Envelope<Vec<BaselineSnapshot>>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    validate_url(&q.url).map_err(|e| err_response(&request_id, e))?;
    let limit = q.limit.unwrap_or(50).clamp(1, 1000);

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let hist = store
            .load_history(&claims.org_id, &q.url, limit)
            .await
            .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(Envelope::ok(request_id, hist)));
    }
    let _ = state;
    let _ = claims;
    let _ = limit;
    Err(err_response(
        &request_id,
        QuarryError::new(ErrorCode::Unsupported, "postgres-queue feature required"),
    ))
}

// ============================================================================
// GET /v1/change/diff — the two versions of a tracked page and the diff (INNH-4).
//
// `?url=` alone compares the latest two baselines; `&from=&to=` compare two named baselines of
// the url; `?diff_id=` reads the diff a change webhook announced. `include=markdown` adds the
// two pages' text. 501 unless QUARRY_EDGE__CHANGE_TEXT_DIFF is on; refused for a ZDR reader.
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub diff_id: Option<String>,
    #[serde(default)]
    pub include: Option<String>,
}

#[cfg(feature = "postgres-queue")]
pub async fn diff(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<DiffQuery>,
) -> Result<
    Json<Envelope<quarry_core::change_history::ChangeDiffView>>,
    (StatusCode, Json<Envelope<()>>),
> {
    use crate::change_diff::{change_text_diff_enabled, diff_core, gate, DiffParams};

    let request_id = crate::request_id::current();
    gate(
        change_text_diff_enabled(),
        claims.effective_zdr_mode(None).is_active(),
    )
    .map_err(|e| err_response(&request_id, e))?;
    let params = DiffParams::parse(
        q.url.as_deref(),
        q.from.as_deref(),
        q.to.as_deref(),
        q.diff_id.as_deref(),
        q.include.as_deref(),
    )
    .map_err(|e| err_response(&request_id, e))?;
    if let Some(url) = params.url.as_deref() {
        validate_url(url).map_err(|e| err_response(&request_id, e))?;
    }
    let Some(store) = state.baseline_store.as_ref() else {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "change text diffs need the postgres-queue feature + DATABASE_URL on the edge",
            ),
        ));
    };
    let view = diff_core(
        store.as_ref(),
        state.artifacts.as_ref(),
        &claims.org_id,
        &params,
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, view)))
}

#[cfg(not(feature = "postgres-queue"))]
pub async fn diff(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<DiffQuery>,
) -> Result<
    Json<Envelope<quarry_core::change_history::ChangeDiffView>>,
    (StatusCode, Json<Envelope<()>>),
> {
    let request_id = crate::request_id::current();
    let _ = (&state, &claims, &q);
    Err(err_response(
        &request_id,
        QuarryError::new(
            ErrorCode::Unsupported,
            "change text diffs need the postgres-queue feature",
        ),
    ))
}

// ============================================================================
// POST /v1/change/snapshot — PromoteTrackedResultToSnapshot.
//
// Projects a stored baseline (and its predecessor, when one exists) into
// the public `resources::Snapshot` wire shape so dashboards can render a
// tracked URL's latest version through the same surface as source-driven
// snapshots. Pure projection: nothing is persisted here.
// ============================================================================

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Deserialize)]
pub struct PromoteQuery {
    pub url: String,
}

/// `POST /v1/change/snapshot?url=…` — promote the latest tracked baseline
/// to the public Snapshot shape. Requires `postgres-queue` + DATABASE_URL
/// (reads through PostgresBaselineStore); 501 with a hint otherwise.
#[cfg(feature = "postgres-queue")]
pub async fn promote_snapshot(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<PromoteQuery>,
) -> Result<Json<Envelope<Snapshot>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "change_snapshot",
        "snapshot",
    )?;
    validate_url(&q.url).map_err(|e| err_response(&request_id, e))?;
    let Some(store) = state.baseline_store.as_ref() else {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "snapshot promotion requires postgres-queue feature + DATABASE_URL",
            ),
        ));
    };
    // load_history(2) returns [latest, prev] when both exist; the chain
    // variant derives new/unchanged/modified from the fingerprint pair.
    let chain: Vec<BaselineSnapshot> = store
        .load_history(&claims.org_id, &q.url, 2)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    let latest = match chain.first() {
        Some(b) => b,
        None => {
            return Err(err_response(
                &request_id,
                QuarryError::new(ErrorCode::NotFound, format!("no baseline for {}", q.url)),
            ))
        }
    };
    if latest.org_id != claims.org_id {
        // Defense in depth: the store is org-scoped at the SQL level, but
        // the projection must never widen it.
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::NotFound, "no baseline for this org"),
        ));
    }
    let prev_fingerprint = chain.get(1).map(|p| p.fingerprint.as_str());
    let snapshot = baseline_chain_to_snapshot(latest, prev_fingerprint)
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, snapshot)))
}

#[cfg(not(feature = "postgres-queue"))]
pub async fn promote_snapshot(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<UrlQuery>,
) -> Result<Json<Envelope<()>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "change_snapshot",
        "snapshot",
    )?;
    validate_url(&q.url).map_err(|e| err_response(&request_id, e))?;
    let _ = (&state, &claims);
    Err(err_response(
        &request_id,
        QuarryError::new(
            ErrorCode::Unsupported,
            "postgres-queue feature required for snapshot promotion",
        ),
    ))
}

// ============================================================================
// POST /v1/change/refresh — ScheduleRefreshRun.
//
// Enqueues an immediate re-check of a tracked URL onto the org-scoped
// durable frontier (`PostgresRequestQueue`, the same SKIP LOCKED bridge
// the orchestrator already pops via /v1/internal/queues). The queue row
// is created on demand, so a refresh works even for URLs that never had
// a crawl. Payload carries kind:"change_refresh" + url so any consumer
// (orchestrator activity or edge poller) knows what to execute.
// ============================================================================

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub url: String,
    #[serde(default)]
    pub priority: Option<quarry_runtime::request_queue::Priority>,
}

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub request_id: String,
    /// False when an identical request was already queued (idempotent enqueue).
    pub accepted: bool,
}

#[cfg(feature = "postgres-queue")]
pub async fn schedule_refresh(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<Envelope<RefreshResponse>>, (StatusCode, Json<Envelope<()>>)> {
    use quarry_runtime::postgres_queue::PostgresRequestQueue;
    use quarry_runtime::request_queue::{Priority, RequestQueue};

    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "change_snapshot",
        "refresh",
    )?;
    validate_url(&req.url).map_err(|e| err_response(&request_id, e))?;
    // D2-018: a URL queued here is fetched later by a worker, so the web policy's host
    // rules apply before it is queued, as they do for crawl. A denied URL is never queued.
    let decision = crate::web_policy_edge::decide(&state.policy, &claims, false, None)
        .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    crate::web_policy_edge::gate_url(&decision, &req.url)
        .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    let Some(pool) = state.queue_pool.as_ref() else {
        return Err(err_response(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "refresh scheduling requires postgres-queue feature + DATABASE_URL",
            ),
        ));
    };
    let queue = PostgresRequestQueue::bind(
        pool.clone(),
        claims.org_id.clone(),
        "change-refresh",
        "scrape",
        std::time::Duration::from_secs(300),
    )
    .await
    .map_err(|e| err_response(&request_id, e))?;
    let refresh_request_id = format!("chg_{}", SnapshotKind::new().ulid());
    let payload = serde_json::json!({
        "kind": "change_refresh",
        "url": req.url,
        "org_id": claims.org_id,
    });
    let priority = req.priority.unwrap_or(Priority::Default);
    let accepted = queue
        .enqueue(refresh_request_id.clone(), req.url, priority, payload)
        .await
        .map_err(|e| err_response(&request_id, e))?;
    Ok(Json(Envelope::ok(
        request_id,
        RefreshResponse {
            request_id: refresh_request_id,
            accepted,
        },
    )))
}

#[cfg(not(feature = "postgres-queue"))]
pub async fn schedule_refresh(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Envelope<()>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::zdr_refusal::refuse_for_claims(
        &state,
        &claims,
        None,
        &request_id,
        "change_snapshot",
        "refresh",
    )?;
    let _ = (&state, &claims, &req);
    Err(err_response(
        &request_id,
        QuarryError::new(
            ErrorCode::Unsupported,
            "postgres-queue feature required for refresh scheduling",
        ),
    ))
}

// ============================================================================
// /v1/internal/change/record — the WRITE side of change tracking.
//
// W2 recurring change-monitoring. The orchestrator's Go CheckChange activity
// can't touch the Rust in-process baseline store, so persistence routes
// through this internal endpoint. Unlike the JWT-gated /v1/change/* routes,
// org_id arrives in the BODY because the trusted orchestrator calls on behalf
// of MANY tenants with one service identity — the org_id was already verified
// at schedule-creation time (the edge stamped it from the JWT in create_schedule).
//
// Auth is a shared service token (QUARRY_EDGE_INTERNAL_TOKEN), NOT a JWT, so
// this route lives OUTSIDE require_auth. It runs the full
// compare → save_baseline → (on change) store-diff-artifact + create_diff_record
// sequence in-process in Rust and returns the populated result.
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RecordRequest {
    pub org_id: String,
    pub url: String,
    pub fresh_fingerprint: String,
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue` in record_check
    pub run_id: String,
    /// Zero-data-retention posture of the schedule that produced this
    /// fingerprint, declared by the trusted orchestrator. When true,
    /// post-persistence side effects (the durable `ChangeDetected` event AND
    /// the control webhook) are suppressed — fail closed.
    #[serde(default)]
    pub zdr: Option<bool>,
    /// INNH-4: the stored markdown of the fresh fetch, as `run_page` returned it
    /// (`markdown_artifact_id`). Kept on the new baseline only with
    /// `QUARRY_EDGE__CHANGE_TEXT_DIFF` on, after it is verified to be a markdown artifact of
    /// `org_id`. Absent under ZDR and from an older orchestrator.
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue` in record_core
    pub markdown_artifact_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RecordResponse {
    /// "new" | "unchanged" | "changed" | "unreachable".
    pub status: String,
    pub changed: bool,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prev_fingerprint: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub baseline_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_id: Option<String>,
}

/// Returns true when the edge is running with auth dev-bypass — used to let
/// internal calls through without a token in local dev (mirrors require_auth).
fn internal_dev_bypass() -> bool {
    std::env::var("QUARRY_EDGE_AUTH_DEV_BYPASS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Pure token check, extracted so it's testable without mutating process env.
/// fail-closed: an unset `expected` token (and no dev bypass) refuses the call.
fn check_internal_token(
    expected: &str,
    provided: Option<&str>,
    dev_bypass: bool,
) -> Result<(), QuarryError> {
    if dev_bypass {
        return Ok(());
    }
    if expected.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "internal change endpoint not configured (set QUARRY_EDGE_INTERNAL_TOKEN)",
        ));
    }
    let token = provided
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if !token.is_empty() && ct_eq(token.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(QuarryError::new(
            ErrorCode::Unauthorized,
            "invalid internal token",
        ))
    }
}

/// Constant-time byte comparison for shared-secret tokens — avoids the
/// first-mismatch timing side-channel of `==`. A length difference returns
/// early (token length is not itself secret).
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub(crate) fn verify_internal_token(headers: &HeaderMap) -> Result<(), QuarryError> {
    let expected = std::env::var("QUARRY_EDGE_INTERNAL_TOKEN").unwrap_or_default();
    let provided = headers.get("authorization").and_then(|v| v.to_str().ok());
    check_internal_token(&expected, provided, internal_dev_bypass())
}

pub async fn record_internal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RecordRequest>,
) -> Result<Json<RecordResponse>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    verify_internal_token(&headers).map_err(|e| err_response(&request_id, e))?;
    validate_url(&req.url).map_err(|e| err_response(&request_id, e))?;
    if req.org_id.trim().is_empty() {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "org_id required"),
        ));
    }
    if req.fresh_fingerprint.trim().is_empty() {
        return Err(err_response(
            &request_id,
            QuarryError::new(ErrorCode::BadRequest, "fresh_fingerprint required"),
        ));
    }

    // WS4.H: a run declared ZDR by the orchestrator must not persist a baseline. The
    // check sits before the store is touched, so a refusal writes nothing.
    crate::zdr_refusal::refuse(
        &state,
        &req.org_id,
        req.zdr.unwrap_or(false),
        &request_id,
        "change_record_internal",
        "record",
    )?;

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let text_diff = crate::change_diff::change_text_diff_enabled();
        let resp = record_check(
            &state,
            store.as_ref(),
            state.artifacts.as_ref(),
            &req,
            text_diff,
        )
        .await
        .map_err(|e| err_response(&request_id, e))?;
        return Ok(Json(resp));
    }

    let _ = &state;
    Err(err_response(
        &request_id,
        QuarryError::new(
            ErrorCode::Unsupported,
            "change recording requires postgres-queue feature + DATABASE_URL on the edge",
        ),
    ))
}

/// Runs [`record_core`], then the post-persistence fan-out (durable event + signed webhook).
#[cfg(feature = "postgres-queue")]
async fn record_check(
    state: &AppState,
    store: &dyn crate::change_diff::ChangeStore,
    artifacts: &dyn quarry_runtime::artifact_store::ArtifactStore,
    req: &RecordRequest,
    text_diff: bool,
) -> Result<RecordResponse, QuarryError> {
    let (resp, record, run_kind) = record_core(store, artifacts, req, text_diff).await?;
    let zdr = quarry_core::zdr::ZdrMode::from(req.zdr.unwrap_or(false));
    crate::change_webhook::emit_change_side_effects(state, &record, run_kind, zdr).await;
    Ok(resp)
}

/// The compare → save_baseline → (on change) diff sequence. The baseline/diff store is
/// Rust-local; the diff artifact body is stored via the same ArtifactStore that page runs use.
///
/// `text_diff` is `QUARRY_EDGE__CHANGE_TEXT_DIFF`. Off, the sequence is what it always was: the
/// baseline keeps no artifact, a change stores a one-line fingerprint note (format `text`), and
/// the returned `ChangeRecord` has no `new_baseline` or `diff_id`. On (and the schedule not ZDR),
/// INNH-4 applies: the baseline keeps the fetch's markdown artifact, a change stores the JSON
/// paragraph diff (format `json`), and the record that goes to the event log and the webhook
/// carries `new_baseline` and `diff_id`.
#[cfg(feature = "postgres-queue")]
async fn record_core(
    store: &dyn crate::change_diff::ChangeStore,
    artifacts: &dyn quarry_runtime::artifact_store::ArtifactStore,
    req: &RecordRequest,
    text_diff: bool,
) -> Result<
    (
        RecordResponse,
        ChangeRecord,
        Option<quarry_core::ids::kinds::RunKind>,
    ),
    QuarryError,
> {
    use chrono::Utc;
    use quarry_core::change_history::{ChangeStatus, DiffRecord};
    // These are concrete prefixed-ULID types (`kinds::X` is a type alias for
    // `Id<X marker>`); `X::new()` mints one and `.ulid()` is the raw ULID.
    use quarry_core::ids::kinds::{ArtifactKind, RunKind};

    let mut record = store
        .compare_snapshot(&req.org_id, &req.url, &req.fresh_fingerprint)
        .await?;
    let now = Utc::now();

    let prev_fingerprint = record
        .prev_baseline
        .as_ref()
        .map(|b| b.fingerprint.clone())
        .unwrap_or_default();

    // The orchestrator's scheduled run ids derive from Temporal UUIDs and are
    // not ULIDs, so they won't parse into a typed RunKind — that's fine: the
    // run linkage lives in the event log, and baseline.run_id is optional.
    let run_kind: Option<RunKind> = req.run_id.parse().ok();

    let status_str = match record.status {
        ChangeStatus::New => "new",
        ChangeStatus::Unchanged => "unchanged",
        ChangeStatus::Changed => "changed",
        ChangeStatus::Unreachable => "unreachable",
    };

    let mut resp = RecordResponse {
        status: status_str.to_string(),
        changed: record.status == ChangeStatus::Changed,
        fingerprint: req.fresh_fingerprint.clone(),
        prev_fingerprint,
        baseline_id: String::new(),
        diff_id: None,
    };

    // Persist a new baseline for New or Changed; Unchanged/Unreachable persist
    // nothing (intentional storage saving — compare_snapshot is side-effect-free).
    let persists = matches!(record.status, ChangeStatus::New | ChangeStatus::Changed);
    // The text path needs the flag and a schedule that is not ZDR. A ZDR-declared run stores no
    // page text (run_page returns no markdown id for it), and in observe mode, where its
    // baseline row is still written, no text or diff may follow it either.
    let text_mode = text_diff && !req.zdr.unwrap_or(false);

    // Fail closed before anything is saved: the id came over the wire, so it is checked to be a
    // markdown artifact this org can read. Another org's id reads as not found and is refused.
    let markdown_id: Option<ArtifactKind> = match req
        .markdown_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(raw) if text_mode && persists => {
            Some(crate::change_diff::verify_markdown_artifact(artifacts, &req.org_id, raw).await?)
        }
        _ => None,
    };

    let mut new_baseline: Option<BaselineSnapshot> = None;
    if persists {
        let baseline_id = format!("bln_{}", ArtifactKind::new().ulid());
        let baseline = BaselineSnapshot {
            baseline_id: baseline_id.clone(),
            org_id: req.org_id.clone(),
            source_url: req.url.clone(),
            fingerprint: req.fresh_fingerprint.clone(),
            artifact_id: markdown_id,
            // Chain from the already-loaded prev — no second load_latest.
            prev_baseline_id: record.prev_baseline.as_ref().map(|b| b.baseline_id.clone()),
            captured_at: now,
            run_id: run_kind.clone(),
        };
        store.save_baseline(&baseline).await?;
        resp.baseline_id = baseline_id;
        new_baseline = Some(baseline);
    }

    // On Changed, persist a diff record. DiffRecord.artifact_id is required, so
    // store the diff body first.
    if record.status == ChangeStatus::Changed {
        let prev = record.prev_baseline.as_ref().ok_or_else(|| {
            QuarryError::new(ErrorCode::Internal, "changed status without prev baseline")
        })?;
        let artifact_run: RunKind = run_kind.clone().unwrap_or_default();
        let diff_id = format!("diff_{}", ArtifactKind::new().ulid());
        let diff = if text_mode {
            let to = new_baseline.as_ref().ok_or_else(|| {
                QuarryError::new(ErrorCode::Internal, "changed status without a new baseline")
            })?;
            let (handle, summary) = crate::change_diff::store_text_diff(
                artifacts,
                &req.org_id,
                &artifact_run,
                prev,
                to,
            )
            .await?;
            DiffRecord {
                diff_id: diff_id.clone(),
                org_id: req.org_id.clone(),
                from_baseline_id: prev.baseline_id.clone(),
                to_baseline_id: resp.baseline_id.clone(),
                source_url: req.url.clone(),
                format: quarry_core::change_history::TEXT_DIFF_FORMAT.to_string(),
                artifact_id: handle.artifact_id,
                summary: Some(summary),
                created_at: now,
            }
        } else {
            // We hold fingerprints (not the prev content), so the honest diff artifact
            // records the fingerprint transition.
            let summary = format!(
                "content fingerprint changed: {} → {}",
                prev.fingerprint, req.fresh_fingerprint
            );
            let handle = artifacts
                .put(
                    &req.org_id,
                    &artifact_run,
                    &req.fresh_fingerprint,
                    "raw",
                    summary.clone().into_bytes(),
                )
                .await?;
            DiffRecord {
                diff_id: diff_id.clone(),
                org_id: req.org_id.clone(),
                from_baseline_id: prev.baseline_id.clone(),
                to_baseline_id: resp.baseline_id.clone(),
                source_url: req.url.clone(),
                format: "text".to_string(),
                artifact_id: handle.artifact_id,
                summary: Some(summary),
                created_at: now,
            }
        };
        store.create_diff_record(&diff).await?;
        resp.diff_id = Some(diff_id);
    }

    // The record that reaches the event log and the webhook names the baseline it saved and the
    // diff it stored (before INNH-4 both were empty, and the event's idempotency key ended in
    // an empty baseline id).
    if text_mode {
        record.new_baseline = new_baseline;
        record.diff_id = resp.diff_id.clone();
    }

    Ok((resp, record, run_kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_request_decodes_minimal_shape() {
        let raw = serde_json::json!({"url": "https://example.com"});
        let r: CheckRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(r.url, "https://example.com");
        assert!(r.fresh_fingerprint.is_none());
    }

    #[test]
    fn check_request_decodes_full_shape() {
        let raw = serde_json::json!({
            "url": "https://example.com",
            "fresh_fingerprint": "blake3:abc"
        });
        let r: CheckRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(r.fresh_fingerprint.as_deref(), Some("blake3:abc"));
    }

    #[test]
    fn validate_url_rejects_empty_and_bad() {
        assert!(validate_url("").is_err());
        assert!(validate_url("   ").is_err());
        assert!(validate_url("not-a-url").is_err());
        assert!(validate_url("https://example.com").is_ok());
    }

    #[test]
    fn record_request_decodes_full_and_minimal() {
        let full: RecordRequest = serde_json::from_value(serde_json::json!({
            "org_id": "org_a",
            "url": "https://example.com",
            "fresh_fingerprint": "blake3:abc",
            "run_id": "run_xyz"
        }))
        .unwrap();
        assert_eq!(full.org_id, "org_a");
        assert_eq!(full.run_id, "run_xyz");

        // run_id is optional (scheduled fires may omit it pre-mint).
        let minimal: RecordRequest = serde_json::from_value(serde_json::json!({
            "org_id": "org_a",
            "url": "https://example.com",
            "fresh_fingerprint": "blake3:abc"
        }))
        .unwrap();
        assert_eq!(minimal.run_id, "");

        // zdr posture is optional and decodes as a plain bool.
        assert!(minimal.zdr.is_none());
        let zdr_on: RecordRequest = serde_json::from_value(serde_json::json!({
            "org_id": "org_a",
            "url": "https://example.com",
            "fresh_fingerprint": "blake3:abc",
            "zdr": true
        }))
        .unwrap();
        assert_eq!(zdr_on.zdr, Some(true));
    }

    #[cfg(feature = "postgres-queue")]
    #[test]
    fn refresh_request_decodes_with_optional_priority() {
        let full: RefreshRequest = serde_json::from_value(serde_json::json!({
            "url": "https://example.com/pricing",
            "priority": "high"
        }))
        .unwrap();
        assert_eq!(
            full.priority,
            Some(quarry_runtime::request_queue::Priority::High)
        );
        // Default priority when omitted.
        let minimal: RefreshRequest = serde_json::from_value(serde_json::json!({
            "url": "https://example.com/pricing"
        }))
        .unwrap();
        assert!(minimal.priority.is_none());
    }

    #[cfg(feature = "postgres-queue")]
    #[test]
    fn refresh_response_serializes_snake_case_wire() {
        let r = RefreshResponse {
            request_id: "chg_01ARZ3NDeKTSJMdNG7gZ6pvhgp".into(),
            accepted: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"request_id\":\"chg_"));
        assert!(s.contains("\"accepted\":true"));
    }

    #[test]
    fn internal_token_dev_bypass_allows() {
        // Dev bypass accepts even with no expected token + no header.
        assert!(check_internal_token("", None, true).is_ok());
    }

    #[test]
    fn internal_token_unconfigured_is_fail_closed() {
        let err = check_internal_token("", Some("Bearer anything"), false).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
    }

    #[test]
    fn internal_token_matches_and_rejects() {
        assert!(check_internal_token("s3cret", Some("Bearer s3cret"), false).is_ok());
        let wrong = check_internal_token("s3cret", Some("Bearer nope"), false).unwrap_err();
        assert_eq!(wrong.code, ErrorCode::Unauthorized);
        let missing = check_internal_token("s3cret", None, false).unwrap_err();
        assert_eq!(missing.code, ErrorCode::Unauthorized);
    }
}

#[cfg(all(test, feature = "postgres-queue"))]
mod record_tests {
    use super::*;
    use crate::change_diff::testing::MemStore;
    use quarry_core::change_history::TextDiffArtifact;
    use quarry_core::ids::kinds::{ArtifactKind, RunKind};
    use quarry_runtime::artifact_store::{ArtifactStore, InMemoryStore};

    const ORG: &str = "org_a";
    const URL: &str = "https://example.com/pricing";

    async fn page(artifacts: &InMemoryStore, org: &str, kind: &str, body: &str) -> ArtifactKind {
        artifacts
            .put(
                org,
                &RunKind::new(),
                "page_hash",
                kind,
                body.as_bytes().to_vec(),
            )
            .await
            .unwrap()
            .artifact_id
    }

    fn request(
        org: &str,
        fingerprint: &str,
        markdown: Option<String>,
        zdr: Option<bool>,
    ) -> RecordRequest {
        serde_json::from_value(serde_json::json!({
            "org_id": org,
            "url": URL,
            "fresh_fingerprint": fingerprint,
            "run_id": "temporal-run-1",
            "zdr": zdr,
            "markdown_artifact_id": markdown,
        }))
        .unwrap()
    }

    async fn stored_diff(store: &MemStore, artifacts: &InMemoryStore) -> TextDiffArtifact {
        let record = store
            .diffs
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a diff record");
        let bytes = artifacts.get(ORG, &record.artifact_id).await.unwrap();
        serde_json::from_slice(&bytes).expect("the stored diff is JSON")
    }

    #[test]
    fn the_record_request_decodes_the_optional_markdown_id() {
        let with: RecordRequest = serde_json::from_value(serde_json::json!({
            "org_id": "org_a", "url": URL, "fresh_fingerprint": "blake3:a",
            "markdown_artifact_id": "art_01"
        }))
        .unwrap();
        assert_eq!(with.markdown_artifact_id.as_deref(), Some("art_01"));
        let without: RecordRequest = serde_json::from_value(serde_json::json!({
            "org_id": "org_a", "url": URL, "fresh_fingerprint": "blake3:a"
        }))
        .unwrap();
        assert!(without.markdown_artifact_id.is_none());
    }

    #[tokio::test]
    async fn a_new_baseline_keeps_the_markdown_and_an_unchanged_check_keeps_nothing() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let md = page(&artifacts, ORG, "markdown", "first page").await;

        let (resp, record, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some(md.to_string()), None),
            true,
        )
        .await
        .unwrap();
        assert_eq!(resp.status, "new");
        let saved = store.baselines.lock().unwrap().clone();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].artifact_id, Some(md));
        assert_eq!(saved[0].baseline_id, resp.baseline_id);
        assert_eq!(record.new_baseline.unwrap().baseline_id, resp.baseline_id);
        assert!(resp.diff_id.is_none());

        // The same fingerprint again: nothing new is saved and the markdown id is not even read.
        let (again, record, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some("not-an-id".into()), None),
            true,
        )
        .await
        .unwrap();
        assert_eq!(again.status, "unchanged");
        assert_eq!(store.baselines.lock().unwrap().len(), 1);
        assert!(record.new_baseline.is_none());
    }

    #[tokio::test]
    async fn a_change_stores_the_json_diff_and_names_it_in_the_record() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let md1 = page(&artifacts, ORG, "markdown", "keep\n\nold price").await;
        let md2 = page(&artifacts, ORG, "markdown", "keep\n\nnew price").await;
        let (first, _, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some(md1.to_string()), None),
            true,
        )
        .await
        .unwrap();

        let (resp, record, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:b", Some(md2.to_string()), None),
            true,
        )
        .await
        .unwrap();
        assert!(resp.changed);
        let diff_id = resp.diff_id.clone().expect("a diff id");

        let diffs = store.diffs.lock().unwrap().clone();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].format, "json");
        assert_eq!(diffs[0].from_baseline_id, first.baseline_id);
        assert_eq!(diffs[0].to_baseline_id, resp.baseline_id);
        assert_eq!(diffs[0].summary.as_deref(), Some("+1 -1 paragraphs"));
        let artifact = stored_diff(&store, &artifacts).await;
        assert!(artifact.available);
        let diff = artifact.diff.unwrap();
        assert_eq!((diff.added, diff.removed, diff.moved), (1, 1, 0));
        assert!(diff.real_delta);

        // What the event log and the webhook see names the baseline and the diff.
        assert_eq!(record.diff_id.as_deref(), Some(diff_id.as_str()));
        let new_baseline = record.new_baseline.clone().expect("new_baseline filled");
        assert_eq!(new_baseline.baseline_id, resp.baseline_id);
        assert_eq!(new_baseline.artifact_id, Some(md2));
        let payload = crate::change_webhook::change_webhook_payload(&record).expect("a webhook");
        assert_eq!(payload["diff_id"], diff_id);
        assert_eq!(payload["new_baseline"]["baseline_id"], resp.baseline_id);
        assert!(
            crate::change_webhook::change_detected_idempotency_key(&record)
                .ends_with(&resp.baseline_id)
        );
    }

    #[tokio::test]
    async fn a_baseline_from_before_the_text_diff_gives_an_unavailable_diff_not_an_error() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        // As the recorder wrote it before INNH-4: a baseline with no artifact.
        let (old, _, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", None, None),
            false,
        )
        .await
        .unwrap();
        let md = page(&artifacts, ORG, "markdown", "page after deploy").await;

        let (resp, _, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:b", Some(md.to_string()), None),
            true,
        )
        .await
        .unwrap();
        assert!(resp.changed && resp.diff_id.is_some());
        let artifact = stored_diff(&store, &artifacts).await;
        assert!(!artifact.available && artifact.diff.is_none());
        assert_eq!(
            artifact.reason,
            Some(quarry_core::change_history::DiffUnavailable::NoPreviousText)
        );
        assert_eq!(artifact.from_baseline_id, old.baseline_id);
        let summary = store.diffs.lock().unwrap()[0].summary.clone().unwrap();
        assert_eq!(summary, "text diff unavailable: no_previous_text");
        // The new baseline now holds text, so the next change diffs.
        let saved = store.baselines.lock().unwrap().clone();
        assert!(saved[0].artifact_id.is_none() && saved[1].artifact_id.is_some());
    }

    #[tokio::test]
    async fn a_markdown_id_of_another_org_is_refused_and_nothing_is_saved() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let foreign = page(&artifacts, "org_b", "markdown", "someone else's page").await;
        let err = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some(foreign.to_string()), None),
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(store.baselines.lock().unwrap().is_empty());
        assert!(store.diffs.lock().unwrap().is_empty());

        // Not a markdown artifact, and not an id at all: refused the same way.
        let html = page(&artifacts, ORG, "html", "<p>x</p>").await;
        for bad in [html.to_string(), "garbage".to_string()] {
            let err = record_core(
                &store,
                &artifacts,
                &request(ORG, "blake3:a", Some(bad), None),
                true,
            )
            .await
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest);
        }
        assert!(store.baselines.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn with_the_flag_off_the_recorder_is_what_it_was() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let md1 = page(&artifacts, ORG, "markdown", "one").await;
        let md2 = page(&artifacts, ORG, "markdown", "two").await;
        record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some(md1.to_string()), None),
            false,
        )
        .await
        .unwrap();
        let (resp, record, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:b", Some(md2.to_string()), None),
            false,
        )
        .await
        .unwrap();

        assert!(store
            .baselines
            .lock()
            .unwrap()
            .iter()
            .all(|b| b.artifact_id.is_none()));
        let diffs = store.diffs.lock().unwrap().clone();
        assert_eq!(diffs[0].format, "text");
        let note = artifacts.get(ORG, &diffs[0].artifact_id).await.unwrap();
        assert!(String::from_utf8(note)
            .unwrap()
            .starts_with("content fingerprint changed: blake3:a"));
        // And the record the webhook gets is unfilled, as before.
        assert!(record.new_baseline.is_none() && record.diff_id.is_none());
        assert!(
            resp.diff_id.is_some(),
            "the response still names the diff it stored"
        );
    }

    #[tokio::test]
    async fn a_zdr_declared_check_never_stores_text_even_with_the_flag_on() {
        // Observe mode: the baseline row is still written for a ZDR-declared body. No text and no
        // diff may follow it, and a markdown id the orchestrator should never send is not trusted.
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let md = page(&artifacts, ORG, "markdown", "page").await;
        record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some(md.to_string()), Some(true)),
            true,
        )
        .await
        .unwrap();
        let (resp, record, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:b", Some("garbage".into()), Some(true)),
            true,
        )
        .await
        .unwrap();
        assert!(store
            .baselines
            .lock()
            .unwrap()
            .iter()
            .all(|b| b.artifact_id.is_none()));
        assert_eq!(store.diffs.lock().unwrap()[0].format, "text");
        assert!(resp.changed && record.new_baseline.is_none() && record.diff_id.is_none());
    }

    #[tokio::test]
    async fn a_raw_byte_change_with_the_same_text_is_reported_as_no_real_delta() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let md1 = page(&artifacts, ORG, "markdown", "same text\n\nsecond").await;
        let md2 = page(&artifacts, ORG, "markdown", "same  text\n\nsecond").await;
        record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:a", Some(md1.to_string()), None),
            true,
        )
        .await
        .unwrap();
        let (resp, _, _) = record_core(
            &store,
            &artifacts,
            &request(ORG, "blake3:b", Some(md2.to_string()), None),
            true,
        )
        .await
        .unwrap();
        assert!(resp.changed, "the raw-byte fingerprint still says changed");
        let artifact = stored_diff(&store, &artifacts).await;
        assert!(!artifact.diff.unwrap().real_delta);
        let summary = store.diffs.lock().unwrap()[0].summary.clone().unwrap();
        assert!(summary.starts_with("no paragraph change"), "{summary}");
    }
}
