//! `/v1/browser/runs*` — Phase 2 durable browser-agent run trigger.
//!
//! This module starts a durable, server-side, multi-step browser-agent run
//! and lets the caller control it (pause/resume/stop). It deliberately does
//! **not** stream anything itself: `run_browser_agent_loop`
//! (execution-core) is already a complete bounded multi-step loop, and a
//! single `ExecutionCore.ExecuteStep(tool_name = "browser_agent")` call
//! drives it end to end while emitting `BrowserActionDispatched` /
//! `BrowserObservationReceived` / `BrowserRunPaused` / `BrowserRunResumed`
//! events onto session-core's existing orchestration-event backbone.
//! Callers observe progress via the existing `GET /v1/runs/:run_id/events`
//! SSE route — no new streaming transport is introduced.
//!
//! See `docs/BROWSER_WORKSPACE_PLAN.md` ("Phase 2 implementation spec —
//! durable browser-agent run") for the full architectural rationale,
//! including why this calls `ExecuteStep` directly rather than routing
//! through the generic multi-tool chat loop.

use std::collections::BTreeSet;
use std::net::IpAddr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    CancelRunRequest, ExecuteStepRequest, PauseRunRequest, ResumeRunRequest, ValidateGrantRequest,
};
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::{
    auth::{
        Claims, VerifiedBrowserBearer, VerifiedDataPlaneBearer as VerifiedBearer,
        VerifiedExecutionBearer, VerifiedInferenceBearer, VerifiedSessionBearer,
    },
    http_routes::grpc_status_to_http,
    state::AppState,
};

type ApiError = (StatusCode, Json<Value>);

/// `/v1/browser/runs*` routes. Merged into the auth-gated router next to the
/// existing `/v1/browser/suggest-action`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/browser/runs", post(browser_run_start))
        .route(
            "/v1/browser/runs/:run_id/control",
            post(browser_run_control),
        )
}

fn bad_request(message: impl Into<String>) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
}

/// The standalone browser-run endpoint has no durable continuation descriptor
/// that can resume a browser-agent loop after an approval decision. Until that
/// state machine exists, accepting `require_approval=true` would leave a run in
/// a misleading non-terminal/"completed" projection. Reject before any grant,
/// thread, or run write rather than pretending an approval workflow exists.
fn reject_unsupported_browser_approval(request: &BrowserRunStartRequest) -> Result<(), ApiError> {
    if request.require_approval == Some(true) {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(json!({
                "error": "browser_approval_continuation_unavailable"
            })),
        ));
    }
    Ok(())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(Debug, Deserialize)]
struct BrowserRunStartRequest {
    #[serde(default)]
    thread_id: Option<String>,
    goal: String,
    grant_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    profile_id: Option<String>,
    /// Where to navigate first — see `PlanConfig::start_url` (execution-core).
    /// A freshly acquired Quarry lease has no page loaded; without this the
    /// loop's first action fails with "no current page". Typically the URL
    /// of the tab the run was launched from.
    #[serde(default)]
    start_url: Option<String>,
    // `Option`, not a bare `Vec`, because callers (the Velion gateway) may
    // send an explicit JSON `null` for "no restriction yet" rather than
    // omitting the key — `Vec<String>` rejects `null` even with
    // `#[serde(default)]` (default only covers a *missing* key), which
    // otherwise 422s a well-formed request.
    #[serde(default)]
    allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    max_steps: Option<i32>,
    #[serde(default)]
    max_runtime_s: Option<i32>,
    #[serde(default)]
    stop_criteria: Option<String>,
    #[serde(default)]
    require_approval: Option<bool>,
    #[serde(default)]
    max_cost_usd: Option<f64>,
    /// Accepted so a well-formed caller body never 4xxs, but never trusted:
    /// this endpoint is called by the Velion gateway, which derives `zdr`
    /// server-side from the browser session's own persisted metadata and
    /// never forwards a client-supplied flag. Kept `false`-default here as
    /// the safe fallback for any other caller.
    #[serde(default)]
    zdr: bool,
}

/// Broker-issued browser authority resolved before the gateway creates a
/// thread/run. This is deliberately distinct from the caller's JSON request:
/// only the broker can set the grant id and domain policy that reaches
/// execution-core.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedBrowserGrant {
    grant_id: String,
    allowed_domains: Vec<String>,
}

