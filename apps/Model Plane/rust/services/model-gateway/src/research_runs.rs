//! The gateway's half of the research run store (Quarry v2 E4.6, slice Q4-062b): the durable, addressable
//! research phase of a chat turn, as a client of Session Core and three routes that address one run.
//!
//! Session Core owns the run: its id, status, stop reason, halt code, budget, counters and partial result
//! (`ResearchRunService`, migration 0115). This module is the gateway's client of that service, and it serves:
//!
//! - `GET /v1/research/runs/:run_id`: one research run the caller owns, as `{data: {...}}`.
//! - `GET /v1/threads/:thread_id/research-run`: the running research run of a thread the caller owns, or 404.
//! - `POST /v1/research/runs/:run_id/cancel`: asks the live chat stream of the run to stop (here, or on the replica
//!   that holds it when the run-control bus is on), and answers 202. That stream ends the run as cancelled.
//!
//! Every call carries the caller's own session bearer, and the org comes from the verified claims. The owner is
//! the verified caller, never a request field, so a run of another owner reads as not found. A zero-retention or
//! waived run keeps ids, status and budget counters only: the routes never return its `partial` or its
//! `reportArtifactId`, and nor do they for a caller who is not persistent.
//!
//! A thread holds at most one running research run. A begin refused for that is `Aborted`, with the code
//! `research_run_active` on its `x-mp-error-code` trailer. The running run's id is on `x-mp-active-run-id` for that
//! run's owner only, so another owner's refusal has no such trailer. [`research_run_active`] reads that refusal, and
//! [`active_run_conflict`] is its HTTP 409, with the id in `details.run_id`. The chat stream answers the same refusal
//! with an error frame.
//!
//! Built dark. Every route answers 404 `research_runs_disabled` unless `MODEL_GATEWAY_RESEARCH_RUNS` is the literal
//! `true`, read at each call. Session Core has its own switch (`RESEARCH_RUNS_ENABLED`), and while it is off the
//! routes answer the same 404.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    research_run_service_client::ResearchRunServiceClient, BeginRequest, FinishRequest,
    GetActiveRequest, GetRequest, ProgressRequest, ResearchBudget, ResearchRun,
};
use serde_json::{json, Map, Value};
use tonic::{transport::Channel, Code, Status};

use crate::agent_api_routes::{envelope, invalid_request, is_id};
use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{prost_struct_to_json, timestamp_to_rfc3339, HttpJsonError};
use crate::run_control::CancelRoute;
use crate::state::AppState;
use crate::zdr_posture::{self, Route, TurnPosture};

/// The gateway's switch for the research run routes. Only the literal `true` turns them on.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_RESEARCH_RUNS";

/// What Session Core answers while its own switch (`RESEARCH_RUNS_ENABLED`) is off.
const STORE_DISABLED: &str = "research_runs_disabled";

/// The code of a begin refused because the thread already has a running research run.
const ACTIVE_CODE: &str = "research_run_active";

/// The trailer that carries a refusal's code (Session Core's `ERROR_CODE_TRAILER`).
const ERROR_CODE_TRAILER: &str = "x-mp-error-code";

/// The trailer that names the research run already running on the thread (Session Core's `ACTIVE_RUN_TRAILER`).
const ACTIVE_RUN_TRAILER: &str = "x-mp-active-run-id";

/// Whether the research run routes are on in this process. Read at each call, as the other turn switches are.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The gateway's client of Session Core's `ResearchRunService`. Each call carries the caller's own session bearer,
/// so Session Core binds it to the caller's org and owner.
#[derive(Clone)]
pub(crate) struct ResearchRunsClient {
    client: ResearchRunServiceClient<Channel>,
}

impl ResearchRunsClient {
    pub(crate) fn new(client: ResearchRunServiceClient<Channel>) -> Self {
        Self { client }
    }

