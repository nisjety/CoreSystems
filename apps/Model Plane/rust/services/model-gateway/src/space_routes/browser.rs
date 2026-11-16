//! Room browser run start, the model-gateway side of the Control decision (build plan WS20-S2, WS20-S3).
//!
//! `browser_run.rs` calls these in the start, after the zero-retention refusal and before any thread, run
//! or grant is written:
//!
//! 1. [`check_profile_request`]: a room run takes no profile, no approval hold and no ZDR flag.
//! 2. [`admit_room_run`]: the rollout switch (404 `feature_unavailable` while off), the Control decision and
//!    its claims (every check in `mp_contracts::space_browser`), the goal turn's audience agreement (409
//!    `space_authority_stale`), and the per-Space cap of two, counted by Session Core
//!    (`CountActiveRoomBrowserRuns`). A count that cannot be read is 503 and admits nothing.
//! 3. [`derive_room_run_grant`]: the narrowed, ephemeral run grant derived from the starter's session grant
//!    with the room's short lifetime. The grant names no Space (C1).
//! 4. [`append_room_result`]: the one durable result turn, written when the run reaches a terminal state.
//!
//! Errors are `{error:{code,message,details}}`. Every verification failure is one fixed 403 whatever the
//! reason, so a response never says which check failed. The rollout switch is a control, never an
//! authorization input. Nothing here holds state beyond the policy read once at start-up.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::{http::StatusCode, Json};
use chrono::Utc;
use mp_contracts::model_plane::v1::{
    browser_broker_client::BrowserBrokerClient,
    space_browser_read_client::SpaceBrowserReadClient, CountActiveRoomBrowserRunsRequest,
    DeriveRunGrantRequest, DeriveRunGrantResponse,
};
use mp_contracts::space_browser::{
    capacity_admits, check_audience_revisions_agree, goal_digest, profile_request_admitted,
    room_grant_ttl_seconds, space_browser_run_api_enabled, verify_browser_run_decision,
    BrowserRunExpectation, BrowserRunRefusal, ControlDecisionKey, VerifiedBrowserRun,
    CONTROL_KEY_ID_ENV, CONTROL_PUBLIC_KEY_ENV, SPACE_BROWSER_RUN_API_ENV,
};
use mp_contracts::zdr_refusal::Refusal;
use serde::Deserialize;
use serde_json::{json, Value};
use tonic::transport::Channel;
use tracing::warn;

use crate::auth::{VerifiedBrowserBearer, VerifiedSessionBearer};
use crate::browser_run::{authenticated_browser_request, keyed_run_key};
use crate::session_flow::{SessionRun, ThreadSpaceContext};
use crate::state::AppState;

/// The handler error `browser_run.rs` returns: a status and a JSON body.
pub(crate) type ApiError = (StatusCode, Json<Value>);

/// The `DeriveRunGrant` purpose of a room run: an ordinary AI run, not an attach.
const PURPOSE_AI_RUN: &str = "ai_run";
/// The grant kind a run grant reports.
const GRANT_KIND_RUN: &str = "run";
/// The budget claim source: the step and runtime bounds come from the verified Space decision.
const BUDGET_SOURCE_REQUEST: &str = "request";
/// How long the per-Space count may take before the start refuses (503) rather than wait on it.
const COUNT_TIMEOUT: Duration = Duration::from_secs(5);
/// The cost a room run asks when its request names none, and the most it may ask. The decision signs no
/// money cap, so the gateway bounds the cost here (WS20-S2 depth).
pub(crate) const ROOM_DEFAULT_MAX_COST_USD: f64 = 0.25;
pub(crate) const ROOM_MAX_COST_USD: f64 = 1.0;
/// The most characters of a room run's result turn (plain text, bounded).
const RESULT_MAX_CHARS: usize = 2_000;

/// The browser run verifier's configuration, read once at start-up. Off, it holds no key.
#[derive(Clone)]
pub struct SpaceBrowserRunPolicy {
    enabled: bool,
    control_key: Option<ControlDecisionKey>,
}

