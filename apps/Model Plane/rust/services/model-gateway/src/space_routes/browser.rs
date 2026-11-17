//! Room browser run start, the model-gateway side of the Control decision (build plan WS20-S2, WS20-S3), and the
//! room reads of the same runs (WS20-S4, R2 to R4: the list, the timeline and one evidence image). The reads carry the
//! caller's Control `model.thread.read` decision in the query, as `artifacts.rs` does, and Session Core verifies it.
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

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use chrono::Utc;
use mp_contracts::model_plane::v1::{
    browser_broker_client::BrowserBrokerClient, space_browser_read_client::SpaceBrowserReadClient,
    AuthorizeRoomBrowserEvidenceRequest, CountActiveRoomBrowserRunsRequest, DeriveRunGrantRequest,
    DeriveRunGrantResponse, ListRoomBrowserRunsRequest, ReadRoomBrowserTimelineRequest,
    RoomBrowserEvent, RoomBrowserRun, RoomBrowserRunHeader,
};
use mp_contracts::space_browser::{
    capacity_admits, check_audience_revisions_agree, goal_digest, profile_request_admitted,
    room_grant_ttl_seconds, space_browser_run_api_enabled, verify_browser_run_decision,
    BrowserRunExpectation, BrowserRunRefusal, ControlDecisionKey, VerifiedBrowserRun,
    CONTROL_KEY_ID_ENV, CONTROL_PUBLIC_KEY_ENV, SPACE_BROWSER_RUN_API_ENV, THREAD_MODE_NEW,
};
use mp_contracts::zdr_refusal::Refusal;
use serde::Deserialize;
use serde_json::{json, Value};
use tonic::transport::Channel;
use tracing::warn;

use crate::auth::{Claims, VerifiedBrowserBearer, VerifiedSessionBearer};
use crate::browser_run::{authenticated_browser_request, keyed_room_run_key};
use crate::quarry::QuarryError;
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
    /// The thread the run runs in: the one the gateway created for a new-thread decision (D4), or the body's own
    /// thread. Never empty; the room run key takes no grant (W3-DEC-BROWSER).
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
    // The gateway names the run's thread on every room start: the one it created for a new-thread decision (D4), or the
    // body's own. A thread create decision is never a room run's, since the gateway creates the thread itself.
    if input.thread_id.is_empty() {
        return Err(invalid_run("the room run's thread is required"));
    }
    if input.space_context.is_some() {
        return Err(invalid_run("a room run takes no thread create decision"));
    }
    // A new-thread decision binds no thread id (D4), so its expectation is empty. The run's own thread is the one the
    // gateway created for it, whose goal is that thread's first post; an existing-thread decision binds the thread.
    let new_thread = input.authority.claims.get("thread_mode").and_then(Value::as_str) == Some(THREAD_MODE_NEW);
    let decision_thread = if new_thread { "" } else { input.thread_id };
    let goal_hash = goal_digest(input.goal);
    let claims_json = input.authority.claims.to_string();
    let expect = BrowserRunExpectation {
        org_id,
        subject_id: user_id,
        space_ref: &space_id,
        thread_id: decision_thread,
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
    // The run key the handler derives from the decision's idempotency key and the verified org and user only
    // (`keyed_room_run_key`), so a retry that presents a new grant still names the same run. The count leaves that run
    // out, so a keyed replay of an admitted run is not refused by its own run.
    let run_key = keyed_room_run_key(org_id, user_id, &input.authority.idempotency_key);
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

// ---------------------------------------------------------------------------------------------
// Room reads (WS20-S4, R2 to R4): the room's list, the run's timeline and one evidence image.
// ---------------------------------------------------------------------------------------------

/// How long a Session Core room read may take before the read answers 503 rather than wait on it.
const ROOM_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The room read page sizes: the list's default and most, and the timeline's.
const ROOM_LIST_DEFAULT_LIMIT: u32 = 20;
const ROOM_LIST_MAX_LIMIT: u32 = 50;
const ROOM_TIMELINE_DEFAULT_LIMIT: u32 = 50;
const ROOM_TIMELINE_MAX_LIMIT: u32 = 200;
/// The longest id a room read accepts, and the longest decision reference and token it carries.
const ROOM_MAX_ID_BYTES: usize = 128;
const ROOM_MAX_REF_BYTES: usize = 512;
const ROOM_MAX_TOKEN_BYTES: usize = 8 * 1024;
/// The largest evidence image a room read relays (WS20-S4, R4: 4 MB).
pub(crate) const ROOM_MAX_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;
/// The image types an evidence read relays. Any other artifact type is refused before it leaves the gateway.
const ROOM_EVIDENCE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

/// The room read routes, merged by `space_routes::router`. User principals only: `authorize_principal_route`
/// admits no service or API key here. The Control read decision is checked in Session Core, never in this file.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/:space_id/browser/runs", get(list_room_browser_runs))
        .route(
            "/v1/spaces/:space_id/browser/runs/:run_id/timeline",
            get(read_room_browser_timeline),
        )
        .route(
            "/v1/spaces/:space_id/browser/runs/:run_id/evidence/:evidence_id",
            get(read_room_browser_evidence),
        )
}