    /// Begins a research run on a thread for the bearer's owner. A thread that already has a running run refuses
    /// with `Aborted`; [`research_run_active`] reads that refusal.
    #[allow(dead_code)] // the chat research branch (Q4 E4.6 062b) is the first caller
    pub(crate) async fn begin(
        &self,
        bearer: &VerifiedSessionBearer,
        request: BeginRequest,
    ) -> Result<ResearchRun, Status> {
        let request = crate::sse::authenticated_session_request(request, bearer)?;
        let answer = self.client.clone().begin(request).await?.into_inner();
        answer.run.ok_or_else(|| {
            Status::internal("session-core began a research run without returning it")
        })
    }

    /// Reports progress on a run the bearer's owner holds. The answer says whether the report applied: false once
    /// the run has ended.
    #[allow(dead_code)] // the chat research branch (Q4 E4.6 062b) is the first caller
    pub(crate) async fn progress(
        &self,
        bearer: &VerifiedSessionBearer,
        request: ProgressRequest,
    ) -> Result<bool, Status> {
        let request = crate::sse::authenticated_session_request(request, bearer)?;
        let answer = self.client.clone().progress(request).await?.into_inner();
        Ok(answer.applied)
    }

    /// Ends a run the bearer's owner holds. The answer says whether the end applied: false when the run had already
    /// ended.
    #[allow(dead_code)] // the chat research branch (Q4 E4.6 062b) is the first caller
    pub(crate) async fn finish(
        &self,
        bearer: &VerifiedSessionBearer,
        request: FinishRequest,
    ) -> Result<bool, Status> {
        let request = crate::sse::authenticated_session_request(request, bearer)?;
        let answer = self.client.clone().finish(request).await?.into_inner();
        Ok(answer.applied)
    }

    /// Reads one run the bearer's owner holds. Another owner's run is not found.
    pub(crate) async fn get(
        &self,
        bearer: &VerifiedSessionBearer,
        id: &str,
    ) -> Result<ResearchRun, Status> {
        let request =
            crate::sse::authenticated_session_request(GetRequest { id: id.to_owned() }, bearer)?;
        self.client
            .clone()
            .get(request)
            .await?
            .into_inner()
            .run
            .ok_or_else(|| Status::not_found("research run not found"))
    }

    /// Reads the running run of a thread, when the bearer's owner holds one inside its deadline.
    pub(crate) async fn get_active(
        &self,
        bearer: &VerifiedSessionBearer,
        thread_id: &str,
    ) -> Result<Option<ResearchRun>, Status> {
        let request = crate::sse::authenticated_session_request(
            GetActiveRequest {
                thread_id: thread_id.to_owned(),
            },
            bearer,
        )?;
        let answer = self.client.clone().get_active(request).await?.into_inner();
        Ok(answer.found.then_some(answer.run).flatten())
    }
}

/// The running research run a refused begin names, when the refusal is `research_run_active`: an `Aborted` status
/// whose `x-mp-error-code` trailer is that code. The run's id is the `x-mp-active-run-id` trailer, which session-core
/// sends only to the run's owner: for another owner's run the id is empty. `None` for any other status.
#[allow(dead_code)] // the chat research branch (Q4 E4.6 062b) is the first caller
pub(crate) fn research_run_active(status: &Status) -> Option<String> {
    if status.code() != Code::Aborted {
        return None;
    }
    let metadata = status.metadata();
    let code = metadata
        .get(ERROR_CODE_TRAILER)
        .and_then(|value| value.to_str().ok());
    if code != Some(ACTIVE_CODE) {
        return None;
    }
    let run_id = metadata
        .get(ACTIVE_RUN_TRAILER)
        .and_then(|value| value.to_str().ok());
    Some(run_id.unwrap_or_default().to_owned())
}

/// The HTTP 409 of a begin refused for a running research run, naming that run in `details.run_id` (empty when the
/// refusal named none).
#[allow(dead_code)] // the chat research branch (Q4 E4.6 062b) is the first caller
pub(crate) fn active_run_conflict(run_id: &str) -> HttpJsonError {
    envelope(
        StatusCode::CONFLICT,
        ACTIVE_CODE,
        "this thread already has a research run in progress",
        json!({ "run_id": run_id }),
    )
}