async fn resolve_browser_grant(
    state: &AppState,
    request: &BrowserRunStartRequest,
    browser_bearer: &VerifiedBrowserBearer,
) -> Result<ResolvedBrowserGrant, ApiError> {
    let grant_id = request.grant_id.trim();
    if grant_id.is_empty() {
        return Err(bad_request("grant_id must not be empty"));
    }

    let mut broker = state.browser_client.clone();
    let response = broker
        .validate_grant(authenticated_browser_request(
            ValidateGrantRequest {
                grant_id: grant_id.to_owned(),
            },
            browser_bearer,
        )?)
        .await
        .map_err(|status| browser_grant_status_to_http(&status))?
        .into_inner();

    if !response.active || response.grant_id.trim() != grant_id {
        // Never distinguish forged, expired, revoked, or cross-tenant grants.
        return Err(browser_grant_forbidden());
    }
    let broker_policy = canonical_browser_domains(&response.allowed_domains)
        .map_err(|()| browser_grant_forbidden())?;
    // A broker response must already be canonical. Accepting a loosely parsed
    // response here could make gateway and execution-core derive different
    // policies for the same grant.
    if broker_policy != response.allowed_domains {
        return Err(browser_grant_forbidden());
    }
    let allowed_domains = match request.allowed_domains.as_deref() {
        None => broker_policy,
        Some(requested) => {
            let requested = canonical_browser_domains(requested)
                .map_err(|()| bad_request("allowed_domains must be a bounded hostname policy"))?;
            // The request cannot select a different policy. A narrower policy
            // belongs on a newly issued grant, so it remains broker-signed at
            // both gateway and execution dispatch.
            if requested != broker_policy {
                return Err(bad_request(
                    "allowed_domains must exactly match the broker grant policy",
                ));
            }
            requested
        }
    };
    if let Some(start_url) = request
        .start_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        if !url_is_allowed_by_policy(start_url, &allowed_domains) {
            return Err(bad_request("start_url is outside the broker grant policy"));
        }
    }

    Ok(ResolvedBrowserGrant {
        grant_id: grant_id.to_owned(),
        allowed_domains,
    })
}

fn browser_grant_forbidden() -> ApiError {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": "browser grant is not authorized" })),
    )
}

fn browser_grant_status_to_http(status: &tonic::Status) -> ApiError {
    if matches!(
        status.code(),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
    ) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "browser grant validation unavailable" })),
        )
    } else {
        browser_grant_forbidden()
    }
}

fn canonical_browser_domains(domains: &[String]) -> Result<Vec<String>, ()> {
    const MAX_ALLOWED_DOMAINS: usize = 32;
    if domains.is_empty() || domains.len() > MAX_ALLOWED_DOMAINS {
        return Err(());
    }
    let mut canonical = BTreeSet::new();
    for raw in domains {
        let domain = raw.trim().to_ascii_lowercase();
        if !is_canonical_browser_domain(&domain) {
            return Err(());
        }
        canonical.insert(domain);
    }
    if canonical.is_empty() {
        return Err(());
    }
    Ok(canonical.into_iter().collect())
}

