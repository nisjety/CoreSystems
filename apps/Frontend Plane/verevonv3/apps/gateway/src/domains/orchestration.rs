//! `/api/v1/orchestration/*` — human-in-the-loop run control for agentic runs.
//!
//! Thin authenticated proxy from the SPA to model-gateway's orchestration HTTP
//! surface (plans, todos, approvals, run pause/resume/cancel). This is the API
//! the chat "internal Claude Code" approval UI + plan mode drive: when an
//! agentic run pauses for approval (the `run_paused_for_approval` SSE event),
//! the SPA lists pending approvals and POSTs a decision here, which flows
//! through model-gateway → session-core → execution-core's `resume_run`.
//!
//! **Dual-purpose since Phase 5 ("Approvals & policy"):** the `approvals`
//! routes below (`list_approvals`, `get_approval`, `decide_approval`) are
//! generic over `run_id`/`approval_id` and were never chat-specific to begin
//! with — execution-core's browser-agent loop (`browser.rs`'s `start_ai_run`)
//! now creates durable approvals for risky browser actions (login, checkout,
//! posting forms, destructive actions, cross-domain navigation, persistent
//! cookie use) through the exact same `CreateApproval`/`GetApproval`/
//! `DecideApproval` RPCs a chat run's tool-call approval uses, scoped under
//! the browser AI run's own `run_id`. **Do not add a browser-specific
//! decide/list route elsewhere** — extend these handlers if a browser-only
//! need arises; `browser.rs` deliberately has none of its own. The
//! browser-specific detail (action type, URL, selector, risk category) is
//! *not* on the generic `Approval` object returned here — it rides
//! separately on the run's SSE stream (`GET /api/v1/runs/:run_id/events`,
//! `browser_action_approval_required` / `browser_action_decided` events);
//! these HTTP routes are the decide/list/reconcile surface, not the
//! rich-detail surface.
//!
//! Every route carries the model-plane audience token (or dev-bypass) and the
//! verified user id via [`proxy_model_json`]; nothing trusts client input beyond
//! the path id and the decision body, which model-gateway re-validates.
//! Known gap (pre-existing, not introduced or fixed here): model-gateway's
//! `GetApprovalRequest`/`DecideApprovalRequest` carry no `org_id`, so this
//! proxy cannot enforce org-ownership beyond what the upstream itself checks
//! (today, nothing) — any authenticated caller who knows/guesses an
//! `approval_id` can read or decide it regardless of org. Flagged for a
//! dedicated fix in session-core/model-gateway, not in scope for this proxy.

use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared::{
        data_plane_token, delegated_auth_unavailable, invalid_chat_request, model_token,
        proxy_model_json_with_data_plane, proxy_model_json_with_session, required_execution_token,
        required_inference_token, required_session_token,
    },
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::{authorized_org_id, proxy_sse_stream_with_data_plane},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // ── reads ──────────────────────────────────────────────────────────
        .route("/api/v1/orchestration/runs/{run_id}/plans", get(list_plans))
        .route("/api/v1/orchestration/plans/{plan_id}", get(get_plan))
        .route(
            "/api/v1/orchestration/threads/{thread_id}/todos",
            get(list_todos),
        )
        .route(
            "/api/v1/orchestration/runs/{run_id}/approvals",
            get(list_approvals),
        )
        .route(
            "/api/v1/orchestration/runs/{run_id}/proof-bundle",
            get(get_run_proof_bundle),
        )
        .route(
            "/api/v1/orchestration/verification-metrics",
            get(get_verification_metrics),
        )
        .route(
            "/api/v1/orchestration/approvals/{approval_id}",
            get(get_approval),
        )
        .route(
            "/api/v1/orchestration/threads/{thread_id}/lineage",
            get(get_lineage),
        )
        // ── decisions / run control ──────────────────────────────────────────
        .route(
            "/api/v1/orchestration/approvals/{approval_id}/decide",
            post(decide_approval),
        )
        .route(
            "/api/v1/orchestration/plans/{plan_id}/approve",
            post(approve_plan),
        )
        .route(
            "/api/v1/orchestration/plans/{plan_id}/reject",
            post(reject_plan),
        )
        .route(
            "/api/v1/orchestration/todos/{todo_id}/status",
            post(update_todo_status),
        )
        .route(
            "/api/v1/orchestration/runs/{run_id}/resume",
            post(resume_run),
        )
        .route(
            "/api/v1/orchestration/runs/{run_id}/continue",
            post(continue_run),
        )
        .route(
            "/api/v1/orchestration/runs/{run_id}/cancel",
            post(cancel_run),
        )
        // Per-org/user rate limiting on this high-risk human-in-the-loop group.
        // Ordering: `require_session` (written last → outer) runs first and
        // inserts `AuthenticatedUser`, so `rate_limit_middleware` (written first
        // → inner) keys by the validated org/user rather than client IP. The
        // shared `RateLimiter` is provided by the global `Extension` layer in
        // `main.rs`, which is outer to this whole router.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Proxy a model-gateway orchestration GET, forwarding the model-plane token +
/// verified user id.
async fn mg_get(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
) -> (StatusCode, Json<Value>) {
    mg_proxy(state, user, headers, Method::GET, path, None, false).await
}

fn requires_execution_delegation(path: &str) -> bool {
    path.ends_with("/decide") || path.ends_with("/resume")
}

/// A response worth retrying once: a literal upstream 502 (model-gateway's own
/// quarantine placeholder for not-yet-durable continuation delivery fires
/// unconditionally on every first-time approval grant — see
/// `mg_post_decide_with_retry` — and the generic `Err(_) => 502` fallback in
/// `proxy_model_json_with_delegations` for a dropped connection also lands
/// here, whether or not that connection ever reached the network), or a 503
/// from our own `delegated_auth_unavailable` short-circuit (that one always
/// fires before any upstream request is sent, so retrying it is trivially
/// side-effect-free).
fn is_retryable_decide_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE
    )
}