impl SpaceBrowserRunPolicy {
    /// Off: every start answers 404 `feature_unavailable`, and no key is read.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            enabled: false,
            control_key: None,
        }
    }

    /// The policy the raw values mean. With the switch on, the Control key pair is required. The
    /// error names the variable that is missing or invalid, never its value.
    pub fn from_values(
        api: Option<&str>,
        key_id: Option<&str>,
        public_key: Option<&str>,
    ) -> Result<Self, &'static str> {
        if !space_browser_run_api_enabled(api) {
            return Ok(Self::off());
        }
        let control_key = ControlDecisionKey::from_config(key_id, public_key)?;
        Ok(Self {
            enabled: true,
            control_key: Some(control_key),
        })
    }

    /// The process environment right now. Start-up only.
    pub fn from_env() -> Result<Self, &'static str> {
        Self::from_values(
            std::env::var(SPACE_BROWSER_RUN_API_ENV).ok().as_deref(),
            std::env::var(CONTROL_KEY_ID_ENV).ok().as_deref(),
            std::env::var(CONTROL_PUBLIC_KEY_ENV).ok().as_deref(),
        )
    }

    /// Whether room browser run starts are on in this process.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Start-up refusal: with `SPACE_BROWSER_RUN_API` on, the Control key pair must be configured and
/// valid. Off, nothing else is read and the gateway starts as before.
pub fn validate_startup() -> anyhow::Result<()> {
    SpaceBrowserRunPolicy::from_env().map(|_| ()).map_err(|variable| {
        anyhow::anyhow!(
            "{} is on but {} is missing or invalid",
            SPACE_BROWSER_RUN_API_ENV,
            variable
        )
    })
}

/// The typed error envelope every room browser run answers with.
pub(crate) fn typed_error(status: StatusCode, code: &str, message: &str) -> ApiError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
}

fn invalid_run(message: &str) -> ApiError {
    typed_error(StatusCode::BAD_REQUEST, "invalid_browser_run", message)
}

/// The start's Control browser-run authority, as the BFF forwards it: the decision token, the claims Control
/// returned beside it, and the idempotency key it was minted under. Read strictly. The gateway never mints
/// or caches any of it; the decision must verify against the configured key.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpaceBrowserAuthority {
    pub(crate) decision_token: String,
    pub(crate) claims: Value,
    pub(crate) idempotency_key: String,
}

/// The request fields of a room start that admission reads.
pub(crate) struct RoomStartInput<'a> {
    pub(crate) goal: &'a str,
    /// The starter's session grant the run derives from (the body's `grant_id`); it keys the run.
    pub(crate) grant_id: &'a str,
    /// The thread the run uses; empty when the run starts a new thread.
    pub(crate) thread_id: &'a str,
    pub(crate) authority: &'a SpaceBrowserAuthority,
    /// The thread decision (a new thread only).
    pub(crate) space_context: Option<ThreadSpaceContext>,
    /// The goal turn's append decision (always).
    pub(crate) append_context: Option<ThreadSpaceContext>,
}

/// A room run the start admitted: the Space, the start key the decision was minted under, the verified
/// claims, and the Space contexts the Session Core path takes.
#[derive(Debug, Clone)]
pub(crate) struct RoomRunStart {
    pub(crate) space_id: String,
    pub(crate) start_key: String,
    pub(crate) verified: VerifiedBrowserRun,
    pub(crate) space_context: Option<ThreadSpaceContext>,
    pub(crate) append_context: ThreadSpaceContext,
}

/// Refuses the profile fields a room run must not carry (400 `room_browser_profile_forbidden`).
pub(crate) fn check_profile_request(
    profile_id_set: bool,
    require_approval_set: bool,
    zdr: bool,
) -> Result<(), ApiError> {
    if profile_request_admitted(profile_id_set, require_approval_set, zdr) {
        Ok(())
    } else {
        Err(typed_error(
            StatusCode::BAD_REQUEST,
            "room_browser_profile_forbidden",
            "a room browser run takes no profile, approval hold or zero-retention flag",
        ))
    }
}