/// The routes, merged into the authed router beside the thread goal routes.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/research/runs/:run_id", get(get_research_run))
        .route(
            "/v1/research/runs/:run_id/cancel",
            post(cancel_research_run),
        )
        .route(
            "/v1/threads/:thread_id/research-run",
            get(get_thread_research_run),
        )
}

/// The client for one request: a clone of the gateway's ResearchRunService channel.
fn runs_client(state: &AppState) -> ResearchRunsClient {
    ResearchRunsClient::new(state.research_run_client.clone())
}

/// The 404 every route answers while the switch is off, or while Session Core's own switch is.
fn disabled() -> HttpJsonError {
    envelope(
        StatusCode::NOT_FOUND,
        STORE_DISABLED,
        "research runs are not enabled on this deployment",
        json!({}),
    )
}

/// The switch first, so a deployment without the feature answers the same for every caller.
fn switch_gate() -> Result<(), HttpJsonError> {
    if enabled() {
        Ok(())
    } else {
        Err(disabled())
    }
}

/// The 404 of a run that does not exist, or that another owner holds.
fn run_not_found() -> HttpJsonError {
    envelope(
        StatusCode::NOT_FOUND,
        "research_run_not_found",
        "research run not found",
        json!({}),
    )
}

/// The 404 of a thread that has no running research run inside its deadline.
fn no_active_run() -> HttpJsonError {
    envelope(
        StatusCode::NOT_FOUND,
        "research_run_not_found",
        "no research run is running on this thread",
        json!({}),
    )
}

/// Session Core's refusals in the run routes' vocabulary. Not found and permission denied are one 404, so a run of
/// another owner reads like one that does not exist.
fn upstream_error(status: &Status) -> HttpJsonError {
    match status.code() {
        Code::Unavailable if status.message() == STORE_DISABLED => disabled(),
        Code::NotFound | Code::PermissionDenied => run_not_found(),
        Code::InvalidArgument => invalid_request("id"),
        Code::Unauthenticated => envelope(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
            json!({}),
        ),
        Code::Unavailable | Code::DeadlineExceeded | Code::ResourceExhausted => envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_unavailable",
            "a dependency is unavailable; try again",
            json!({}),
        ),
        _ => envelope(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "the gateway failed unexpectedly",
            json!({}),
        ),
    }
}

/// A run or thread id from a path, as Session Core writes them. Anything else is refused before any call.
fn path_id(raw: &str, field: &str) -> Result<String, HttpJsonError> {
    if is_id(raw) {
        Ok(raw.to_owned())
    } else {
        Err(invalid_request(field))
    }
}

/// Whether the caller may see what a run keeps. A run that keeps none (zero retention) never shows it, and a caller
/// who is not persistent (ZDR, or a waiver on a route it does not name) does not either. A posture that cannot be
/// decided shows nothing: this fails closed.
fn keeps_content(claims: &Claims, run: &ResearchRun) -> bool {
    !run.zdr
        && matches!(
            zdr_posture::decide_for(claims, &Route::plain(false)),
            Ok(TurnPosture::Persistent)
        )
}

/// Micro-dollars as the USD figure the routes return.
#[allow(clippy::cast_precision_loss)] // a spend in micro-dollars is far below 2^52
fn usd(micros: i64) -> f64 {
    micros as f64 / 1_000_000.0
}

/// Text that is empty as absent (`null`).
fn text_or_null(raw: &str) -> Value {
    if raw.is_empty() {
        Value::Null
    } else {
        Value::String(raw.to_owned())
    }
}

/// The budget a run was given. A dimension the run does not bound is left out, so a client reads it as absent.
fn budget_value(budget: Option<&ResearchBudget>) -> Value {
    let mut value = Map::new();
    if let Some(budget) = budget {
        if let Some(pages) = budget.max_pages {
            value.insert("maxPages".to_owned(), json!(pages));
        }
        if let Some(sub_queries) = budget.max_sub_queries {
            value.insert("maxSubQueries".to_owned(), json!(sub_queries));
        }
        if let Some(micros) = budget.usd_ceiling_micro {
            value.insert("usdCeiling".to_owned(), json!(usd(micros)));
        }
    }
    Value::Object(value)
}