fn is_canonical_browser_domain(domain: &str) -> bool {
    if domain.is_empty()
        || domain.len() > 253
        || domain.parse::<IpAddr>().is_ok()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn url_is_allowed_by_policy(url: &str, allowed_domains: &[String]) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    allowed_domains.iter().any(|domain| {
        host == *domain
            || (host.len() > domain.len()
                && host.ends_with(domain)
                && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
    })
}

/// `POST /v1/browser/runs` — start a durable browser-agent run (Phase 2).
///
/// 1. Ensures a session-core thread (reuses `thread_id` when supplied).
/// 2. `StartRun(agent_id="browser-agent")` → a durable `run_id`.
/// 3. Fire-and-forgets one `ExecutionCore.ExecuteStep(tool_name="browser_agent")`
///    call — `permission_mode: "auto"` deliberately, so the run doesn't pause
///    for HITL approval before the loop's own gates (`max_steps`,
///    `allowed_domains`, `stop_criteria`, `max_cost_usd`) ever run; see the
///    plan doc's "Permission mode" section for the full rationale.
/// 4. Returns immediately; the caller streams progress from the existing
///    `GET /v1/runs/:run_id/events`.
// Cohesive single-flow handler (validate → thread → run → dispatch); splitting
// it would obscure the linear session-core/execution-core sequencing.
#[allow(clippy::too_many_lines)]
async fn browser_run_start(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    data_plane_bearer: Option<Extension<VerifiedBearer>>,
    execution_bearer: VerifiedExecutionBearer,
    session_bearer: VerifiedSessionBearer,
    inference_bearer: VerifiedInferenceBearer,
    browser_bearer: VerifiedBrowserBearer,
    Json(req): Json<BrowserRunStartRequest>,
) -> Result<Json<Value>, ApiError> {
    if browser_run_zdr_blocked(&claims, req.zdr) {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(json!({
                "error": "ZDR browser runs are disabled until the durable run lifecycle is bypassed"
            })),
        ));
    }
    let data_plane_bearer = data_plane_bearer
        .map(|Extension(value)| value)
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "verified user credential required" })),
            )
        })?;
    if req.goal.trim().is_empty() {
        return Err(bad_request("goal must not be empty"));
    }
    reject_unsupported_browser_approval(&req)?;
    // Tenant/user identity is always server-derived from the verified bearer
    // claims — never trusted from the request body (the same IDOR-safety
    // rule the rest of model-gateway follows, e.g. `browser_suggest_action`).
    let org_id = claims.org_id.clone();
    let user_id = claims.user_id.clone();

    // Validate the opaque caller-provided id before *any* thread/run write.
    // A cross-tenant/forged/missing policy grant must not create a durable run
    // that later fails asynchronously in execution-core.
    let browser_grant = resolve_browser_grant(&state, &req, &browser_bearer).await?;

    let run = crate::session_flow::prepare_managed_run_authenticated(
        &state,
        req.thread_id.as_deref(),
        None,
        &org_id,
        &user_id,
        &req.goal,
        "browser-agent",
        "execute",
        &format!("browser-{}", new_ulid()),
        mp_contracts::model_plane::v1::ManagedRunSource::GatewayBrowser,
        false,
        &session_bearer,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("session-core managed start failed: {error}") })),
        )
    })?;
    if run.already_started {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "managed browser run already exists; observe or resume the existing run",
                "run_id": run.run_id,
            })),
        ));
    }
    // The browser endpoint has a GatewayBrowser managed-run obligation even
    // though Execution Core performs the action. Fail before dispatch if the
    // Gateway's scoped workload credential cannot renew that exact lease;
    // never substitute the delegated Session Core user bearer.
    crate::session_flow::ensure_browser_agent_run_liveness(&state, &run)
        .await
        .map_err(|error| {
            warn!(%error, run_id = %run.run_id, "initial browser managed-run liveness heartbeat failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "session-core liveness heartbeat failed" })),
            )
        })?;
    let terminal_run = run.clone();
    let thread_id = run.thread_id;
    let run_id = run.run_id;
    let plan_id = non_empty(req.plan_id.as_deref()).map_or_else(new_ulid, str::to_owned);

    // `run_id` is embedded in the tool_input JSON (not just the
    // ExecuteStepRequest envelope) because `tool_bridge::execute_browser_agent`
    // reads `PlanConfig.run_id` from here — it's what every `BrowserEventSink`
    // method and the new pause/cancel `StateStore` gate key off of. Without
    // it the loop runs "blind": no events, no pause/resume/stop.
    let tool_input = json!({
        "grant_id": browser_grant.grant_id,
        "plan_id": plan_id,
        "run_id": run_id,
        "org_id": org_id,
        "system_prompt": req.goal,
        "max_steps": req.max_steps,
        "max_runtime_s": req.max_runtime_s,
        "allowed_domains": browser_grant.allowed_domains,
        "stop_criteria": req.stop_criteria,
        "require_approval": req.require_approval,
        "max_cost_usd": req.max_cost_usd,
        "zdr": req.zdr,
        "profile_id": req.profile_id,
        "start_url": req.start_url,
    })
    .to_string();

    let mut execution_client = state.execution_client.clone();
    let terminal_state = state.clone();
    let step_id = new_ulid();
    let dispatch_run_id = run_id.clone();
    tokio::spawn(async move {
        let mut execution_request = tonic::Request::new(ExecuteStepRequest {
            run_id: dispatch_run_id.clone(),
            step_id,
            tool_name: "browser_agent".to_owned(),
            tool_input,
            permission_mode: "auto".to_owned(),
            hook_context: String::new(),
            org_id,
            user_id,
            zdr: false,
        });
        let authorization = format!("Bearer {}", execution_bearer.as_str())
            .parse()
            .expect("verified bearer is valid gRPC metadata");
        execution_request
            .metadata_mut()
            .insert("authorization", authorization);
        execution_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {}", data_plane_bearer.as_str())
                .parse()
                .expect("verified Data Plane bearer is valid gRPC metadata"),
        );
        execution_request.metadata_mut().insert(
            "x-session-authorization",
            format!("Bearer {}", session_bearer.as_str())
                .parse()
                .expect("verified Session Core bearer is valid gRPC metadata"),
        );
        execution_request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {}", inference_bearer.as_str())
                .parse()
                .expect("verified Inference Core bearer is valid gRPC metadata"),
        );
        execution_request.metadata_mut().insert(
            "x-browser-authorization",
            format!("Bearer {}", browser_bearer.as_str())
                .parse()
                .expect("verified Browser Broker bearer is valid gRPC metadata"),
        );
        let execution_result = {
            let execution = execution_client.execute_step(execution_request);
            tokio::pin!(execution);
            let mut heartbeat =
                tokio::time::interval(crate::session_flow::MANAGED_RUN_HEARTBEAT_INTERVAL);
            // The request path obtained the initial liveness receipt. Start
            // periodic renewal only after that five-minute cadence elapses.
            heartbeat.tick().await;
            loop {
                tokio::select! {
                    result = &mut execution => break result,
                    _ = heartbeat.tick() => match crate::session_flow::heartbeat_browser_agent_run(
                        &terminal_state,
                        &terminal_run,
                    ).await {
                        Ok(true) => {}
                        Ok(false) => {
                            warn!(run_id = %dispatch_run_id, "browser managed run became terminal while execution was still pending; refusing to accept a late outcome");
                            return;
                        }
                        Err(error) => {
                            warn!(%error, run_id = %dispatch_run_id, "browser managed-run liveness heartbeat failed while execution was pending");
                            return;
                        }
                    },
                }
            }
        };
        match execution_result {
            Ok(resp) => {
                let resp = resp.into_inner();
                let outcome = classify_browser_execution_status(&resp.status);
                match browser_run_finalization(outcome) {
                    BrowserRunFinalization::Complete(terminal) => {
                        if let Err(error) =
                            crate::session_flow::terminalize_browser_agent_run_authenticated(
                                &terminal_state,
                                &terminal_run,
                                terminal,
                            )
                            .await
                        {
                            // No in-memory retry is a durable recovery mechanism.
                            // Keep the run non-terminal and make the receipt gap
                            // explicit for the operator/reconciler.
                            warn!(%error, run_id = %dispatch_run_id, "confirmed browser-agent outcome could not be durably terminalized; run remains retriable");
                        }
                    }
                    BrowserRunFinalization::Cancel(reason) => {
                        if let Err(error) =
                            crate::session_flow::cancel_browser_agent_run_authenticated(
                                &terminal_state,
                                &dispatch_run_id,
                                reason,
                                &session_bearer,
                            )
                            .await
                        {
                            // Cancellation is a distinct durable lifecycle
                            // operation. Never fall back to CompleteStep, which
                            // would be able to fabricate a completed terminal.
                            warn!(%error, run_id = %dispatch_run_id, "confirmed browser-agent cancellation could not be durably recorded; run remains retriable");
                        }
                    }
                    BrowserRunFinalization::Observe => match outcome {
                        BrowserExecutionOutcome::AwaitingApproval => {
                            // A pause is a governed non-terminal state. In
                            // particular, this background dispatcher must never
                            // turn it into a terminal failure/completion merely
                            // because the HTTP start response has already returned.
                            info!(run_id = %dispatch_run_id, "browser-agent run is awaiting approval");
                        }
                        BrowserExecutionOutcome::Unknown => {
                            // A new/unknown response status cannot safely be
                            // interpreted as terminal. Leave the durable run for
                            // observation/retry rather than guessing a transition.
                            warn!(run_id = %dispatch_run_id, status = %resp.status, "browser-agent ExecuteStep returned an unknown lifecycle status");
                        }
                        BrowserExecutionOutcome::Completed
                        | BrowserExecutionOutcome::Failed(_)
                        | BrowserExecutionOutcome::Cancelled(_) => {
                            unreachable!("terminal browser outcome must have a finalization action")
                        }
                    },
                }
            }
            Err(error) => {
                warn!(
                    error = %error,
                    run_id = %dispatch_run_id,
                    "browser-agent ExecuteStep dispatch failed"
                );
            }
        }
    });

    Ok(Json(json!({
        "run_id": run_id,
        "thread_id": thread_id,
        "plan_id": plan_id,
    })))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserExecutionOutcome {
    Completed,
    Failed(&'static str),
    Cancelled(&'static str),
    AwaitingApproval,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserRunFinalization {
    Complete(crate::session_flow::BrowserAgentTerminal),
    Cancel(&'static str),
    Observe,
}

/// Classify only documented, confirmed `ExecuteStep` outcomes. The browser
/// endpoint may terminalize these after the response; a missing or unknown
/// response deliberately remains non-terminal for reconciliation.
fn classify_browser_execution_status(status: &str) -> BrowserExecutionOutcome {
    match status {
        "completed" => BrowserExecutionOutcome::Completed,
        "failed" => BrowserExecutionOutcome::Failed("browser_agent_failed"),
        "permission_denied" => BrowserExecutionOutcome::Failed("browser_agent_permission_denied"),
        "timed_out" => BrowserExecutionOutcome::Failed("browser_agent_timed_out"),
        "resource_exhausted" => BrowserExecutionOutcome::Failed("browser_agent_resource_exhausted"),
        "cancelled" => BrowserExecutionOutcome::Cancelled("browser_agent_cancelled"),
        "aborted" => BrowserExecutionOutcome::Cancelled("browser_agent_aborted"),
        "awaiting_approval" => BrowserExecutionOutcome::AwaitingApproval,
        _ => BrowserExecutionOutcome::Unknown,
    }
}

/// Map a confirmed execution result to the one legal Session Core lifecycle
/// write. Only the explicit `completed` response can complete a run. Browser
/// cancellation/abort must use `RunService.CancelRun`, and approval denial or
/// timeout remains a failed terminal step.
fn browser_run_finalization(outcome: BrowserExecutionOutcome) -> BrowserRunFinalization {
    match outcome {
        BrowserExecutionOutcome::Completed => {
            BrowserRunFinalization::Complete(crate::session_flow::BrowserAgentTerminal::Completed)
        }
        BrowserExecutionOutcome::Failed(code) => BrowserRunFinalization::Complete(
            crate::session_flow::BrowserAgentTerminal::Failed(code),
        ),
        BrowserExecutionOutcome::Cancelled(reason) => BrowserRunFinalization::Cancel(reason),
        BrowserExecutionOutcome::AwaitingApproval | BrowserExecutionOutcome::Unknown => {
            BrowserRunFinalization::Observe
        }
    }
}

fn browser_run_zdr_blocked(claims: &Claims, request_zdr: bool) -> bool {
    claims.effective_zdr(request_zdr)
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BrowserRunControlAction {
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Deserialize)]
struct BrowserRunControlRequest {
    action: BrowserRunControlAction,
}

/// `POST /v1/browser/runs/:run_id/control` — pause / resume / stop a durable
/// browser-agent run (Phase 2 B5). A thin 1:1 mapping onto `ExecutionCore`'s
/// `PauseRun` / `ResumeRun` / `CancelRun` RPCs.
async fn browser_run_control(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    execution_bearer: VerifiedExecutionBearer,
    session_bearer: VerifiedSessionBearer,
    Path(run_id): Path<String>,
    Json(req): Json<BrowserRunControlRequest>,
) -> Result<Json<Value>, ApiError> {
    // These controls mutate the lifecycle of a retained run. In particular,
    // Resume can restart browser actions and session events for a pre-existing
    // durable run, so issuer-enforced ZDR must stop every action before any
    // Execution Core request is constructed or forwarded.
    if browser_run_zdr_blocked(&claims, false) {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(json!({
                "error": "ZDR browser-run controls are disabled until the durable run lifecycle is bypassed"
            })),
        ));
    }
    if run_id.trim().is_empty() {
        return Err(bad_request("run_id must not be empty"));
    }

    let mut execution_client = state.execution_client.clone();
    let status = match req.action {
        BrowserRunControlAction::Pause => {
            let request = authenticated_execution_request(
                PauseRunRequest {
                    run_id: run_id.clone(),
                    org_id: claims.org_id.clone(),
                },
                &execution_bearer,
                &session_bearer,
            )?;
            let resp = execution_client
                .pause_run(request)
                .await
                .map_err(|e| grpc_status_to_http(&e))?
                .into_inner();
            if resp.paused {
                "paused"
            } else {
                "running"
            }
        }
        BrowserRunControlAction::Resume => {
            let request = authenticated_execution_request(
                ResumeRunRequest {
                    run_id: run_id.clone(),
                    checkpoint_id: String::new(),
                    org_id: claims.org_id.clone(),
                    approval_id: String::new(),
                },
                &execution_bearer,
                &session_bearer,
            )?;
            let resp = execution_client
                .resume_run(request)
                .await
                .map_err(|e| grpc_status_to_http(&e))?
                .into_inner();
            if resp.resumed {
                "running"
            } else {
                "paused"
            }
        }
        BrowserRunControlAction::Stop => {
            let request = authenticated_execution_request(
                CancelRunRequest {
                    run_id: run_id.clone(),
                    reason: "user_stop".to_owned(),
                },
                &execution_bearer,
                &session_bearer,
            )?;
            let resp = execution_client
                .cancel_run(request)
                .await
                .map_err(|e| grpc_status_to_http(&e))?
                .into_inner();
            if resp.cancelled {
                "cancelled"
            } else {
                "running"
            }
        }
    };

    Ok(Json(json!({ "status": status })))
}

fn authenticated_session_request<T>(
    message: T,
    session_bearer: &VerifiedSessionBearer,
) -> Result<tonic::Request<T>, ApiError> {
    let authorization = format!("Bearer {}", session_bearer.as_str())
        .parse()
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid session credential" })),
            )
        })?;
    let mut request = tonic::Request::new(message);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

