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
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_data_plane,
        proxy_model_json_with_session, required_execution_token, required_session_token,
    },
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // ── reads ──────────────────────────────────────────────────────────
        .route("/api/v1/orchestration/runs/:run_id/plans", get(list_plans))
        .route("/api/v1/orchestration/plans/:plan_id", get(get_plan))
        .route(
            "/api/v1/orchestration/threads/:thread_id/todos",
            get(list_todos),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/approvals",
            get(list_approvals),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/proof-bundle",
            get(get_run_proof_bundle),
        )
        .route(
            "/api/v1/orchestration/verification-metrics",
            get(get_verification_metrics),
        )
        .route(
            "/api/v1/orchestration/approvals/:approval_id",
            get(get_approval),
        )
        .route(
            "/api/v1/orchestration/threads/:thread_id/lineage",
            get(get_lineage),
        )
        // ── decisions / run control ──────────────────────────────────────────
        .route(
            "/api/v1/orchestration/approvals/:approval_id/decide",
            post(decide_approval),
        )
        .route(
            "/api/v1/orchestration/plans/:plan_id/approve",
            post(approve_plan),
        )
        .route(
            "/api/v1/orchestration/plans/:plan_id/reject",
            post(reject_plan),
        )
        .route(
            "/api/v1/orchestration/todos/:todo_id/status",
            post(update_todo_status),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/resume",
            post(resume_run),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/cancel",
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
async fn decide_approval(
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

async fn resume_run(
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

async fn cancel_run(
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
        is_retryable_decide_status, mg_post_decide_with_retry_bounded,
        requires_execution_delegation, verification_metrics_path,
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
            audience_token_cache: new_audience_token_cache(),
            browser_run_store: crate::domains::browser::new_browser_run_store(),
            rate_limiter: crate::rate_limit::RateLimiter::from_cache(&ResultCache::disabled()),
            cache: ResultCache::disabled(),
            chat_history_store: crate::domains::chat::history::ChatHistoryStore::new(),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers: false,
            allow_dev_auth_bypass,
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
}