async fn mg_proxy(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    path: &str,
    body: Option<Value>,
    require_execution: bool,
) -> (StatusCode, Json<Value>) {
    let token = model_token(state, user, headers).await;
    let session = match required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error),
    };
    let url = format!("{}{}", state.model_gateway_url, path);
    if require_execution {
        let execution = match required_execution_token(state, user, headers).await {
            Ok(token) => token,
            Err(error) => return delegated_auth_unavailable(error),
        };
        return proxy_model_json_with_data_plane(
            state,
            method,
            &url,
            body,
            token.as_deref(),
            None,
            None,
            Some(&execution),
            None,
            Some(&session),
            user,
        )
        .await;
    }
    proxy_model_json_with_session(
        state,
        method,
        &url,
        body,
        token.as_deref(),
        Some(&session),
        user,
    )
    .await
}

/// Proxy a model-gateway orchestration POST with a JSON body (decisions).
async fn mg_post(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
    body: Value,
) -> (StatusCode, Json<Value>) {
    mg_proxy(
        state,
        user,
        headers,
        Method::POST,
        path,
        Some(body),
        requires_execution_delegation(path),
    )
    .await
}

const DECIDE_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const DECIDE_RETRY_TIMEOUT: Duration = Duration::from_secs(8);

/// Retries `/decide` exactly once on a retryable status, after a short fixed
/// backoff. Safe ONLY for this route: session-core's `DecideApproval` is a
/// single CAS-guarded state transition with no side effects of its own (a
/// repeat of the same decision is a clean no-op, a genuine conflict cleanly
/// 412s — never a silent re-execution), unlike `/resume`/`/cancel`/plan
/// approve-reject, which are NOT confirmed idempotent and must never be
/// retried this way. The retry also self-heals model-gateway's own quarantine
/// placeholder: it 502s only on the literal Requested->Granted transition, and
/// by the second attempt the DB row is already Granted, so that branch is
/// skipped.
async fn mg_post_decide_with_retry(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
    body: Value,
) -> (StatusCode, Json<Value>) {
    mg_post_decide_with_retry_bounded(state, user, headers, path, body, DECIDE_RETRY_TIMEOUT).await
}

/// `mg_post_decide_with_retry` with the retry's own timeout as a parameter —
/// split out so tests can shrink it instead of waiting out the real
/// `DECIDE_RETRY_TIMEOUT`. The timeout keeps a hung retry from doubling the
/// worst-case wait past `state.client`'s own 25s ceiling; if it fires, the
/// original (already-known) response is returned rather than surfacing a
/// second, less informative failure.
async fn mg_post_decide_with_retry_bounded(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
    body: Value,
    retry_timeout: Duration,
) -> (StatusCode, Json<Value>) {
    let first = mg_post(state, user, headers, path, body.clone()).await;
    if !is_retryable_decide_status(first.0) {
        return first;
    }
    tracing::warn!(
        path,
        status = %first.0,
        "decide_approval: retrying after a transient/quarantine response"
    );
    tokio::time::sleep(DECIDE_RETRY_BACKOFF).await;
    match tokio::time::timeout(retry_timeout, mg_post(state, user, headers, path, body)).await {
        Ok(second) => second,
        Err(_elapsed) => {
            tracing::warn!(
                path,
                original_status = %first.0,
                retry_timeout = ?retry_timeout,
                "decide_approval: the retry itself timed out; returning the original response"
            );
            first
        }
    }
}

/// Proxy a model-gateway orchestration POST with no body (run resume/cancel).
async fn mg_post_empty(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
) -> (StatusCode, Json<Value>) {
    mg_proxy(
        state,
        user,
        headers,
        Method::POST,
        path,
        None,
        requires_execution_delegation(path),
    )
    .await
}

fn enc(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

// ── read handlers ────────────────────────────────────────────────────────────

async fn list_plans(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/plans", enc(&run_id)),
    )
    .await
}

async fn get_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/plans/{}", enc(&plan_id)),
    )
    .await
}

async fn list_todos(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/threads/{}/todos", enc(&thread_id)),
    )
    .await
}

/// Lists pending/decided approvals for a run. Also the Phase 5 browser-run
/// listing surface: pass the AI run's own `run_id` (from `start_ai_run`'s
/// response), not the browser `session_id`.
async fn list_approvals(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/approvals", enc(&run_id)),
    )
    .await
}

/// The Verevon Proof Bundle for one run — the portable evidence record of what
/// was authorized, executed, observed, and independently verified. Read-only;
/// model-gateway scopes it to the caller's verified org.
async fn get_run_proof_bundle(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/proof-bundle", enc(&run_id)),
    )
    .await
}

#[derive(Debug, Default, Deserialize)]
struct VerificationMetricsQuery {
    since: Option<String>,
}

/// Aggregate Verified Outcome Foundation metrics for the caller's own org —
/// verified completion, false success, verification coverage, human approval
/// effort. Read-only; model-gateway scopes it to the caller's verified org,
/// same as the proof bundle above. `since` (RFC 3339) is forwarded verbatim;
/// model-gateway rejects it if malformed, this proxy does not pre-validate.
async fn get_verification_metrics(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<VerificationMetricsQuery>,
) -> impl IntoResponse {
    let path = verification_metrics_path(query.since.as_deref());
    mg_get(&state, &user, &headers, &path).await
}

/// Blank or absent `since` forwards no query string at all - `since=` (an
/// explicit empty value) reaching model-gateway would fail its RFC 3339
/// parse and 400, rather than being silently treated as "all time."
fn verification_metrics_path(since: Option<&str>) -> String {
    match since.map(str::trim) {
        Some(value) if !value.is_empty() => {
            format!(
                "/v1/orchestration/verification-metrics?since={}",
                enc(value)
            )
        }
        _ => "/v1/orchestration/verification-metrics".to_string(),
    }
}

async fn get_approval(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/approvals/{}", enc(&approval_id)),
    )
    .await
}

async fn get_lineage(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/threads/{}/lineage", enc(&thread_id)),
    )
    .await
}

// ── decision / run-control handlers ───────────────────────────────────────────