/// Admits a room run: the switch, the decision and its claims, the audience agreement, and the cap. Every
/// step fails closed. Nothing is written before it returns.
pub(crate) async fn admit_room_run(
    state: &AppState,
    session_bearer: &VerifiedSessionBearer,
    (org_id, user_id): (&str, &str),
    input: RoomStartInput<'_>,
) -> Result<RoomRunStart, ApiError> {
    if !state.space_browser_run.enabled() {
        return Err(typed_error(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "room browser runs are not enabled",
        ));
    }
    let append = input
        .append_context
        .ok_or_else(|| invalid_run("the goal's append decision is required"))?;
    let space_id = append.space_id.clone();
    if space_id.trim().is_empty() {
        return Err(invalid_run("the goal's append decision names no Space"));
    }
    match (input.thread_id.is_empty(), input.space_context.as_ref()) {
        (true, None) => return Err(invalid_run("the new thread's create decision is required")),
        (false, Some(_)) => {
            return Err(invalid_run("a thread create decision is only for a new thread"))
        }
        _ => {}
    }
    if input
        .space_context
        .as_ref()
        .is_some_and(|context| context.space_id != space_id)
    {
        return Err(invalid_run("the thread and goal decisions name different Spaces"));
    }
    let goal_hash = goal_digest(input.goal);
    let claims_json = input.authority.claims.to_string();
    let expect = BrowserRunExpectation {
        org_id,
        subject_id: user_id,
        space_ref: &space_id,
        thread_id: input.thread_id,
        goal_digest: &goal_hash,
        idempotency_key: &input.authority.idempotency_key,
        now: Utc::now(),
    };
    let verified = verify_start(
        &state.space_browser_run,
        &input.authority.decision_token,
        &claims_json,
        &expect,
    )?;
    check_audience_agreement(
        &verified,
        i64::try_from(append.recipient_audience_revision).unwrap_or(i64::MAX),
    )?;
    // The run key the handler derives from the same decision key, grant and org (`keyed_run_key`). The count
    // leaves that run out, so a keyed replay of an admitted run is not refused by its own run.
    let run_key = keyed_run_key(
        org_id,
        user_id,
        input.grant_id.trim(),
        &input.authority.idempotency_key,
    );
    let active = count_room_runs(state, org_id, &space_id, &run_key, session_bearer).await?;
    check_capacity(active)?;
    Ok(RoomRunStart {
        space_id,
        start_key: input.authority.idempotency_key.clone(),
        verified,
        space_context: input.space_context,
        append_context: append,
    })
}

/// Verifies the start's Control decision and its claims. The rollout switch, then the configured key, then
/// the decision. The refusal is ready to return.
fn verify_start(
    policy: &SpaceBrowserRunPolicy,
    decision_token: &str,
    claims_json: &str,
    expect: &BrowserRunExpectation<'_>,
) -> Result<VerifiedBrowserRun, ApiError> {
    if !policy.enabled() {
        return Err(typed_error(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "room browser runs are not enabled",
        ));
    }
    let Some(key) = policy.control_key.as_ref() else {
        return Err(typed_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_decision_unavailable",
            "the Space decision key is not configured",
        ));
    };
    verify_browser_run_decision(key, decision_token, claims_json, expect).map_err(refusal_for)
}

/// The goal turn's append decision and the browser decision must name one recipient audience revision
/// (409 `space_authority_stale`, so the caller re-mints once).
fn check_audience_agreement(
    verified: &VerifiedBrowserRun,
    append_audience_revision: i64,
) -> Result<(), ApiError> {
    check_audience_revisions_agree(verified.recipient_audience_revision, append_audience_revision)
        .map_err(refusal_for)
}

/// Refuses a start while the Space already has its two browser runs in flight (429 `browser_capacity`).
fn check_capacity(active_runs: u32) -> Result<(), ApiError> {
    if capacity_admits(active_runs) {
        Ok(())
    } else {
        Err(typed_error(
            StatusCode::TOO_MANY_REQUESTS,
            "browser_capacity",
            "this Space already has the maximum number of browser runs",
        ))
    }
}

/// The browser runs of the Space in flight, read from Session Core, not counting the run the start key
/// names. A count that cannot be read is 503: the start admits nothing.
async fn count_room_runs(
    state: &AppState,
    org_id: &str,
    space_id: &str,
    start_key: &str,
    bearer: &VerifiedSessionBearer,
) -> Result<u32, ApiError> {
    let mut client: SpaceBrowserReadClient<Channel> = state.space_browser_read_client.clone();
    let request = session_request(
        CountActiveRoomBrowserRunsRequest {
            org_id: org_id.to_owned(),
            space_id: space_id.to_owned(),
            exclude_start_key: start_key.to_owned(),
        },
        bearer,
    )?;
    match tokio::time::timeout(COUNT_TIMEOUT, client.count_active_room_browser_runs(request)).await
    {
        Ok(Ok(reply)) => Ok(reply.into_inner().active_runs),
        Ok(Err(status)) => {
            warn!(code = ?status.code(), "room browser run count refused");
            Err(count_refusal(&status))
        }
        Err(_elapsed) => {
            warn!("room browser run count timed out");
            Err(count_unavailable())
        }
    }
}