/// The Control read decision a room read carries in its query string, as `artifacts.rs` carries it. The pair is
/// never logged or echoed here.
#[derive(Debug, Default, Deserialize)]
struct RoomReadDecisionQuery {
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ListRoomRunsQuery {
    thread_id: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RoomTimelineQuery {
    after: Option<String>,
    limit: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

/// A room read id: non-empty, bounded, and free of control characters.
fn room_id_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= ROOM_MAX_ID_BYTES && !value.chars().any(char::is_control)
}

fn room_invalid() -> ApiError {
    typed_error(StatusCode::BAD_REQUEST, "invalid_request", "the browser run request is invalid")
}

/// The one 403 for a caller that may not read this Space's runs, whatever the reason.
fn room_not_authorized() -> ApiError {
    typed_error(
        StatusCode::FORBIDDEN,
        "space_read_not_authorized",
        "reading this Space's browser runs is not permitted",
    )
}

/// The typed `browser_run` refusal of a zero-retention caller or turn (the catalogue body, 412).
fn room_zdr_refused() -> ApiError {
    (
        StatusCode::PRECONDITION_FAILED,
        Json(Refusal::feature_not_supported("browser_run").json_body()),
    )
}

/// The 503 of a Session Core room read that failed, timed out or could not be reached.
fn room_unavailable() -> ApiError {
    typed_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "session_unavailable",
        "the Space's browser runs could not be read",
    )
}

/// The gate every room read passes before Session Core is asked, each refusal failing closed in this order: the
/// rollout switch (404 `feature_unavailable` while `SPACE_BROWSER_RUN_API` is off, the same switch as the start),
/// the Space reference's shape, a user principal, the zero-retention posture of the credential (412), and the
/// decision pair (403). Returns the decision pair, trimmed.
fn room_read_gate(
    state: &AppState,
    claims: &Claims,
    space_id: &str,
    decision: (Option<&str>, Option<&str>),
) -> Result<(String, String), ApiError> {
    if !state.space_browser_run.enabled() {
        return Err(typed_error(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "room browser runs are not enabled",
        ));
    }
    if !room_id_ok(space_id.trim()) {
        return Err(room_invalid());
    }
    if claims.user_id.trim().is_empty() {
        return Err(room_not_authorized());
    }
    if crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false))? {
        return Err(room_zdr_refused());
    }
    let reference = decision.0.map(str::trim).unwrap_or_default();
    let token = decision.1.map(str::trim).unwrap_or_default();
    if reference.is_empty()
        || reference.len() > ROOM_MAX_REF_BYTES
        || token.is_empty()
        || token.len() > ROOM_MAX_TOKEN_BYTES
    {
        return Err(room_not_authorized());
    }
    Ok((reference.to_owned(), token.to_owned()))
}

/// A page size from its query text: absent is the default, and a value outside `1..=most` is refused.
fn room_limit(raw: Option<&str>, default: u32, most: u32) -> Result<u32, ApiError> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(default),
        Some(value) => match value.parse::<u32>() {
            Ok(limit) if (1..=most).contains(&limit) => Ok(limit),
            _ => Err(typed_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "limit is out of range",
            )),
        },
    }
}