/// Approves or rejects a pending approval by id. Also the Phase 5
/// browser-action decision surface (a granted decision lets execution-core's
/// in-loop poller in `browser_agent.rs` proceed with the exact action that
/// was gated; a denial/timeout aborts the run without dispatching it).
/// Body: `{"decision": "approve" | "reject", "reason"?: string}`.
pub(crate) async fn decide_approval(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post_decide_with_retry(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/approvals/{}/decide", enc(&approval_id)),
        body,
    )
    .await
}

async fn approve_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/plans/{}/approve", enc(&plan_id)),
        body,
    )
    .await
}

async fn reject_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/plans/{}/reject", enc(&plan_id)),
        body,
    )
    .await
}

async fn update_todo_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(todo_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/todos/{}/status", enc(&todo_id)),
        body,
    )
    .await
}

pub(crate) async fn resume_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_post_empty(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/resume", enc(&run_id)),
    )
    .await
}

/// The body a continuation accepts. Only the approval id and its claim token: the
/// tools and authority the continuation runs under are the pause's own snapshot,
/// held upstream, so no client value can widen them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContinueRunBody {
    approval_id: String,
    claim_token: String,
}

/// `None` when either identifier is blank. Otherwise the upstream body is rebuilt
/// from the two fields alone, so nothing else a client sent is forwarded. The
/// claim token is forwarded as sent: trimming it would change the credential.
///
/// The one exception (WS03 S8-A) is the PARK form, the resume of a run the executor
/// parked, which has no approval: with `park_form` on (`AppState::continue_park_form`),
/// an `approval_id` that is EXACTLY empty is forwarded, present and empty, together with
/// a non-blank token. Everything else, a whitespace-only id included, and every input
/// with the flag off, is exactly as it was. An absent `approval_id` key is not a park
/// form: the extractor refuses it (422) before this runs.
fn continue_upstream_body(body: &ContinueRunBody, park_form: bool) -> Option<Value> {
    let park = park_form && body.approval_id.is_empty();
    if (!park && body.approval_id.trim().is_empty()) || body.claim_token.trim().is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "approval_id": body.approval_id,
        "claim_token": body.claim_token,
    }))
}

/// The refusals model-gateway's continue route answers before any stream opens,
/// that the browser is told by their closed code: (code, upstream status, fixed
/// message). The messages are this BFF's own words; an upstream message is never
/// relayed. A code relays only under its own status, so a body that merely names a
/// code does not turn another failure into it. Mirrored by the SPA's
/// `CHAT_RUN_CONTINUE_HTTP_REFUSALS` and pinned against it in
/// `tests/continue_error_vocabulary.rs`. Nothing pins it to model-gateway's own
/// list yet (those constants land with build plan WS03 S7c-A4).
///
/// There is no switch on this relay: with every Model Plane switch off the route
/// already answers these codes in place of the `model_error` it used to be turned
/// into (a committed gateway with the continue switch off answers 409
/// `continue_disabled`). That visible change is accepted because no UI calls the
/// route yet; `the_continue_relay_has_no_switch_and_names_closed_codes_with_every_model_plane_switch_off`
/// pins it.
///
/// Not here, on purpose: `continue_incomplete` (this handler's own blank check
/// answers first) and `missing_bearer` (a 401, which stays `unauthorized`).
/// Everything else the gateway answers (the owner check, a read, a consume
/// PermissionDenied or Unauthenticated, a zero-retention typed refusal) stays
/// `model_error`.
pub(crate) const CONTINUE_RELAYED_ERRORS: &[(&str, u16, &str)] = &[
    (
        "continue_disabled",
        409,
        "Continuing an approved run is not enabled.",
    ),
    ("not_resumable", 409, "This run cannot be continued."),
    (
        "approval_not_found",
        404,
        "The approval was not found for this run.",
    ),
    (
        "approval_not_granted",
        409,
        "The approval has not been granted.",
    ),
    ("approval_expired", 409, "The approval has expired."),
    (
        "approval_not_resumable",
        409,
        "The approval is not in a state that can continue the run.",
    ),
    (
        "continuation_already_consumed",
        409,
        "This approval has already been used. Check the run to see what became of it.",
    ),
    (
        "continue_unavailable",
        503,
        "The continuation could not be confirmed. The same request can be sent again.",
    ),
];

/// `POST /api/v1/orchestration/runs/{run_id}/continue` (build plan WS03 S3): resumes
/// a run that paused for a granted loop approval, relaying the continuation as SSE.
///
/// The bearers are minted the way `chat::streams::stream_chat` mints them, because
/// model-gateway's continue route verifies this same set (model, session, inference,
/// execution, data plane). Cost, capability, ingestion and sandbox bearers are not
/// part of a continuation and are not minted.
///
/// A refusal before the stream opens is relayed by its closed code
/// ([`CONTINUE_RELAYED_ERRORS`]); the SSE body itself is relayed byte for byte, so
/// the `approval_spent` flag and the closed error frames reach the browser as the
/// gateway wrote them. A relayed HTTP code is not the whole story for the client: a
/// request that fails here after it reached model-gateway (a send failure, a
/// dropped connection) is answered 502 `upstream_unavailable`, and the gateway may
/// have consumed the approval by then. Only the listed codes say what happened to
/// the approval; the SPA reads them through `chatRunContinueHttpRefusalOf`.
pub(crate) async fn continue_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(body): Json<ContinueRunBody>,
) -> Response {
    let Some(upstream_body) = continue_upstream_body(&body, state.continue_park_form) else {
        return invalid_chat_request("continue needs an approval id and a claim token")
            .into_response();
    };
    let model = model_token(&state, &user, &headers).await;
    let data_plane = data_plane_token(&state, &user, &headers).await;
    let inference = match required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let execution = match required_execution_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let session = match required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    // The org's standing posture, so the `x-zdr` header reaches model-gateway. The
    // gateway refuses a continuation under ZDR on its own verified claims as well.
    let zdr = crate::zdr::org_zdr_enabled(&state, &user).await;
    let org_id = authorized_org_id(&state, &user).await;
    let url = format!(
        "{}/v1/orchestration/runs/{}/continue",
        state.model_gateway_url,
        enc(&run_id)
    );
    proxy_sse_stream_with_data_plane(
        &state,
        Method::POST,
        &url,
        Some(upstream_body),
        model.as_deref(),
        data_plane.as_deref(),
        None,
        Some(&inference),
        Some(&execution),
        None,
        Some(&session),
        None,
        None,
        None,
        Some((&user.user_id, org_id.as_str())),
        zdr,
        CONTINUE_RELAYED_ERRORS,
    )
    .await
}