fn count_refusal(status: &tonic::Status) -> ApiError {
    if Refusal::from_status(status).is_some() {
        return (
            StatusCode::PRECONDITION_FAILED,
            Json(Refusal::feature_not_supported("browser_run").json_body()),
        );
    }
    count_unavailable()
}

fn count_unavailable() -> ApiError {
    typed_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "session_unavailable",
        "the Space's browser runs could not be counted",
    )
}

/// A Session Core request carrying the caller's verified user credential.
fn session_request<T>(
    message: T,
    bearer: &VerifiedSessionBearer,
) -> Result<tonic::Request<T>, ApiError> {
    let authorization = format!("Bearer {}", bearer.as_str())
        .parse()
        .map_err(|_| {
            typed_error(
                StatusCode::UNAUTHORIZED,
                "delegated_auth_unavailable",
                "the session credential is not forwardable",
            )
        })?;
    let mut request = tonic::Request::new(message);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// The `DeriveRunGrant` request of a room run: the decision's hosts, the purpose `ai_run`, the room's short
/// lifetime, and the step and runtime bounds the decision signs. It carries no Space field (C1) and no money
/// claim (the gateway bounds cost in `browser_run.rs`).
#[must_use]
fn room_run_derive_request(
    parent_grant_id: &str,
    run_key: &str,
    org_id: &str,
    verified: &VerifiedBrowserRun,
) -> DeriveRunGrantRequest {
    DeriveRunGrantRequest {
        parent_grant_id: parent_grant_id.to_owned(),
        session_key: run_key.to_owned(),
        org_id: org_id.to_owned(),
        allowed_domains: verified.claims.allowed_domains.clone(),
        purpose: PURPOSE_AI_RUN.to_owned(),
        ttl_seconds: i32::try_from(room_grant_ttl_seconds(verified.claims.max_runtime_s))
            .unwrap_or(i32::MAX),
        budget_micro_usd: 0,
        budget_max_steps: i32::try_from(verified.claims.max_steps).unwrap_or(i32::MAX),
        budget_max_runtime_s: i32::try_from(verified.claims.max_runtime_s).unwrap_or(i32::MAX),
        budget_source: BUDGET_SOURCE_REQUEST.to_owned(),
        budget_minted_at_unix: 0,
        resource_id: String::new(),
    }
}

/// Asks the broker for the run grant derived from the starter's session grant. The answer must be a run
/// grant under the same parent whose hosts are a subset of the decision's, or it is refused.
pub(crate) async fn derive_room_run_grant(
    broker: &mut BrowserBrokerClient<Channel>,
    bearer: &VerifiedBrowserBearer,
    parent_grant_id: &str,
    run_key: &str,
    org_id: &str,
    verified: &VerifiedBrowserRun,
) -> Result<DeriveRunGrantResponse, ApiError> {
    let request = authenticated_browser_request(
        room_run_derive_request(parent_grant_id, run_key, org_id, verified),
        bearer,
    )?;
    let derived = broker
        .derive_run_grant(request)
        .await
        .map_err(|status| broker_refusal(&status))?
        .into_inner();
    check_derived_grant(&derived, parent_grant_id, verified)?;
    Ok(derived)
}

fn check_derived_grant(
    derived: &DeriveRunGrantResponse,
    parent_grant_id: &str,
    verified: &VerifiedBrowserRun,
) -> Result<(), ApiError> {
    let grant_id = derived.grant_id.trim();
    let narrower = !derived.allowed_domains.is_empty()
        && derived
            .allowed_domains
            .iter()
            .all(|host| verified.claims.allowed_domains.contains(host));
    if grant_id.is_empty()
        || grant_id == parent_grant_id
        || derived.kind != GRANT_KIND_RUN
        || !narrower
    {
        return Err(typed_error(
            StatusCode::BAD_GATEWAY,
            "browser_grant_unavailable",
            "the browser grant for this run could not be issued",
        ));
    }
    Ok(())
}

/// Maps a broker refusal to the closed codes the browser run routes already use.
fn broker_refusal(status: &tonic::Status) -> ApiError {
    match status.code() {
        tonic::Code::PermissionDenied => typed_error(
            StatusCode::FORBIDDEN,
            "browser_grant_forbidden",
            "the browser grant is not allowed",
        ),
        tonic::Code::ResourceExhausted => typed_error(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_browser_grants",
            "too many browser grants are active",
        ),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => typed_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "browser_broker_unavailable",
            "the browser broker is unavailable",
        ),
        tonic::Code::AlreadyExists => typed_error(
            StatusCode::CONFLICT,
            "idempotency_key_conflict",
            "this run key already names a different browser grant",
        ),
        _ => typed_error(
            StatusCode::BAD_GATEWAY,
            "browser_grant_unavailable",
            "the browser grant for this run could not be issued",
        ),
    }
}