fn authenticated_browser_request<T>(
    message: T,
    browser_bearer: &VerifiedBrowserBearer,
) -> Result<tonic::Request<T>, ApiError> {
    let authorization = format!("Bearer {}", browser_bearer.as_str())
        .parse()
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid browser credential" })),
            )
        })?;
    let mut request = tonic::Request::new(message);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

fn authenticated_execution_request<T>(
    message: T,
    execution_bearer: &VerifiedExecutionBearer,
    session_bearer: &VerifiedSessionBearer,
) -> Result<tonic::Request<T>, ApiError> {
    let authorization = format!("Bearer {}", execution_bearer.as_str())
        .parse()
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid credential" })),
            )
        })?;
    let mut request = tonic::Request::new(message);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    request.metadata_mut().insert(
        "x-session-authorization",
        format!("Bearer {}", session_bearer.as_str())
            .parse()
            .map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "invalid credential" })),
                )
            })?,
    );
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use mp_contracts::model_plane::v1::{
        browser_broker_client::BrowserBrokerClient,
        browser_broker_server::{BrowserBroker, BrowserBrokerServer},
        execution_core_client::ExecutionCoreClient,
        execution_core_server::{ExecutionCore, ExecutionCoreServer},
        AcquireGrantRequest, AcquireGrantResponse, BrowserHealthRequest, BrowserHealthResponse,
        CancelRunRequest, CancelRunResponse, CreateThreadRequest, ExecuteStepRequest,
        ExecuteStepResponse, PauseRunRequest, PauseRunResponse, ResumeRunRequest,
        ResumeRunResponse, RevokeGrantRequest, RevokeGrantResponse, RunAgentRequest,
        RunAgentResponse, ValidateGrantResponse,
    };
    use tonic::{Response, Status};

    #[derive(Clone)]
    struct TestBrowserBroker {
        validation: Result<ValidateGrantResponse, tonic::Code>,
        authorizations: Arc<Mutex<Vec<String>>>,
    }

    #[tonic::async_trait]
    impl BrowserBroker for TestBrowserBroker {
        async fn acquire_grant(
            &self,
            _request: tonic::Request<AcquireGrantRequest>,
        ) -> Result<Response<AcquireGrantResponse>, Status> {
            Err(Status::unimplemented("not used by validation test"))
        }

        async fn revoke_grant(
            &self,
            _request: tonic::Request<RevokeGrantRequest>,
        ) -> Result<Response<RevokeGrantResponse>, Status> {
            Err(Status::unimplemented("not used by validation test"))
        }

        async fn validate_grant(
            &self,
            request: tonic::Request<ValidateGrantRequest>,
        ) -> Result<Response<ValidateGrantResponse>, Status> {
            if let Some(value) = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
            {
                self.authorizations
                    .lock()
                    .expect("authorization recorder")
                    .push(value.to_owned());
            }
            match &self.validation {
                Ok(response) => Ok(Response::new(response.clone())),
                Err(code) => Err(Status::new(*code, "test broker rejection")),
            }
        }

        async fn health(
            &self,
            _request: tonic::Request<BrowserHealthRequest>,
        ) -> Result<Response<BrowserHealthResponse>, Status> {
            Ok(Response::new(BrowserHealthResponse {
                status: "ok".to_owned(),
            }))
        }
    }

    async fn state_with_browser_broker(
        broker: TestBrowserBroker,
    ) -> (AppState, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test broker");
        let address = listener.local_addr().expect("test broker address");
        let handle = tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(BrowserBrokerServer::new(broker))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.browser_client = BrowserBrokerClient::connect(format!("http://{address}"))
            .await
            .expect("connect test broker");
        (state, handle)
    }

    #[derive(Clone)]
    struct ExecutionControlRecorder {
        calls: Arc<AtomicUsize>,
    }

    #[tonic::async_trait]
    impl ExecutionCore for ExecutionControlRecorder {
        async fn execute_step(
            &self,
            _request: tonic::Request<ExecuteStepRequest>,
        ) -> Result<Response<ExecuteStepResponse>, Status> {
            Err(Status::unimplemented("not used by browser control test"))
        }

        async fn resume_run(
            &self,
            _request: tonic::Request<ResumeRunRequest>,
        ) -> Result<Response<ResumeRunResponse>, Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(Status::unimplemented("browser control must be blocked"))
        }

        async fn cancel_run(
            &self,
            _request: tonic::Request<CancelRunRequest>,
        ) -> Result<Response<CancelRunResponse>, Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(Status::unimplemented("browser control must be blocked"))
        }

        async fn pause_run(
            &self,
            _request: tonic::Request<PauseRunRequest>,
        ) -> Result<Response<PauseRunResponse>, Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(Status::unimplemented("browser control must be blocked"))
        }

        async fn run_agent(
            &self,
            _request: tonic::Request<RunAgentRequest>,
        ) -> Result<Response<RunAgentResponse>, Status> {
            Err(Status::unimplemented("not used by browser control test"))
        }
    }

    async fn state_with_execution_control_recorder(
        calls: Arc<AtomicUsize>,
    ) -> (AppState, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind execution control recorder");
        let address = listener
            .local_addr()
            .expect("execution control recorder address");
        let handle = tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(ExecutionCoreServer::new(ExecutionControlRecorder { calls }))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.execution_client = ExecutionCoreClient::connect(format!("http://{address}"))
            .await
            .expect("connect execution control recorder");
        (state, handle)
    }

    fn browser_request(grant_id: &str) -> BrowserRunStartRequest {
        BrowserRunStartRequest {
            thread_id: None,
            goal: "safe browser task".to_owned(),
            grant_id: grant_id.to_owned(),
            plan_id: None,
            profile_id: None,
            start_url: Some("https://example.com/start".to_owned()),
            allowed_domains: None,
            max_steps: None,
            max_runtime_s: None,
            stop_criteria: None,
            require_approval: None,
            max_cost_usd: None,
            zdr: false,
        }
    }

    fn browser_claims(zdr: bool) -> Claims {
        Claims {
            sub: "user-a".to_owned(),
            iss: "auth-core".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    #[test]
    fn control_action_parses_snake_case() {
        let pause: BrowserRunControlRequest =
            serde_json::from_str(r#"{"action":"pause"}"#).expect("valid pause body");
        assert_eq!(pause.action, BrowserRunControlAction::Pause);

        let resume: BrowserRunControlRequest =
            serde_json::from_str(r#"{"action":"resume"}"#).expect("valid resume body");
        assert_eq!(resume.action, BrowserRunControlAction::Resume);

        let stop: BrowserRunControlRequest =
            serde_json::from_str(r#"{"action":"stop"}"#).expect("valid stop body");
        assert_eq!(stop.action, BrowserRunControlAction::Stop);
    }

    #[test]
    fn control_action_rejects_unknown_values() {
        let result: Result<BrowserRunControlRequest, _> =
            serde_json::from_str(r#"{"action":"terminate"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn start_request_defaults_optional_fields() {
        let req: BrowserRunStartRequest =
            serde_json::from_str(r#"{"goal":"find flights","grant_id":"session:s1"}"#)
                .expect("minimal valid body");
        assert_eq!(req.goal, "find flights");
        assert_eq!(req.grant_id, "session:s1");
        assert!(req.thread_id.is_none());
        assert!(req.plan_id.is_none());
        assert!(req.profile_id.is_none());
        assert!(req.allowed_domains.is_none());
        assert!(!req.zdr);
    }

    #[test]
    fn start_request_accepts_explicit_null_allowed_domains() {
        // Regression: the Velion gateway sends an explicit JSON `null` (not a
        // missing key) when it has no restriction to forward — `Vec<String>`
        // rejects that even with `#[serde(default)]` (a 422 seen live before
        // this field became `Option<Vec<String>>`).
        let req: BrowserRunStartRequest =
            serde_json::from_str(r#"{"goal":"g","grant_id":"session:s1","allowed_domains":null}"#)
                .expect("explicit null allowed_domains must deserialize");
        assert!(req.allowed_domains.is_none());
    }

    #[test]
    fn require_approval_is_quarantined_before_browser_run_dispatch_exists() {
        let mut request = browser_request("grant-approval");
        request.require_approval = Some(true);

        let error = reject_unsupported_browser_approval(&request)
            .expect_err("browser approval cannot be accepted without durable continuation");
        assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            error.1 .0["error"],
            "browser_approval_continuation_unavailable"
        );

        request.require_approval = Some(false);
        assert!(reject_unsupported_browser_approval(&request).is_ok());
    }

    #[test]
    fn browser_execute_step_outcomes_preserve_non_success_terminal_meaning() {
        assert_eq!(
            classify_browser_execution_status("completed"),
            BrowserExecutionOutcome::Completed
        );
        assert_eq!(
            classify_browser_execution_status("failed"),
            BrowserExecutionOutcome::Failed("browser_agent_failed")
        );
        assert_eq!(
            classify_browser_execution_status("permission_denied"),
            BrowserExecutionOutcome::Failed("browser_agent_permission_denied")
        );
        assert_eq!(
            classify_browser_execution_status("timed_out"),
            BrowserExecutionOutcome::Failed("browser_agent_timed_out")
        );
        assert_eq!(
            classify_browser_execution_status("resource_exhausted"),
            BrowserExecutionOutcome::Failed("browser_agent_resource_exhausted")
        );
        assert_eq!(
            classify_browser_execution_status("cancelled"),
            BrowserExecutionOutcome::Cancelled("browser_agent_cancelled")
        );
        assert_eq!(
            classify_browser_execution_status("aborted"),
            BrowserExecutionOutcome::Cancelled("browser_agent_aborted")
        );
        assert_eq!(
            classify_browser_execution_status("awaiting_approval"),
            BrowserExecutionOutcome::AwaitingApproval,
            "approval pauses must never be terminalized by the browser starter"
        );
        assert_eq!(
            classify_browser_execution_status("transport_unknown"),
            BrowserExecutionOutcome::Unknown,
            "unrecognized responses remain durable/retriable"
        );
    }

    #[test]
    fn denial_timeout_and_cancel_never_terminalize_as_completed() {
        let cases = [
            (
                "permission_denied",
                BrowserRunFinalization::Complete(
                    crate::session_flow::BrowserAgentTerminal::Failed(
                        "browser_agent_permission_denied",
                    ),
                ),
            ),
            (
                "timed_out",
                BrowserRunFinalization::Complete(
                    crate::session_flow::BrowserAgentTerminal::Failed("browser_agent_timed_out"),
                ),
            ),
            (
                "resource_exhausted",
                BrowserRunFinalization::Complete(
                    crate::session_flow::BrowserAgentTerminal::Failed(
                        "browser_agent_resource_exhausted",
                    ),
                ),
            ),
            (
                "cancelled",
                BrowserRunFinalization::Cancel("browser_agent_cancelled"),
            ),
            (
                "aborted",
                BrowserRunFinalization::Cancel("browser_agent_aborted"),
            ),
        ];

        for (status, expected) in cases {
            let finalization = browser_run_finalization(classify_browser_execution_status(status));
            assert_eq!(finalization, expected, "status={status}");
            assert_ne!(
                finalization,
                BrowserRunFinalization::Complete(
                    crate::session_flow::BrowserAgentTerminal::Completed,
                ),
                "status={status} must never complete the browser run"
            );
        }
    }

    #[test]
    fn non_empty_trims_and_filters_blank() {
        assert_eq!(non_empty(Some("  run_123  ")), Some("run_123"));
        assert_eq!(non_empty(Some("   ")), None);
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn browser_session_writes_use_only_the_exact_session_audience_credential() {
        let request = authenticated_session_request(
            CreateThreadRequest::default(),
            &VerifiedSessionBearer::for_test("session-core-token"),
        )
        .expect("verified session bearer must be forwardable");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer session-core-token")
        );
        assert!(request
            .metadata()
            .get("x-execution-authorization")
            .is_none());
    }

    #[test]
    fn browser_broker_writes_use_only_the_exact_browser_audience_credential() {
        let request = authenticated_browser_request(
            ValidateGrantRequest {
                grant_id: "grant-1".to_owned(),
            },
            &VerifiedBrowserBearer::for_test("browser-broker-token"),
        )
        .expect("verified browser bearer must be forwardable");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer browser-broker-token")
        );
        assert!(request
            .metadata()
            .get("x-execution-authorization")
            .is_none());
    }

    #[test]
    fn broker_domain_policy_is_canonical_and_never_unrestricted() {
        assert_eq!(
            canonical_browser_domains(&[
                " EXAMPLE.com ".to_owned(),
                "api.example.com".to_owned(),
                "example.com".to_owned(),
            ])
            .unwrap(),
            vec!["api.example.com", "example.com"]
        );
        for invalid in [
            vec![],
            vec!["*.example.com".to_owned()],
            vec!["https://example.com".to_owned()],
            vec!["127.0.0.1".to_owned()],
        ] {
            assert!(canonical_browser_domains(&invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn arbitrary_start_url_is_rejected_by_broker_policy() {
        let policy = vec!["example.com".to_owned()];
        assert!(url_is_allowed_by_policy(
            "https://api.example.com/start",
            &policy
        ));
        assert!(!url_is_allowed_by_policy(
            "https://evil.example/start",
            &policy
        ));
        assert!(!url_is_allowed_by_policy("ftp://example.com/file", &policy));
    }

    #[tokio::test]
    async fn broker_validation_binds_authorization_and_rejects_forged_or_cross_tenant_grants() {
        let authorizations = Arc::new(Mutex::new(Vec::new()));
        let (state, handle) = state_with_browser_broker(TestBrowserBroker {
            validation: Err(tonic::Code::NotFound),
            authorizations: authorizations.clone(),
        })
        .await;
        let error = resolve_browser_grant(
            &state,
            &browser_request("forged-or-other-tenant"),
            &VerifiedBrowserBearer::for_test("browser-user-token"),
        )
        .await
        .expect_err("broker absence must fail closed");
        assert_eq!(error.0, StatusCode::FORBIDDEN);
        assert_eq!(
            authorizations
                .lock()
                .expect("authorization recorder")
                .as_slice(),
            ["Bearer browser-user-token"]
        );
        handle.abort();
    }

    #[tokio::test]
    async fn broker_validation_accepts_only_a_nonempty_server_owned_policy() {
        let authorizations = Arc::new(Mutex::new(Vec::new()));
        let (state, handle) = state_with_browser_broker(TestBrowserBroker {
            validation: Ok(ValidateGrantResponse {
                grant_id: "grant-valid".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            }),
            authorizations,
        })
        .await;
        let grant = resolve_browser_grant(
            &state,
            &browser_request("grant-valid"),
            &VerifiedBrowserBearer::for_test("browser-user-token"),
        )
        .await
        .expect("valid broker grant");
        assert_eq!(grant.grant_id, "grant-valid");
        assert_eq!(grant.allowed_domains, ["example.com"]);
        handle.abort();

        let (state, handle) = state_with_browser_broker(TestBrowserBroker {
            validation: Ok(ValidateGrantResponse {
                grant_id: "grant-empty-policy".to_owned(),
                active: true,
                ..Default::default()
            }),
            authorizations: Arc::new(Mutex::new(Vec::new())),
        })
        .await;
        let error = resolve_browser_grant(
            &state,
            &browser_request("grant-empty-policy"),
            &VerifiedBrowserBearer::for_test("browser-user-token"),
        )
        .await
        .expect_err("empty broker policy must never be unrestricted");
        assert_eq!(error.0, StatusCode::FORBIDDEN);
        handle.abort();
    }

    #[tokio::test]
    async fn issuer_zdr_blocks_all_browser_controls_before_execution_forwarding() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (state, handle) = state_with_execution_control_recorder(calls.clone()).await;

        for action in [
            BrowserRunControlAction::Pause,
            BrowserRunControlAction::Resume,
            BrowserRunControlAction::Stop,
        ] {
            let error = browser_run_control(
                State(state.clone()),
                Extension(browser_claims(true)),
                VerifiedExecutionBearer::for_test("must-not-forward"),
                VerifiedSessionBearer::for_test("must-not-forward"),
                Path("run-retained".to_owned()),
                Json(BrowserRunControlRequest { action }),
            )
            .await
            .expect_err("issuer-ZDR browser control must fail before execution forwarding");
            assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "ZDR browser controls must not reach Execution Core"
        );
        handle.abort();
    }

    #[test]
    fn issuer_zdr_cannot_be_downgraded_by_browser_run_body() {
        let claims = browser_claims(true);

        assert!(browser_run_zdr_blocked(&claims, false));
    }
}