/// The run as the routes return it. `content` is whether the caller may see what the run keeps, its partial result
/// and its report artifact id; without it those two keys are left out.
fn run_value(run: &ResearchRun, content: bool) -> Value {
    // Every write to a run is guarded on `running`, so the last change to an ended run is the moment it ended.
    let ended_at = if run.status == "running" {
        Value::Null
    } else {
        text_or_null(&timestamp_to_rfc3339(run.updated_at.clone()))
    };
    let mut value = json!({
        "id": run.id,
        "status": run.status,
        "stopReason": text_or_null(&run.stop_reason),
        "haltCode": text_or_null(&run.halt_code),
        "budget": budget_value(run.budget.as_ref()),
        "spentUsd": usd(run.spent_usd_micro),
        "pagesRead": run.pages_read,
        "sourcesFound": run.sources_found,
        "createdAt": text_or_null(&timestamp_to_rfc3339(run.created_at.clone())),
        "endedAt": ended_at,
    });
    if content {
        let partial = run.partial.as_ref();
        value["partial"] = partial.map_or(Value::Null, prost_struct_to_json);
        value["reportArtifactId"] = text_or_null(&run.report_artifact_id);
    }
    value
}

/// `GET /v1/research/runs/:run_id`: one research run the caller owns.
async fn get_research_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    let run_id = path_id(&run_id, "run_id")?;
    let run = runs_client(&state)
        .get(&bearer, &run_id)
        .await
        .map_err(|status| upstream_error(&status))?;
    let body = json!({ "data": run_value(&run, keeps_content(&claims, &run)) });
    Ok(Json(body))
}

/// `GET /v1/threads/:thread_id/research-run`: the running research run of a thread the caller owns. 404 when the
/// thread has none inside its deadline.
async fn get_thread_research_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    let thread_id = path_id(&thread_id, "thread_id")?;
    let active = runs_client(&state)
        .get_active(&bearer, &thread_id)
        .await
        .map_err(|status| upstream_error(&status))?
        .ok_or_else(no_active_run)?;
    let body = json!({ "data": run_value(&active, keeps_content(&claims, &active)) });
    Ok(Json(body))
}

/// `POST /v1/research/runs/:run_id/cancel`: asks the live chat stream of a research run the caller owns to stop. The
/// stream registers under the chat session run that the research run belongs to, so the stop is keyed by that run's
/// id. The run-control route resolves it: here through `cancel_run_for`, or on the replica that holds the stream when
/// the run-control bus is on. `stopRequested` is true when a live stream was asked; that stream then ends the run as
/// cancelled. A run that has already ended asks no stream: it is answered 202 with `stopRequested` false, and its
/// chat run is left alone. Answered 202 once the caller is known to own the run, and 503 when the bus could not tell.
async fn cancel_research_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(run_id): Path<String>,
) -> Result<Response, HttpJsonError> {
    switch_gate()?;
    let run_id = path_id(&run_id, "run_id")?;
    let run = runs_client(&state)
        .get(&bearer, &run_id)
        .await
        .map_err(|status| upstream_error(&status))?;
    // A research run that has ended has nothing left to stop, but its chat run may still be answering the user.
    // So the chat run is left alone: this route answers `NoOwner`, as it does for a run with no live stream.
    let route = if run.status == "running" {
        state
            .run_control
            .cancel_run(&state.cancels, &run.run_id, &claims.org_id, &claims.user_id)
            .await
    } else {
        CancelRoute::NoOwner
    };
    let stop_requested = match route {
        CancelRoute::Local | CancelRoute::Peer => true,
        CancelRoute::NoOwner => false,
        CancelRoute::Unavailable => {
            return Err(envelope(
                StatusCode::SERVICE_UNAVAILABLE,
                "run_control_unavailable",
                "the stop could not be confirmed; try again",
                json!({ "retryable": true }),
            ));
        }
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "data": { "id": run.id, "stopRequested": stop_requested } })),
    )
        .into_response())
}