pub(crate) async fn cancel_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_post_empty(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/cancel", enc(&run_id)),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::{HeaderMap, StatusCode};
    use axum::Json;
    use serde_json::json;

    use crate::{
        audience_tokens::new_audience_token_cache, cache::ResultCache, config::AppState,
        middleware::AuthenticatedUser,
    };

    use super::{
        continue_upstream_body, is_retryable_decide_status, mg_post_decide_with_retry_bounded,
        requires_execution_delegation, verification_metrics_path, ContinueRunBody,
        CONTINUE_RELAYED_ERRORS,
    };

    #[test]
    fn verification_metrics_path_omits_the_query_entirely_when_since_is_absent_or_blank() {
        assert_eq!(
            verification_metrics_path(None),
            "/v1/orchestration/verification-metrics"
        );
        assert_eq!(
            verification_metrics_path(Some("   ")),
            "/v1/orchestration/verification-metrics",
            "a blank since must not become an explicit since= that model-gateway would 400 on"
        );
    }

    #[test]
    fn verification_metrics_path_percent_encodes_a_real_since_value() {
        assert_eq!(
            verification_metrics_path(Some("2026-08-01T00:00:00+00:00")),
            "/v1/orchestration/verification-metrics?since=2026-08-01T00%3A00%3A00%2B00%3A00",
        );
    }

    #[test]
    fn execution_delegation_is_required_only_for_execution_mutations() {
        assert!(requires_execution_delegation(
            "/v1/orchestration/approvals/apr-1/decide"
        ));
        assert!(requires_execution_delegation(
            "/v1/orchestration/runs/run-1/resume"
        ));
        assert!(!requires_execution_delegation(
            "/v1/orchestration/plans/plan-1/approve"
        ));
        assert!(!requires_execution_delegation(
            "/v1/orchestration/runs/run-1/cancel"
        ));
    }

    #[test]
    fn only_502_and_503_are_treated_as_retryable() {
        assert!(is_retryable_decide_status(StatusCode::BAD_GATEWAY));
        assert!(is_retryable_decide_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!is_retryable_decide_status(StatusCode::OK));
        assert!(!is_retryable_decide_status(StatusCode::NOT_FOUND));
        // 412 is session-core's real "already decided differently" conflict —
        // must never be retried, it isn't transient.
        assert!(!is_retryable_decide_status(StatusCode::PRECONDITION_FAILED));
        assert!(!is_retryable_decide_status(
            StatusCode::INTERNAL_SERVER_ERROR
        ));
    }

    fn test_state(allow_dev_auth_bypass: bool) -> AppState {
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            enforcement_mode: "off".to_string(),
            auth_core_url: "http://127.0.0.1:1".into(),
            verevon_public_origin: "http://localhost:5173".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            session_core_service_token: "0123456789abcdef0123456789abcdef".into(),
            user_core_service_token: "abcdef0123456789abcdef0123456789".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            billing_core_service_token: "billing-test-secret-at-least-32-bytes".into(),
            cost_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            org_core_service_token: "org-test-secret-at-least-32-bytes".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            audit_core_service_token: "audit-test-secret-at-least-32-bytes".into(),
            insight_core_url: "http://127.0.0.1:1".into(),
            leads_core_url: "http://127.0.0.1:1".into(),
            shipping_core_url: "http://127.0.0.1:1".into(),
            user_core_url: "http://127.0.0.1:1".into(),
            application_convex_url: String::new(),
            application_convex_service_key: String::new(),
            graph_index_url: "http://127.0.0.1:1".into(),
            quarry_edge_url: "http://127.0.0.1:1".into(),
            model_recommend_url: "http://127.0.0.1:1".into(),
            model_gateway_url: "http://127.0.0.1:1".into(),
            model_gateway_dev_bearer: String::new(),
            inference_core_url: "http://127.0.0.1:1".into(),
            documents_api_url: "http://127.0.0.1:1".into(),
            retrieval_engine_url: "http://127.0.0.1:1".into(),
            wiki_store_url: "http://127.0.0.1:1".into(),
            embedding_engine_url: "http://127.0.0.1:1".into(),
            quickwit_adapter_url: "http://127.0.0.1:1".into(),
            finspo_core_url: "http://127.0.0.1:1".into(),
            imports_api_url: "http://127.0.0.1:1".into(),
            notification_core_url: "http://127.0.0.1:1".into(),
            notification_core_service_token: "notification-test-secret-at-least-32-bytes".into(),
            conversation_core_service_token: "conversation-test-secret-at-least-32-bytes".into(),
            information_core_url: "http://127.0.0.1:1".into(),
            conversation_core_url: "http://127.0.0.1:1".into(),
            social_core_url: "http://127.0.0.1:1".into(),
            searxng_url: "http://127.0.0.1:1".into(),
            autocomplete_core_url: "http://127.0.0.1:1".into(),
            autocomplete_token: String::new(),
            zammad_api_url: "http://127.0.0.1:1".into(),
            zammad_api_token: String::new(),
            remote_support_rendezvous_url: String::new(),
            remote_support_relay_url: String::new(),
            remote_support_server_public_key: String::new(),
            audience_token_cache: new_audience_token_cache(),
            browser_run_store: crate::domains::browser::new_browser_run_store(),
            rate_limiter: crate::rate_limit::RateLimiter::from_cache(&ResultCache::disabled()),
            cache: ResultCache::disabled(),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers: false,
            allow_dev_auth_bypass,
            mail_task_ingest_service_token: "mail-task-test-secret-at-least-32-bytes".into(),
            mail_task_rate_limiter: crate::rate_limit::RateLimiter::from_cache_with_rpm(
                &ResultCache::disabled(),
                20.0,
            ),
            mail_task_nats_url: String::new(),
            mail_task_nats_user: String::new(),
            mail_task_nats_password: String::new(),
            continue_park_form: false,
            mail_task_pipeline: None,
        }
    }

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn mount_token_mocks(
        server: &wiremock::MockServer,
    ) -> impl std::future::Future<Output = ()> + '_ {
        use wiremock::{
            matchers::{method, path},
            Mock, ResponseTemplate,
        };
        async move {
            Mock::given(method("GET"))
                .and(path("/api/session-core/token"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"token": "session-test-token"})),
                )
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/execution-core/token"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"token": "execution-test-token"})),
                )
                .mount(server)
                .await;
        }
    }

    /// Regression coverage for task_d6420100: a real approve/reject decision
    /// that comes back as a transient 502 — whether from model-gateway's own
    /// quarantine placeholder on a first-time grant, or a dropped connection —
    /// must not be surfaced to the browser as a failure when a single retry
    /// would have succeeded.
    #[tokio::test]
    async fn decide_retries_once_after_502_and_returns_the_eventual_success() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        mount_token_mocks(&server).await;

        // First attempt: the deterministic model-gateway quarantine 502 (or an
        // equally-transient network blip) — must be consumed exactly once.
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/approvals/apr-1/decide"))
            .respond_with(ResponseTemplate::new(502))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        // Second attempt: the decision has already flipped to Granted, so
        // model-gateway's quarantine branch is skipped and this succeeds.
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/approvals/apr-1/decide"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"approval": {"id": "apr-1", "status": "GRANTED"}})),
            )
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();

        let (status, Json(body)) = mg_post_decide_with_retry_bounded(
            &state,
            &test_user(),
            &HeaderMap::new(),
            "/v1/orchestration/approvals/apr-1/decide",
            json!({"decision": "approve", "reason": ""}),
            Duration::from_secs(8),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["approval"]["status"], "GRANTED");
        // Both `.expect(1)` mocks are verified when `server` drops at the end
        // of this test — proving exactly one 502 followed by exactly one 200.
    }

    /// A non-retryable status (a real, permanent conflict) must be returned
    /// immediately — retrying it would just be a wasted round trip, and
    /// retrying a 412 in particular would misrepresent an already-settled
    /// conflict as still in flight.
    #[tokio::test]
    async fn decide_does_not_retry_a_non_retryable_status() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        mount_token_mocks(&server).await;
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/approvals/apr-2/decide"))
            .respond_with(
                ResponseTemplate::new(412)
                    .set_body_json(json!({"error": {"code": "already_decided", "message": "approval is already decided"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();

        let (status, _) = mg_post_decide_with_retry_bounded(
            &state,
            &test_user(),
            &HeaderMap::new(),
            "/v1/orchestration/approvals/apr-2/decide",
            json!({"decision": "approve", "reason": ""}),
            Duration::from_secs(8),
        )
        .await;

        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        // The `.expect(1)` mock is verified on drop — proving no retry happened.
    }

    /// If the retry itself hangs past its own timeout, the original (already
    /// fully known) response must be returned rather than leaving the caller
    /// waiting on a second, less-informative failure. Uses a short bounded
    /// timeout (not the real 8s `DECIDE_RETRY_TIMEOUT`) so this stays fast.
    #[tokio::test]
    async fn decide_falls_back_to_the_original_response_if_the_retry_itself_times_out() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        mount_token_mocks(&server).await;

        Mock::given(method("POST"))
            .and(path("/v1/orchestration/approvals/apr-3/decide"))
            .respond_with(ResponseTemplate::new(502))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        // The retry attempt: deliberately slower than the test's short retry
        // timeout below, so the `tokio::time::timeout` wrapping it elapses.
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/approvals/apr-3/decide"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
            .with_priority(2)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();

        let (status, _) = mg_post_decide_with_retry_bounded(
            &state,
            &test_user(),
            &HeaderMap::new(),
            "/v1/orchestration/approvals/apr-3/decide",
            json!({"decision": "approve", "reason": ""}),
            Duration::from_millis(100),
        )
        .await;

        // The retry's own 100ms timeout elapsed before the 500ms-delayed
        // response arrived, so the original 502 is what comes back.
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn continue_forwards_only_the_approval_id_and_the_claim_token() {
        let body = ContinueRunBody {
            approval_id: "apr-1".to_owned(),
            claim_token: "claim-1".to_owned(),
        };
        assert_eq!(
            continue_upstream_body(&body, false),
            Some(json!({"approval_id": "apr-1", "claim_token": "claim-1"}))
        );
    }

    #[test]
    fn continue_refuses_a_blank_identifier_before_any_upstream_call() {
        for (approval_id, claim_token) in [
            ("", "claim-1"),
            ("   ", "claim-1"),
            ("apr-1", ""),
            ("apr-1", " \t"),
        ] {
            let body = ContinueRunBody {
                approval_id: approval_id.to_owned(),
                claim_token: claim_token.to_owned(),
            };
            assert_eq!(
                continue_upstream_body(&body, false),
                None,
                "approval {approval_id:?}, claim {claim_token:?}"
            );
        }
    }

    #[test]
    fn continue_body_rejects_any_field_the_client_does_not_own() {
        // A client cannot smuggle tools or authority into a continuation: the
        // extractor refuses the request outright rather than dropping the field.
        let with_tools = json!({
            "approval_id": "apr-1",
            "claim_token": "claim-1",
            "tool_names": ["shell"],
        });
        assert!(serde_json::from_value::<ContinueRunBody>(with_tools).is_err());
    }

    fn continue_body(approval_id: &str, claim_token: &str) -> ContinueRunBody {
        ContinueRunBody {
            approval_id: approval_id.to_owned(),
            claim_token: claim_token.to_owned(),
        }
    }

    /// The committed `continue_upstream_body` (before the park form), kept here as the oracle
    /// the flag-off body must equal for every input.
    fn committed_upstream_body(body: &ContinueRunBody) -> Option<serde_json::Value> {
        if body.approval_id.trim().is_empty() || body.claim_token.trim().is_empty() {
            return None;
        }
        Some(json!({
            "approval_id": body.approval_id,
            "claim_token": body.claim_token,
        }))
    }

    #[test]
    fn continue_forwards_the_park_form_only_with_the_flag_and_an_exact_empty_approval() {
        // On, an exactly empty approval id and a non-blank token: the park form, id present.
        assert_eq!(
            continue_upstream_body(&continue_body("", "claim-1"), true),
            Some(json!({"approval_id": "", "claim_token": "claim-1"}))
        );
        // The token is forwarded as sent (trimming it would change the credential).
        assert_eq!(
            continue_upstream_body(&continue_body("", " claim-1 "), true),
            Some(json!({"approval_id": "", "claim_token": " claim-1 "}))
        );
        // Off: the same body is refused.
        assert_eq!(continue_upstream_body(&continue_body("", "claim-1"), false), None);
        // On, but not exactly empty: a whitespace-only id is still refused, never read as the
        // park form.
        for approval_id in [" ", "   ", "\t", "\n", "\u{a0}"] {
            assert_eq!(
                continue_upstream_body(&continue_body(approval_id, "claim-1"), true),
                None,
                "approval {approval_id:?}"
            );
        }
        // On, an empty id and a blank token: refused.
        for claim_token in ["", " ", " \t"] {
            assert_eq!(
                continue_upstream_body(&continue_body("", claim_token), true),
                None,
                "claim {claim_token:?}"
            );
        }
    }

    #[test]
    fn the_flag_changes_nothing_for_an_ordinary_approval_continue() {
        let cases = [
            ("apr-1", "claim-1"),
            (" apr-1 ", "claim-1"),
            ("apr-1", " "),
            ("apr-1", ""),
            ("   ", "claim-1"),
            ("", ""),
            ("", "claim-1"),
        ];
        for (approval_id, claim_token) in cases {
            let body = continue_body(approval_id, claim_token);
            // Flag off: byte for byte the committed body, for every input.
            assert_eq!(
                continue_upstream_body(&body, false),
                committed_upstream_body(&body),
                "off: approval {approval_id:?}, claim {claim_token:?}"
            );
            // Flag on: only the exactly-empty id with a token differs.
            if !approval_id.is_empty() {
                assert_eq!(
                    continue_upstream_body(&body, true),
                    committed_upstream_body(&body),
                    "on: approval {approval_id:?}, claim {claim_token:?}"
                );
            }
        }
    }

    /// Drives the real route through axum's extractors, as a browser's request would reach it.
    /// Returns the status and the body text. `upstream` is a mock model-gateway.
    async fn post_continue(
        state: AppState,
        body: serde_json::Value,
    ) -> (StatusCode, String) {
        use axum::{body::Body, http::Request, routing::post, Extension, Router};
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let app = Router::new()
            .route("/runs/{run_id}/continue", post(super::continue_run))
            .layer(Extension(test_user()))
            .with_state(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/run-1/continue")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("route answers");
        let status = response.status();
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A state whose every upstream is a mock that must receive nothing.
    fn state_with_unreachable_upstreams(park_form: bool) -> AppState {
        let mut state = test_state(false);
        state.continue_park_form = park_form;
        state
    }

    #[tokio::test]
    async fn with_the_flag_off_a_blank_approval_is_the_committed_400_and_an_absent_key_the_committed_422() {
        // A blank approval id (empty or whitespace): the committed 400, before any upstream call
        // (every upstream of this state refuses connections).
        for approval_id in ["", "   "] {
            let (status, text) = post_continue(
                state_with_unreachable_upstreams(false),
                json!({"approval_id": approval_id, "claim_token": "claim-1"}),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{approval_id:?}: {text}");
            let body: serde_json::Value = serde_json::from_str(&text).expect("a JSON envelope");
            assert_eq!(body["error"]["code"], "invalid_request", "{text}");
        }
        // An absent key is axum's own 422, with the flag off AND on: there is no absent-key form.
        for park_form in [false, true] {
            let (status, text) = post_continue(
                state_with_unreachable_upstreams(park_form),
                json!({"claim_token": "claim-1"}),
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "park_form {park_form}: {text}");
            let (status, text) = post_continue(
                state_with_unreachable_upstreams(park_form),
                json!({"approval_id": "apr-1"}),
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "park_form {park_form}: {text}");
        }
    }

    #[tokio::test]
    async fn the_park_form_with_a_blank_id_that_is_not_empty_or_a_blank_token_is_still_the_400() {
        for (approval_id, claim_token) in [("   ", "claim-1"), ("", "   "), ("", "")] {
            let (status, text) = post_continue(
                state_with_unreachable_upstreams(true),
                json!({"approval_id": approval_id, "claim_token": claim_token}),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{approval_id:?}/{claim_token:?}: {text}");
            let body: serde_json::Value = serde_json::from_str(&text).expect("a JSON envelope");
            assert_eq!(body["error"]["code"], "invalid_request", "{text}");
        }
    }

    #[tokio::test]
    async fn deny_unknown_fields_still_holds() {
        // A client cannot smuggle a field into the park form either: with the flag on and off the
        // extractor refuses the request outright (422), before any upstream call.
        for park_form in [false, true] {
            for body in [
                json!({"approval_id": "", "claim_token": "claim-1", "tool_names": ["shell"]}),
                json!({"approval_id": "apr-1", "claim_token": "claim-1", "mode": "park"}),
                json!({"approval_id": "", "claim_token": "claim-1", "park": true}),
            ] {
                let (status, text) =
                    post_continue(state_with_unreachable_upstreams(park_form), body.clone()).await;
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {text}");
            }
        }
        assert!(serde_json::from_value::<ContinueRunBody>(
            json!({"approval_id": "", "claim_token": "claim-1", "tool_names": ["shell"]})
        )
        .is_err());
    }

    /// With the flag on, the handler posts the park form to model-gateway's continue route:
    /// the id present and empty, the token as sent, and nothing else.
    #[tokio::test]
    async fn the_park_form_reaches_model_gateway_as_an_empty_id_and_the_token_with_the_flag_on() {
        use wiremock::{
            matchers::{body_json, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        mount_continue_token_mocks(&server).await;
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/runs/run-1/continue"))
            .and(body_json(json!({"approval_id": "", "claim_token": "claim-1"})))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "event: run_parked\ndata: {\"run_id\":\"run-1\"}\n\n",
                "text/event-stream",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();
        state.org_core_url = server.uri();
        state.model_gateway_dev_bearer = "dev-model-bearer".into();
        state.continue_park_form = true;

        let (status, text) = post_continue(state, json!({"approval_id": "", "claim_token": "claim-1"})).await;

        assert_eq!(status, StatusCode::OK, "{text}");
        assert!(text.contains("run_parked"), "the stream is relayed: {text}");
    }

    /// With the flag off, the same request never leaves the BFF.
    #[tokio::test]
    async fn the_park_form_never_reaches_model_gateway_with_the_flag_off() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        mount_continue_token_mocks(&server).await;
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/runs/run-1/continue"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();
        state.org_core_url = server.uri();
        state.model_gateway_dev_bearer = "dev-model-bearer".into();
        state.continue_park_form = false;

        let (status, _) = post_continue(state, json!({"approval_id": "", "claim_token": "claim-1"})).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn the_flag_is_an_app_state_field_and_the_handler_passes_it_to_the_body_builder() {
        let source = include_str!("orchestration.rs");
        let handler = source
            .split("pub(crate) async fn continue_run(")
            .nth(1)
            .and_then(|rest| rest.split("pub(crate) async fn cancel_run(").next())
            .expect("the continue handler");
        assert!(
            handler.contains("continue_upstream_body(&body, state.continue_park_form)"),
            "{handler}"
        );
        assert_eq!(handler.matches("continue_park_form").count(), 1);
    }

    const UPSTREAM_TEXT: &str = "UPSTREAM-ONLY-TEXT-must-never-reach-the-client";

    /// The mints `continue_run` makes (model-gateway's dev bearer stands in for the
    /// model audience, so there is no model mint).
    async fn mount_continue_token_mocks(server: &wiremock::MockServer) {
        use wiremock::{
            matchers::{method, path},
            Mock, ResponseTemplate,
        };
        mount_token_mocks(server).await;
        Mock::given(method("GET"))
            .and(path("/api/inference-core/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"token": "inference-test-token"})),
            )
            .mount(server)
            .await;
        // The org's standing posture. Answered here so the lookup does not wait on
        // a refused connection (slow on Windows); a record with no posture is not ZDR.
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"metadata": {}}})))
            .mount(server)
            .await;
    }

    /// Runs the real `continue_run` handler against a model-gateway that answers
    /// `/continue` with the given status and body. Returns the status, the JSON
    /// body and its text as the browser would see them.
    async fn continue_answered_with(
        upstream_status: u16,
        upstream_body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value, String) {
        use axum::{
            extract::{Path, State},
            response::IntoResponse,
            Extension,
        };
        use http_body_util::BodyExt;
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        mount_continue_token_mocks(&server).await;
        Mock::given(method("POST"))
            .and(path("/v1/orchestration/runs/run-1/continue"))
            .respond_with(
                ResponseTemplate::new(upstream_status)
                    .set_body_raw(upstream_body.to_string(), "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();
        state.org_core_url = server.uri();
        state.model_gateway_dev_bearer = "dev-model-bearer".into();

        let response = super::continue_run(
            State(state),
            Extension(test_user()),
            HeaderMap::new(),
            Path("run-1".to_owned()),
            Json(ContinueRunBody {
                approval_id: "apr-1".to_owned(),
                claim_token: "claim-1".to_owned(),
            }),
        )
        .await
        .into_response();
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect the answer")
            .to_bytes();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let value = serde_json::from_str(&text).expect("a JSON error envelope");
        (status, value, text)
    }

    #[test]
    fn the_relayed_continue_errors_are_the_eight_closed_codes_with_their_statuses() {
        let listed: Vec<(&str, u16)> = CONTINUE_RELAYED_ERRORS
            .iter()
            .map(|(code, status, _)| (*code, *status))
            .collect();
        assert_eq!(
            listed,
            vec![
                ("continue_disabled", 409),
                ("not_resumable", 409),
                ("approval_not_found", 404),
                ("approval_not_granted", 409),
                ("approval_expired", 409),
                ("approval_not_resumable", 409),
                ("continuation_already_consumed", 409),
                ("continue_unavailable", 503),
            ]
        );
        // The two the gateway sends that the BFF does not relay: its own guard
        // answers a blank body first, and a missing bearer is `unauthorized` here.
        for never_relayed in ["continue_incomplete", "missing_bearer"] {
            assert!(!listed.iter().any(|(code, _)| *code == never_relayed));
        }
    }

    #[test]
    fn every_relayed_continue_message_is_the_bffs_own_and_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for (code, _, message) in CONTINUE_RELAYED_ERRORS {
            assert!(!message.trim().is_empty(), "{code}");
            assert!(message.is_ascii(), "{code}");
            assert!(!message.contains('_'), "{code}: a message is words, not a code");
            assert!(seen.insert(*message), "{code}: {message} repeats");
        }
    }

    #[tokio::test]
    async fn continue_relays_a_409_with_its_closed_code() {
        for (code, status, message) in CONTINUE_RELAYED_ERRORS {
            let (got, body, text) = continue_answered_with(
                *status,
                json!({"error": {"code": code, "message": UPSTREAM_TEXT}}),
            )
            .await;
            assert_eq!(got.as_u16(), *status, "{code}");
            assert_eq!(body["error"]["code"], *code);
            assert_eq!(body["error"]["message"], *message);
            assert!(!text.contains(UPSTREAM_TEXT), "{code}: {text}");
        }
    }

    #[tokio::test]
    async fn the_same_upstream_body_through_a_non_continue_call_still_yields_model_error() {
        use http_body_util::BodyExt;
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/relay"))
            .respond_with(ResponseTemplate::new(409).set_body_raw(
                json!({"error": {"code": "continuation_already_consumed", "message": UPSTREAM_TEXT}})
                    .to_string(),
                "application/json",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let response = crate::upstream::proxy_sse_stream_with_data_plane(
            &test_state(false),
            reqwest::Method::POST,
            &format!("{}/relay", server.uri()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &[],
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(body["error"]["code"], "model_error");
        assert_eq!(body["error"]["message"], "Model gateway returned 409");
        assert!(!String::from_utf8_lossy(&bytes).contains(UPSTREAM_TEXT));
    }

    #[tokio::test]
    async fn continue_keeps_every_other_upstream_error_closed_and_never_echoes_its_message() {
        // The refusals the gateway does not give a closed continue code, in the
        // nested `{error: {code, message}}` envelope, with a message the browser
        // must not see. (grpc_status_to_http's own body is FLAT, `{"error":
        // "<message>"}`; the test after this one covers that shape.) The BFF answers
        // with its own words and no closed continue code.
        let cases = [
            (403, "permission_denied", "model_error"),
            (401, "unauthenticated", "unauthorized"),
            (403, "approval_not_found", "model_error"),
            (404, "continuation_already_consumed", "model_error"),
            (409, "approval_not_found", "model_error"),
            (400, "continue_incomplete", "model_error"),
            (401, "missing_bearer", "unauthorized"),
            (422, "zdr_feature_not_supported", "model_error"),
            (500, "internal", "model_error"),
            (502, "upstream_unavailable", "model_error"),
        ];
        for (status, code, expected) in cases {
            let (got, body, text) = continue_answered_with(
                status,
                json!({"error": {"code": code, "message": UPSTREAM_TEXT}}),
            )
            .await;
            assert_eq!(got.as_u16(), status, "{code}");
            assert_eq!(body["error"]["code"], expected, "{code} under {status}: {text}");
            assert_eq!(
                body["error"]["message"],
                format!("Model gateway returned {status}")
            );
            assert!(!text.contains(UPSTREAM_TEXT), "{code}: {text}");
            assert!(!text.contains(code), "{code}: {text}");
        }
    }

    #[tokio::test]
    async fn a_flat_grpc_status_body_is_never_echoed_for_a_permission_or_auth_refusal() {
        // grpc_status_to_http answers `{"error": "<message>"}` for a consume
        // PermissionDenied or Unauthenticated, an owner check and the like. The
        // BFF must give the client its own words whatever the message says, and a
        // message that is a listed closed code must not become that code.
        let cases = [
            (403, UPSTREAM_TEXT, "model_error"),
            (401, UPSTREAM_TEXT, "unauthorized"),
            (403, "permission denied: UPSTREAM-ONLY-TEXT-for-this-org", "model_error"),
            (401, "unauthenticated: UPSTREAM-ONLY-TEXT-for-this-token", "unauthorized"),
            (403, "continuation_already_consumed", "model_error"),
            (401, "continuation_already_consumed", "unauthorized"),
            (403, "approval_not_found", "model_error"),
            (401, "continue_unavailable", "unauthorized"),
            // The same flat body under a status the list does name.
            (409, "continuation_already_consumed", "model_error"),
            (404, "approval_not_found", "model_error"),
            (503, "continue_unavailable", "model_error"),
        ];
        for (status, message, expected) in cases {
            let (got, body, text) = continue_answered_with(status, json!({ "error": message })).await;
            assert_eq!(got.as_u16(), status, "{message}");
            assert_eq!(body["error"]["code"], expected, "{message} under {status}: {text}");
            assert_eq!(
                body["error"]["message"],
                format!("Model gateway returned {status}"),
                "{message}"
            );
            assert!(!text.contains("UPSTREAM-ONLY-TEXT"), "{message}: {text}");
            assert!(!text.contains(message), "{message}: {text}");
        }
    }

    #[tokio::test]
    async fn the_continue_relay_has_no_switch_and_names_closed_codes_with_every_model_plane_switch_off() {
        // ACCEPTED CHANGE (WS03 S7c-A5): the continue route's error vocabulary is
        // not behind a switch. A committed model-gateway with its continue switch
        // off answers 409 `continue_disabled`, which this BFF used to turn into
        // `model_error` and now relays by its closed code. No UI calls the route
        // yet. This test pins the behaviour so a later change to it is deliberate:
        // the answer needs no switch, and the handler hands the closed list to the
        // relay unconditionally.
        let (got, body, text) = continue_answered_with(
            409,
            json!({"error": {"code": "continue_disabled", "message": UPSTREAM_TEXT}}),
        )
        .await;
        assert_eq!(got.as_u16(), 409);
        assert_eq!(body["error"]["code"], "continue_disabled", "{text}");
        assert_ne!(body["error"]["code"], "model_error");

        let source = include_str!("orchestration.rs").replace("\r\n", "\n");
        let handler = source
            .split("pub(crate) async fn continue_run(")
            .nth(1)
            .expect("continue_run exists")
            .split("pub(crate) async fn cancel_run(")
            .next()
            .expect("cancel_run follows");
        assert_eq!(
            handler.matches("CONTINUE_RELAYED_ERRORS").count(),
            1,
            "continue_run passes the closed list once, unconditionally"
        );
        assert!(!handler.contains("&[]"), "continue_run never passes an empty list");
        assert!(!handler.contains("env::var"), "no switch is read here");
    }

    #[test]
    fn the_already_used_message_does_not_claim_a_run_was_continued() {
        // After continue_unavailable a same-token retry can answer
        // continuation_already_consumed although no run was launched.
        let message = CONTINUE_RELAYED_ERRORS
            .iter()
            .find(|(code, _, _)| *code == "continuation_already_consumed")
            .map(|(_, _, message)| *message)
            .expect("the code is listed");
        assert!(!message.contains("continue the run"), "{message}");
        assert!(message.contains("Check the run"), "{message}");
    }
}
