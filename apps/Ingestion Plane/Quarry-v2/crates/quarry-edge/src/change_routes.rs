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
use quarry_core::ids::kinds::RequestKind;
#[cfg(feature = "postgres-queue")]
use quarry_core::ids::kinds::SnapshotKind;
#[cfg(feature = "postgres-queue")]
use quarry_core::resources::Snapshot;
#[cfg(feature = "postgres-queue")]
use quarry_core::tracked_snapshot::baseline_chain_to_snapshot;

use crate::state::AppState;

// Fan-out helper signatures reference RunKind only under postgres-queue;
// an ungated import would be unused (warn) in default builds.
#[cfg(feature = "postgres-queue")]
use quarry_core::ids::kinds::RunKind;

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
    let request_id = RequestKind::new().to_string();
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
    let request_id = RequestKind::new().to_string();
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
    let request_id = RequestKind::new().to_string();
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
    let request_id = RequestKind::new().to_string();
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
    let request_id = RequestKind::new().to_string();
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

    let request_id = RequestKind::new().to_string();
    validate_url(&req.url).map_err(|e| err_response(&request_id, e))?;
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
    let request_id = RequestKind::new().to_string();
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
    #[allow(dead_code)] // read only under `postgres-queue` in record_check
    pub zdr: Option<bool>,
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

fn verify_internal_token(headers: &HeaderMap) -> Result<(), QuarryError> {
    let expected = std::env::var("QUARRY_EDGE_INTERNAL_TOKEN").unwrap_or_default();
    let provided = headers.get("authorization").and_then(|v| v.to_str().ok());
    check_internal_token(&expected, provided, internal_dev_bypass())
}