/// The HTTP form of a Session Core room-read refusal. A zero-retention refusal is the typed catalogue body; the
/// others are fixed words, never the upstream text. `not_found` is the code and message of a NotFound.
fn room_read_refusal(status: &tonic::Status, not_found: (&'static str, &'static str)) -> ApiError {
    warn!(code = ?status.code(), "room browser read refused");
    if Refusal::from_status(status).is_some() {
        return room_zdr_refused();
    }
    match status.code() {
        tonic::Code::Unauthenticated => {
            typed_error(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::PermissionDenied => room_not_authorized(),
        tonic::Code::NotFound => typed_error(StatusCode::NOT_FOUND, not_found.0, not_found.1),
        tonic::Code::InvalidArgument => room_invalid(),
        _ => room_unavailable(),
    }
}

/// A list row as the room sees it: ids, the starter's subject id, the phase, counts and hosts. The control, hand-off
/// and view fields are fixed in this slice: no Quarry lease (WS20-S6), no hand-off (WS20-S7) and no room view
/// (SD-23: the view stays with the starter) exist yet.
fn room_run_json(run: &RoomBrowserRun) -> Value {
    json!({
        "run_id": run.run_id,
        "thread_id": run.thread_id,
        "started_by": { "subject_id": run.started_by_subject_id },
        "phase": run.phase,
        "started_at_unix": run.started_at_unix,
        "finished_at_unix": unix_or_null(run.finished_at_unix),
        "hosts": run.hosts,
        "step_count": run.step_count,
        "control": { "mode": "agent" },
        "handoff": { "open": false },
        "view": { "scope": "owner" },
    })
}

fn room_header_json(run: &RoomBrowserRunHeader) -> Value {
    json!({
        "run_id": run.run_id,
        "thread_id": run.thread_id,
        "started_by": { "subject_id": run.started_by_subject_id },
        "phase": run.phase,
        "started_at_unix": run.started_at_unix,
        "finished_at_unix": unix_or_null(run.finished_at_unix),
        "control": { "mode": "agent" },
        "handoff": { "open": false },
        "view": { "scope": "owner" },
    })
}

fn unix_or_null(value: i64) -> Value {
    if value > 0 {
        json!(value)
    } else {
        Value::Null
    }
}

/// A timeline row as the room sees it: the event id, the time, the kind, and only the closed fields the projector
/// set. An empty field is left out.
fn room_event_json(event: &RoomBrowserEvent) -> Value {
    let mut out = json!({
        "event_id": event.event_id,
        "at_unix": event.at_unix,
        "kind": event.kind,
    });
    let optional = [
        ("step", &event.step),
        ("action_type", &event.action_type),
        ("host", &event.host),
        ("status", &event.status),
        ("risk_category", &event.risk_category),
        ("evidence_id", &event.evidence_id),
        ("approval_id", &event.approval_id),
        ("decision", &event.decision),
        ("handoff_kind", &event.handoff_kind),
        ("outcome", &event.outcome),
    ];
    for (key, value) in optional {
        if !value.is_empty() {
            out[key] = json!(value);
        }
    }
    out
}

/// `GET /v1/spaces/:space_id/browser/runs` (WS20-S4, R2): the Space's browser runs, newest first, with the Control
/// read decision in the query.
async fn list_room_browser_runs(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    Query(query): Query<ListRoomRunsQuery>,
) -> Response {
    let (reference, token) = match room_read_gate(
        &state,
        &claims,
        &space_id,
        (
            query.space_read_decision_ref.as_deref(),
            query.space_read_decision_token.as_deref(),
        ),
    ) {
        Ok(pair) => pair,
        Err(refusal) => return refusal.into_response(),
    };
    let limit = match room_limit(
        query.limit.as_deref(),
        ROOM_LIST_DEFAULT_LIMIT,
        ROOM_LIST_MAX_LIMIT,
    ) {
        Ok(limit) => limit,
        Err(refusal) => return refusal.into_response(),
    };
    let thread_id = query.thread_id.unwrap_or_default().trim().to_owned();
    let cursor = query.cursor.unwrap_or_default().trim().to_owned();
    if (!thread_id.is_empty() && !room_id_ok(&thread_id))
        || (!cursor.is_empty() && !room_id_ok(&cursor))
    {
        return room_invalid().into_response();
    }
    let request = match session_request(
        ListRoomBrowserRunsRequest {
            space_id: space_id.trim().to_owned(),
            thread_id,
            limit,
            before_run_id: cursor,
            space_read_decision_ref: reference,
            space_read_decision_token: token,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err(refusal) => return refusal.into_response(),
    };
    let mut client: SpaceBrowserReadClient<Channel> = state.space_browser_read_client.clone();
    let reply = match tokio::time::timeout(ROOM_READ_TIMEOUT, client.list_room_browser_runs(request))
        .await
    {
        Ok(Ok(reply)) => reply.into_inner(),
        Ok(Err(status)) => {
            return room_read_refusal(&status, ("run_not_found", "the browser runs are not available"))
                .into_response()
        }
        Err(_elapsed) => return room_unavailable().into_response(),
    };
    let runs: Vec<Value> = reply.runs.iter().map(room_run_json).collect();
    let next_cursor = if reply.next_run_id.is_empty() {
        Value::Null
    } else {
        json!(reply.next_run_id)
    };
    (
        StatusCode::OK,
        Json(json!({
            "runs": runs,
            "next_cursor": next_cursor,
            "valid_until": reply.valid_until_unix,
        })),
    )
        .into_response()
}

/// `GET /v1/spaces/:space_id/browser/runs/:run_id/timeline` (WS20-S4, R3): one page of the run's projected events.
async fn read_room_browser_timeline(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((space_id, run_id)): Path<(String, String)>,
    Query(query): Query<RoomTimelineQuery>,
) -> Response {
    let (reference, token) = match room_read_gate(
        &state,
        &claims,
        &space_id,
        (
            query.space_read_decision_ref.as_deref(),
            query.space_read_decision_token.as_deref(),
        ),
    ) {
        Ok(pair) => pair,
        Err(refusal) => return refusal.into_response(),
    };
    let run_id = run_id.trim().to_owned();
    if !room_id_ok(&run_id) {
        return room_invalid().into_response();
    }
    let limit = match room_limit(
        query.limit.as_deref(),
        ROOM_TIMELINE_DEFAULT_LIMIT,
        ROOM_TIMELINE_MAX_LIMIT,
    ) {
        Ok(limit) => limit,
        Err(refusal) => return refusal.into_response(),
    };
    let after = query.after.unwrap_or_default().trim().to_owned();
    if !after.is_empty() && !room_id_ok(&after) {
        return room_invalid().into_response();
    }
    let request = match session_request(
        ReadRoomBrowserTimelineRequest {
            space_id: space_id.trim().to_owned(),
            run_id,
            after_event_id: after,
            limit,
            space_read_decision_ref: reference,
            space_read_decision_token: token,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err(refusal) => return refusal.into_response(),
    };
    let mut client: SpaceBrowserReadClient<Channel> = state.space_browser_read_client.clone();
    let reply = match tokio::time::timeout(
        ROOM_READ_TIMEOUT,
        client.read_room_browser_timeline(request),
    )
    .await
    {
        Ok(Ok(reply)) => reply.into_inner(),
        Ok(Err(status)) => {
            return room_read_refusal(&status, ("run_not_found", "the browser run is not available"))
                .into_response()
        }
        Err(_elapsed) => return room_unavailable().into_response(),
    };
    let events: Vec<Value> = reply.events.iter().map(room_event_json).collect();
    let run = reply.run.as_ref().map_or(Value::Null, room_header_json);
    let next_event_id = if reply.next_event_id.is_empty() {
        Value::Null
    } else {
        json!(reply.next_event_id)
    };
    (
        StatusCode::OK,
        Json(json!({
            "run": run,
            "events": events,
            "next_event_id": next_event_id,
            "valid_until": reply.valid_until_unix,
        })),
    )
        .into_response()
}

/// `GET /v1/spaces/:space_id/browser/runs/:run_id/evidence/:evidence_id` (WS20-S4, R4): one evidence image. Session
/// Core checks that the id is one of this run's observations; model-gateway then fetches the bytes from Quarry with
/// its own `scrape:read` credential. The response is the image alone, sandboxed and never cached.
async fn read_room_browser_evidence(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((space_id, run_id, evidence_id)): Path<(String, String, String)>,
    Query(query): Query<RoomReadDecisionQuery>,
) -> Response {
    let (reference, token) = match room_read_gate(
        &state,
        &claims,
        &space_id,
        (
            query.space_read_decision_ref.as_deref(),
            query.space_read_decision_token.as_deref(),
        ),
    ) {
        Ok(pair) => pair,
        Err(refusal) => return refusal.into_response(),
    };
    let run_id = run_id.trim().to_owned();
    let evidence_id = evidence_id.trim().to_owned();
    if !room_id_ok(&run_id) || !room_id_ok(&evidence_id) {
        return room_invalid().into_response();
    }
    let request = match session_request(
        AuthorizeRoomBrowserEvidenceRequest {
            space_id: space_id.trim().to_owned(),
            run_id,
            evidence_id,
            space_read_decision_ref: reference,
            space_read_decision_token: token,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err(refusal) => return refusal.into_response(),
    };
    let mut client: SpaceBrowserReadClient<Channel> = state.space_browser_read_client.clone();
    let artifact_id = match tokio::time::timeout(
        ROOM_READ_TIMEOUT,
        client.authorize_room_browser_evidence(request),
    )
    .await
    {
        Ok(Ok(reply)) => reply.into_inner().artifact_id,
        Ok(Err(status)) => {
            return room_read_refusal(&status, ("evidence_not_found", "the evidence is not available"))
                .into_response()
        }
        Err(_elapsed) => return room_unavailable().into_response(),
    };
    let (content_type, bytes) = match state
        .quarry
        .artifact_bytes(&artifact_id, &claims.org_id, ROOM_MAX_EVIDENCE_BYTES)
        .await
    {
        Ok(found) => found,
        Err(QuarryError::Typed { status: 404 | 410, .. }) => {
            return typed_error(
                StatusCode::NOT_FOUND,
                "evidence_not_found",
                "the evidence is not available",
            )
            .into_response()
        }
        Err(QuarryError::Typed { status: 413, .. }) => {
            return typed_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "evidence_too_large",
                "the evidence is too large",
            )
            .into_response()
        }
        Err(error) => {
            warn!(error_class = %error.log_class(), "room browser evidence fetch failed");
            return typed_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "evidence_unavailable",
                "the evidence is unavailable",
            )
            .into_response();
        }
    };
    let media = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let Some(media_type) = ROOM_EVIDENCE_TYPES
        .iter()
        .copied()
        .find(|allowed| *allowed == media.as_str())
    else {
        return typed_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "evidence_unsupported",
            "this evidence cannot be shown",
        )
        .into_response();
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, media_type),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CONTENT_SECURITY_POLICY, "sandbox"),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::Json;

    /// WS20-S2: a room run takes no profile, no approval requirement and no zero-retention caller.
    #[test]
    fn a_room_run_refuses_a_profile_an_approval_or_a_zero_retention_caller() {
        assert!(check_profile_request(false, false, false).is_ok());
        for (profile, approval, zdr) in [(true, false, false), (false, true, false), (false, false, true)] {
            let (status, Json(body)) =
                check_profile_request(profile, approval, zdr).expect_err("refused");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{profile} {approval} {zdr}");
            assert!(body.to_string().contains("room_browser_profile_forbidden"), "{body}");
        }
    }

    /// WS20-S2: the room browser capacity admits below its limit and refuses at two active runs with 429.
    #[test]
    fn check_capacity_admits_below_the_limit_and_refuses_at_two() {
        assert!(check_capacity(0).is_ok());
        assert!(check_capacity(1).is_ok());
        let (status, Json(body)) = check_capacity(2).expect_err("at the limit");
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(body.to_string().contains("browser_capacity"), "{body}");
    }

    /// WS20-S4: a requested limit defaults when absent or blank, is admitted up to the most, and is refused at zero,
    /// beyond the most, or when it is not a number.
    #[test]
    fn room_limit_defaults_when_absent_and_refuses_zero_or_beyond_the_most() {
        assert_eq!(room_limit(None, 20, 100).expect("default"), 20);
        assert_eq!(room_limit(Some("  "), 20, 100).expect("blank reads as absent"), 20);
        assert_eq!(room_limit(Some("100"), 20, 100).expect("the most is admitted"), 100);
        for refused in ["0", "101", "abc"] {
            assert!(room_limit(Some(refused), 20, 100).is_err(), "{refused}");
        }
    }
}