/// The room run's cost cap: the request's, bounded to the most a room run may ask.
#[must_use]
pub(crate) fn room_cost_cap(requested: Option<f64>) -> f64 {
    requested
        .unwrap_or(ROOM_DEFAULT_MAX_COST_USD)
        .min(ROOM_MAX_COST_USD)
        .max(0.0)
}

/// Every verification failure is one fixed 403, except the two that tell the caller to re-mint (409).
fn refusal_for(reason: BrowserRunRefusal) -> ApiError {
    warn!(reason = reason.label(), "room browser run refused");
    match reason {
        BrowserRunRefusal::Stale | BrowserRunRefusal::Window => typed_error(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "the Space authority changed; retry the request",
        ),
        BrowserRunRefusal::Malformed
        | BrowserRunRefusal::Untrusted
        | BrowserRunRefusal::Mismatch
        | BrowserRunRefusal::Policy => typed_error(
            StatusCode::FORBIDDEN,
            "space_browser_not_authorized",
            "Space browser run is not authorized",
        ),
    }
}

/// The durable result turn of a room run: plain text, bounded, marked `provenance: web` (it is page-derived),
/// with metadata that names the run and its outcome only: no URL, title, rationale or host.
pub(crate) fn result_turn(run_id: &str, outcome: &str, output: &str) -> (String, prost_types::Struct) {
    let text: String = output.trim().chars().take(RESULT_MAX_CHARS).collect();
    let content = if text.is_empty() {
        format!("The browser run ended: {outcome}.")
    } else {
        text
    };
    let mut run_fields = BTreeMap::new();
    run_fields.insert("v".to_owned(), number_value(1.0));
    run_fields.insert("run_id".to_owned(), text_value(run_id));
    run_fields.insert("outcome".to_owned(), text_value(outcome));
    let mut fields = BTreeMap::new();
    fields.insert("browser_run".to_owned(), struct_value(run_fields));
    fields.insert("provenance".to_owned(), text_value("web"));
    (content, prost_types::Struct { fields })
}

fn text_value(text: &str) -> prost_types::Value {
    prost_types::Value {
        kind: Some(prost_types::value::Kind::StringValue(text.to_owned())),
    }
}

fn number_value(number: f64) -> prost_types::Value {
    prost_types::Value {
        kind: Some(prost_types::value::Kind::NumberValue(number)),
    }
}

fn struct_value(fields: BTreeMap<String, prost_types::Value>) -> prost_types::Value {
    prost_types::Value {
        kind: Some(prost_types::value::Kind::StructValue(prost_types::Struct {
            fields,
        })),
    }
}

/// Appends the room run's one result turn to its thread, under the starter's verified session credential.
/// Called once the run reached `completed` or `failed`.
pub(crate) async fn append_room_result(
    state: &AppState,
    run: &SessionRun,
    bearer: &VerifiedSessionBearer,
    outcome: &str,
    output: &str,
) -> anyhow::Result<()> {
    let (content, metadata) = result_turn(&run.run_id, outcome, output);
    crate::session_flow::append_assistant_message_authenticated(
        state,
        &run.thread_id,
        &content,
        bearer,
        None,
        Some(metadata),
    )
    .await
}