pub async fn record_internal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RecordRequest>,
) -> Result<Json<RecordResponse>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
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

    #[cfg(feature = "postgres-queue")]
    if let Some(store) = state.baseline_store.as_ref() {
        let resp = record_check(&state, store, state.artifacts.as_ref(), &req)
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

/// Runs the compare → save_baseline → (on change) diff sequence in-process.
/// The baseline/diff store is Rust-local; the diff artifact body is stored via
/// the same ArtifactStore that page runs use.
#[cfg(feature = "postgres-queue")]
async fn record_check(
    state: &AppState,
    store: &quarry_runtime::postgres_baseline_store::PostgresBaselineStore,
    artifacts: &dyn quarry_runtime::artifact_store::ArtifactStore,
    req: &RecordRequest,
) -> Result<RecordResponse, QuarryError> {
    use chrono::Utc;
    use quarry_core::change_history::{ChangeStatus, DiffRecord};
    // These are concrete prefixed-ULID types (`kinds::X` is a type alias for
    // `Id<X marker>`); `X::new()` mints one and `.ulid()` is the raw ULID.
    use quarry_core::ids::kinds::{ArtifactKind, RunKind};

    let record = store
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
    if matches!(record.status, ChangeStatus::New | ChangeStatus::Changed) {
        let baseline_id = format!("bln_{}", ArtifactKind::new().ulid());
        let new_baseline = BaselineSnapshot {
            baseline_id: baseline_id.clone(),
            org_id: req.org_id.clone(),
            source_url: req.url.clone(),
            fingerprint: req.fresh_fingerprint.clone(),
            artifact_id: None,
            // Chain from the already-loaded prev — no second load_latest.
            prev_baseline_id: record.prev_baseline.as_ref().map(|b| b.baseline_id.clone()),
            captured_at: now,
            run_id: run_kind.clone(),
        };
        store.save_baseline(&new_baseline).await?;
        resp.baseline_id = baseline_id;
    }

    // On Changed, persist a diff record. DiffRecord.artifact_id is required, so
    // store the diff body first. We hold fingerprints (not the prev content),
    // so the honest diff artifact records the fingerprint transition.
    if record.status == ChangeStatus::Changed {
        let prev = record.prev_baseline.as_ref().ok_or_else(|| {
            QuarryError::new(ErrorCode::Internal, "changed status without prev baseline")
        })?;
        let summary = format!(
            "content fingerprint changed: {} → {}",
            prev.fingerprint, req.fresh_fingerprint
        );
        let artifact_run: RunKind = run_kind.clone().unwrap_or_default();
        let handle = artifacts
            .put(
                &req.org_id,
                &artifact_run,
                &req.fresh_fingerprint,
                "raw",
                summary.clone().into_bytes(),
            )
            .await?;

        let diff_id = format!("diff_{}", ArtifactKind::new().ulid());
        let diff = DiffRecord {
            diff_id: diff_id.clone(),
            org_id: req.org_id.clone(),
            from_baseline_id: prev.baseline_id.clone(),
            to_baseline_id: resp.baseline_id.clone(),
            source_url: req.url.clone(),
            format: "text".to_string(),
            artifact_id: handle.artifact_id,
            summary: Some(summary),
            created_at: now,
        };
        store.create_diff_record(&diff).await?;
        resp.diff_id = Some(diff_id);
    }

    // Post-persistence fan-out (durable event + signed webhook).
    let zdr = quarry_core::zdr::ZdrMode::from(req.zdr.unwrap_or(false));
    emit_change_side_effects(state, &record, run_kind, zdr).await;

    Ok(resp)
}

/// Post-persistence fan-out for a completed check: a durable
/// `ChangeDetected` event on the event sink (mirrors PageRunner's
/// ChangeDetected emissions so `/v1/runs/:id/events` consumers see the
/// change even though this path never ran a page pipeline) and, on real
/// changes, a signed webhook to control's delivery pipeline (subject
/// `quarry.change.detected`). Best-effort: a control outage warn-logs and
/// never fails the caller's check.
///
/// Fail closed under ZDR: when the declaring schedule runs
/// zero-data-retention, NEITHER side effect fires — the durable event log
/// is content-bearing and the webhook would push the same payload
/// off-process.
#[cfg(feature = "postgres-queue")]
async fn emit_change_side_effects(
    state: &AppState,
    record: &ChangeRecord,
    run_kind: Option<RunKind>,
    zdr: quarry_core::zdr::ZdrMode,
) {
    use quarry_core::change_history::ChangeStatus;
    use quarry_core::event::EventType;

    if zdr.is_active() {
        tracing::debug!(
            org_id = %record.org_id,
            url = %record.source_url,
            "zdr active: change event + webhook suppressed"
        );
        return;
    }
    if record.status != ChangeStatus::Changed {
        return;
    }

    // Durable event log entry.
    let run_id = run_kind.unwrap_or_default();
    let idem = format!(
        "change_detected:{}:{}:{}",
        record.org_id,
        record.source_url,
        record
            .new_baseline
            .as_ref()
            .map(|b| b.baseline_id.clone())
            .unwrap_or_default()
    );
    let payload = serde_json::to_value(record).unwrap_or(serde_json::Value::Null);
    state
        .event_sink
        .emit(run_id, EventType::ChangeDetected, payload, idem)
        .await;

    // Signed webhook → control (subject quarry.change.detected).
    // Fire-and-forget transport: subscribers poll /v1/change/latest
    // anyway, so a missed push is availability loss, not correctness loss.
    if let Some(payload) = crate::change_webhook::change_webhook_payload(record) {
        if state.control_base_url.is_empty() {
            tracing::warn!("change webhook not sent: control base url empty");
        } else if let Some(signer) = state.internal_signer.as_ref() {
            match crate::change_webhook::sign_change_webhook(signer, &record.org_id, &payload) {
                Ok((path_q, body, headers)) => {
                    let base = state.control_base_url.trim_end_matches('/').to_string();
                    tokio::spawn(async move {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(10))
                            .build();
                        let Ok(client) = client else {
                            tracing::warn!("change webhook client build failed");
                            return;
                        };
                        let res = client
                            .post(format!("{base}{path_q}"))
                            .header(crate::internal_auth::HEADER_SIG, headers.signature)
                            .header(crate::internal_auth::HEADER_TS, headers.timestamp)
                            .header(crate::internal_auth::HEADER_NONCE, headers.nonce)
                            .header("content-type", "application/json")
                            .body(body)
                            .send()
                            .await;
                        match res {
                            Ok(r) if r.status().is_success() => {}
                            Ok(r) => {
                                tracing::warn!(status = %r.status(), "change webhook delivery failed")
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "change webhook transport failed")
                            }
                        }
                    });
                }
                Err(e) => tracing::warn!(error = %e, "change webhook signing failed"),
            }
        } else {
            tracing::warn!("change webhook skipped: QUARRY_EDGE__INTERNAL_SECRET not configured (unsigned delivery refused, fail closed)");
        }
    }
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
