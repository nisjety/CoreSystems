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

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    CancelRunRequest, CreateThreadRequest, ExecuteStepRequest, PauseRunRequest, ResumeRunRequest,
    StartRunRequest,
};
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::{
    auth::{
        Claims, VerifiedDataPlaneBearer as VerifiedBearer, VerifiedExecutionBearer,
        VerifiedInferenceBearer, VerifiedSessionBearer,
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
    if req.grant_id.trim().is_empty() {
        return Err(bad_request("grant_id must not be empty"));
    }

    // Tenant/user identity is always server-derived from the verified bearer
    // claims — never trusted from the request body (the same IDOR-safety
    // rule the rest of model-gateway follows, e.g. `browser_suggest_action`).
    let org_id = claims.org_id.clone();
    let user_id = claims.user_id.clone();

    let mut session_client = state.session_client.clone();
    let thread_id = match non_empty(req.thread_id.as_deref()) {
        Some(thread_id) => thread_id.to_owned(),
        None => {
            session_client
                .create_thread(authenticated_session_request(
                    CreateThreadRequest {
                        session_key: new_ulid(),
                        org_id: org_id.clone(),
                        user_id: user_id.clone(),
                        metadata: None,
                    },
                    &session_bearer,
                )?)
                .await
                .map_err(|e| grpc_status_to_http(&e))?
                .into_inner()
                .thread_id
        }
    };

    let run = session_client
        .start_run(authenticated_session_request(
            StartRunRequest {
                thread_id: thread_id.clone(),
                parent_run_id: String::new(),
                agent_id: "browser-agent".to_owned(),
                goal: req.goal.clone(),
                mode: "execute".to_owned(),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
            },
            &session_bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let run_id = run.run_id;
    let plan_id = non_empty(req.plan_id.as_deref()).map_or_else(new_ulid, str::to_owned);

    // `run_id` is embedded in the tool_input JSON (not just the
    // ExecuteStepRequest envelope) because `tool_bridge::execute_browser_agent`
    // reads `PlanConfig.run_id` from here — it's what every `BrowserEventSink`
    // method and the new pause/cancel `StateStore` gate key off of. Without
    // it the loop runs "blind": no events, no pause/resume/stop.
    let tool_input = json!({
        "grant_id": req.grant_id,
        "plan_id": plan_id,
        "run_id": run_id,
        "org_id": org_id,
        "system_prompt": req.goal,
        "max_steps": req.max_steps,
        "max_runtime_s": req.max_runtime_s,
        "allowed_domains": req.allowed_domains,
        "stop_criteria": req.stop_criteria,
        "require_approval": req.require_approval,
        "max_cost_usd": req.max_cost_usd,
        "zdr": req.zdr,
        "profile_id": req.profile_id,
        "start_url": req.start_url,
    })
    .to_string();

    let mut execution_client = state.execution_client.clone();
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
        match execution_client.execute_step(execution_request).await {
            Ok(resp) => {
                let resp = resp.into_inner();
                if resp.status == "failed" || resp.status == "permission_denied" {
                    warn!(
                        run_id = %dispatch_run_id,
                        status = %resp.status,
                        error = %resp.error,
                        "browser-agent run finished with an error"
                    );
                } else {
                    info!(
                        run_id = %dispatch_run_id,
                        status = %resp.status,
                        "browser-agent run finished"
                    );
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
    fn issuer_zdr_cannot_be_downgraded_by_browser_run_body() {
        let claims = Claims {
            sub: "user-a".to_owned(),
            iss: "auth-core".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: true,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        };

        assert!(browser_run_zdr_blocked(&claims, false));
    }
}
