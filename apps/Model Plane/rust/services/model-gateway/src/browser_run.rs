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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    BrowserAgentCapabilitiesRequest, CancelRunRequest, DeriveRunGrantRequest, ExecuteStepRequest, PauseRunRequest, ResumeRunRequest,
    RevokeGrantRequest, ValidateGrantRequest,
};
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

async fn dispatch_browser_step(
    client: &mut mp_contracts::model_plane::v1::execution_core_client::ExecutionCoreClient<tonic::transport::Channel>,
    request: tonic::Request<ExecuteStepRequest>,
    restricted: bool,
) -> Result<tonic::Response<mp_contracts::model_plane::v1::ExecuteStepResponse>, tonic::Status> {
    if restricted {
        client.execute_restricted_browser_step(request).await
    } else {
        client.execute_step(request).await
    }
}

/// `/v1/browser/runs*` routes. Merged into the auth-gated router next to the
/// existing `/v1/browser/suggest-action`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/browser/runs/capabilities", get(browser_run_capabilities))
        .route("/v1/browser/runs", post(browser_run_start))
        .route(
            "/v1/browser/runs/:run_id/control",
            post(browser_run_control),
        )
}

/// An authenticated handshake with execution-core, not a claim about Quarry's
/// running version: Quarry must acknowledge each restricted attachment itself.
async fn verify_attached_policy_protocol(
    state: &AppState,
    org_id: &str,
    bearer: &VerifiedExecutionBearer,
) -> Result<(), ApiError> {
    let unavailable = || (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"browser_attach_policy_unavailable"})));
    let authorization = format!("Bearer {}", bearer.as_str()).parse().map_err(|_| unavailable())?;
    let mut request = tonic::Request::new(BrowserAgentCapabilitiesRequest { org_id: org_id.to_owned() });
    request.metadata_mut().insert("authorization", authorization);
    let reply = tokio::time::timeout(Duration::from_secs(3), state.execution_client.clone().browser_agent_capabilities(request))
        .await.map_err(|_| unavailable())?.map_err(|_| unavailable())?.into_inner();
    if reply.attached_policy_protocol < 1 { return Err(unavailable()); }
    Ok(())
}

async fn browser_run_capabilities(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedExecutionBearer,
) -> Result<Json<Value>, ApiError> {
    let enabled = attach_flag(std::env::var("MODEL_GATEWAY_BROWSER_ATTACH").ok().as_deref())
        && policy_ask_flag(std::env::var("MODEL_GATEWAY_BROWSER_POLICY_ASK").ok().as_deref())
        && attach_flag(std::env::var("MODEL_GATEWAY_BROWSER_ATTACH_POLICY_ENFORCED").ok().as_deref());
    if !enabled {
        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"browser_attach_policy_unavailable"}))));
    }
    let quotas_enabled = planner_quotas_flag(std::env::var("MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS").ok().as_deref());
    let quota = planner_quota(&state, &claims.org_id).await;
    verify_restricted_planner_quota(quotas_enabled, quota.flag)?;
    verify_attached_policy_protocol(&state, &claims.org_id, &bearer).await?;
    Ok(Json(json!({"protocol_version":1,"attached_policy_supported":true})))
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

/// The standalone browser-run path creates a personal run and does not verify a
/// Space decision. Refuse an explicit Space target before creating any grant or
/// run instead of silently ignoring it and reporting a personal run as shared.
fn reject_unbound_space(request: &BrowserRunStartRequest) -> Result<(), ApiError> {
    if request.space_id.is_some()
        || request.space_ref.is_some()
        || request.space_context.is_some()
        || request.space_browser_authority.is_some()
    {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(json!({ "error": "space_browser_run_unavailable" })),
        ));
    }
    Ok(())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Request header that makes `POST /v1/browser/runs` safe to retry.
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const IDEMPOTENCY_KEY_MAX_LEN: usize = 128;

/// The client's `Idempotency-Key`, or `None` when the header is absent. One
/// header, 1 to 128 characters of `A-Z a-z 0-9 - _ . :` (a UUID or ULID fits);
/// anything else is refused before any broker or Session Core call, so a key
/// can never carry text into a log line, a grant key or a thread key.
fn parse_idempotency_key(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_KEY_HEADER).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(bad_request("idempotency_key_invalid"));
    }
    let key = first
        .to_str()
        .map_err(|_| bad_request("idempotency_key_invalid"))?
        .trim();
    if key.is_empty()
        || key.len() > IDEMPOTENCY_KEY_MAX_LEN
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(bad_request("idempotency_key_invalid"));
    }
    Ok(Some(key.to_owned()))
}

/// The run key of a keyed start: one stable name for the managed run, its
/// thread and its derived run grant. It is scoped to the verified org and
/// user and to the session grant the run derives from, so the same client key
/// under another grant (or from another user) is a different run, and the raw
/// key never leaves this function. Parts are length-prefixed, so `("a","bc")`
/// and `("ab","c")` cannot collide.
fn keyed_run_key(org_id: &str, user_id: &str, parent_grant_id: &str, key: &str) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    for part in [
        "model-gateway/browser-run/v1",
        org_id,
        user_id,
        parent_grant_id,
        key,
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for byte in &digest[..16] {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("browser-idem-{hex}")
}

/// The plan id of a keyed start whose body names none, so a replay answers
/// with the plan the first attempt reported.
fn keyed_plan_id(run_key: &str) -> String {
    format!("plan-idem-{}", run_key.trim_start_matches("browser-idem-"))
}

/// The answer to a replayed keyed start. Session Core says the run exists;
/// before answering 200 the run must still be live, because a run that ended
/// or was cancelled before it was dispatched would otherwise read as a run
/// that is working. Liveness is a read (`GetRun`), never a heartbeat: a replay
/// that renewed the run's 15-minute terminalization lease would keep a run
/// whose dispatch task is gone (its process died after the managed start)
/// alive for as long as the client keeps retrying, instead of letting Session
/// Core terminalize it so a new key can start fresh. If liveness cannot be
/// checked the replay is refused rather than guessed.
///
/// Known limit: a run that is live but was never dispatched (the process died
/// between the managed start and the dispatch spawn) still reads as live until
/// its lease expires, because no durable dispatch marker exists yet.
async fn browser_run_replay(
    state: &AppState,
    run: &crate::session_flow::SessionRun,
    plan_id: &str,
    session_bearer: &VerifiedSessionBearer,
) -> Result<Json<Value>, ApiError> {
    match crate::session_flow::browser_agent_run_is_live(state, run, session_bearer).await {
        Ok(true) => Ok(Json(json!({
            "run_id": run.run_id,
            "thread_id": run.thread_id,
            "plan_id": plan_id,
            "idempotent_replay": true,
        }))),
        Ok(false) => Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "idempotency_key_run_ended",
                "run_id": run.run_id,
            })),
        )),
        Err(error) => {
            warn!(%error, run_id = %run.run_id, "idempotent browser run replay could not be verified");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "idempotency_replay_unverified" })),
            ))
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
struct BrowserRunStartRequest {
    /// Reserved until the Space-aware start path verifies a Control decision.
    /// Accept arbitrary non-null JSON only so every explicit target fails closed.
    #[serde(default, alias = "spaceId")]
    space_id: Option<Value>,
    #[serde(default, alias = "spaceRef")]
    space_ref: Option<Value>,
    #[serde(default, alias = "spaceContext")]
    space_context: Option<Value>,
    #[serde(
        default,
        alias = "spaceBrowserAuthority",
        alias = "space_authority",
        alias = "spaceAuthority"
    )]
    space_browser_authority: Option<Value>,
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
    // `Option`, not a bare `Vec`, because callers (the Verevon gateway) may
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
    /// Token budget for the LLM planner of this run (WS11.S1). Tighten-only: the run gets
    /// `min(this, the org's browser_agent_max_tokens_per_run)`; it can never raise a ceiling.
    /// Absent or non-positive asks for nothing.
    #[serde(default)]
    max_planner_tokens: Option<i64>,
    /// Accepted so a well-formed caller body never 4xxs, but never trusted:
    /// this endpoint is called by the Verevon gateway, which derives `zdr`
    /// server-side from the browser session's own persisted metadata and
    /// never forwards a client-supplied flag. Kept `false`-default here as
    /// the safe fallback for any other caller.
    #[serde(default)]
    zdr: bool,
    /// Plan mode (WS7.L): the run may read but not type, interact, upload or
    /// download. Expanded here into the four `disable` keys of the ask. Needs
    /// `MODEL_GATEWAY_BROWSER_POLICY_ASK=on`, else the start is refused.
    #[serde(default)]
    plan_mode: Option<bool>,
    /// A tighten-only ask (WS7.L), e.g. a Space restriction the caller
    /// resolved. Same flag. Unknown keys (`robots_max`, `proxy_max`) refuse the
    /// whole body.
    #[serde(default)]
    policy: Option<RunPolicyAsk>,
    /// Which product surface started the run (`chat`, `space`, `support`,
    /// `agent_run_console`, `scheduled_cron`, `routine`, `background_review`,
    /// `dreaming`, `mail_task`, `direct_service`). Only a label for the run
    /// trace (WS10.S4), never read for authorization; an unknown value is the
    /// default.
    #[serde(default)]
    run_source: Option<String>,
    /// Attach mode (WS8.G): the id of the caller's own browser session (the run
    /// the person is driving). The run does not start a Quarry run of its own; it
    /// steps on that session's agent tab under an `attach` grant derived here with
    /// the caller's bearer. Needs `MODEL_GATEWAY_BROWSER_ATTACH` on, else the start
    /// is refused 400 `browser_attach_unavailable`.
    #[serde(default)]
    attach_session_id: Option<String>,
    #[serde(default)]
    attach_tab: Option<AttachTabTarget>,
    /// What the agent leaves on its tab when an attached run completes (WS8.J):
    /// `none`, `deliverable` (the default) or `handoff`. Only meaningful with
    /// `attach_session_id`; named without one it is refused 400
    /// `tab_mark_requires_attach`, outside the set 400 `tab_mark_on_complete_invalid`.
    #[serde(default)]
    tab_mark_on_complete: Option<String>,
    /// A saved recipe the run replays (Q4-E4.2-3i): a person starts it, the Model
    /// never chooses one. Needs `MODEL_GATEWAY_BROWSER_RECIPE=true`, else the start
    /// is refused 400 `browser_recipe_unavailable`. The goal still labels the run
    /// but does not steer a recipe. Absent, the run is an ordinary browser run.
    #[serde(default)]
    recipe: Option<RecipeAsk>,
}

/// The saved recipe a start names (Q4-E4.2-3i), read strictly: an unknown key
/// fails the body. The person it belongs to is never read from here; the gateway
/// writes `for_user_id` from the verified claims.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeAsk {
    /// The saved procedure's identifier: `proc_` and identifier characters.
    id: String,
    /// The version to replay. Exclusive with `tag`.
    #[serde(default)]
    version: Option<u32>,
    /// The tag to replay, `^[a-z0-9._-]{1,32}$`. Exclusive with `version`.
    #[serde(default)]
    tag: Option<String>,
    /// The values a person supplies for the recipe's typed steps, by slot name.
    /// User data: never logged, never put in a run event.
    #[serde(default)]
    slots: BTreeMap<String, String>,
}

/// Slot values are user data, so a debug print names the slot count only.
impl std::fmt::Debug for RecipeAsk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecipeAsk")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("tag", &self.tag)
            .field("slots", &self.slots.len())
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct AttachTabTarget {
    tab_id: String,
    expected_epoch: u64,
    expected_origin: String,
}

/// The three keys a model-driven run may ask Quarry to tighten on. Same shape
/// as execution-core's `WireRequestAsk`; `robots_max` and `proxy_max` are
/// deliberately absent and, with any other key, fail the parse.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RunPolicyAsk {
    #[serde(default)]
    disable: Vec<String>,
    #[serde(default)]
    deny_domains: Vec<String>,
    #[serde(default)]
    allow_domains: Option<Vec<String>>,
}

/// `MODEL_GATEWAY_BROWSER_POLICY_ASK`: only `on` lets a start carry a policy
/// ask or plan mode. Off (the default) refuses such a start rather than run it
/// without the restriction, so an older Quarry can never drop it silently.
fn policy_ask_flag(value: Option<&str>) -> bool {
    budget_claim_flag(value)
}

/// `MODEL_GATEWAY_BROWSER_ATTACH` (WS8.G): only `1`, `true` or `on` lets a start
/// carry `attach_session_id`. Off (the default) refuses such a start 400
/// `browser_attach_unavailable` before any grant, thread or run write, so a stack
/// whose broker or Quarry has attach dark never half-runs one.
fn attach_flag(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        let value = value.trim();
        value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("on")
    })
}

/// Longest attach session id this route forwards (the broker bounds a resource
/// id at 256 bytes; session ids are far shorter).
const MAX_ATTACH_SESSION_ID_BYTES: usize = 128;

/// The attach session id of a start, checked. `Ok(None)` when the start does not
/// attach. With the flag off a start that names one is refused
/// `browser_attach_unavailable`; a blank, oversized or non-printable id is
/// refused `attach_session_id_invalid`.
fn attach_session_for(enabled: bool, req: &BrowserRunStartRequest) -> Result<Option<String>, ApiError> {
    let Some(raw) = req.attach_session_id.as_deref() else {
        if req.attach_tab.is_some() {
            return Err(bad_request("attach_tab_requires_session"));
        }
        return Ok(None);
    };
    if let Some(tab) = &req.attach_tab {
        let origin = reqwest::Url::parse(&tab.expected_origin)
            .map_err(|_| bad_request("attach_tab_origin_invalid"))?;
        if tab.tab_id.is_empty()
            || tab.tab_id.len() > MAX_ATTACH_SESSION_ID_BYTES
            || !tab.tab_id.bytes().all(|byte| byte.is_ascii_graphic())
            || req.start_url.is_some()
            || !matches!(origin.scheme(), "http" | "https")
            || origin.origin().ascii_serialization() != tab.expected_origin
            || !req.allowed_domains.as_deref().is_some_and(|hosts|
                matches!(hosts, [host] if Some(host.as_str()) == origin.host_str()))
        {
            return Err(bad_request("attach_tab_scope_invalid"));
        }
    }
    if !enabled {
        return Err(bad_request("browser_attach_unavailable"));
    }
    let id = raw.trim();
    let printable = id.bytes().all(|b| b.is_ascii_graphic());
    if id.is_empty() || id.len() > MAX_ATTACH_SESSION_ID_BYTES || !printable {
        return Err(bad_request("attach_session_id_invalid"));
    }
    Ok(Some(id.to_owned()))
}

/// Attached delegate steps currently inherit only the owner session policy. The
/// run's tighten-only ask is not yet forwarded to the attachment, so refusing it
/// here prevents a restricted run from silently executing without restrictions.
fn reject_attached_policy(attach_session: Option<&str>, ask: Option<&Value>, enforced: bool) -> Result<(), ApiError> {
    // Only enable after execution-core enforces Quarry's positive policy receipt.
    // During mixed-version rollouts, old execution-core could drop the ask.
    if attach_session.is_some() && ask.is_some() && !enforced {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(json!({ "error": "browser_attach_policy_unavailable" })),
        ));
    }
    Ok(())
}

/// The tab mark of a start (WS8.J), checked against the closed set. `Ok(None)`
/// when the start names none (execution-core then takes its default). Named
/// without an attach session it is refused: a leased run has no person's tab to
/// hand over, and a mark that silently did nothing would mislead the caller.
fn tab_mark_for(
    req: &BrowserRunStartRequest,
    attach_session: Option<&str>,
) -> Result<Option<&'static str>, ApiError> {
    let Some(raw) = req.tab_mark_on_complete.as_deref() else {
        return Ok(None);
    };
    let mark = match raw.trim().to_ascii_lowercase().as_str() {
        "none" => "none",
        "deliverable" => "deliverable",
        "handoff" => "handoff",
        _ => return Err(bad_request("tab_mark_on_complete_invalid")),
    };
    if attach_session.is_none() {
        return Err(bad_request("tab_mark_requires_attach"));
    }
    Ok(Some(mark))
}

/// Write the tab mark into the browser `tool_input` (WS8.J) when the start named
/// one; a start that names none carries the `tool_input` it always did.
fn insert_tab_mark(tool_input: &mut Value, mark: Option<&str>) {
    let (Some(mark), Some(fields)) = (mark, tool_input.as_object_mut()) else {
        return;
    };
    fields.insert("tab_mark_on_complete".to_owned(), json!(mark));
}

/// The four capabilities plan mode switches off (Quarry capability names).
const PLAN_MODE_DISABLED: [&str; 4] = [
    "browser.act.input",
    "browser.act.interact",
    "browser.act.upload",
    "browser.act.download",
];
/// Most entries per list of an ask.
const MAX_POLICY_ASK_ENTRIES: usize = 64;

fn policy_ask_invalid() -> ApiError {
    bad_request("browser_policy_ask_invalid")
}

fn is_ask_capability(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

fn is_ask_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// The ask this start sends to execution-core, as the `tool_input.policy`
/// object: plan mode's four disables unioned with the caller's ask, denied
/// hosts unioned, the allow list as given. Sorted and de-duplicated. `None`
/// when the start asks for nothing (no plan mode, no entries), whatever the
/// flag. With the flag off, a start that does ask is refused 400
/// `browser_policy_ask_unavailable` (fail closed); an ask with a bad shape is
/// refused 400 `browser_policy_ask_invalid`. Tighten-only by construction: the
/// output can only disable, deny or narrow.
fn policy_ask_for(enabled: bool, req: &BrowserRunStartRequest) -> Result<Option<Value>, ApiError> {
    let caller = req.policy.clone().unwrap_or_default();
    let plan_mode = req.plan_mode == Some(true);
    let allow = caller
        .allow_domains
        .as_ref()
        .map(|hosts| hosts.iter().map(|h| h.trim().to_ascii_lowercase()).collect::<Vec<_>>());
    if !plan_mode && caller.disable.is_empty() && caller.deny_domains.is_empty() && allow.is_none() {
        return Ok(None);
    }
    if !enabled {
        return Err(bad_request("browser_policy_ask_unavailable"));
    }
    let lists_fit = caller.disable.len() <= MAX_POLICY_ASK_ENTRIES
        && caller.deny_domains.len() <= MAX_POLICY_ASK_ENTRIES
        && allow.as_ref().map_or(true, |hosts| hosts.len() <= MAX_POLICY_ASK_ENTRIES);
    if !lists_fit {
        return Err(policy_ask_invalid());
    }
    let mut disable: Vec<String> = caller.disable.iter().map(|c| c.trim().to_owned()).collect();
    if plan_mode {
        disable.extend(PLAN_MODE_DISABLED.iter().map(|c| (*c).to_owned()));
    }
    let mut deny: Vec<String> = caller
        .deny_domains
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    disable.sort_unstable();
    disable.dedup();
    deny.sort_unstable();
    deny.dedup();
    let mut allow = allow;
    if let Some(hosts) = allow.as_mut() {
        hosts.sort_unstable();
        hosts.dedup();
    }
    let shapes_ok = disable.iter().all(|c| is_ask_capability(c))
        && deny.iter().all(|h| is_ask_host(h))
        && allow.as_ref().map_or(true, |hosts| hosts.iter().all(|h| is_ask_host(h)));
    if !shapes_ok {
        return Err(policy_ask_invalid());
    }
    let mut ask = serde_json::Map::new();
    if !disable.is_empty() {
        ask.insert("disable".to_owned(), json!(disable));
    }
    if !deny.is_empty() {
        ask.insert("deny_domains".to_owned(), json!(deny));
    }
    if let Some(hosts) = allow {
        ask.insert("allow_domains".to_owned(), json!(hosts));
    }
    Ok(Some(Value::Object(ask)))
}

/// Where a browser run came from, as a label for the run trace. Never
/// authority: Quarry's `origin_is_not_authz` gate keeps it out of admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunSource {
    /// A person typing in chat.
    Chat,
    /// A person in a Space.
    Space,
    /// A person in a support session.
    Support,
    /// A person in the agent-run console.
    AgentRunConsole,
    /// capability-core cron (`0028_space_scoped_cron`).
    ScheduledCron,
    /// A capability-core routine.
    Routine,
    /// Background review.
    BackgroundReview,
    /// Dreaming.
    Dreaming,
    /// A `mail_task`.
    MailTask,
    /// A service calling the route directly.
    DirectService,
}

/// The run-origin vocabulary of the run trace envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunOrigin {
    User,
    Schedule,
    Agent,
    Webhook,
    Api,
}

impl RunOrigin {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Schedule => "SCHEDULE",
            Self::Agent => "AGENT",
            Self::Webhook => "WEBHOOK",
            Self::Api => "API",
        }
    }
}

impl RunSource {
    /// Every source; the mapping test walks this list.
    const ALL: [Self; 10] = [
        Self::Chat,
        Self::Space,
        Self::Support,
        Self::AgentRunConsole,
        Self::ScheduledCron,
        Self::Routine,
        Self::BackgroundReview,
        Self::Dreaming,
        Self::MailTask,
        Self::DirectService,
    ];

    const fn wire(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Space => "space",
            Self::Support => "support",
            Self::AgentRunConsole => "agent_run_console",
            Self::ScheduledCron => "scheduled_cron",
            Self::Routine => "routine",
            Self::BackgroundReview => "background_review",
            Self::Dreaming => "dreaming",
            Self::MailTask => "mail_task",
            Self::DirectService => "direct_service",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|source| source.wire() == value)
    }

    /// People acting on a surface are `USER`; scheduled work is `SCHEDULE`;
    /// unattended agent work is `AGENT`; mail-triggered work is `WEBHOOK`; a
    /// service calling directly is `API`.
    const fn origin(self) -> RunOrigin {
        match self {
            Self::Chat | Self::Space | Self::Support | Self::AgentRunConsole => RunOrigin::User,
            Self::ScheduledCron | Self::Routine => RunOrigin::Schedule,
            Self::BackgroundReview | Self::Dreaming => RunOrigin::Agent,
            Self::MailTask => RunOrigin::Webhook,
            Self::DirectService => RunOrigin::Api,
        }
    }
}

/// The source of a start: the named one when it is known, else `Chat`. The
/// route requires a verified user credential, so the unlabelled case is a
/// person; a hostile or unknown label gets no say.
fn run_source_of(req: &BrowserRunStartRequest) -> RunSource {
    req.run_source
        .as_deref()
        .and_then(RunSource::from_wire)
        .unwrap_or(RunSource::Chat)
}

const REQUEST_ID_HEADER: &str = "x-request-id";

/// The request id for every Quarry call of this run: the inbound `x-request-id`
/// when it is 1 to 64 chars of `[A-Za-z0-9_-]`, else a fresh one. A hostile
/// value is replaced, never repaired or forwarded.
fn request_id_for(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .map_or_else(|| format!("req-{}", new_ulid()), str::to_owned)
}

/// Write the attach session into the browser `tool_input` (WS8.G): the
/// `attach_session_id` execution-core attaches to (`grant_id` is already the
/// derived `attach` grant). A saved browser profile is never loaded into someone
/// else's session, so `profile_id` is cleared. A start that does not attach
/// carries the `tool_input` it always did.
fn insert_attach_fields(tool_input: &mut Value, attach_session_id: Option<&str>) {
    let (Some(session), Some(fields)) = (attach_session_id, tool_input.as_object_mut()) else {
        return;
    };
    fields.insert("attach_session_id".to_owned(), json!(session));
    fields.insert("profile_id".to_owned(), Value::Null);
}

/// `MODEL_GATEWAY_BROWSER_RECIPE` (Q4-E4.2-3i): strict, only the literal `true`
/// lets a start name a saved recipe. Off (the default) refuses such a start 400
/// `browser_recipe_unavailable` before any grant, thread or run write.
fn recipe_flag(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Longest saved procedure id a start may name (execution-core's bound).
const MAX_RECIPE_ID_BYTES: usize = 64;
/// Longest recipe tag (capability-core's bound).
const MAX_RECIPE_TAG_BYTES: usize = 32;
/// Most slots one start may fill.
const MAX_RECIPE_SLOTS: usize = 32;
/// Longest slot name.
const MAX_RECIPE_SLOT_KEY_BYTES: usize = 40;
/// Longest slot value, in characters.
const MAX_RECIPE_SLOT_VALUE_CHARS: usize = 2000;

/// Whether `id` is a saved procedure id: `proc_` then identifier characters
/// (letters, digits, `_`, `-`), at most 64 bytes.
fn is_recipe_procedure_id(id: &str) -> bool {
    id.len() <= MAX_RECIPE_ID_BYTES
        && id.strip_prefix("proc_").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

/// Whether `tag` matches `^[a-z0-9._-]{1,32}$`.
fn is_recipe_ask_tag(tag: &str) -> bool {
    (1..=MAX_RECIPE_TAG_BYTES).contains(&tag.len())
        && tag.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// Whether `key` is a slot name, `^[a-z0-9_]{1,40}$`.
fn is_recipe_slot_key(key: &str) -> bool {
    (1..=MAX_RECIPE_SLOT_KEY_BYTES).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The recipe of a start, checked (Q4-E4.2-3i). `Ok(None)` when the start names
/// none. With the flag off a start that names one is refused
/// `browser_recipe_unavailable`. A recipe types and clicks, so an attached start
/// is refused `recipe_attach_unsupported` and a plan-mode start
/// `recipe_plan_mode_unsupported`. Every refusal comes before any write, and no
/// refusal echoes a slot value.
fn recipe_for(enabled: bool, req: &BrowserRunStartRequest) -> Result<Option<RecipeAsk>, ApiError> {
    let Some(recipe) = req.recipe.as_ref() else {
        return Ok(None);
    };
    if !enabled {
        return Err(bad_request("browser_recipe_unavailable"));
    }
    if req.attach_session_id.is_some() || req.attach_tab.is_some() {
        return Err(bad_request("recipe_attach_unsupported"));
    }
    if req.plan_mode == Some(true) {
        return Err(bad_request("recipe_plan_mode_unsupported"));
    }
    if !is_recipe_procedure_id(&recipe.id) {
        return Err(bad_request("recipe_id_invalid"));
    }
    if recipe.version.is_some() && recipe.tag.is_some() {
        return Err(bad_request("recipe_version_and_tag"));
    }
    if recipe.tag.as_deref().is_some_and(|tag| !is_recipe_ask_tag(tag)) {
        return Err(bad_request("recipe_tag_invalid"));
    }
    if recipe.slots.len() > MAX_RECIPE_SLOTS
        || recipe.slots.iter().any(|(key, value)| {
            !is_recipe_slot_key(key) || value.chars().count() > MAX_RECIPE_SLOT_VALUE_CHARS
        })
    {
        return Err(bad_request("recipe_slots_invalid"));
    }
    Ok(Some(recipe.clone()))
}

/// Write the saved recipe into the browser `tool_input` (Q4-E4.2-3i) as
/// execution-core's `tool_input.recipe`, with `for_user_id` from the VERIFIED
/// claims, never from the body. A start that names no recipe carries the
/// `tool_input` it always did. The `zdr` flag already in `tool_input` is left
/// unchanged.
fn insert_recipe_fields(tool_input: &mut Value, recipe: Option<&RecipeAsk>, user_id: &str) {
    let (Some(recipe), Some(fields)) = (recipe, tool_input.as_object_mut()) else {
        return;
    };
    let mut object = serde_json::Map::new();
    object.insert("id".to_owned(), json!(recipe.id));
    if let Some(version) = recipe.version {
        object.insert("version".to_owned(), json!(version));
    }
    if let Some(tag) = &recipe.tag {
        object.insert("tag".to_owned(), json!(tag));
    }
    object.insert("slots".to_owned(), json!(recipe.slots));
    object.insert("for_user_id".to_owned(), json!(user_id));
    fields.insert("recipe".to_owned(), Value::Object(object));
}

/// Write the policy ask, trace context and request id into the browser
/// `tool_input`. Keys the run has no value for are not written, so a start with
/// none of them carries the `tool_input` it always did, except the request id
/// and trace, which every start has.
fn insert_policy_and_trace_fields(
    tool_input: &mut Value,
    policy: Option<Value>,
    source: RunSource,
    request_id: &str,
) {
    let Some(fields) = tool_input.as_object_mut() else {
        return;
    };
    if let Some(policy) = policy {
        fields.insert("policy".to_owned(), policy);
    }
    fields.insert("request_id".to_owned(), json!(request_id));
    fields.insert(
        "trace".to_owned(),
        json!({
            "origin": source.origin().as_str(),
            "agent": { "id": "browser-agent", "role": "browser" },
        }),
    );
}

/// How long a presented run grant stays claimed by the run it started when the
/// run's end is never observed (an approval pause, a dispatch that failed). The
/// claim is released earlier when the run ends or its start fails.
const PRESENTED_GRANT_CLAIM_TTL: Duration = Duration::from_secs(2 * 60 * 60);
/// How long the request fingerprint of a keyed start is remembered.
const START_FINGERPRINT_TTL: Duration = Duration::from_secs(60 * 60);
/// Upper bound on remembered entries of each kind; past it a new entry is not
/// remembered (the start then behaves as it did before the guard).
const START_GUARD_MAX_ENTRIES: usize = 10_000;

/// What a keyed start's remembered fingerprint says about a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FingerprintCheck {
    /// Nothing remembered for this key (first request, expiry, or a restart).
    Unknown,
    Match,
    Mismatch,
}

/// How a claim on a presented run grant came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrantClaim {
    /// No run held the grant; this run holds it now.
    Fresh,
    /// The same run (a keyed retry) already holds it.
    Held,
    /// Another run holds it.
    Refused,
}

/// Process-local bookkeeping for `POST /v1/browser/runs` (WS1.B depth, D1-001
/// and D1-019 limit c). The broker's run-grant record carries no session key and
/// Session Core compares only the goal of a keyed start, so neither can tell
/// that a presented run grant already backs a live run, or that a keyed retry
/// changed the run's limits. This guard answers both inside one gateway
/// process; it is not durable and not shared between replicas. The durable
/// half of the grant claim is `run_grant_is_bound_to_another_run` (the broker
/// reports the run key a run grant was derived for); the durable half of the
/// fingerprint (Session Core storing the start fingerprint) is recorded in the
/// depth register.
#[derive(Debug, Default)]
pub(crate) struct RunStartGuard {
    inner: Mutex<StartGuardInner>,
}

#[derive(Debug, Default)]
struct StartGuardInner {
    /// presented run grant id -> (run key of the run that holds it, claimed at)
    grants: HashMap<String, (String, Instant)>,
    /// run key -> (request fingerprint, remembered at)
    fingerprints: HashMap<String, (String, Instant)>,
    /// Local proof of this gateway's outstanding browser dispatch, not durable
    /// authority: absence after a restart or on another replica fails closed.
    dispatches: HashMap<String, (BrowserDispatchState, Instant)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserDispatchState {
    Active,
    StopRequested,
    Parked,
    CancellingParked,
}

impl RunStartGuard {
    fn claim_grant(&self, grant_id: &str, run_key: &str) -> GrantClaim {
        self.claim_grant_at(grant_id, run_key, Instant::now())
    }

    fn claim_grant_at(&self, grant_id: &str, run_key: &str, now: Instant) -> GrantClaim {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .grants
            .retain(|_, (_, at)| now.saturating_duration_since(*at) < PRESENTED_GRANT_CLAIM_TTL);
        match inner.grants.get(grant_id) {
            Some((holder, _)) if holder == run_key => GrantClaim::Held,
            Some(_) => GrantClaim::Refused,
            None => {
                if inner.grants.len() >= START_GUARD_MAX_ENTRIES {
                    // Full: fail open to the behaviour before this guard rather
                    // than refuse every presented run grant.
                    return GrantClaim::Fresh;
                }
                inner
                    .grants
                    .insert(grant_id.to_owned(), (run_key.to_owned(), now));
                GrantClaim::Fresh
            }
        }
    }

    fn release_grant(&self, grant_id: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.grants.remove(grant_id);
    }

    fn begin_dispatch(&self, run_id: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        inner.dispatches.retain(|_, (_, at)| now.saturating_duration_since(*at) < START_FINGERPRINT_TTL);
        if inner.dispatches.len() < START_GUARD_MAX_ENTRIES {
            inner.dispatches.insert(run_id.to_owned(), (BrowserDispatchState::Active, now));
        }
    }

    /// Only a locally confirmed parked response can authorize a durable stop.
    /// An in-flight dispatch records the intent but stays live until it reports
    /// either cancellation or an actual parked result.
    fn request_dispatch_stop(&self, run_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.dispatches.get_mut(run_id) {
            Some((state @ BrowserDispatchState::Active, _)) => { *state = BrowserDispatchState::StopRequested; false }
            Some((state @ BrowserDispatchState::Parked, _)) => { *state = BrowserDispatchState::CancellingParked; true }
            _ => false,
        }
    }

    /// Called only after ExecuteStep returned awaiting_approval, never while an
    /// action may still be in flight. A racing stop is reconciled here.
    fn dispatch_parked(&self, run_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.dispatches.get_mut(run_id) {
            Some((state @ BrowserDispatchState::Active, _)) => { *state = BrowserDispatchState::Parked; false }
            Some((state @ BrowserDispatchState::StopRequested, _)) => { *state = BrowserDispatchState::CancellingParked; true }
            _ => false,
        }
    }

    /// A resume can invalidate a previously observed parked result. Fence the
    /// local state before forwarding it; do not race an in-flight cancellation.
    fn prepare_dispatch_resume(&self, run_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.dispatches.get_mut(run_id) {
            Some((state @ BrowserDispatchState::Parked, _)) => { *state = BrowserDispatchState::Active; true }
            Some((BrowserDispatchState::StopRequested | BrowserDispatchState::CancellingParked, _)) => false,
            _ => true,
        }
    }

    fn restore_parked(&self, run_id: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((state @ BrowserDispatchState::CancellingParked, _)) = inner.dispatches.get_mut(run_id) {
            *state = BrowserDispatchState::Parked;
        }
    }

    fn forget_dispatch(&self, run_id: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.dispatches.remove(run_id);
    }

    fn check_fingerprint(&self, run_key: &str, fingerprint: &str) -> FingerprintCheck {
        self.check_fingerprint_at(run_key, fingerprint, Instant::now())
    }

    fn check_fingerprint_at(&self, run_key: &str, fingerprint: &str, now: Instant) -> FingerprintCheck {
        let inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.fingerprints.get(run_key) {
            Some((_, at)) if now.saturating_duration_since(*at) >= START_FINGERPRINT_TTL => {
                FingerprintCheck::Unknown
            }
            Some((known, _)) if known == fingerprint => FingerprintCheck::Match,
            Some(_) => FingerprintCheck::Mismatch,
            None => FingerprintCheck::Unknown,
        }
    }

    fn remember_fingerprint(&self, run_key: &str, fingerprint: &str) {
        self.remember_fingerprint_at(run_key, fingerprint, Instant::now());
    }

    fn remember_fingerprint_at(&self, run_key: &str, fingerprint: &str, now: Instant) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .fingerprints
            .retain(|_, (_, at)| now.saturating_duration_since(*at) < START_FINGERPRINT_TTL);
        if inner.fingerprints.len() >= START_GUARD_MAX_ENTRIES
            && !inner.fingerprints.contains_key(run_key)
        {
            return;
        }
        inner
            .fingerprints
            .entry(run_key.to_owned())
            .or_insert_with(|| (fingerprint.to_owned(), now));
    }
}

/// A digest of what in a start request shapes the run it makes: the goal, the
/// step limit, the stop criteria, the start page, the approval and profile
/// settings and the requested host policy. A keyed retry that changes any of
/// them is a different request under the same key. Left out on purpose:
/// `thread_id` (a keyed start refuses one), the grant id (the run key already
/// names it), `plan_id`, and the cost and runtime limits: those are bounded by
/// the grant's budget claim, which may move between a request and its retry, and
/// a retry that only differs there still answers the same run (pinned by
/// `a_keyed_retry_whose_claim_moved_still_answers_the_same_run`).
fn run_request_fingerprint(req: &BrowserRunStartRequest) -> String {
    use std::fmt::Write as _;
    let mut domains: Vec<&str> = req
        .allowed_domains
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();
    domains.sort_unstable();
    let mut hasher = Sha256::new();
    let mut part = |label: &str, value: &str| {
        for piece in [label, value] {
            hasher.update((piece.len() as u64).to_be_bytes());
            hasher.update(piece.as_bytes());
        }
    };
    part("v", "model-gateway/browser-run-fp/v1");
    part("goal", &req.goal);
    part("max_steps", &format!("{:?}", req.max_steps));
    if req.max_planner_tokens.is_some() {
        // Only when named, so a request that never names it keeps the fingerprint it had.
        part("max_planner_tokens", &format!("{:?}", req.max_planner_tokens));
    }
    part("stop_criteria", req.stop_criteria.as_deref().unwrap_or(""));
    part("start_url", req.start_url.as_deref().unwrap_or(""));
    part("require_approval", &format!("{:?}", req.require_approval));
    part("profile_id", req.profile_id.as_deref().unwrap_or(""));
    part("allowed_domains", &domains.join(","));
    if let Some(session) = non_empty(req.attach_session_id.as_deref()) {
        // Only when attaching: a retry that names another session is another request.
        part("attach_session_id", session);
    }
    if let Some(tab) = &req.attach_tab {
        part("attach_tab_id", &tab.tab_id);
        part("attach_tab_epoch", &tab.expected_epoch.to_string());
        part("attach_tab_origin", &tab.expected_origin);
    }
    if let Some(mark) = non_empty(req.tab_mark_on_complete.as_deref()) {
        // Only when named: a retry that asks for another mark is another request.
        part("tab_mark_on_complete", &mark.to_ascii_lowercase());
    }
    if req.plan_mode == Some(true) {
        // Only when asked, so a request that never names it keeps the fingerprint it had.
        part("plan_mode", "true");
    }
    if let Some(policy) = req.policy.as_ref().filter(|policy| **policy != RunPolicyAsk::default()) {
        part("policy", &format!("{policy:?}"));
    }
    if let Some(recipe) = &req.recipe {
        // Only when named, so a request without one keeps the fingerprint it had.
        // Slot values enter the hash only; the digest never reveals them.
        part("recipe_id", &recipe.id);
        part("recipe_version", &format!("{:?}", recipe.version));
        part("recipe_tag", recipe.tag.as_deref().unwrap_or(""));
        for (key, value) in &recipe.slots {
            part("recipe_slot", key);
            part("recipe_slot_value", value);
        }
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for byte in &digest[..16] {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Broker-issued browser authority resolved before the gateway creates a
/// thread/run. This is deliberately distinct from the caller's JSON request:
/// only the broker can set the grant id and domain policy that reaches
/// execution-core.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedBrowserGrant {
    grant_id: String,
    allowed_domains: Vec<String>,
    /// The session grant a run grant was derived from (WS1.B); `None` when
    /// the caller already presented a run grant.
    parent_grant_id: Option<String>,
    /// The bound the run executes under (WS6.E), when
    /// `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM` is on. For a run grant derived
    /// here it is the claim the broker stored, so it includes any bound
    /// inherited from the parent session grant. For a run grant the caller
    /// presented it is the tighter of the claim that grant carries and what
    /// this request mints (the organization's per-run ceiling and the request's
    /// own values), because a presented grant is never re-minted and a member
    /// can derive one with no claim at all.
    budget: Option<BudgetClaim>,
}

/// A budget claim: an upper bound on a run's spend. Zero is "no bound" on that
/// dimension. It can only tighten what the request asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BudgetClaim {
    micro_usd: i64,
    max_steps: i32,
    max_runtime_s: i32,
}

impl BudgetClaim {
    /// The claim a broker message carries, or `None` when it bounds nothing
    /// (an old broker, or a grant minted without one). A negative number is
    /// not a bound.
    fn from_wire(micro_usd: i64, max_steps: i32, max_runtime_s: i32) -> Option<Self> {
        let claim = Self {
            micro_usd: micro_usd.max(0),
            max_steps: max_steps.max(0),
            max_runtime_s: max_runtime_s.max(0),
        };
        (claim.micro_usd > 0 || claim.max_steps > 0 || claim.max_runtime_s > 0).then_some(claim)
    }

    /// The tighter of two claims per dimension (0 is open, so the other side
    /// stands).
    fn tighter(self, other: Self) -> Self {
        fn bound<T: Ord + Copy + Default>(a: T, b: T) -> T {
            let open = T::default();
            if a == open {
                b
            } else if b == open {
                a
            } else {
                a.min(b)
            }
        }
        Self {
            micro_usd: bound(self.micro_usd, other.micro_usd),
            max_steps: bound(self.max_steps, other.max_steps),
            max_runtime_s: bound(self.max_runtime_s, other.max_runtime_s),
        }
    }

    /// Whether `self` is no wider than `ceiling` in every dimension the
    /// ceiling bounds (a dimension the ceiling bounds must be bounded here too).
    fn within(self, ceiling: Self) -> bool {
        fn ok<T: Ord + Copy + Default>(own: T, ceiling: T) -> bool {
            let open = T::default();
            ceiling == open || (own != open && own <= ceiling)
        }
        ok(self.micro_usd, ceiling.micro_usd)
            && ok(self.max_steps, ceiling.max_steps)
            && ok(self.max_runtime_s, ceiling.max_runtime_s)
    }
}

/// The labels this route puts on a claim it mints (the broker accepts
/// `request`, `org_default` and `entitlement`; the last is the WS6.D resolver's).
const BUDGET_SOURCE_REQUEST: &str = "request";
const BUDGET_SOURCE_ORG_DEFAULT: &str = "org_default";

/// What this route asks the broker to put on the run grant (WS6.E), and
/// whether broker-returned claims are in force at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BudgetMint {
    /// `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM=on`. Off: no claim is sent, none
    /// is read back, and the run parameters are exactly the request's.
    enabled: bool,
    /// The claim to mint, when the request or the organization bounds
    /// anything, and who set it.
    minted: Option<(BudgetClaim, &'static str)>,
}

/// `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM`: only `on` turns it on, so a misspelt
/// value is off, never a silent half-on.
fn budget_claim_flag(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.trim().eq_ignore_ascii_case("on"))
}

/// The claim for a run: `min(request, org per-run ceiling)` for cost, the
/// request's own steps and runtime. The label says which side bound the cost
/// (`org_default` when the organization's ceiling did, else `request`).
/// `None` when neither bounds anything. A non-finite or non-positive request
/// value is no bound, as everywhere else in this route.
fn mint_budget_from(
    req: &BrowserRunStartRequest,
    org_ceiling_usd: Option<f64>,
) -> Option<(BudgetClaim, &'static str)> {
    let usable = |usd: f64| usd.is_finite() && usd > 0.0;
    let asked = req.max_cost_usd.filter(|usd| usable(*usd));
    let ceiling = org_ceiling_usd.filter(|usd| usable(*usd));
    let (cost_usd, source) = match (asked, ceiling) {
        (Some(asked), Some(ceiling)) if ceiling < asked => {
            (Some(ceiling), BUDGET_SOURCE_ORG_DEFAULT)
        }
        (Some(asked), _) => (Some(asked), BUDGET_SOURCE_REQUEST),
        (None, Some(ceiling)) => (Some(ceiling), BUDGET_SOURCE_ORG_DEFAULT),
        (None, None) => (None, BUDGET_SOURCE_REQUEST),
    };
    let claim = BudgetClaim::from_wire(
        // At least one micro-dollar: a positive ceiling that rounds to zero is
        // still a bound.
        cost_usd.map_or(0, |usd| crate::org_quota::usd_to_micros(usd).max(1)),
        req.max_steps.unwrap_or(0),
        req.max_runtime_s.unwrap_or(0),
    )?;
    Some((claim, source))
}

/// The run's mint: reads the organization's per-run ceiling from org-core (an
/// unreachable or unconfigured org-core is fail-open, see
/// `org_quota::fetch_org_limits`: Quarry's own ceiling still applies) and
/// decides what to ask the broker for. Off by default.
async fn mint_budget(state: &AppState, req: &BrowserRunStartRequest, org_id: &str) -> BudgetMint {
    let enabled = state.browser_budget_claim.unwrap_or_else(|| {
        let flag = std::env::var("MODEL_GATEWAY_BROWSER_BUDGET_CLAIM").ok();
        budget_claim_flag(flag.as_deref())
    });
    mint_budget_with(state, req, org_id, enabled).await
}

async fn mint_budget_with(
    state: &AppState,
    req: &BrowserRunStartRequest,
    org_id: &str,
    enabled: bool,
) -> BudgetMint {
    if !enabled {
        return BudgetMint::default();
    }
    let limits = crate::org_quota::fetch_org_limits(
        &state.http_client,
        &state.org_core_base_url,
        org_id,
        &state.org_core_service_id,
        &state.org_core_service_token,
    )
    .await;
    BudgetMint {
        enabled,
        minted: mint_budget_from(req, limits.max_cost_usd),
    }
}

/// The run parameters execution-core is given: the request's, tightened by the
/// grant's claim. A claim never raises a value (an absent or zero request value
/// takes the claim's, a larger one is replaced by it).
fn bounded_run_params(
    req: &BrowserRunStartRequest,
    claim: Option<&BudgetClaim>,
) -> (Option<i32>, Option<i32>, Option<f64>) {
    let Some(claim) = claim else {
        return (req.max_steps, req.max_runtime_s, req.max_cost_usd);
    };
    let tighten_i32 = |asked: Option<i32>, bound: i32| match asked.filter(|value| *value > 0) {
        _ if bound <= 0 => asked,
        Some(asked) => Some(asked.min(bound)),
        None => Some(bound),
    };
    let cost = if claim.micro_usd > 0 {
        let bound = crate::org_quota::micros_to_usd(claim.micro_usd);
        match req.max_cost_usd.filter(|usd| usd.is_finite() && *usd > 0.0) {
            Some(asked) => Some(asked.min(bound)),
            None => Some(bound),
        }
    } else {
        req.max_cost_usd
    };
    (
        tighten_i32(req.max_steps, claim.max_steps),
        tighten_i32(req.max_runtime_s, claim.max_runtime_s),
        cost,
    )
}

/// `bounded_run_params` plus the LLM planner's token budget (WS11.S1): the three values it
/// returns, unchanged, and `min(request.max_planner_tokens, org ceiling)` as a fourth. The
/// first three come from `bounded_run_params` itself, so the two never disagree about a bound.
fn bounded_run_params_with_planner_tokens(
    req: &BrowserRunStartRequest,
    claim: Option<&BudgetClaim>,
    org_planner_token_ceiling: Option<i64>,
) -> (Option<i32>, Option<i32>, Option<f64>, Option<i64>) {
    let (max_steps, max_runtime_s, max_cost_usd) = bounded_run_params(req, claim);
    (
        max_steps,
        max_runtime_s,
        max_cost_usd,
        crate::org_quota::planner_token_ceiling_for(
            req.max_planner_tokens,
            org_planner_token_ceiling,
        ),
    )
}

/// `MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS`: only `on` makes a browser run start read the org's
/// planner quota keys (the token ceiling and the planner switch). Off: nothing is read, the
/// ceiling is the request's own and no `planner_flag` is written. Off by default.
fn planner_quotas_flag(value: Option<&str>) -> bool {
    budget_claim_flag(value)
}

/// What the org's quota rows say about the LLM planner, resolved once per run start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlannerQuota {
    /// `browser_agent_max_tokens_per_run`, when set and readable (fail open).
    token_ceiling: Option<i64>,
    /// `browser_agent_llm_planner` (fail closed: an unreadable read is `Unavailable`).
    flag: crate::org_quota::PlannerFlag,
}

/// A restricted interactive assistant must not proceed if the organisation's
/// planner switch could not be read. An absent row is a real, readable unset;
/// the execution plane's scoped allowlist still decides whether it may run.
fn verify_restricted_planner_quota(enabled: bool, flag: crate::org_quota::PlannerFlag) -> Result<(), ApiError> {
    use crate::org_quota::PlannerFlag;
    if !enabled || flag == PlannerFlag::Unavailable {
        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"browser_planner_policy_unavailable"}))));
    }
    if flag == PlannerFlag::Off {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"browser_planner_disabled"}))));
    }
    Ok(())
}

impl PlannerQuota {
    /// The quota read was not asked for: no ceiling, no flag.
    const OFF: Self = Self {
        token_ceiling: None,
        flag: crate::org_quota::PlannerFlag::Unset,
    };

    fn from_keys(keys: &crate::org_quota::OrgQuotaKeys) -> Self {
        Self {
            token_ceiling: keys.planner_token_ceiling(),
            flag: keys.planner_flag(),
        }
    }
}

async fn planner_quota(state: &AppState, org_id: &str) -> PlannerQuota {
    let flag = std::env::var("MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS").ok();
    if !planner_quotas_flag(flag.as_deref()) {
        return PlannerQuota::OFF;
    }
    PlannerQuota::from_keys(
        &crate::org_quota::fetch_org_quota_keys(
            &state.http_client,
            &state.org_core_base_url,
            org_id,
            &state.org_core_service_id,
            &state.org_core_service_token,
        )
        .await,
    )
}

/// Write the planner's inputs into the browser `tool_input`. Both keys are written only when
/// they have a value, so a run with no planner policy carries the `tool_input` it always did.
/// `planner_flag` comes from the org's quota row ONLY: the request type has no such field, and a
/// key of that name in a client body is dropped at deserialisation.
fn insert_planner_fields(
    tool_input: &mut serde_json::Value,
    max_planner_tokens: Option<i64>,
    planner_flag: crate::org_quota::PlannerFlag,
) {
    let Some(fields) = tool_input.as_object_mut() else {
        return;
    };
    if let Some(tokens) = max_planner_tokens {
        fields.insert("max_planner_tokens".to_owned(), json!(tokens));
    }
    if let Some(flag) = planner_flag.wire() {
        fields.insert("planner_flag".to_owned(), json!(flag));
    }
}

/// The bound a run executes under when its run grant was issued before this
/// request (a presented run grant, or the grant a keyed retry looked up): the
/// tighter of the claim the grant carries and the claim this request mints.
/// Either alone when the other is absent; never wider than either.
fn presented_run_budget(
    granted: Option<BudgetClaim>,
    minted: Option<BudgetClaim>,
) -> Option<BudgetClaim> {
    match (granted, minted) {
        (Some(granted), Some(minted)) => Some(granted.tighter(minted)),
        (granted, minted) => granted.or(minted),
    }
}

/// The `DeriveRunGrant` purpose of an ordinary run.
const DERIVE_PURPOSE_AI_RUN: &str = "ai_run";
/// The `DeriveRunGrant` purpose of an attach start (WS8.G): an `attach` grant
/// whose `resource_id` is the session id. The broker takes it from a signed-in
/// user only.
const DERIVE_PURPOSE_ATTACH: &str = "attach";
/// Most hosts a run may be derived with from a `human_browse` parent (WS8.C).
/// The broker applies its own bound (`BROWSER_BROKER_MAX_RUN_DOMAINS`); this
/// keeps an oversized list from reaching it.
const MAX_HUMAN_BROWSE_RUN_DOMAINS: usize = 8;

/// Resolves the caller's grant into the run grant the browser run executes
/// under. `run_key` names both the managed run and the derived grant, so the
/// broker's (parent, session_key) idempotency is keyed on this run. Without a
/// client `Idempotency-Key` it is minted per start request: a client that
/// retries the POST starts a second run with a second run grant (bounded per
/// session grant by the broker). With one it is derived from the key (see
/// `keyed_run_key`), so a retry derives the same grant and finds the same
/// run. `browser_run_start` revokes an unkeyed derived grant when the start
/// fails after the derive, and every derived grant when the run reaches a
/// terminal state.
async fn resolve_browser_grant(
    state: &AppState,
    request: &BrowserRunStartRequest,
    browser_bearer: &VerifiedBrowserBearer,
    org_id: &str,
    run_key: &str,
    mint: &BudgetMint,
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
    // Attach mode (WS8.G): the grant that backs the run is an `attach` grant derived
    // with the caller's bearer, bound to the named session; never a run grant.
    if non_empty(request.attach_session_id.as_deref()).is_some() {
        return resolve_attach_grant(
            state,
            request,
            browser_bearer,
            org_id,
            run_key,
            grant_id,
            (response.kind.as_str(), response.zdr, &response.allowed_domains),
            mint,
            BudgetClaim::from_wire(
                response.budget_micro_usd,
                response.budget_max_steps,
                response.budget_max_runtime_s,
            ),
        )
        .await;
    }
    // A `human_browse` grant (WS8.C) is a person's own open web: it has no host
    // list to canonicalize and is never lent to a run as it is. The run must name
    // its hosts (at most MAX_HUMAN_BROWSE_RUN_DOMAINS); the broker also refuses a
    // host under its deny-list. The hosts the broker hands back must not exceed
    // the ones asked for, which stand in for the parent policy below.
    if response.kind == "human_browse" {
        // A ZDR grant must never back a durable run, whatever the request says.
        if response.zdr {
            return Err(browser_run_zdr_refusal());
        }
        let hosts = match request.allowed_domains.as_deref() {
            None | Some([]) => {
                return Err(bad_request(
                    "allowed_domains required: a human_browse session never lends open web",
                ))
            }
            Some(requested) => canonical_browser_domains(requested)
                .map_err(|()| bad_request("allowed_domains must be a bounded hostname policy"))?,
        };
        if hosts.len() > MAX_HUMAN_BROWSE_RUN_DOMAINS {
            return Err(bad_request(
                "allowed_domains names too many hosts for a human_browse session",
            ));
        }
        return derive_run_grant(
            state,
            request,
            browser_bearer,
            org_id,
            run_key,
            grant_id,
            &hosts,
            mint,
            BudgetClaim::from_wire(
                response.budget_micro_usd,
                response.budget_max_steps,
                response.budget_max_runtime_s,
            ),
        )
        .await;
    }
    let broker_policy = canonical_browser_domains(&response.allowed_domains)
        .map_err(|()| browser_grant_forbidden())?;
    // A broker response must already be canonical. Accepting a loosely parsed
    // response here could make gateway and execution-core derive different
    // policies for the same grant.
    if broker_policy != response.allowed_domains {
        return Err(browser_grant_forbidden());
    }
    // A ZDR grant must never back a durable run, whatever the request says.
    if response.zdr {
        return Err(browser_run_zdr_refusal());
    }
    match response.kind.as_str() {
        "session" => {
            return derive_run_grant(
                state,
                request,
                browser_bearer,
                org_id,
                run_key,
                grant_id,
                &broker_policy,
                mint,
                BudgetClaim::from_wire(
                    response.budget_micro_usd,
                    response.budget_max_steps,
                    response.budget_max_runtime_s,
                ),
            )
            .await;
        }
        // A run grant (or a grant from a broker that predates kinds) keeps
        // the exact-equality path below. One this gateway derived for an
        // earlier run is bound to that run and backs no other (D1-001).
        "run" | "" => {
            if run_grant_is_bound_to_another_run(&response.session_key, run_key) {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "browser_grant_in_use" })),
                ));
            }
        }
        "sensitive_approval" => {
            return Err(bad_request(
                "a sensitive approval grant cannot start a browser run",
            ));
        }
        _ => return Err(browser_grant_forbidden()),
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
        parent_grant_id: None,
        // A run grant the caller presented keeps the claim it was issued with and
        // is not re-minted, but the run is still bounded by the organization's
        // ceiling: a member can derive a run grant at the broker with no claim,
        // and this is where it would otherwise slip past the ceiling.
        budget: if mint.enabled {
            presented_run_budget(
                BudgetClaim::from_wire(
                    response.budget_micro_usd,
                    response.budget_max_steps,
                    response.budget_max_runtime_s,
                ),
                mint.minted.map(|(claim, _)| claim),
            )
        } else {
            None
        },
    })
}

/// Resolves the grant of an attach start (WS8.G). The parent must be the person's
/// own `session` or `human_browse` grant, never ZDR (the broker refuses it too),
/// and the request must name its hosts: an attach never inherits a parent's list,
/// and a `human_browse` parent has none to inherit. The `attach` grant is derived
/// with the caller's bearer, bound (`resource_id`) to the named session, before
/// the caller's thread or run exists.
///
/// `parent` is the validated parent's `(kind, zdr, allowed_domains)`.
// Nine inputs, all distinct facts about one derive; see `derive_run_grant`.
#[allow(clippy::too_many_arguments)]
async fn resolve_attach_grant(
    state: &AppState,
    request: &BrowserRunStartRequest,
    browser_bearer: &VerifiedBrowserBearer,
    org_id: &str,
    run_key: &str,
    parent_grant_id: &str,
    parent: (&str, bool, &[String]),
    mint: &BudgetMint,
    parent_budget: Option<BudgetClaim>,
) -> Result<ResolvedBrowserGrant, ApiError> {
    let (kind, zdr, parent_domains) = parent;
    // A ZDR grant must never back a durable run, whatever the request says.
    if zdr {
        return Err(browser_run_zdr_refusal());
    }
    if !matches!(kind, "session" | "human_browse") {
        // A run grant, an approval grant or anything else is not the person's own
        // session: an attach is derived from that and from nothing narrower.
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "browser_grant_not_derivable" })),
        ));
    }
    let hosts = match request.allowed_domains.as_deref() {
        None | Some([]) => {
            return Err(bad_request(
                "allowed_domains required: an attach session names its hosts",
            ))
        }
        Some(requested) => canonical_browser_domains(requested)
            .map_err(|()| bad_request("allowed_domains must be a bounded hostname policy"))?,
    };
    if hosts.len() > MAX_HUMAN_BROWSE_RUN_DOMAINS {
        return Err(bad_request(
            "allowed_domains names too many hosts for an attach session",
        ));
    }
    let parent_policy = if kind == "human_browse" {
        // An open-web parent has no host list; the named hosts stand in for it.
        hosts.clone()
    } else {
        let broker_policy =
            canonical_browser_domains(parent_domains).map_err(|()| browser_grant_forbidden())?;
        if broker_policy != parent_domains {
            return Err(browser_grant_forbidden());
        }
        broker_policy
    };
    derive_run_grant(
        state,
        request,
        browser_bearer,
        org_id,
        run_key,
        parent_grant_id,
        &parent_policy,
        mint,
        parent_budget,
    )
    .await
}

/// True when a presented run grant was derived (by this gateway) for a run other than
/// the one starting (D1-001). The broker reports the key a grant was issued under
/// (`ValidateGrantResponse.session_key`); for a run grant this gateway derived it is
/// that run's `run_key` (`browser-<ulid>` or `browser-idem-<hash>`). The check
/// survives a restart, a second replica and the end of the first run, which the
/// process-local `RunStartGuard` claim does not. A key of any other shape (a caller
/// that derived the grant itself) or none (a broker that predates the field) is not
/// bound here and keeps only the guard's live-run claim.
fn run_grant_is_bound_to_another_run(grant_session_key: &str, run_key: &str) -> bool {
    let key = grant_session_key.trim();
    key.starts_with("browser-") && key != run_key
}

/// Derives a run grant from a validated session grant. The broker enforces
/// that the run grant is never wider than its parent; the response is checked
/// again here so a misbehaving broker cannot widen what reaches execution-core.
// Nine inputs, all distinct facts about one derive (the budget claim added two
// to the seven it had); a parameter struct would only rename them.
#[allow(clippy::too_many_arguments)]
async fn derive_run_grant(
    state: &AppState,
    request: &BrowserRunStartRequest,
    browser_bearer: &VerifiedBrowserBearer,
    org_id: &str,
    run_key: &str,
    parent_grant_id: &str,
    parent_policy: &[String],
    mint: &BudgetMint,
    parent_budget: Option<BudgetClaim>,
) -> Result<ResolvedBrowserGrant, ApiError> {
    // `allowed_domains` narrows the session grant; absent (or explicit null)
    // inherits it, which the broker refuses for a parent wider than one run.
    let requested = match request.allowed_domains.as_deref() {
        None | Some([]) => Vec::new(),
        Some(requested) => canonical_browser_domains(requested)
            .map_err(|()| bad_request("allowed_domains must be a bounded hostname policy"))?,
    };
    // The claim to ask for: the minted one, no wider than the parent's own (the
    // broker refuses a derive that widens its parent, and a run should not fail
    // to start because the organization's ceiling is above a session's bound).
    // None asks for none: a parent's claim is inherited by the broker.
    //
    // An attach (WS8.G) sends none: the broker's attach derive does take the claim
    // fields (the same `tightenBudget` as a run grant), but an attach grant is a
    // lease on the person's own session, so it carries the parent's claim as the
    // broker inherits it and this run's numbers are tightened here instead.
    let attach = non_empty(request.attach_session_id.as_deref());
    let asked = if mint.enabled && attach.is_none() {
        mint.minted.map(|(claim, source)| {
            (
                parent_budget.map_or(claim, |parent| claim.tighter(parent)),
                source,
            )
        })
    } else {
        None
    };
    let (ask_claim, ask_source) = asked.unzip();
    let derive_request = |claim: Option<BudgetClaim>, source: Option<&'static str>| {
        DeriveRunGrantRequest {
            parent_grant_id: parent_grant_id.to_owned(),
            session_key: run_key.to_owned(),
            org_id: org_id.to_owned(),
            allowed_domains: requested.clone(),
            purpose: if attach.is_some() {
                DERIVE_PURPOSE_ATTACH
            } else {
                DERIVE_PURPOSE_AI_RUN
            }
            .to_owned(),
            ttl_seconds: 0,
            budget_micro_usd: claim.map_or(0, |claim| claim.micro_usd),
            budget_max_steps: claim.map_or(0, |claim| claim.max_steps),
            budget_max_runtime_s: claim.map_or(0, |claim| claim.max_runtime_s),
            budget_source: source.unwrap_or_default().to_owned(),
            budget_minted_at_unix: 0,
            // Only an attach grant takes a resource: the session it is bound to.
            resource_id: attach.unwrap_or_default().to_owned(),
        }
    };
    let mut broker = state.browser_client.clone();
    let first = broker
        .derive_run_grant(authenticated_browser_request(
            derive_request(ask_claim, ask_source),
            browser_bearer,
        )?)
        .await;
    // The claim is recomputed on every attempt (from the request and from the
    // organization's ceiling, which org-core can fail to answer), and the broker
    // answers a replay that asks for a different claim than the one issued with
    // `AlreadyExists`. A retry of a keyed start must still find its grant, so
    // when a claim was asked for and the broker says the key is already used,
    // ask once more for no claim: that is a lookup, and the broker returns the
    // grant it issued with the claim it carries. A second `AlreadyExists` is a
    // real conflict (a different host list) and is reported as one.
    let (derived, replayed) = match first {
        Err(status) if status.code() == tonic::Code::AlreadyExists && ask_claim.is_some() => {
            let lookup = broker
                .derive_run_grant(authenticated_browser_request(
                    derive_request(None, None),
                    browser_bearer,
                )?)
                .await
                .map_err(|status| browser_derive_status_to_http(&status))?
                .into_inner();
            (lookup, true)
        }
        first => (
            first
                .map_err(|status| browser_derive_status_to_http(&status))?
                .into_inner(),
            false,
        ),
    };

    let grant_id = derived.grant_id.trim();
    if grant_id.is_empty()
        || grant_id == parent_grant_id
        || derived.kind != if attach.is_some() { "attach" } else { "run" }
        // An attach grant is bound to the session it was asked for and no other.
        || attach.is_some_and(|session| derived.resource_id != session)
        || derived.parent_grant_id != parent_grant_id
        // The parent was validated as non-ZDR above; a ZDR run grant would
        // mean the broker and this route disagree about retention.
        || derived.zdr
    {
        return Err(browser_grant_forbidden());
    }
    let allowed_domains = canonical_browser_domains(&derived.allowed_domains)
        .map_err(|()| browser_grant_forbidden())?;
    if allowed_domains != derived.allowed_domains
        || !allowed_domains
            .iter()
            .all(|host| host_within_policy(host, parent_policy))
    {
        return Err(browser_grant_forbidden());
    }
    // The claim the broker stored. It must be no wider than what was asked for
    // (a broker cannot hand back a looser bound), and than the parent's. A grant
    // found by the lookup above was issued under an earlier attempt's claim, so
    // only the parent's bound can be checked against it; nothing is dispatched
    // for a replay, and a first attempt's claim was checked when it was issued.
    let returned = BudgetClaim::from_wire(
        derived.budget_micro_usd,
        derived.budget_max_steps,
        derived.budget_max_runtime_s,
    );
    if mint.enabled {
        let asked_ceiling = if replayed { None } else { ask_claim };
        for ceiling in [asked_ceiling, parent_budget].into_iter().flatten() {
            if !returned.is_some_and(|claim| claim.within(ceiling)) {
                return Err(browser_grant_forbidden());
            }
        }
    }
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
        parent_grant_id: Some(parent_grant_id.to_owned()),
        // The run is bounded by this attempt's claim as well as the grant's own.
        // A grant found by the lookup keeps the claim it was issued with, which
        // can be none (the first attempt ran with the flag off) or looser than
        // the organization's ceiling now (it was lowered since); the retry
        // dispatches a fresh run under it, so the minted claim is applied again.
        // For a first derive the broker's claim is already within the asked one
        // and this changes nothing.
        budget: if mint.enabled {
            presented_run_budget(returned, mint.minted.map(|(claim, _)| claim))
        } else {
            None
        },
    })
}

/// Why model-gateway revokes a run grant it derived. A short label, never a
/// host or URL (the broker stores it as the revoke reason).
const REVOKE_REASON_START_FAILED: &str = "run_start_failed";
const REVOKE_REASON_RUN_ENDED: &str = "run_ended";

/// Best-effort revoke of a run grant this route derived. A run grant the
/// caller presented (`parent_grant_id` is `None`) is never touched: it is not
/// ours to end. A failed revoke is only logged; the broker still bounds the
/// grant by its parent's lifetime and its per-parent cap.
async fn revoke_derived_run_grant(
    state: &AppState,
    browser_bearer: &VerifiedBrowserBearer,
    grant: &ResolvedBrowserGrant,
    reason: &'static str,
) -> bool {
    if grant.parent_grant_id.is_none() {
        // Not ours to revoke, but the run that held it is over: let the grant
        // back another run (D1-001 claim, see `RunStartGuard`).
        state.run_start_guard.release_grant(&grant.grant_id);
        return false;
    }
    let Ok(request) = authenticated_browser_request(
        RevokeGrantRequest {
            grant_id: grant.grant_id.clone(),
            reason: reason.to_owned(),
        },
        browser_bearer,
    ) else {
        warn!(reason, "derived browser run grant revoke could not be authenticated");
        return false;
    };
    let mut broker = state.browser_client.clone();
    match broker.revoke_grant(request).await {
        Ok(_) => true,
        Err(status) => {
            warn!(reason, code = ?status.code(), "derived browser run grant revoke failed");
            false
        }
    }
}

/// Status map for `DeriveRunGrant`. A widening request is 403; a parent that
/// cannot seed a run (kind, lifetime, ZDR posture, a conflicting replay) is
/// 409 `browser_grant_not_derivable`; anything unexpected fails closed.
fn browser_derive_status_to_http(status: &tonic::Status) -> ApiError {
    if let Some(refusal) = attach_derive_refusal(status) {
        return refusal;
    }
    match status.code() {
        tonic::Code::FailedPrecondition | tonic::Code::AlreadyExists => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "browser_grant_not_derivable" })),
        ),
        tonic::Code::InvalidArgument => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "browser_grant_hosts_invalid" })),
        ),
        tonic::Code::ResourceExhausted => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "browser_grant_limit_reached" })),
        ),
        // A broker that predates WS1.B (a rollback) cannot derive: the run
        // fails, nothing falls back to the parent grant.
        tonic::Code::Unimplemented => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "browser_grant_derive_unavailable" })),
        ),
        _ => browser_grant_status_to_http(status),
    }
}

// The broker's attach refusals. Its `errors.go` names them `attach_zdr_refused`,
// `attach_needs_owner` and `attach_resource_invalid` only as metric outcomes; on
// the wire they are a gRPC code plus the error text (`grant.ErrAttach*` in
// `humanbrowse.go` and the user-only check in `server.go`), which is what is
// matched here, code and text together.
const BROKER_ATTACH_ZDR_REFUSED: &str = "an attach grant cannot derive from a zero data retention grant";
const BROKER_ATTACH_NEEDS_OWNER: &str = "an attach grant needs a user owner";
const BROKER_ATTACH_USER_ONLY: &str = "an attach grant is derived by a signed-in user only";

/// The three attach refusals of `DeriveRunGrant`, by the codes `errors.go` gives
/// them: 412 `attach_zdr_refused` (the parent is ZDR; same status as this
/// route's own ZDR refusal), 403 `attach_needs_owner` (the caller is not a
/// signed-in user), 400 `attach_resource_invalid` (the session id was refused).
/// `None` for every other status.
fn attach_derive_refusal(status: &tonic::Status) -> Option<ApiError> {
    let (http, error) = match (status.code(), status.message()) {
        (tonic::Code::FailedPrecondition, BROKER_ATTACH_ZDR_REFUSED) => {
            (StatusCode::PRECONDITION_FAILED, "attach_zdr_refused")
        }
        (
            tonic::Code::PermissionDenied,
            BROKER_ATTACH_NEEDS_OWNER | BROKER_ATTACH_USER_ONLY,
        ) => (StatusCode::FORBIDDEN, "attach_needs_owner"),
        (tonic::Code::InvalidArgument, message) if message.contains("resource_id") => {
            (StatusCode::BAD_REQUEST, "attach_resource_invalid")
        }
        _ => return None,
    };
    Some((http, Json(json!({ "error": error }))))
}

fn browser_run_zdr_refusal() -> ApiError {
    (
        StatusCode::PRECONDITION_FAILED,
        Json(
            mp_contracts::zdr_refusal::Refusal::feature_not_supported("browser_run")
                .json_body(),
        ),
    )
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
    host_within_policy(&host, allowed_domains)
}

/// True when `host` equals a policy host or is a subdomain of one, on a dot
/// boundary (`badexample.com` is not under `example.com`).
fn host_within_policy(host: &str, allowed_domains: &[String]) -> bool {
    allowed_domains.iter().any(|domain| {
        host == domain
            || (host.len() > domain.len()
                && host.ends_with(domain.as_str())
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
///
/// **`Idempotency-Key`** (optional header, see `parse_idempotency_key`). With
/// it, a retry of the same POST returns the same run: the key (scoped to the
/// verified org and user and to the session grant) names the run grant, the
/// thread and the managed run, so the broker, Session Core and this route each
/// answer a retry from what the first attempt made. A replay answers 200 with
/// the original `run_id`, `thread_id` and `plan_id` and
/// `"idempotent_replay": true`, dispatches nothing, appends nothing and does
/// not touch the grant. Refusals: 400 `idempotency_key_invalid`; 400
/// `idempotency_key_requires_a_new_thread` (a keyed start owns its thread, so
/// `thread_id` is refused); 409 `idempotency_key_reused` (the same key with a
/// different goal); 409 `idempotency_key_run_ended` (the run is terminal; use
/// a new key); 503 `idempotency_replay_unverified` (liveness could not be
/// checked). A key whose first attempt ended its own run (the liveness check
/// failed) also ended its grant, so its retry is 409
/// `browser_grant_not_derivable`: use a new key. A start that fails earlier
/// (Session Core unreachable) keeps the grant, so the same key can retry.
/// Without the header every POST is a new run, as before.
///
/// A keyed start runs as a detached task that this handler awaits, so a client
/// that disconnects (a timeout, the retry the key exists for) cannot drop the
/// future between the managed start and the dispatch and leave a live run that
/// was never dispatched. An unkeyed start is not detached, as before. A
/// replay's liveness is a read, not a lease renewal (see
/// `browser_run_replay`). Remaining window: a process that dies after the
/// managed start and before the dispatch leaves a live, undispatched run until
/// its lease expires; no durable dispatch marker exists to tell a replay.
async fn browser_run_start(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    data_plane_bearer: Option<Extension<VerifiedBearer>>,
    execution_bearer: VerifiedExecutionBearer,
    session_bearer: VerifiedSessionBearer,
    inference_bearer: VerifiedInferenceBearer,
    browser_bearer: VerifiedBrowserBearer,
    headers: HeaderMap,
    Json(req): Json<BrowserRunStartRequest>,
) -> Result<Json<Value>, ApiError> {
    let keyed = headers.contains_key(IDEMPOTENCY_KEY_HEADER);
    let start = browser_run_start_inner(
        State(state),
        Extension(claims),
        data_plane_bearer,
        execution_bearer,
        session_bearer,
        inference_bearer,
        browser_bearer,
        headers,
        Json(req),
    );
    if !keyed {
        return start.await;
    }
    match tokio::spawn(start).await {
        Ok(result) => result,
        Err(error) => {
            warn!(%error, "keyed browser run start task did not finish");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "browser_run_start_interrupted" })),
            ))
        }
    }
}

// Cohesive single-flow handler (validate → thread → run → dispatch); splitting
// it would obscure the linear session-core/execution-core sequencing.
#[allow(clippy::too_many_lines)]
async fn browser_run_start_inner(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    data_plane_bearer: Option<Extension<VerifiedBearer>>,
    execution_bearer: VerifiedExecutionBearer,
    session_bearer: VerifiedSessionBearer,
    inference_bearer: VerifiedInferenceBearer,
    browser_bearer: VerifiedBrowserBearer,
    headers: HeaderMap,
    Json(req): Json<BrowserRunStartRequest>,
) -> Result<Json<Value>, ApiError> {
    reject_unbound_space(&req)?;
    if browser_run_zdr_blocked(&claims, req.zdr)? {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(
                mp_contracts::zdr_refusal::Refusal::feature_not_supported("browser_run")
                    .json_body(),
            ),
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
    // A saved recipe (Q4-E4.2-3i) needs its strict flag and a well-formed ask;
    // refused before any grant, thread or run write. It belongs to the person
    // of the verified claims, so a credential without one cannot name a recipe.
    let recipe = recipe_for(
        recipe_flag(std::env::var("MODEL_GATEWAY_BROWSER_RECIPE").ok().as_deref()),
        &req,
    )?;
    if recipe.is_some() && claims.user_id.trim().is_empty() {
        return Err(bad_request("recipe_requires_user"));
    }
    // A restriction the run cannot honour is refused before any grant, thread
    // or run write: flag off, or a malformed ask, never "run without it".
    let policy_ask = policy_ask_for(
        policy_ask_flag(std::env::var("MODEL_GATEWAY_BROWSER_POLICY_ASK").ok().as_deref()),
        &req,
    )?;
    // Attach mode needs its flag; refused the same way, before any write.
    let attach_session = attach_session_for(
        attach_flag(std::env::var("MODEL_GATEWAY_BROWSER_ATTACH").ok().as_deref()),
        &req,
    )?;
    // Rollout attestation is off by default; old execution-core would drop it.
    reject_attached_policy(attach_session.as_deref(), policy_ask.as_ref(),
        attach_flag(std::env::var("MODEL_GATEWAY_BROWSER_ATTACH_POLICY_ENFORCED").ok().as_deref()))?;
    let restricted_planner = if attach_session.is_some() && policy_ask.is_some() {
        let enabled = planner_quotas_flag(std::env::var("MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS").ok().as_deref());
        let quota = planner_quota(&state, &claims.org_id).await;
        verify_restricted_planner_quota(enabled, quota.flag)?;
        Some(quota)
    } else { None };
    if attach_session.is_some() && policy_ask.is_some() {
        verify_attached_policy_protocol(&state, &claims.org_id, &execution_bearer).await?;
    }
    // The mark of a completed attached run: closed set, attach only; refused
    // the same way, before any write.
    let tab_mark = tab_mark_for(&req, attach_session.as_deref())?;
    let run_source = run_source_of(&req);
    let request_id = request_id_for(&headers);
    let idempotency_key = parse_idempotency_key(&headers)?;
    if idempotency_key.is_some() && non_empty(req.thread_id.as_deref()).is_some() {
        return Err(bad_request("idempotency_key_requires_a_new_thread"));
    }
    // Tenant/user identity is always server-derived from the verified bearer
    // claims — never trusted from the request body (the same IDOR-safety
    // rule the rest of model-gateway follows, e.g. `browser_suggest_action`).
    let org_id = claims.org_id.clone();
    let user_id = claims.user_id.clone();

    // One key names both the managed run and its derived run grant, so the
    // broker's (parent, session_key) idempotency lines up with this run. It is
    // fresh per request unless the client sent an `Idempotency-Key`: then it
    // is derived from that key, and a retried POST finds the same run and the
    // same run grant.
    let run_key = match idempotency_key.as_deref() {
        Some(key) => keyed_run_key(&org_id, &user_id, req.grant_id.trim(), key),
        None => format!("browser-{}", new_ulid()),
    };
    let plan_id = non_empty(req.plan_id.as_deref()).map_or_else(
        || {
            if idempotency_key.is_some() {
                keyed_plan_id(&run_key)
            } else {
                new_ulid()
            }
        },
        str::to_owned,
    );

    // Validate the opaque caller-provided id before *any* thread/run write.
    // A cross-tenant/forged/missing policy grant must not create a durable run
    // that later fails asynchronously in execution-core.
    let fingerprint = run_request_fingerprint(&req);
    if idempotency_key.is_some()
        && state.run_start_guard.check_fingerprint(&run_key, &fingerprint)
            == FingerprintCheck::Mismatch
    {
        // The same key with a different body (limits, stop criteria, start
        // page ...) is not a retry of the first request (D1-019 limit c).
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "idempotency_key_reused" })),
        ));
    }
    let mint = mint_budget(&state, &req, &org_id).await;
    let browser_grant =
        resolve_browser_grant(&state, &req, &browser_bearer, &org_id, &run_key, &mint).await?;
    // A run grant the caller presented may back one live run at a time: the
    // broker's record of it carries no session key to bind it to the run being
    // started (D1-001). A keyed retry of the run that holds it is not a second
    // run.
    let claimed_presented_grant = if browser_grant.parent_grant_id.is_none() {
        match state
            .run_start_guard
            .claim_grant(&browser_grant.grant_id, &run_key)
        {
            GrantClaim::Refused => {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "browser_grant_in_use" })),
                ));
            }
            GrantClaim::Fresh => true,
            GrantClaim::Held => false,
        }
    } else {
        false
    };

    let run = match idempotency_key {
        Some(_) => {
            crate::session_flow::prepare_idempotent_browser_run(
                &state,
                &org_id,
                &user_id,
                &req.goal,
                &run_key,
                &session_bearer,
            )
            .await
        }
        None => {
            crate::session_flow::prepare_managed_run_authenticated(
                &state,
                req.thread_id.as_deref(),
                None,
                &org_id,
                &user_id,
                &req.goal,
                "browser-agent",
                "execute",
                &run_key,
                mp_contracts::model_plane::v1::ManagedRunSource::GatewayBrowser,
                false,
                &session_bearer,
                None,
                None,
                false,
                None,
            )
            .await
        }
    };
    let run = match run {
        Ok(run) => run,
        Err(error) => {
            if claimed_presented_grant {
                state.run_start_guard.release_grant(&browser_grant.grant_id);
            }
            if idempotency_key.is_some() {
                // The derived grant stays. This request may be a retry of a
                // start whose run is live, and a same-key retry needs the
                // grant back; the broker bounds it by its parent's lifetime
                // and its per-parent cap.
                let reused = error
                    .downcast_ref::<tonic::Status>()
                    .is_some_and(|status| status.code() == tonic::Code::AlreadyExists);
                if reused {
                    return Err((
                        StatusCode::CONFLICT,
                        Json(json!({ "error": "idempotency_key_reused" })),
                    ));
                }
            } else {
                // The run grant was derived for a run that does not exist.
                revoke_derived_run_grant(
                    &state,
                    &browser_bearer,
                    &browser_grant,
                    REVOKE_REASON_START_FAILED,
                )
                .await;
            }
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("session-core managed start failed: {error}") })),
            ));
        }
    };
    if run.already_started {
        if idempotency_key.is_some() {
            // A retry of a keyed start: the same run, no second dispatch, and
            // the grant it shares with the live run is left alone.
            let replay = browser_run_replay(&state, &run, &plan_id, &session_bearer).await;
            if replay.is_err() && claimed_presented_grant {
                state.run_start_guard.release_grant(&browser_grant.grant_id);
            }
            return replay;
        }
        revoke_derived_run_grant(
            &state,
            &browser_bearer,
            &browser_grant,
            REVOKE_REASON_START_FAILED,
        )
        .await;
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "managed browser run already exists; observe or resume the existing run",
                "run_id": run.run_id,
            })),
        ));
    }
    if idempotency_key.is_some() {
        state
            .run_start_guard
            .remember_fingerprint(&run_key, &fingerprint);
    }
    // The browser endpoint has a GatewayBrowser managed-run obligation even
    // though Execution Core performs the action. Fail before dispatch if the
    // Gateway's scoped workload credential cannot renew that exact lease;
    // never substitute the delegated Session Core user bearer.
    if let Err(error) = crate::session_flow::ensure_browser_agent_run_liveness(&state, &run).await
    {
        warn!(%error, run_id = %run.run_id, "initial browser managed-run liveness heartbeat failed");
        if idempotency_key.is_some() {
            // This request created the run and cannot dispatch it. End it, so
            // a replay of the key finds a terminal run (and the revoke below
            // ends the grant) instead of a live run that never started.
            if let Err(cancel_error) =
                crate::session_flow::cancel_browser_agent_run_authenticated(
                    &state,
                    &run.run_id,
                    "browser_run_start_failed",
                    &session_bearer,
                )
                .await
            {
                warn!(%cancel_error, run_id = %run.run_id, "undispatched browser run could not be cancelled");
            }
        }
        revoke_derived_run_grant(
            &state,
            &browser_bearer,
            &browser_grant,
            REVOKE_REASON_START_FAILED,
        )
        .await;
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "session-core liveness heartbeat failed" })),
        ));
    }
    let terminal_run = run.clone();
    let thread_id = run.thread_id;
    let run_id = run.run_id;
    // Whether the run executes under a grant derived here (session parent) or
    // one the caller presented. Never the hosts.
    info!(
        run_id = %run_id,
        derived_run_grant = browser_grant.parent_grant_id.is_some(),
        "browser run grant resolved"
    );

    // `run_id` is embedded in the tool_input JSON (not just the
    // ExecuteStepRequest envelope) because `tool_bridge::execute_browser_agent`
    // reads `PlanConfig.run_id` from here — it's what every `BrowserEventSink`
    // method and the new pause/cancel `StateStore` gate key off of. Without
    // it the loop runs "blind": no events, no pause/resume/stop.
    // The grant's budget claim tightens what execution-core is told, so the
    // planner and Quarry bound the run by the same numbers.
    let planner = match restricted_planner {
        Some(quota) => quota,
        None => planner_quota(&state, &org_id).await,
    };
    let (max_steps, max_runtime_s, max_cost_usd, max_planner_tokens) =
        bounded_run_params_with_planner_tokens(
            &req,
            browser_grant.budget.as_ref(),
            planner.token_ceiling,
        );
    let mut tool_input = json!({
        "grant_id": browser_grant.grant_id,
        "plan_id": plan_id,
        "run_id": run_id,
        "org_id": org_id,
        "system_prompt": req.goal,
        "max_steps": max_steps,
        "max_runtime_s": max_runtime_s,
        "allowed_domains": browser_grant.allowed_domains,
        "stop_criteria": req.stop_criteria,
        "require_approval": req.require_approval,
        "max_cost_usd": max_cost_usd,
        "zdr": req.zdr,
        "profile_id": req.profile_id,
        "start_url": req.start_url,
    });
    let restricted_dispatch = attach_session.is_some() && policy_ask.is_some();
    insert_attach_fields(&mut tool_input, attach_session.as_deref());
    if let Some(tab) = &req.attach_tab {
        tool_input["attach_tab"] = json!(tab);
    }
    insert_tab_mark(&mut tool_input, tab_mark);
    insert_recipe_fields(&mut tool_input, recipe.as_ref(), user_id.trim());
    insert_planner_fields(&mut tool_input, max_planner_tokens, planner.flag);
    insert_policy_and_trace_fields(&mut tool_input, policy_ask, run_source, &request_id);
    let tool_input = tool_input.to_string();

    let mut execution_client = state.execution_client.clone();
    let terminal_state = state.clone();
    // Resolved before the spawn, while `state` and `org_id` are both still
    // borrowable here; the rules then move into the task with the request.
    let hook_context = crate::runtime_registries::hook_context_json(&state.hooks, &org_id);
    let step_id = new_ulid();
    let dispatch_run_id = run_id.clone();
    let revoke_bearer = browser_bearer.clone();
    let revoke_grant = browser_grant.clone();
    state.run_start_guard.begin_dispatch(&dispatch_run_id);
    tokio::spawn(async move {
        let mut execution_request = tonic::Request::new(ExecuteStepRequest {
            run_id: dispatch_run_id.clone(),
            step_id,
            tool_name: "browser_agent".to_owned(),
            tool_input,
            permission_mode: "auto".to_owned(),
            hook_context,
            org_id,
            user_id,
            zdr: false,
            // browser_agent never reaches knowledge_search, so there is no
            // sovereignty derivation for this field to feed.
            min_privacy_tier: mp_contracts::model_plane::v1::PrivacyTier::Unspecified as i32,
            // Always empty here, not yet a deliberate choice: BrowserRunStartRequest
            // (this handler's own HTTP request type) carries no Space context
            // field at all today, unlike the chat/invoke path's ThreadSpaceContext
            // (see S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md §3.5 phase A,
            // which wired code_interpreter's ExecuteStepRequest.space_id but
            // explicitly left this path's own separate gap for a follow-up).
            space_id: String::new(),
            thread_id: String::new(),
            // browser_agent reads no attached files.
            attachments: Vec::new(),
            // Only code_interpreter steps mount skill bundles.
            skill_mounts: Vec::new(),
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
            // An old instance returns UNIMPLEMENTED for the restricted RPC;
            // `dispatch_browser_step` never retries with the legacy one.
            let execution = dispatch_browser_step(&mut execution_client, execution_request, restricted_dispatch);
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
                            revoke_derived_run_grant(&terminal_state, &revoke_bearer, &revoke_grant, REVOKE_REASON_RUN_ENDED).await;
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
                        terminal_state.run_start_guard.forget_dispatch(&dispatch_run_id);
                        // The loop has finished: its run grant has no further use.
                        revoke_derived_run_grant(&terminal_state, &revoke_bearer, &revoke_grant, REVOKE_REASON_RUN_ENDED).await;
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
                        terminal_state.run_start_guard.forget_dispatch(&dispatch_run_id);
                        revoke_derived_run_grant(&terminal_state, &revoke_bearer, &revoke_grant, REVOKE_REASON_RUN_ENDED).await;
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
                            if terminal_state.run_start_guard.dispatch_parked(&dispatch_run_id) {
                                match crate::session_flow::cancel_browser_agent_run_authenticated(
                                    &terminal_state, &dispatch_run_id, "user_stop", &session_bearer,
                                ).await {
                                    Ok(_) => {
                                        terminal_state.run_start_guard.forget_dispatch(&dispatch_run_id);
                                        revoke_derived_run_grant(&terminal_state, &revoke_bearer, &revoke_grant, REVOKE_REASON_RUN_ENDED).await;
                                    }
                                    Err(error) => {
                                        terminal_state.run_start_guard.restore_parked(&dispatch_run_id);
                                        warn!(%error, run_id = %dispatch_run_id, "parked browser cancellation could not be durably recorded");
                                    }
                                }
                            }
                        }
                        BrowserExecutionOutcome::Unknown => {
                            terminal_state.run_start_guard.forget_dispatch(&dispatch_run_id);
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
                terminal_state.run_start_guard.forget_dispatch(&dispatch_run_id);
                if restricted_dispatch && error.code() == tonic::Code::Unimplemented {
                    // The selected execution instance does not implement the
                    // versioned RPC; it definitively did not run this step.
                    revoke_derived_run_grant(&terminal_state, &revoke_bearer, &revoke_grant, REVOKE_REASON_RUN_ENDED).await;
                    if let Err(terminal_error) = crate::session_flow::terminalize_browser_agent_run_authenticated(
                        &terminal_state, &terminal_run,
                        crate::session_flow::BrowserAgentTerminal::Failed("browser_attach_policy_unavailable"),
                    ).await {
                        warn!(%terminal_error, run_id = %dispatch_run_id, "could not record unsupported restricted-browser dispatch");
                    }
                    return;
                }
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

/// Whether a browser action is refused under ZDR. A browser run is a feature the persistence-free
/// path does not run, and a waiver names a model connection, never a browser route, so a waived
/// token is refused as a route mismatch here.
fn browser_run_zdr_blocked(claims: &Claims, request_zdr: bool) -> Result<bool, ApiError> {
    crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(request_zdr))
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
    /// E3.3-f: text the person typed for the live run, sent with `pause` or
    /// `resume`. Held to the queued-input caps here and queued by execution-core
    /// (which refuses it while its steer switch is off). Never with `stop`.
    #[serde(default)]
    text: Option<String>,
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
    if browser_run_zdr_blocked(&claims, false)? {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(
                mp_contracts::zdr_refusal::Refusal::feature_not_supported("browser_run_control")
                    .json_body(),
            ),
        ));
    }
    if run_id.trim().is_empty() {
        return Err(bad_request("run_id must not be empty"));
    }
    let steer_text = match crate::queued_input::validate_steer_text(req.text.as_deref()) {
        Ok(text) => text,
        Err(crate::queued_input::SteerRefusal::Empty) => {
            return Err(bad_request("steer_text_empty"));
        }
        Err(crate::queued_input::SteerRefusal::TooLong { chars }) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "steer_text_too_long",
                    "chars": chars,
                    "max_chars": crate::queued_input::MAX_QUEUED_CHARS,
                })),
            ));
        }
    };
    if steer_text.is_some() && req.action == BrowserRunControlAction::Stop {
        return Err(bad_request("steer_text_not_allowed_with_stop"));
    }

    let mut execution_client = state.execution_client.clone();
    let status = match req.action {
        BrowserRunControlAction::Pause => {
            let request = authenticated_execution_request(
                PauseRunRequest {
                    run_id: run_id.clone(),
                    org_id: claims.org_id.clone(),
                    steer_text: steer_text.clone().unwrap_or_default(),
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
            // Resolve the owner before touching the local guard: the execution
            // RPC also verifies ownership, but happens after this fence.
            crate::session_flow::require_durable_run_owner_with_token(
                &state, &run_id, &claims.org_id, &claims.sub, session_bearer.as_str(),
            ).await.map_err(|error| grpc_status_to_http(&error))?;
            if !state.run_start_guard.prepare_dispatch_resume(&run_id) {
                return Err((StatusCode::CONFLICT, Json(json!({ "error": "browser_stop_in_progress" }))));
            }
            let request = authenticated_execution_request(
                ResumeRunRequest {
                    run_id: run_id.clone(),
                    checkpoint_id: String::new(),
                    org_id: claims.org_id.clone(),
                    approval_id: String::new(),
                    steer_text: steer_text.clone().unwrap_or_default(),
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
            if !resp.cancelled {
                "running"
            } else if state.run_start_guard.request_dispatch_stop(&run_id) {
                // The local dispatcher has received the parked response, so it
                // cannot still be executing a browser step. Session Core owns
                // the terminal receipt and may reject a competing terminal.
                match crate::session_flow::cancel_browser_agent_run_authenticated(
                    &state, &run_id, "user_stop", &session_bearer,
                ).await {
                    Ok(true) => { state.run_start_guard.forget_dispatch(&run_id); "cancelled" }
                    Ok(false) => { state.run_start_guard.forget_dispatch(&run_id); "terminal" }
                    Err(_) => {
                        state.run_start_guard.restore_parked(&run_id);
                        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "browser_stop_not_confirmed" }))));
                    }
                }
            } else {
                // Latch acceptance alone is not a durable cancellation receipt.
                "stopping"
            }
        }
    };

    Ok(Json(json!({ "status": status })))
}

pub(crate) fn authenticated_browser_request<T>(
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

    #[test]
    fn parked_browser_stop_requires_a_confirmed_dispatch_response() {
        let guard = RunStartGuard::default();
        // A different replica, process restart, or unknown run is never proof.
        assert!(!guard.request_dispatch_stop("unknown"));
        guard.begin_dispatch("normal");
        assert!(!guard.request_dispatch_stop("normal"));
        // The still-running dispatch cannot be durably cancelled yet. Its
        // later parked result safely hands the cancellation to that worker.
        assert!(guard.dispatch_parked("normal"));
        assert!(!guard.request_dispatch_stop("normal"));
        // A failed Session Core write is retriable without fabricating success.
        guard.restore_parked("normal");
        assert!(guard.request_dispatch_stop("normal"));
        guard.forget_dispatch("normal");
        assert!(!guard.request_dispatch_stop("normal"));

        // The response arrives first: exactly one caller owns cancellation.
        guard.begin_dispatch("parked");
        assert!(!guard.dispatch_parked("parked"));
        assert!(guard.request_dispatch_stop("parked"));
        assert!(!guard.request_dispatch_stop("parked"));
        guard.forget_dispatch("parked");

        // A concurrent resume invalidates the locally parked proof, and a
        // cancellation already in progress fences a later resume.
        guard.begin_dispatch("resumed");
        assert!(!guard.dispatch_parked("resumed"));
        assert!(guard.prepare_dispatch_resume("resumed"));
        assert!(!guard.request_dispatch_stop("resumed"));
        assert!(!guard.prepare_dispatch_resume("resumed"));
        guard.forget_dispatch("resumed");

        // A completed/unknown dispatch gives no authority to rewrite outcome.
        guard.begin_dispatch("completed");
        guard.forget_dispatch("completed");
        assert!(!guard.request_dispatch_stop("completed"));
        assert!(!guard.dispatch_parked("completed"));
    }
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
        CancelRunRequest, CancelRunResponse, DeriveRunGrantResponse, ExecuteStepRequest,
        ExecuteStepResponse, PauseRunRequest, PauseRunResponse, ResumeRunRequest,
        RenewGrantRequest, RenewGrantResponse, ResumeRunResponse, RevokeGrantRequest,
        RevokeGrantResponse, RunAgentRequest, RunAgentResponse, ValidateGrantResponse,
    };
    use tonic::{Response, Status};

    #[derive(Clone)]
    struct TestBrowserBroker {
        validation: Result<ValidateGrantResponse, tonic::Code>,
        authorizations: Arc<Mutex<Vec<String>>>,
        /// `None`: DeriveRunGrant is `Unimplemented`, as for a broker that
        /// predates WS1.B.
        derive: Option<Result<DeriveRunGrantResponse, tonic::Code>>,
        derive_requests: Arc<Mutex<Vec<DeriveRunGrantRequest>>>,
        revoke_requests: Arc<Mutex<Vec<RevokeGrantRequest>>>,
        /// Models the broker's replay rule for `derive` (see `sameDeriveBudget`
        /// in the broker): the first derive of a session_key issues the grant with
        /// the claim it asked for; a replay that asks for a claim must ask for the
        /// issued one (else `AlreadyExists`); a replay that asks for none gets the
        /// issued grant back. `derive` is the template of the grant.
        stateful_derive: bool,
        issued: Arc<Mutex<std::collections::HashMap<String, DeriveRunGrantResponse>>>,
    }

    impl TestBrowserBroker {
        fn validating(validation: Result<ValidateGrantResponse, tonic::Code>) -> Self {
            Self {
                validation,
                authorizations: Arc::new(Mutex::new(Vec::new())),
                derive: None,
                derive_requests: Arc::new(Mutex::new(Vec::new())),
                revoke_requests: Arc::new(Mutex::new(Vec::new())),
                stateful_derive: false,
                issued: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }
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
            request: tonic::Request<RevokeGrantRequest>,
        ) -> Result<Response<RevokeGrantResponse>, Status> {
            self.revoke_requests
                .lock()
                .expect("revoke recorder")
                .push(request.into_inner());
            Ok(Response::new(RevokeGrantResponse { revoked: true }))
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

        async fn derive_run_grant(
            &self,
            request: tonic::Request<DeriveRunGrantRequest>,
        ) -> Result<Response<DeriveRunGrantResponse>, Status> {
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
            let message = request.into_inner();
            self.derive_requests
                .lock()
                .expect("derive recorder")
                .push(message.clone());
            if let (true, Some(Ok(template))) = (self.stateful_derive, &self.derive) {
                let claim_of = |m: &DeriveRunGrantRequest| {
                    (
                        m.budget_micro_usd,
                        m.budget_max_steps,
                        m.budget_max_runtime_s,
                        m.budget_source.clone(),
                    )
                };
                let asks = message.budget_micro_usd > 0
                    || message.budget_max_steps > 0
                    || message.budget_max_runtime_s > 0;
                let mut issued = self.issued.lock().expect("issued grants");
                return match issued.get(&message.session_key) {
                    Some(stored)
                        if asks
                            && (
                                stored.budget_micro_usd,
                                stored.budget_max_steps,
                                stored.budget_max_runtime_s,
                                stored.budget_source.clone(),
                            ) != claim_of(&message) =>
                    {
                        Err(Status::already_exists("run grant already derived with a different policy"))
                    }
                    Some(stored) => Ok(Response::new(stored.clone())),
                    None => {
                        let mut response = template.clone();
                        response.budget_micro_usd = message.budget_micro_usd;
                        response.budget_max_steps = message.budget_max_steps;
                        response.budget_max_runtime_s = message.budget_max_runtime_s;
                        response.budget_source = message.budget_source.clone();
                        response.budget_minted_at_unix = if asks { 1_760_000_000 } else { 0 };
                        issued.insert(message.session_key.clone(), response.clone());
                        Ok(Response::new(response))
                    }
                };
            }
            match &self.derive {
                None => Err(Status::unimplemented("not used by validation test")),
                Some(Ok(response)) => Ok(Response::new(response.clone())),
                Some(Err(code)) => Err(Status::new(*code, "test broker rejection")),
            }
        }

        async fn renew_grant(
            &self,
            _request: tonic::Request<RenewGrantRequest>,
        ) -> Result<Response<RenewGrantResponse>, Status> {
            Err(Status::unimplemented("not used by validation test"))
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
    async fn execute_restricted_browser_step(
        &self,
        request: tonic::Request<mp_contracts::model_plane::v1::ExecuteStepRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ExecuteStepResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not used by this test"))
    }

    async fn browser_agent_capabilities(
        &self,
        _request: tonic::Request<mp_contracts::model_plane::v1::BrowserAgentCapabilitiesRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::BrowserAgentCapabilitiesResponse>, tonic::Status> {
        Ok(tonic::Response::new(mp_contracts::model_plane::v1::BrowserAgentCapabilitiesResponse { attached_policy_protocol: 1 }))
    }

        async fn execute_step(
            &self,
            _request: tonic::Request<ExecuteStepRequest>,
        ) -> Result<Response<ExecuteStepResponse>, Status> {
            Err(Status::unimplemented("not used by browser control test"))
        }

        async fn execute_scheduled_step(
            &self,
            _request: tonic::Request<mp_contracts::model_plane::v1::ExecuteScheduledStepRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ExecuteScheduledStepResponse>, Status>
        {
            Err(Status::unimplemented(
                "scheduled steps are not used by browser control test",
            ))
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
            space_id: None,
            space_ref: None,
            space_context: None,
            space_browser_authority: None,
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
            max_planner_tokens: None,
            zdr: false,
            plan_mode: None,
            policy: None,
            run_source: None,
            attach_session_id: None,
            attach_tab: None,
            tab_mark_on_complete: None,
            recipe: None,
        }
    }

    fn browser_claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "auth-core".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    #[test]
    fn standalone_browser_start_refuses_explicit_space_binding() {
        for field in [
            "space_id",
            "spaceId",
            "space_ref",
            "spaceRef",
            "space_context",
            "spaceContext",
            "space_browser_authority",
            "spaceBrowserAuthority",
            "space_authority",
            "spaceAuthority",
        ] {
            let mut body = json!({ "goal": "read the page", "grant_id": "grant-1" });
            body[field] = json!({ "id": "space-1" });
            let request: BrowserRunStartRequest =
                serde_json::from_value(body).expect("parse request");
            let (status, Json(error)) =
                reject_unbound_space(&request).expect_err("Space target must be refused");
            assert_eq!(status, StatusCode::PRECONDITION_FAILED);
            assert_eq!(error["error"], "space_browser_run_unavailable");
        }
        let personal: BrowserRunStartRequest = serde_json::from_value(json!({
            "goal": "read the page", "grant_id": "grant-1", "space_id": null
        }))
        .expect("parse personal request");
        assert!(reject_unbound_space(&personal).is_ok());
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
        // Regression: the Verevon gateway sends an explicit JSON `null` (not a
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
            authorizations: authorizations.clone(),
            ..TestBrowserBroker::validating(Err(tonic::Code::NotFound))
        })
        .await;
        let error = resolve_browser_grant(
            &state,
            &browser_request("forged-or-other-tenant"),
            &VerifiedBrowserBearer::for_test("browser-user-token"),
            "org-a",
            "browser-run-key",
            &BudgetMint::default(),
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
            authorizations,
            ..TestBrowserBroker::validating(Ok(ValidateGrantResponse {
                grant_id: "grant-valid".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            }))
        })
        .await;
        let grant = resolve_browser_grant(
            &state,
            &browser_request("grant-valid"),
            &VerifiedBrowserBearer::for_test("browser-user-token"),
            "org-a",
            "browser-run-key",
            &BudgetMint::default(),
        )
        .await
        .expect("valid broker grant");
        assert_eq!(grant.grant_id, "grant-valid");
        assert_eq!(grant.allowed_domains, ["example.com"]);
        handle.abort();

        let (state, handle) =
            state_with_browser_broker(TestBrowserBroker::validating(Ok(ValidateGrantResponse {
                grant_id: "grant-empty-policy".to_owned(),
                active: true,
                ..Default::default()
            })))
            .await;
        let error = resolve_browser_grant(
            &state,
            &browser_request("grant-empty-policy"),
            &VerifiedBrowserBearer::for_test("browser-user-token"),
            "org-a",
            "browser-run-key",
            &BudgetMint::default(),
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
                Json(BrowserRunControlRequest { action, text: None }),
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

    fn session_validation(grant_id: &str, domains: &[&str]) -> ValidateGrantResponse {
        ValidateGrantResponse {
            grant_id: grant_id.to_owned(),
            active: true,
            allowed_domains: domains.iter().map(|d| (*d).to_owned()).collect(),
            kind: "session".to_owned(),
            owner_id: "user-a".to_owned(),
            ..Default::default()
        }
    }

    fn derived_run(grant_id: &str, parent: &str, domains: &[&str]) -> DeriveRunGrantResponse {
        DeriveRunGrantResponse {
            grant_id: grant_id.to_owned(),
            allowed_domains: domains.iter().map(|d| (*d).to_owned()).collect(),
            parent_grant_id: parent.to_owned(),
            kind: "run".to_owned(),
            ..Default::default()
        }
    }

    /// Resolves `request` against `broker` and returns the result together
    /// with the broker, whose recorders the caller can inspect.
    async fn resolve_with(
        broker: TestBrowserBroker,
        request: BrowserRunStartRequest,
    ) -> (Result<ResolvedBrowserGrant, ApiError>, TestBrowserBroker) {
        resolve_with_mint(broker, request, BudgetMint::default()).await
    }

    async fn resolve_with_mint(
        broker: TestBrowserBroker,
        request: BrowserRunStartRequest,
        mint: BudgetMint,
    ) -> (Result<ResolvedBrowserGrant, ApiError>, TestBrowserBroker) {
        let (state, handle) = state_with_browser_broker(broker.clone()).await;
        let result = resolve_browser_grant(
            &state,
            &request,
            &VerifiedBrowserBearer::for_test("browser-user-token"),
            "org-a",
            "browser-run-key",
            &mint,
        )
        .await;
        handle.abort();
        (result, broker)
    }

    fn session_broker(
        domains: &[&str],
        derive: Result<DeriveRunGrantResponse, tonic::Code>,
    ) -> TestBrowserBroker {
        TestBrowserBroker {
            derive: Some(derive),
            ..TestBrowserBroker::validating(Ok(session_validation("session-1", domains)))
        }
    }

    #[tokio::test]
    async fn a_session_grant_derives_a_run_grant_keyed_on_the_run_and_narrowed_by_the_request() {
        let mut request = browser_request("session-1");
        request.allowed_domains = Some(vec![" API.example.com ".to_owned()]);
        request.start_url = Some("https://api.example.com/start".to_owned());
        let (result, broker) = resolve_with(
            session_broker(
                &["example.com", "example.org"],
                Ok(derived_run("run-1", "session-1", &["api.example.com"])),
            ),
            request,
        )
        .await;
        let grant = result.expect("derived run grant");
        assert_eq!(grant.grant_id, "run-1");
        assert_eq!(grant.allowed_domains, ["api.example.com"]);
        assert_eq!(grant.parent_grant_id.as_deref(), Some("session-1"));

        let derives = broker.derive_requests.lock().expect("derive recorder");
        assert_eq!(derives.len(), 1);
        let sent = &derives[0];
        assert_eq!(sent.parent_grant_id, "session-1");
        assert_eq!(sent.session_key, "browser-run-key");
        assert_eq!(sent.org_id, "org-a");
        assert_eq!(sent.allowed_domains, ["api.example.com"]);
        assert_eq!(sent.purpose, "ai_run");
        assert_eq!(sent.ttl_seconds, 0);
        assert_eq!(
            broker
                .authorizations
                .lock()
                .expect("authorization recorder")
                .as_slice(),
            ["Bearer browser-user-token", "Bearer browser-user-token"],
            "validate and derive both use the delegated browser bearer"
        );
    }

    fn human_browse_validation(grant_id: &str) -> ValidateGrantResponse {
        ValidateGrantResponse {
            grant_id: grant_id.to_owned(),
            active: true,
            // An open-web grant has no host list: the policy is its mode.
            allowed_domains: Vec::new(),
            kind: "human_browse".to_owned(),
            policy_mode: "open_web".to_owned(),
            denied_domains: vec![".internal".to_owned(), "metadata.goog".to_owned()],
            owner_id: "user-a".to_owned(),
            ..Default::default()
        }
    }

    fn human_browse_broker(
        derive: Result<DeriveRunGrantResponse, tonic::Code>,
    ) -> TestBrowserBroker {
        TestBrowserBroker {
            derive: Some(derive),
            ..TestBrowserBroker::validating(Ok(human_browse_validation("human-1")))
        }
    }

    #[tokio::test]
    async fn derive_from_human_browse_without_domains_is_400() {
        for allowed in [None, Some(Vec::new())] {
            let mut request = browser_request("human-1");
            request.allowed_domains = allowed;
            let (result, broker) = resolve_with(
                human_browse_broker(Ok(derived_run("run-1", "human-1", &["example.com"]))),
                request,
            )
            .await;
            let error = result.expect_err("a human_browse session never lends open web");
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
            assert!(
                error.1 .0["error"]
                    .as_str()
                    .is_some_and(|message| message.contains("never lends open web")),
                "{:?}",
                error.1 .0
            );
            assert!(
                broker.derive_requests.lock().expect("recorder").is_empty(),
                "nothing reaches the broker's derive"
            );
        }
    }

    #[tokio::test]
    async fn derive_from_human_browse_passes_the_named_hosts_and_the_ai_run_purpose() {
        let mut request = browser_request("human-1");
        request.allowed_domains = Some(vec![" API.example.com ".to_owned()]);
        request.start_url = Some("https://api.example.com/start".to_owned());
        let (result, broker) = resolve_with(
            human_browse_broker(Ok(derived_run("run-1", "human-1", &["api.example.com"]))),
            request,
        )
        .await;
        let grant = result.expect("derived run grant");
        assert_eq!(grant.grant_id, "run-1");
        assert_eq!(grant.allowed_domains, ["api.example.com"]);
        assert_eq!(grant.parent_grant_id.as_deref(), Some("human-1"));
        let derives = broker.derive_requests.lock().expect("derive recorder");
        assert_eq!(derives.len(), 1);
        assert_eq!(derives[0].allowed_domains, ["api.example.com"]);
        assert_eq!(derives[0].purpose, "ai_run");
        assert!(derives[0].resource_id.is_empty());
    }

    #[tokio::test]
    async fn derive_from_human_browse_refuses_too_many_hosts_and_a_widening_answer() {
        let mut request = browser_request("human-1");
        request.allowed_domains = Some(
            (0..=MAX_HUMAN_BROWSE_RUN_DOMAINS)
                .map(|i| format!("h{i}.example.com"))
                .collect(),
        );
        let (result, broker) = resolve_with(
            human_browse_broker(Ok(derived_run("run-1", "human-1", &["h0.example.com"]))),
            request,
        )
        .await;
        assert_eq!(result.expect_err("nine hosts").0, StatusCode::BAD_REQUEST);
        assert!(broker.derive_requests.lock().expect("recorder").is_empty());

        // A broker that answers with a host the run did not ask for is refused:
        // the requested hosts stand in for the (absent) parent policy.
        let mut request = browser_request("human-1");
        request.allowed_domains = Some(vec!["api.example.com".to_owned()]);
        request.start_url = None;
        let (result, _) = resolve_with(
            human_browse_broker(Ok(derived_run(
                "run-2",
                "human-1",
                &["api.example.com", "evil.example.net"],
            ))),
            request,
        )
        .await;
        assert_eq!(result.expect_err("widened").0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_zdr_human_browse_grant_never_backs_a_run() {
        let mut validation = human_browse_validation("human-1");
        validation.zdr = true;
        let mut request = browser_request("human-1");
        request.allowed_domains = Some(vec!["api.example.com".to_owned()]);
        let (result, broker) = resolve_with(
            TestBrowserBroker {
                derive: Some(Ok(derived_run("run-1", "human-1", &["api.example.com"]))),
                ..TestBrowserBroker::validating(Ok(validation))
            },
            request,
        )
        .await;
        assert!(result.is_err());
        assert!(broker.derive_requests.lock().expect("recorder").is_empty());
    }

    #[tokio::test]
    async fn a_session_grant_without_requested_hosts_asks_the_broker_to_inherit() {
        for allowed in [None, Some(Vec::new())] {
            let mut request = browser_request("session-1");
            request.allowed_domains = allowed;
            let (result, broker) = resolve_with(
                session_broker(
                    &["example.com"],
                    Ok(derived_run("run-1", "session-1", &["example.com"])),
                ),
                request,
            )
            .await;
            assert_eq!(result.expect("inherited").allowed_domains, ["example.com"]);
            assert!(broker.derive_requests.lock().expect("recorder")[0]
                .allowed_domains
                .is_empty());
        }
    }

    // ── WS8.G: attach mode ──────────────────────────────────────────────────

    const ATTACH_SESSION: &str = "sess-run-01J0USER";

    fn attach_request(grant_id: &str, hosts: &[&str]) -> BrowserRunStartRequest {
        let mut request = browser_request(grant_id);
        request.attach_session_id = Some(ATTACH_SESSION.to_owned());
        request.allowed_domains = Some(hosts.iter().map(|h| (*h).to_owned()).collect());
        request.start_url = None;
        request
    }

    fn attach_derived(grant_id: &str, parent: &str, domains: &[&str]) -> DeriveRunGrantResponse {
        DeriveRunGrantResponse {
            kind: "attach".to_owned(),
            resource_id: ATTACH_SESSION.to_owned(),
            ..derived_run(grant_id, parent, domains)
        }
    }

    fn attach_session_broker(derive: Result<DeriveRunGrantResponse, tonic::Code>) -> TestBrowserBroker {
        session_broker(&["example.com", "example.org"], derive)
    }

    #[tokio::test]
    async fn attach_derives_with_user_bearer_before_any_run_write() {
        // Session parent, then human_browse parent: both derive an attach grant
        // with the caller's bearer, bound to the session, with no budget claim.
        let brokers = [
            ("session-1", attach_session_broker(Ok(attach_derived("att-1", "session-1", &["example.com"])))),
            (
                "human-1",
                human_browse_broker(Ok(attach_derived("att-1", "human-1", &["example.com"]))),
            ),
        ];
        for (parent, broker) in brokers {
            let request = attach_request(parent, &["Example.com"]);
            let mint = minted(5_000_000, 10, 60, "request");
            let (result, broker) = resolve_with_mint(broker, request, mint).await;
            let grant = result.expect("derived attach grant");
            assert_eq!(grant.grant_id, "att-1");
            assert_eq!(grant.parent_grant_id.as_deref(), Some(parent));
            assert_eq!(grant.allowed_domains, ["example.com"]);

            let derives = broker.derive_requests.lock().expect("derive recorder");
            assert_eq!(derives.len(), 1, "{parent}");
            let sent = &derives[0];
            assert_eq!(sent.purpose, "attach");
            assert_eq!(sent.resource_id, ATTACH_SESSION);
            assert_eq!(sent.parent_grant_id, parent);
            assert_eq!(sent.session_key, "browser-run-key");
            assert_eq!(sent.allowed_domains, ["example.com"]);
            assert_eq!(
                (sent.budget_micro_usd, sent.budget_max_steps, sent.budget_max_runtime_s),
                (0, 0, 0),
                "an attach derive sends no claim; the broker inherits the parent's"
            );
            assert!(sent.budget_source.is_empty());
            assert_eq!(
                broker.authorizations.lock().expect("authorization recorder").as_slice(),
                ["Bearer browser-user-token", "Bearer browser-user-token"],
                "validate and derive both use the caller's browser bearer"
            );
        }
    }

    #[tokio::test]
    async fn attach_requires_explicit_domains() {
        for parent in ["session-1", "human-1"] {
            for allowed in [None, Some(Vec::new())] {
                let mut request = attach_request(parent, &[]);
                request.allowed_domains = allowed;
                let broker = if parent == "session-1" {
                    attach_session_broker(Ok(attach_derived("att-1", parent, &["example.com"])))
                } else {
                    human_browse_broker(Ok(attach_derived("att-1", parent, &["example.com"])))
                };
                let (result, broker) = resolve_with(broker, request).await;
                let error = result.expect_err("an attach never inherits hosts");
                assert_eq!(error.0, StatusCode::BAD_REQUEST, "{parent}");
                assert!(
                    error.1 .0["error"]
                        .as_str()
                        .is_some_and(|message| message.contains("names its hosts")),
                    "{:?}",
                    error.1 .0
                );
                assert!(broker.derive_requests.lock().expect("recorder").is_empty());
            }
        }
        // More than the cap is refused too, before the broker is asked.
        let hosts: Vec<String> = (0..=MAX_HUMAN_BROWSE_RUN_DOMAINS)
            .map(|i| format!("h{i}.example.com"))
            .collect();
        let refs: Vec<&str> = hosts.iter().map(String::as_str).collect();
        let (result, broker) = resolve_with(
            human_browse_broker(Ok(attach_derived("att-1", "human-1", &["h0.example.com"]))),
            attach_request("human-1", &refs),
        )
        .await;
        assert_eq!(result.expect_err("nine hosts").0, StatusCode::BAD_REQUEST);
        assert!(broker.derive_requests.lock().expect("recorder").is_empty());
    }

    #[tokio::test]
    async fn attach_rejected_for_zdr() {
        for kind in ["session", "human_browse"] {
            let validation = ValidateGrantResponse {
                kind: kind.to_owned(),
                zdr: true,
                ..session_validation("grant-z", &["example.com"])
            };
            let broker = TestBrowserBroker {
                derive: Some(Ok(attach_derived("att-1", "grant-z", &["example.com"]))),
                ..TestBrowserBroker::validating(Ok(validation))
            };
            let (result, broker) =
                resolve_with(broker, attach_request("grant-z", &["example.com"])).await;
            assert_eq!(
                result.expect_err(kind).0,
                StatusCode::PRECONDITION_FAILED,
                "{kind}"
            );
            assert!(broker.derive_requests.lock().expect("recorder").is_empty());
        }
        // The broker's own refusal (a ZDR parent it was asked to attach to) keeps
        // its name: gRPC FailedPrecondition + the error text of `ErrAttachZdrRefused`.
        let refusal = browser_derive_status_to_http(&tonic::Status::failed_precondition(
            BROKER_ATTACH_ZDR_REFUSED,
        ));
        assert_eq!(refusal.0, StatusCode::PRECONDITION_FAILED);
        assert_eq!(refusal.1 .0["error"], "attach_zdr_refused");
    }

    #[tokio::test]
    async fn attach_rejected_when_parent_is_run_or_approval_kind() {
        for kind in ["run", "", "sensitive_approval", "attach", "origin_extension"] {
            let validation = ValidateGrantResponse {
                kind: kind.to_owned(),
                ..session_validation("grant-x", &["example.com"])
            };
            let broker = TestBrowserBroker {
                derive: Some(Ok(attach_derived("att-1", "grant-x", &["example.com"]))),
                ..TestBrowserBroker::validating(Ok(validation))
            };
            let (result, broker) =
                resolve_with(broker, attach_request("grant-x", &["example.com"])).await;
            let error = result.expect_err(kind);
            assert_eq!(error.0, StatusCode::FORBIDDEN, "{kind:?}");
            assert_eq!(error.1 .0["error"], "browser_grant_not_derivable", "{kind:?}");
            assert!(
                broker.derive_requests.lock().expect("recorder").is_empty(),
                "{kind:?} must not derive"
            );
        }
    }

    #[tokio::test]
    async fn an_attach_answer_for_another_session_or_kind_is_refused() {
        let wrong_session = DeriveRunGrantResponse {
            resource_id: "sess-someone-else".to_owned(),
            ..attach_derived("att-1", "session-1", &["example.com"])
        };
        let no_session = DeriveRunGrantResponse {
            resource_id: String::new(),
            ..attach_derived("att-1", "session-1", &["example.com"])
        };
        let run_kind = derived_run("att-1", "session-1", &["example.com"]);
        let wider = attach_derived("att-1", "session-1", &["example.com", "example.net"]);
        for (name, response) in [
            ("wrong session", wrong_session),
            ("no session", no_session),
            ("run kind", run_kind),
            ("wider", wider),
        ] {
            let (result, _) = resolve_with(
                attach_session_broker(Ok(response)),
                attach_request("session-1", &["example.com"]),
            )
            .await;
            assert_eq!(result.expect_err(name).0, StatusCode::FORBIDDEN, "{name}");
        }
    }

    #[test]
    fn the_brokers_attach_refusals_keep_their_names() {
        let cases = [
            (
                tonic::Status::failed_precondition(BROKER_ATTACH_ZDR_REFUSED),
                StatusCode::PRECONDITION_FAILED,
                "attach_zdr_refused",
            ),
            (
                tonic::Status::permission_denied(BROKER_ATTACH_NEEDS_OWNER),
                StatusCode::FORBIDDEN,
                "attach_needs_owner",
            ),
            (
                tonic::Status::permission_denied(BROKER_ATTACH_USER_ONLY),
                StatusCode::FORBIDDEN,
                "attach_needs_owner",
            ),
            (
                tonic::Status::invalid_argument(
                    "attach grant needs a bounded resource_id, and only an attach grant takes one",
                ),
                StatusCode::BAD_REQUEST,
                "attach_resource_invalid",
            ),
            (
                tonic::Status::invalid_argument("resource_id is required for an attach grant"),
                StatusCode::BAD_REQUEST,
                "attach_resource_invalid",
            ),
        ];
        for (status, want_status, want_error) in cases {
            let mapped = browser_derive_status_to_http(&status);
            assert_eq!(mapped.0, want_status, "{want_error}");
            assert_eq!(mapped.1 .0["error"], want_error);
        }
        // Other refusals keep the mapping they had.
        for status in [
            tonic::Status::failed_precondition("something else"),
            tonic::Status::permission_denied("derive would widen the parent"),
            tonic::Status::invalid_argument("hosts are not grantable"),
        ] {
            assert!(attach_derive_refusal(&status).is_none(), "{status:?}");
        }
    }

    #[test]
    fn the_attach_flag_is_off_unless_one_true_or_on() {
        for on in [Some("1"), Some("true"), Some("on"), Some("ON"), Some(" True ")] {
            assert!(attach_flag(on), "{on:?}");
        }
        for off in [None, Some(""), Some("0"), Some("off"), Some("yes"), Some("enabled")] {
            assert!(!attach_flag(off), "{off:?}");
        }
    }

    #[test]
    fn a_start_with_an_attach_session_is_refused_while_the_flag_is_off() {
        let mut request = browser_request("session-1");
        assert_eq!(attach_session_for(false, &request).expect("no attach"), None);
        assert_eq!(attach_session_for(true, &request).expect("no attach"), None);

        request.attach_session_id = Some(ATTACH_SESSION.to_owned());
        let refused = attach_session_for(false, &request).expect_err("flag off");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert_eq!(refused.1 .0["error"], "browser_attach_unavailable");
        assert_eq!(
            attach_session_for(true, &request).expect("flag on").as_deref(),
            Some(ATTACH_SESSION)
        );

        let oversized = "x".repeat(MAX_ATTACH_SESSION_ID_BYTES + 1);
        for bad in ["", "   ", "has space", oversized.as_str()] {
            request.attach_session_id = Some(bad.to_owned());
            let refused = attach_session_for(true, &request).expect_err(bad);
            assert_eq!(refused.0, StatusCode::BAD_REQUEST);
            assert_eq!(refused.1 .0["error"], "attach_session_id_invalid", "{bad:?}");
        }
    }

    #[test]
    fn an_existing_tab_is_bound_to_one_canonical_origin_and_no_start_url() {
        let mut request = attach_request("session-1", &["example.com"]);
        request.attach_tab = Some(AttachTabTarget {
            tab_id: "tab-1".into(),
            expected_epoch: 0,
            expected_origin: "https://example.com".into(),
        });
        assert!(attach_session_for(true, &request).is_ok());

        request.start_url = Some("https://example.com/".into());
        assert!(attach_session_for(true, &request).is_err());
        request.start_url = None;

        request.allowed_domains = Some(vec!["example.com".into(), "other.example".into()]);
        assert!(attach_session_for(true, &request).is_err());
        request.allowed_domains = Some(vec!["example.com".into()]);

        request.attach_tab.as_mut().unwrap().expected_origin = "https://example.com/".into();
        assert!(attach_session_for(true, &request).is_err());
    }

    #[test]
    fn tab_mark_on_complete_passes_through_for_an_attach_start_only() {
        // Not named: nothing in the tool input, nothing in the fingerprint.
        let plain = attach_request("session-1", &["example.com"]);
        assert_eq!(tab_mark_for(&plain, Some(ATTACH_SESSION)).expect("no mark"), None);
        let mut untouched = json!({ "grant_id": "att-1" });
        insert_tab_mark(&mut untouched, None);
        assert_eq!(untouched, json!({ "grant_id": "att-1" }));

        // Named with an attach session: the closed set, case-insensitive, into the tool input.
        for (asked, sent) in [("none", "none"), ("Deliverable", "deliverable"), (" HANDOFF ", "handoff")] {
            let mut request = attach_request("session-1", &["example.com"]);
            request.tab_mark_on_complete = Some(asked.to_owned());
            let mark = tab_mark_for(&request, Some(ATTACH_SESSION)).expect(asked);
            assert_eq!(mark, Some(sent), "{asked}");
            let mut tool_input = json!({ "grant_id": "att-1", "attach_session_id": ATTACH_SESSION });
            insert_tab_mark(&mut tool_input, mark);
            assert_eq!(tool_input["tab_mark_on_complete"], sent);
            assert_eq!(tool_input["attach_session_id"], ATTACH_SESSION, "the rest is untouched");
        }

        // Outside the set: refused, whatever else is true.
        for bad in ["keep", "", "pinned", "deliverable,none"] {
            let mut request = attach_request("session-1", &["example.com"]);
            request.tab_mark_on_complete = Some(bad.to_owned());
            let refused = tab_mark_for(&request, Some(ATTACH_SESSION)).expect_err(bad);
            assert_eq!(refused.0, StatusCode::BAD_REQUEST);
            assert_eq!(refused.1 .0["error"], "tab_mark_on_complete_invalid", "{bad:?}");
        }

        // A mark on a start that does not attach is refused, not silently dropped.
        let mut leased = browser_request("session-1");
        leased.tab_mark_on_complete = Some("deliverable".to_owned());
        let refused = tab_mark_for(&leased, None).expect_err("no attach session");
        assert_eq!(refused.1 .0["error"], "tab_mark_requires_attach");

        // A retry that asks for another mark is a different request; none named keeps the old fingerprint.
        let base = attach_request("session-1", &["example.com"]);
        let mut marked = base.clone();
        marked.tab_mark_on_complete = Some("handoff".to_owned());
        let mut other = base.clone();
        other.tab_mark_on_complete = Some("deliverable".to_owned());
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&marked));
        assert_ne!(run_request_fingerprint(&marked), run_request_fingerprint(&other));
        assert_eq!(run_request_fingerprint(&base), run_request_fingerprint(&base.clone()));
    }

    #[test]
    fn attach_fields_ride_the_tool_input_only_when_attaching() {
        let mut plain = json!({ "grant_id": "g", "profile_id": "prof-1" });
        insert_attach_fields(&mut plain, None);
        assert_eq!(plain, json!({ "grant_id": "g", "profile_id": "prof-1" }));

        let mut attached = json!({ "grant_id": "att-1", "profile_id": "prof-1" });
        insert_attach_fields(&mut attached, Some(ATTACH_SESSION));
        assert_eq!(attached["grant_id"], "att-1");
        assert_eq!(attached["attach_session_id"], ATTACH_SESSION);
        assert_eq!(attached["profile_id"], Value::Null, "no profile in someone's session");
    }

    #[test]
    fn a_retry_that_names_another_attach_session_is_a_different_request() {
        let base = attach_request("session-1", &["example.com"]);
        assert_eq!(run_request_fingerprint(&base), run_request_fingerprint(&base.clone()));
        let mut other = base.clone();
        other.attach_session_id = Some("sess-other".to_owned());
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&other));
        let mut plain = base.clone();
        plain.attach_session_id = None;
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&plain));
    }

    /// The handler reads the flag from the process environment; every test that
    /// needs it on sets the same value, and none sets it off.
    fn attach_flag_on() {
        std::env::set_var("MODEL_GATEWAY_BROWSER_ATTACH", "1");
    }

    // Plan name: derive_failure_creates_no_thread_or_run (attach variant; the
    // ordinary-run test of the same name is above). A refused attach derive
    // stops the start before Session Core is contacted at all.
    #[tokio::test]
    async fn attach_derive_failure_creates_no_thread_or_run() {
        attach_flag_on();
        for code in [
            tonic::Code::FailedPrecondition,
            tonic::Code::PermissionDenied,
            tonic::Code::InvalidArgument,
            tonic::Code::Unimplemented,
        ] {
            let broker = attach_session_broker(Err(code));
            let (result, session_connections) =
                start_with(broker.clone(), attach_request("session-1", &["example.com"])).await;
            assert!(result.is_err(), "{code:?} must refuse the start");
            assert_eq!(
                broker.derive_requests.lock().expect("recorder").len(),
                1,
                "{code:?}: the derive was attempted"
            );
            assert_eq!(
                session_connections, 0,
                "{code:?}: no thread or run write may follow a refused attach derive"
            );
            assert!(broker.revoke_requests.lock().expect("recorder").is_empty());
        }
    }

    // ── WS6.E: the budget claim minted onto a derived run grant ────────────

    fn minted(micro_usd: i64, max_steps: i32, max_runtime_s: i32, source: &'static str) -> BudgetMint {
        BudgetMint {
            enabled: true,
            minted: Some((
                BudgetClaim {
                    micro_usd,
                    max_steps,
                    max_runtime_s,
                },
                source,
            )),
        }
    }

    fn with_claim(
        response: DeriveRunGrantResponse,
        micro_usd: i64,
        max_steps: i32,
        max_runtime_s: i32,
    ) -> DeriveRunGrantResponse {
        DeriveRunGrantResponse {
            budget_micro_usd: micro_usd,
            budget_max_steps: max_steps,
            budget_max_runtime_s: max_runtime_s,
            budget_source: "request".to_owned(),
            budget_minted_at_unix: 1_760_000_000,
            ..response
        }
    }

    #[test]
    fn mint_budget_takes_the_min_of_request_and_org_ceiling() {
        let mut request = browser_request("session-1");
        request.max_cost_usd = Some(2.0);
        request.max_steps = Some(30);
        request.max_runtime_s = Some(300);

        // The organization's ceiling is lower: it binds the cost, and says so.
        let (claim, source) = mint_budget_from(&request, Some(0.5)).expect("a claim");
        assert_eq!(
            claim,
            BudgetClaim {
                micro_usd: 500_000,
                max_steps: 30,
                max_runtime_s: 300
            }
        );
        assert_eq!(source, "org_default");

        // The request is lower than the ceiling: the request binds it.
        let (claim, source) = mint_budget_from(&request, Some(5.0)).expect("a claim");
        assert_eq!(claim.micro_usd, 2_000_000);
        assert_eq!(source, "request");

        // No ceiling at all (org-core unconfigured): the request's own values.
        let (claim, source) = mint_budget_from(&request, None).expect("a claim");
        assert_eq!((claim.micro_usd, source), (2_000_000, "request"));

        // No request cost, a ceiling: the ceiling is the bound.
        request.max_cost_usd = None;
        let (claim, source) = mint_budget_from(&request, Some(0.25)).expect("a claim");
        assert_eq!((claim.micro_usd, source), (250_000, "org_default"));

        // A positive ceiling that rounds to zero micro-dollars is still a bound.
        let (claim, _) = mint_budget_from(&request, Some(1e-9)).expect("a claim");
        assert_eq!(claim.micro_usd, 1);
    }

    #[test]
    fn claim_is_absent_when_neither_is_set() {
        let request = browser_request("session-1");
        assert_eq!(mint_budget_from(&request, None), None);
        // A zero, negative or non-finite value is no bound, in the request or the ceiling.
        for junk in [Some(0.0), Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
            let mut with_junk = browser_request("session-1");
            with_junk.max_cost_usd = junk;
            assert_eq!(mint_budget_from(&with_junk, junk), None, "{junk:?}");
        }
        let mut zero_steps = browser_request("session-1");
        zero_steps.max_steps = Some(0);
        zero_steps.max_runtime_s = Some(-5);
        assert_eq!(mint_budget_from(&zero_steps, None), None);
        assert_eq!(BudgetClaim::from_wire(0, 0, 0), None);
        assert_eq!(BudgetClaim::from_wire(-1, -1, -1), None);
    }

    #[tokio::test]
    async fn org_core_failure_does_not_block_the_run() {
        let (mut state, handle) = state_with_browser_broker(session_broker(
            &["example.com"],
            Ok(derived_run("run-1", "session-1", &["example.com"])),
        ))
        .await;
        // org-core is configured and unreachable: the read is fail-open, so the claim
        // is the request's own values and the run starts.
        state.org_core_base_url = "http://127.0.0.1:1".to_owned();
        state.org_core_service_token = "test-token".to_owned();
        let mut request = browser_request("session-1");
        request.max_cost_usd = Some(0.75);
        let mint = mint_budget_with(&state, &request, "org-a", true).await;
        assert!(mint.enabled);
        let (claim, source) = mint.minted.expect("the request's own bound");
        assert_eq!((claim.micro_usd, source), (750_000, "request"));

        // Switched off, nothing is read and nothing is minted.
        assert_eq!(
            mint_budget_with(&state, &request, "org-a", false).await,
            BudgetMint::default()
        );
        handle.abort();
    }

    #[test]
    fn only_on_turns_the_claim_flag_on() {
        assert!(budget_claim_flag(Some("on")));
        assert!(budget_claim_flag(Some(" ON ")));
        for off in [None, Some(""), Some("off"), Some("true"), Some("1"), Some("onn")] {
            assert!(!budget_claim_flag(off), "{off:?}");
        }
    }

    #[test]
    fn the_planner_token_budget_rides_bounded_run_params_and_changes_nothing_else() {
        let mut request = browser_request("session-1");
        request.max_steps = Some(40);
        request.max_cost_usd = Some(3.0);
        let claim = BudgetClaim {
            micro_usd: 1_000_000,
            max_steps: 10,
            max_runtime_s: 120,
        };
        // The first three values are exactly `bounded_run_params`'s, claim or none.
        for claim in [Some(&claim), None] {
            let (steps, runtime, cost) = bounded_run_params(&request, claim);
            assert_eq!(
                bounded_run_params_with_planner_tokens(&request, claim, None),
                (steps, runtime, cost, None)
            );
        }
        // The tokens are min(request, org) and the org key alone binds a silent request.
        request.max_planner_tokens = Some(80_000);
        assert_eq!(
            bounded_run_params_with_planner_tokens(&request, None, Some(50_000)).3,
            Some(50_000)
        );
        request.max_planner_tokens = Some(20_000);
        assert_eq!(
            bounded_run_params_with_planner_tokens(&request, None, Some(50_000)).3,
            Some(20_000)
        );
        request.max_planner_tokens = None;
        assert_eq!(
            bounded_run_params_with_planner_tokens(&request, None, Some(50_000)).3,
            Some(50_000)
        );
        assert_eq!(bounded_run_params_with_planner_tokens(&request, None, None).3, None);
    }

    #[test]
    fn only_on_turns_the_planner_quota_read_on() {
        assert!(planner_quotas_flag(Some("on")));
        for off in [None, Some(""), Some("off"), Some("1"), Some("true")] {
            assert!(!planner_quotas_flag(off), "{off:?}");
        }
        // Off: no ceiling, no flag, so nothing is written for an org that never opted in.
        assert_eq!(
            PlannerQuota::OFF,
            PlannerQuota {
                token_ceiling: None,
                flag: crate::org_quota::PlannerFlag::Unset
            }
        );
    }

    #[test]
    fn restricted_assistant_requires_a_readable_non_disabled_org_planner_policy() {
        use crate::org_quota::PlannerFlag;
        assert_eq!(verify_restricted_planner_quota(false, PlannerFlag::Unset).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(verify_restricted_planner_quota(true, PlannerFlag::Unavailable).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(verify_restricted_planner_quota(true, PlannerFlag::Off).unwrap_err().0, StatusCode::FORBIDDEN);
        assert!(verify_restricted_planner_quota(true, PlannerFlag::Unset).is_ok());
        assert!(verify_restricted_planner_quota(true, PlannerFlag::On).is_ok());
    }

    #[test]
    fn the_planner_keys_are_written_only_when_they_have_a_value() {
        let base = json!({ "grant_id": "g", "max_steps": 5 });
        let mut untouched = base.clone();
        insert_planner_fields(&mut untouched, None, crate::org_quota::PlannerFlag::Unset);
        assert_eq!(untouched, base, "no planner policy: the tool_input is exactly what it was");

        let mut written = base.clone();
        insert_planner_fields(&mut written, Some(40_000), crate::org_quota::PlannerFlag::Off);
        assert_eq!(written["max_planner_tokens"], 40_000);
        assert_eq!(written["planner_flag"], "off");
        assert_eq!(written["grant_id"], "g");

        let mut unavailable = base;
        insert_planner_fields(&mut unavailable, None, crate::org_quota::PlannerFlag::Unavailable);
        assert_eq!(unavailable["planner_flag"], "unavailable");
        assert!(unavailable.get("max_planner_tokens").is_none());
    }

    #[test]
    fn origin_mapping_covers_every_run_source() {
        let expected = [
            (RunSource::Chat, "USER"),
            (RunSource::Space, "USER"),
            (RunSource::Support, "USER"),
            (RunSource::AgentRunConsole, "USER"),
            (RunSource::ScheduledCron, "SCHEDULE"),
            (RunSource::Routine, "SCHEDULE"),
            (RunSource::BackgroundReview, "AGENT"),
            (RunSource::Dreaming, "AGENT"),
            (RunSource::MailTask, "WEBHOOK"),
            (RunSource::DirectService, "API"),
        ];
        // Every source in ALL is mapped, and nothing maps that ALL lacks.
        assert_eq!(expected.len(), RunSource::ALL.len());
        for source in RunSource::ALL {
            let (_, origin) = expected
                .iter()
                .find(|(s, _)| *s == source)
                .unwrap_or_else(|| panic!("{source:?} has no expected origin"));
            assert_eq!(source.origin().as_str(), *origin, "{source:?}");
            // Wire names round trip, case-insensitively.
            assert_eq!(RunSource::from_wire(source.wire()), Some(source));
            assert_eq!(RunSource::from_wire(&source.wire().to_uppercase()), Some(source));
            // Strings the trace accepts: 1 to 64 chars of [A-Za-z0-9_-].
            let origin = source.origin().as_str();
            assert!(origin.len() <= 64 && origin.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
        }
        // Unlabelled, unknown or hostile labels are a person at the keyboard.
        let mut request = browser_request("session-1");
        assert_eq!(run_source_of(&request), RunSource::Chat);
        for label in ["", "root", "API", "scheduled cron; drop", "ünï"] {
            request.run_source = Some(label.to_owned());
            assert_eq!(run_source_of(&request), RunSource::Chat, "{label:?}");
        }
        request.run_source = Some("mail_task".to_owned());
        assert_eq!(run_source_of(&request).origin().as_str(), "WEBHOOK");
    }

    #[test]
    fn request_id_is_the_inbound_one_when_valid_else_fresh() {
        let mut headers = HeaderMap::new();
        headers.insert(REQUEST_ID_HEADER, "req_Abc-123".parse().unwrap());
        assert_eq!(request_id_for(&headers), "req_Abc-123");
        for bad in ["a b", "a/b", "x".repeat(65).as_str(), ""] {
            let mut headers = HeaderMap::new();
            headers.insert(REQUEST_ID_HEADER, bad.parse().unwrap());
            let id = request_id_for(&headers);
            assert_ne!(id, bad);
            assert!(id.starts_with("req-") && id.len() <= 64, "{id}");
        }
        let fresh = request_id_for(&HeaderMap::new());
        assert!(fresh.starts_with("req-"));
        assert_ne!(fresh, request_id_for(&HeaderMap::new()));
    }

    #[test]
    fn trace_and_policy_fields_ride_the_tool_input() {
        let mut tool_input = json!({ "grant_id": "g" });
        insert_policy_and_trace_fields(&mut tool_input, None, RunSource::ScheduledCron, "req-1");
        assert_eq!(tool_input["grant_id"], "g");
        assert_eq!(tool_input["request_id"], "req-1");
        assert_eq!(tool_input["trace"]["origin"], "SCHEDULE");
        assert_eq!(tool_input["trace"]["agent"]["id"], "browser-agent");
        assert!(tool_input["trace"].get("run_id").is_none(), "execution-core fills the run id");
        assert!(tool_input.get("policy").is_none(), "no ask, no policy key");

        let mut with_ask = json!({});
        insert_policy_and_trace_fields(
            &mut with_ask,
            Some(json!({ "disable": ["browser.act.upload"] })),
            RunSource::Chat,
            "req-2",
        );
        assert_eq!(with_ask["policy"]["disable"][0], "browser.act.upload");
        assert_eq!(with_ask["trace"]["origin"], "USER");
    }

    #[test]
    fn attached_runs_reject_policy_restrictions_instead_of_dropping_them() {
        let mut request = attach_request("session-1", &["example.com"]);
        request.plan_mode = Some(true);
        let ask = policy_ask_for(true, &request).expect("valid plan ask");
        let failure = reject_attached_policy(Some("session-1"), ask.as_ref(), false).expect_err("must refuse");
        assert_eq!(failure.0, StatusCode::PRECONDITION_FAILED);
        assert_eq!(failure.1 .0["error"], "browser_attach_policy_unavailable");
        assert!(reject_attached_policy(None, ask.as_ref(), false).is_ok());
        assert!(reject_attached_policy(Some("session-1"), None, false).is_ok());
        assert!(reject_attached_policy(Some("session-1"), ask.as_ref(), true).is_ok());
    }

    #[test]
    fn policy_ask_flag_is_off_unless_literally_on() {
        assert!(policy_ask_flag(Some("on")));
        for off in [None, Some(""), Some("off"), Some("1"), Some("true")] {
            assert!(!policy_ask_flag(off), "{off:?}");
        }
    }

    #[test]
    fn policy_ask_is_refused_when_the_flag_is_off_and_absent_asks_pass() {
        let plain = browser_request("session-1");
        assert_eq!(policy_ask_for(false, &plain).unwrap(), None);
        assert_eq!(policy_ask_for(true, &plain).unwrap(), None);

        let mut plan = browser_request("session-1");
        plan.plan_mode = Some(true);
        let refused = policy_ask_for(false, &plan).expect_err("fail closed");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert_eq!(refused.1 .0["error"], "browser_policy_ask_unavailable");

        let mut asked = browser_request("session-1");
        asked.policy = Some(RunPolicyAsk {
            deny_domains: vec!["ads.example".to_owned()],
            ..Default::default()
        });
        assert!(policy_ask_for(false, &asked).is_err());

        // plan_mode=false and an empty policy ask for nothing, flag or not.
        let mut nothing = browser_request("session-1");
        nothing.plan_mode = Some(false);
        nothing.policy = Some(RunPolicyAsk::default());
        assert_eq!(policy_ask_for(false, &nothing).unwrap(), None);
    }

    #[test]
    fn plan_mode_expands_to_the_four_disables_and_unions_with_the_callers_ask() {
        let mut request = browser_request("session-1");
        request.plan_mode = Some(true);
        assert_eq!(
            policy_ask_for(true, &request).unwrap(),
            Some(json!({"disable": [
                "browser.act.download", "browser.act.input",
                "browser.act.interact", "browser.act.upload"
            ]}))
        );
        request.policy = Some(RunPolicyAsk {
            disable: vec!["browser.act.upload".to_owned(), "browser.act.scroll".to_owned()],
            deny_domains: vec![" B.example ".to_owned(), "a.example".to_owned(), "a.example".to_owned()],
            allow_domains: Some(vec!["Docs.Example.com".to_owned()]),
        });
        let ask = policy_ask_for(true, &request).unwrap().expect("ask");
        assert_eq!(
            ask["disable"],
            json!([
                "browser.act.download", "browser.act.input", "browser.act.interact",
                "browser.act.scroll", "browser.act.upload"
            ])
        );
        assert_eq!(ask["deny_domains"], json!(["a.example", "b.example"]));
        assert_eq!(ask["allow_domains"], json!(["docs.example.com"]));
        // Only these keys ever exist in the output.
        let keys: BTreeSet<&str> = ask.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["allow_domains", "deny_domains", "disable"]));
    }

    #[test]
    fn malformed_asks_are_refused_not_trimmed() {
        let with = |policy: RunPolicyAsk| {
            let mut request = browser_request("session-1");
            request.policy = Some(policy);
            policy_ask_for(true, &request)
        };
        for policy in [
            RunPolicyAsk { disable: vec!["Browser Act".to_owned()], ..Default::default() },
            RunPolicyAsk { deny_domains: vec!["https://x.example/".to_owned()], ..Default::default() },
            RunPolicyAsk { allow_domains: Some(vec!["*.example.com".to_owned()]), ..Default::default() },
            RunPolicyAsk { disable: (0..65).map(|i| format!("cap.n{i}")).collect(), ..Default::default() },
        ] {
            let err = with(policy).expect_err("malformed");
            assert_eq!(err.1 .0["error"], "browser_policy_ask_invalid");
        }
        // An allow list that is present but empty is a real ask (allow nothing).
        let none_allowed = with(RunPolicyAsk { allow_domains: Some(vec![]), ..Default::default() });
        assert_eq!(none_allowed.unwrap(), Some(json!({"allow_domains": []})));
    }

    #[test]
    fn the_ask_cannot_name_robots_or_proxy_keys() {
        for key in ["robots_max", "proxy_max", "mode"] {
            let body = json!({"goal": "g", "grant_id": "grant-1", "policy": { key: "off" }});
            assert!(serde_json::from_value::<BrowserRunStartRequest>(body).is_err(), "{key}");
        }
        let ok: BrowserRunStartRequest = serde_json::from_value(json!({
            "goal": "g", "grant_id": "grant-1", "plan_mode": true,
            "policy": {"disable": ["browser.act.upload"]}, "run_source": "space",
        }))
        .unwrap();
        assert_eq!(ok.plan_mode, Some(true));
        assert_eq!(run_source_of(&ok), RunSource::Space);
    }

    #[test]
    fn a_keyed_retry_that_adds_a_restriction_is_a_different_request() {
        let base = browser_request("session-1");
        let mut planned = base.clone();
        planned.plan_mode = Some(true);
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&planned));
        // Absent or empty asks keep the fingerprint a request always had.
        let mut empty = base.clone();
        empty.plan_mode = Some(false);
        empty.policy = Some(RunPolicyAsk::default());
        assert_eq!(run_request_fingerprint(&base), run_request_fingerprint(&empty));
    }

    #[test]
    fn client_supplied_planner_flag_is_ignored() {
        // A caller that names `planner_flag` (or the token key under a different spelling) gets
        // nothing from it: the request type has no such field, and the only writer of
        // `tool_input.planner_flag` is the org's quota row.
        let request: BrowserRunStartRequest = serde_json::from_value(json!({
            "goal": "g",
            "grant_id": "grant-1",
            "planner_flag": "on",
            "max_planner_tokens": 30_000,
        }))
        .expect("a body naming planner_flag still parses");
        assert_eq!(request.max_planner_tokens, Some(30_000));
        let planner = PlannerQuota::OFF;
        let mut tool_input = json!({ "grant_id": "grant-1" });
        let (.., tokens) = bounded_run_params_with_planner_tokens(&request, None, planner.token_ceiling);
        insert_planner_fields(&mut tool_input, tokens, planner.flag);
        assert!(
            tool_input.get("planner_flag").is_none(),
            "a client-supplied planner_flag must not reach tool_input: {tool_input}"
        );
        // And when the org says off, that is what is written whatever the client sent.
        insert_planner_fields(&mut tool_input, tokens, crate::org_quota::PlannerFlag::Off);
        assert_eq!(tool_input["planner_flag"], "off");
    }

    #[test]
    fn a_claim_tightens_the_run_parameters_and_never_raises_them() {
        let mut request = browser_request("session-1");
        request.max_steps = Some(40);
        request.max_runtime_s = Some(600);
        request.max_cost_usd = Some(3.0);
        let tight = BudgetClaim {
            micro_usd: 1_000_000,
            max_steps: 10,
            max_runtime_s: 120,
        };
        assert_eq!(
            bounded_run_params(&request, Some(&tight)),
            (Some(10), Some(120), Some(1.0))
        );
        // A claim above the request leaves the request alone.
        let loose = BudgetClaim {
            micro_usd: 9_000_000,
            max_steps: 400,
            max_runtime_s: 6_000,
        };
        assert_eq!(
            bounded_run_params(&request, Some(&loose)),
            (Some(40), Some(600), Some(3.0))
        );
        // A request that named nothing takes the claim's bounds, and a dimension the
        // claim leaves open stays as the request had it.
        let bare = browser_request("session-1");
        let steps_only = BudgetClaim {
            micro_usd: 0,
            max_steps: 8,
            max_runtime_s: 0,
        };
        assert_eq!(
            bounded_run_params(&bare, Some(&steps_only)),
            (Some(8), None, None)
        );
        // No claim: exactly the request's values.
        assert_eq!(
            bounded_run_params(&request, None),
            (Some(40), Some(600), Some(3.0))
        );
    }

    #[tokio::test]
    async fn a_minted_claim_rides_the_derive_request_and_the_returned_claim_bounds_the_run() {
        let (result, broker) = resolve_with_mint(
            session_broker(
                &["example.com"],
                Ok(with_claim(
                    derived_run("run-1", "session-1", &["example.com"]),
                    500_000,
                    30,
                    300,
                )),
            ),
            browser_request("session-1"),
            minted(500_000, 30, 300, "org_default"),
        )
        .await;
        let grant = result.expect("derived run grant");
        assert_eq!(
            grant.budget,
            Some(BudgetClaim {
                micro_usd: 500_000,
                max_steps: 30,
                max_runtime_s: 300
            })
        );
        let derives = broker.derive_requests.lock().expect("derive recorder");
        let sent = &derives[0];
        assert_eq!(
            (
                sent.budget_micro_usd,
                sent.budget_max_steps,
                sent.budget_max_runtime_s,
                sent.budget_source.as_str(),
                sent.budget_minted_at_unix
            ),
            (500_000, 30, 300, "org_default", 0),
            "the stamp is the broker's, never sent"
        );
    }

    #[tokio::test]
    async fn the_claim_asked_for_is_clipped_to_the_parents_own() {
        // The session grant already carries a $1 bound; the organization's ceiling
        // is $5. Asking for $5 would be refused by the broker as a widening, so
        // the route asks for no more than the parent's.
        let parent = ValidateGrantResponse {
            budget_micro_usd: 1_000_000,
            budget_source: "org_default".to_owned(),
            budget_minted_at_unix: 1_760_000_000,
            ..session_validation("session-1", &["example.com"])
        };
        let broker = TestBrowserBroker {
            derive: Some(Ok(with_claim(
                derived_run("run-1", "session-1", &["example.com"]),
                1_000_000,
                30,
                0,
            ))),
            ..TestBrowserBroker::validating(Ok(parent))
        };
        let (result, broker) = resolve_with_mint(
            broker,
            browser_request("session-1"),
            minted(5_000_000, 30, 0, "org_default"),
        )
        .await;
        result.expect("a run grant");
        let derives = broker.derive_requests.lock().expect("derive recorder");
        assert_eq!(derives[0].budget_micro_usd, 1_000_000);
        assert_eq!(derives[0].budget_max_steps, 30);
    }

    #[tokio::test]
    async fn a_broker_that_widens_or_drops_the_claim_is_refused() {
        let ask = minted(500_000, 30, 0, "org_default");
        for (name, response) in [
            (
                "wider cost",
                with_claim(derived_run("run-1", "session-1", &["example.com"]), 900_000, 30, 0),
            ),
            (
                "wider steps",
                with_claim(derived_run("run-1", "session-1", &["example.com"]), 500_000, 31, 0),
            ),
            (
                "a dropped dimension",
                with_claim(derived_run("run-1", "session-1", &["example.com"]), 500_000, 0, 0),
            ),
            (
                "no claim at all",
                derived_run("run-1", "session-1", &["example.com"]),
            ),
        ] {
            let (result, _) = resolve_with_mint(
                session_broker(&["example.com"], Ok(response)),
                browser_request("session-1"),
                ask.clone(),
            )
            .await;
            let error = result.expect_err(name);
            assert_eq!(error.0, StatusCode::FORBIDDEN, "{name}");
        }
    }

    #[tokio::test]
    async fn a_disabled_mint_sends_no_claim_and_reads_none_back() {
        // The broker answers with a claim (an inherited one); with the flag off the
        // route neither asks for one nor honours it, so the run is exactly as before.
        let (result, broker) = resolve_with_mint(
            session_broker(
                &["example.com"],
                Ok(with_claim(
                    derived_run("run-1", "session-1", &["example.com"]),
                    100,
                    1,
                    1,
                )),
            ),
            browser_request("session-1"),
            BudgetMint::default(),
        )
        .await;
        assert_eq!(result.expect("run grant").budget, None);
        let derives = broker.derive_requests.lock().expect("derive recorder");
        assert_eq!(
            (
                derives[0].budget_micro_usd,
                derives[0].budget_max_steps,
                derives[0].budget_max_runtime_s,
                derives[0].budget_source.as_str()
            ),
            (0, 0, 0, "")
        );
    }

    #[tokio::test]
    async fn a_presented_run_grant_keeps_its_claim_only_when_the_flag_is_on() {
        let run_grant = ValidateGrantResponse {
            kind: "run".to_owned(),
            budget_max_steps: 12,
            budget_source: "request".to_owned(),
            ..session_validation("run-9", &["example.com"])
        };
        let on = BudgetMint {
            enabled: true,
            minted: None,
        };
        let (result, broker) = resolve_with_mint(
            TestBrowserBroker::validating(Ok(run_grant.clone())),
            browser_request("run-9"),
            on,
        )
        .await;
        let grant = result.expect("a presented run grant");
        assert_eq!(
            grant.budget,
            Some(BudgetClaim {
                micro_usd: 0,
                max_steps: 12,
                max_runtime_s: 0
            })
        );
        assert!(
            broker.derive_requests.lock().expect("recorder").is_empty(),
            "a presented run grant is never re-minted"
        );
        let (result, _) = resolve_with_mint(
            TestBrowserBroker::validating(Ok(run_grant)),
            browser_request("run-9"),
            BudgetMint::default(),
        )
        .await;
        assert_eq!(result.expect("a presented run grant").budget, None);
    }

    /// D1-001, the durable half: a run grant the broker says was derived for another run
    /// is refused without any live-run claim, so a restart, a second replica or the end
    /// of the first run does not free it. Other key shapes are not bound.
    #[tokio::test]
    async fn a_presented_run_grant_derived_for_another_run_is_refused() {
        let run_grant = |session_key: &str| ValidateGrantResponse {
            kind: "run".to_owned(),
            session_key: session_key.to_owned(),
            ..session_validation("run-9", &["example.com"])
        };
        for (session_key, refused) in [
            ("browser-01J0OTHERRUN", true),
            ("browser-idem-0123456789abcdef", true),
            ("browser-run-key", false),
            ("caller-derived-key", false),
            ("", false),
        ] {
            let (result, _) = resolve_with_mint(
                TestBrowserBroker::validating(Ok(run_grant(session_key))),
                browser_request("run-9"),
                BudgetMint::default(),
            )
            .await;
            if refused {
                let error = result.expect_err(session_key);
                assert_eq!(error.0, StatusCode::CONFLICT, "{session_key}");
                assert_eq!(error.1 .0["error"], "browser_grant_in_use", "{session_key}");
            } else {
                result.unwrap_or_else(|_| panic!("{session_key:?} must be accepted"));
            }
        }
    }

    /// The parent's own claim is the second ceiling on what the broker may return
    /// (the first is what was asked for). With nothing minted nothing is asked for,
    /// so the parent's is the only one in play and this is the test that reaches it.
    #[tokio::test]
    async fn the_returned_claim_is_checked_against_the_parents_own_when_the_request_asks_for_none() {
        let parent = ValidateGrantResponse {
            budget_micro_usd: 1_000_000,
            budget_max_steps: 20,
            budget_source: "org_default".to_owned(),
            budget_minted_at_unix: 1_760_000_000,
            ..session_validation("session-1", &["example.com"])
        };
        let nothing_minted = BudgetMint {
            enabled: true,
            minted: None,
        };
        let run = || derived_run("run-1", "session-1", &["example.com"]);
        for (name, response, accepted) in [
            ("equal to the parent's", with_claim(run(), 1_000_000, 20, 0), true),
            ("tighter than the parent's", with_claim(run(), 400_000, 10, 60), true),
            ("a wider cost", with_claim(run(), 2_000_000, 20, 0), false),
            ("wider steps", with_claim(run(), 1_000_000, 21, 0), false),
            ("a dropped cost bound", with_claim(run(), 0, 20, 0), false),
            ("a dropped step bound", with_claim(run(), 1_000_000, 0, 0), false),
            ("no claim at all", run(), false),
        ] {
            let broker = TestBrowserBroker {
                derive: Some(Ok(response)),
                ..TestBrowserBroker::validating(Ok(parent.clone()))
            };
            let (result, broker) =
                resolve_with_mint(broker, browser_request("session-1"), nothing_minted.clone())
                    .await;
            if accepted {
                result.unwrap_or_else(|_| panic!("{name}: must be accepted"));
            } else {
                let error = result.expect_err(name);
                assert_eq!(error.0, StatusCode::FORBIDDEN, "{name}");
            }
            let derives = broker.derive_requests.lock().expect("derive recorder");
            assert_eq!(
                (derives[0].budget_micro_usd, derives[0].budget_max_steps),
                (0, 0),
                "{name}: nothing was asked for, so only the parent's bound is in play"
            );
        }
    }

    /// A keyed retry recomputes its claim (a different request, a ceiling that
    /// moved, an org-core that did not answer). The broker refuses a derive replay
    /// that asks for a different claim, so the route asks once more for none, which
    /// the broker answers with the grant it issued.
    #[tokio::test]
    async fn a_derive_replay_whose_claim_moved_looks_up_the_issued_grant() {
        let mut broker = session_broker(
            &["example.com"],
            Ok(derived_run("run-1", "session-1", &["example.com"])),
        );
        broker.stateful_derive = true;
        // Attempt 1: the organization's ceiling binds the cost.
        let (first, _) = resolve_with_mint(
            broker.clone(),
            browser_request("session-1"),
            minted(500_000, 30, 0, "org_default"),
        )
        .await;
        let issued = first.expect("the first derive").budget;
        assert_eq!(
            issued,
            Some(BudgetClaim {
                micro_usd: 500_000,
                max_steps: 30,
                max_runtime_s: 0
            })
        );
        // Attempt 2 (org-core down: the request's own, looser, cost) and attempt 3 (a
        // lower ceiling): each finds the same grant. The run it dispatches is held to
        // the tighter of the issued claim and this attempt's own, never the looser.
        for (name, mint, expected) in [
            ("looser", minted(2_000_000, 30, 0, "request"), issued),
            (
                "tighter",
                minted(250_000, 30, 0, "org_default"),
                Some(BudgetClaim {
                    micro_usd: 250_000,
                    max_steps: 30,
                    max_runtime_s: 0,
                }),
            ),
        ] {
            let (again, _) =
                resolve_with_mint(broker.clone(), browser_request("session-1"), mint).await;
            let again = again.unwrap_or_else(|_| panic!("{name}: the retry must find its grant"));
            assert_eq!(again.grant_id, "run-1", "{name}");
            assert_eq!(again.budget, expected, "{name}");
        }
        let derives = broker.derive_requests.lock().expect("derive recorder");
        // first, then (ask, lookup) twice
        assert_eq!(derives.len(), 5);
        for lookup in [&derives[2], &derives[4]] {
            assert_eq!(
                (
                    lookup.budget_micro_usd,
                    lookup.budget_max_steps,
                    lookup.budget_max_runtime_s,
                    lookup.budget_source.as_str()
                ),
                (0, 0, 0, ""),
                "the lookup asks for no claim"
            );
        }
    }

    /// A keyed start whose first attempt derived the grant and then failed before the
    /// run existed is retried, and the retry dispatches a fresh run under the grant it
    /// finds. If the first attempt had the flag off (or the organization's ceiling was
    /// higher) that grant carries no claim, or a looser one: the retry's own claim must
    /// still bound the run.
    #[tokio::test]
    async fn a_retry_that_finds_an_unclaimed_grant_still_meets_the_organizations_ceiling() {
        let mut broker = session_broker(
            &["example.com"],
            Ok(derived_run("run-1", "session-1", &["example.com"])),
        );
        broker.stateful_derive = true;
        // Attempt 1: flag off, so the grant is issued with no claim.
        let (first, _) = resolve_with_mint(
            broker.clone(),
            browser_request("session-1"),
            BudgetMint::default(),
        )
        .await;
        assert_eq!(first.expect("the first derive").budget, None);
        // Attempt 2: flag on, the organization's ceiling binds.
        let (retry, broker) = resolve_with_mint(
            broker,
            browser_request("session-1"),
            minted(500_000, 30, 0, "org_default"),
        )
        .await;
        let retry = retry.expect("the retry must find its grant");
        assert_eq!(retry.grant_id, "run-1");
        assert_eq!(
            retry.budget,
            Some(BudgetClaim {
                micro_usd: 500_000,
                max_steps: 30,
                max_runtime_s: 0
            }),
            "the run is bounded although the grant it found carries no claim"
        );
        // It was a lookup: the second ask was refused, the third asked for none.
        assert_eq!(broker.derive_requests.lock().expect("recorder").len(), 3);
    }

    #[tokio::test]
    async fn a_derive_conflict_that_is_not_about_the_claim_is_still_a_conflict() {
        // The broker says AlreadyExists to the lookup too (a different host list).
        let (result, broker) = resolve_with_mint(
            session_broker(&["example.com"], Err(tonic::Code::AlreadyExists)),
            browser_request("session-1"),
            minted(500_000, 30, 0, "org_default"),
        )
        .await;
        let error = result.expect_err("a real conflict");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(broker.derive_requests.lock().expect("recorder").len(), 2);

        // Nothing was asked for, so there is nothing to look up.
        let (result, broker) = resolve_with_mint(
            session_broker(&["example.com"], Err(tonic::Code::AlreadyExists)),
            browser_request("session-1"),
            BudgetMint {
                enabled: true,
                minted: None,
            },
        )
        .await;
        assert_eq!(result.expect_err("a conflict").0, StatusCode::CONFLICT);
        assert_eq!(broker.derive_requests.lock().expect("recorder").len(), 1);
    }

    #[tokio::test]
    async fn a_looked_up_grant_is_still_checked_against_the_parents_bound() {
        // The broker hands back a grant wider than the parent's claim (it should
        // never be able to): the lookup path does not waive that check.
        let parent = ValidateGrantResponse {
            budget_micro_usd: 1_000_000,
            budget_source: "org_default".to_owned(),
            budget_minted_at_unix: 1_760_000_000,
            ..session_validation("session-1", &["example.com"])
        };
        let broker = TestBrowserBroker {
            derive: Some(Ok(derived_run("run-1", "session-1", &["example.com"]))),
            stateful_derive: true,
            ..TestBrowserBroker::validating(Ok(parent))
        };
        broker.issued.lock().expect("issued").insert(
            "browser-run-key".to_owned(),
            with_claim(derived_run("run-1", "session-1", &["example.com"]), 2_000_000, 0, 0),
        );
        let (result, _) = resolve_with_mint(
            broker,
            browser_request("session-1"),
            minted(500_000, 0, 0, "org_default"),
        )
        .await;
        assert_eq!(result.expect_err("wider than the parent").0, StatusCode::FORBIDDEN);
    }

    /// The organization's ceiling must reach a run that presents its own run grant.
    /// A member can derive a run grant at the broker with no claim, and that grant is
    /// never re-minted here, so the ceiling is applied to the run's parameters.
    #[tokio::test]
    async fn a_presented_run_grant_is_bounded_by_the_organizations_ceiling() {
        let run_grant = |claim_micro: i64, claim_steps: i32| ValidateGrantResponse {
            kind: "run".to_owned(),
            budget_micro_usd: claim_micro,
            budget_max_steps: claim_steps,
            budget_source: if claim_micro > 0 || claim_steps > 0 {
                "request".to_owned()
            } else {
                String::new()
            },
            ..session_validation("run-9", &["example.com"])
        };
        let ceiling = minted(500_000, 30, 0, "org_default");
        let request = || {
            let mut request = browser_request("run-9");
            request.max_cost_usd = Some(2.0);
            request.max_steps = Some(100);
            request
        };

        // A grant with no claim at all: the ceiling is the bound.
        let (result, broker) = resolve_with_mint(
            TestBrowserBroker::validating(Ok(run_grant(0, 0))),
            request(),
            ceiling.clone(),
        )
        .await;
        let grant = result.expect("a presented run grant");
        assert_eq!(
            grant.budget,
            Some(BudgetClaim {
                micro_usd: 500_000,
                max_steps: 30,
                max_runtime_s: 0
            })
        );
        assert_eq!(
            bounded_run_params(&request(), grant.budget.as_ref()),
            (Some(30), None, Some(0.5)),
            "the request's $2 and 100 steps are clipped to the ceiling"
        );
        assert!(
            broker.derive_requests.lock().expect("recorder").is_empty(),
            "a presented run grant is never re-minted"
        );

        // A grant whose own claim is tighter keeps it; where it is looser, the ceiling wins.
        let (result, _) = resolve_with_mint(
            TestBrowserBroker::validating(Ok(run_grant(100_000, 50))),
            request(),
            ceiling,
        )
        .await;
        assert_eq!(
            result.expect("a presented run grant").budget,
            Some(BudgetClaim {
                micro_usd: 100_000,
                max_steps: 30,
                max_runtime_s: 0
            })
        );

        // Flag off: nothing is applied, as before.
        let (result, _) = resolve_with_mint(
            TestBrowserBroker::validating(Ok(run_grant(0, 0))),
            request(),
            BudgetMint::default(),
        )
        .await;
        assert_eq!(result.expect("a presented run grant").budget, None);
    }

    #[tokio::test]
    async fn derive_refusals_map_to_403_409_400_429_502_and_503() {
        for (code, want_status, want_error) in [
            (
                tonic::Code::PermissionDenied,
                StatusCode::FORBIDDEN,
                "browser grant is not authorized",
            ),
            (
                tonic::Code::FailedPrecondition,
                StatusCode::CONFLICT,
                "browser_grant_not_derivable",
            ),
            (
                tonic::Code::AlreadyExists,
                StatusCode::CONFLICT,
                "browser_grant_not_derivable",
            ),
            (
                tonic::Code::InvalidArgument,
                StatusCode::BAD_REQUEST,
                "browser_grant_hosts_invalid",
            ),
            (
                tonic::Code::ResourceExhausted,
                StatusCode::TOO_MANY_REQUESTS,
                "browser_grant_limit_reached",
            ),
            (
                tonic::Code::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "browser grant validation unavailable",
            ),
            (
                tonic::Code::NotFound,
                StatusCode::FORBIDDEN,
                "browser grant is not authorized",
            ),
            (
                tonic::Code::Unimplemented,
                StatusCode::BAD_GATEWAY,
                "browser_grant_derive_unavailable",
            ),
            (
                tonic::Code::Internal,
                StatusCode::FORBIDDEN,
                "browser grant is not authorized",
            ),
        ] {
            let (result, _) = resolve_with(
                session_broker(&["example.com"], Err(code)),
                browser_request("session-1"),
            )
            .await;
            let error = result.expect_err("refused derive");
            assert_eq!(error.0, want_status, "{code:?}");
            assert_eq!(error.1 .0["error"], want_error, "{code:?}");
        }
    }

    #[tokio::test]
    async fn a_derived_grant_that_widens_or_mislabels_is_refused_even_from_the_broker() {
        let wider = derived_run("run-1", "session-1", &["evil.com"]);
        let sibling = derived_run("run-1", "session-1", &["badexample.com"]);
        let session_kind = DeriveRunGrantResponse {
            kind: "session".to_owned(),
            ..derived_run("run-1", "session-1", &["example.com"])
        };
        let other_parent = derived_run("run-1", "session-2", &["example.com"]);
        let parent_echo = derived_run("session-1", "session-1", &["example.com"]);
        let empty_id = derived_run("", "session-1", &["example.com"]);
        let empty_policy = derived_run("run-1", "session-1", &[]);
        let not_canonical = derived_run("run-1", "session-1", &["EXAMPLE.com"]);
        let zdr = DeriveRunGrantResponse {
            zdr: true,
            ..derived_run("z_run-1", "session-1", &["example.com"])
        };
        for (name, response) in [
            ("wider", wider),
            ("sibling", sibling),
            ("session kind", session_kind),
            ("other parent", other_parent),
            ("parent echoed", parent_echo),
            ("empty id", empty_id),
            ("empty policy", empty_policy),
            ("not canonical", not_canonical),
            ("zdr", zdr),
        ] {
            let mut request = browser_request("session-1");
            request.start_url = None;
            let (result, _) =
                resolve_with(session_broker(&["example.com"], Ok(response)), request).await;
            let error = result.expect_err(name);
            assert_eq!(error.0, StatusCode::FORBIDDEN, "{name}");
        }
    }

    #[tokio::test]
    async fn a_start_url_outside_the_derived_policy_is_refused() {
        let mut request = browser_request("session-1");
        request.start_url = Some("https://example.org/".to_owned());
        let (result, _) = resolve_with(
            session_broker(
                &["example.com", "example.org"],
                Ok(derived_run("run-1", "session-1", &["example.com"])),
            ),
            request,
        )
        .await;
        assert_eq!(result.expect_err("outside").0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn run_and_kindless_grants_keep_exact_equality_and_never_derive() {
        for kind in ["run", ""] {
            let validation = ValidateGrantResponse {
                kind: kind.to_owned(),
                ..session_validation("run-0", &["example.com", "example.org"])
            };
            let broker = TestBrowserBroker {
                derive: Some(Err(tonic::Code::Internal)),
                ..TestBrowserBroker::validating(Ok(validation))
            };
            let (ok, broker) = resolve_with(broker.clone(), browser_request("run-0")).await;
            let grant = ok.expect("exact grant");
            assert_eq!(grant.grant_id, "run-0");
            assert_eq!(grant.parent_grant_id, None);

            let mut narrower = browser_request("run-0");
            narrower.allowed_domains = Some(vec!["example.com".to_owned()]);
            let (error, broker) = resolve_with(broker, narrower).await;
            assert_eq!(
                error.expect_err("narrowing a run grant").0,
                StatusCode::BAD_REQUEST
            );
            assert!(
                broker.derive_requests.lock().expect("recorder").is_empty(),
                "kind {kind:?} must not derive"
            );
        }
    }

    #[tokio::test]
    async fn approval_unknown_and_zdr_grants_cannot_start_a_run() {
        for (kind, zdr, want) in [
            ("sensitive_approval", false, StatusCode::BAD_REQUEST),
            // WS8.C: a human_browse parent may seed a run, but only when the request
            // names its hosts (this request names none), so it is a 400 here. The
            // run-from-a-human_browse-parent paths are tested next to WS8.C.
            ("human_browse", false, StatusCode::BAD_REQUEST),
            ("session", true, StatusCode::PRECONDITION_FAILED),
            ("run", true, StatusCode::PRECONDITION_FAILED),
        ] {
            let validation = ValidateGrantResponse {
                kind: kind.to_owned(),
                zdr,
                ..session_validation("grant-x", &["example.com"])
            };
            let broker = TestBrowserBroker {
                derive: Some(Ok(derived_run("run-1", "grant-x", &["example.com"]))),
                ..TestBrowserBroker::validating(Ok(validation))
            };
            let (result, broker) = resolve_with(broker, browser_request("grant-x")).await;
            let error = result.expect_err(kind);
            assert_eq!(error.0, want, "{kind} zdr={zdr}");
            if zdr {
                // The refusal is the catalogue's typed one, not a bare sentence.
                assert_eq!(error.1 .0["error"]["code"], "zdr_feature_not_supported");
                assert_eq!(error.1 .0["error"]["feature"], "browser_run");
            }
            assert!(broker.derive_requests.lock().expect("recorder").is_empty());
        }
    }

    #[test]
    fn issuer_zdr_cannot_be_downgraded_by_browser_run_body() {
        let claims = browser_claims(true);

        assert!(browser_run_zdr_blocked(&claims, false).expect("a ZDR token decides"));
    }
    /// A Session Core stand-in that counts connections and drops each one, so
    /// any thread or run write fails and is visible as a connection.
    async fn counting_session_core() -> (
        tonic::transport::Channel,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind counting session core");
        let address = listener.local_addr().expect("counting session core address");
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        let handle = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("session core endpoint")
            .connect_lazy();
        (channel, connections, handle)
    }

    /// Calls the real `POST /v1/browser/runs` handler with `broker` behind
    /// the browser client and a counting Session Core behind the session and
    /// managed-run clients.
    async fn start_with(
        broker: TestBrowserBroker,
        request: BrowserRunStartRequest,
    ) -> (Result<Json<Value>, ApiError>, usize) {
        let (mut state, broker_handle) = state_with_browser_broker(broker).await;
        let (channel, connections, session_handle) = counting_session_core().await;
        state.session_client =
            mp_contracts::model_plane::v1::session_core_client::SessionCoreClient::new(
                channel.clone(),
            );
        state.managed_run_client =
            mp_contracts::model_plane::v1::managed_run_lifecycle_client::ManagedRunLifecycleClient::new(
                channel,
            );
        let result = browser_run_start(
            State(state),
            Extension(browser_claims(false)),
            Some(Extension(VerifiedBearer::for_test("data-plane-token"))),
            VerifiedExecutionBearer::for_test("execution-token"),
            VerifiedSessionBearer::for_test("session-token"),
            VerifiedInferenceBearer::for_test("inference-token"),
            VerifiedBrowserBearer::for_test("browser-user-token"),
            HeaderMap::new(),
            Json(request),
        )
        .await;
        broker_handle.abort();
        session_handle.abort();
        (result, connections.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn explicit_space_start_is_refused_before_any_grant_or_run_write() {
        let broker = session_broker(
            &["example.com"],
            Ok(derived_run("run-1", "session-1", &["example.com"])),
        );
        let mut request = browser_request("session-1");
        request.space_ref = Some(json!({ "space_id": "space-1" }));
        let (result, session_connections) = start_with(broker.clone(), request).await;
        let (status, Json(error)) = result.expect_err("Space authority is not implemented");
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(error["error"], "space_browser_run_unavailable");
        assert_eq!(session_connections, 0);
        assert!(broker.derive_requests.lock().expect("recorder").is_empty());
    }

    // Plan name: derive_failure_creates_no_thread_or_run. A refused derive
    // stops the start before Session Core is contacted at all.
    #[tokio::test]
    async fn derive_failure_creates_no_thread_or_run() {
        for code in [
            tonic::Code::FailedPrecondition,
            tonic::Code::PermissionDenied,
            tonic::Code::ResourceExhausted,
            tonic::Code::Unimplemented,
        ] {
            let broker = session_broker(&["example.com"], Err(code));
            let (result, session_connections) =
                start_with(broker.clone(), browser_request("session-1")).await;
            assert!(result.is_err(), "{code:?} must refuse the start");
            assert_eq!(
                broker.derive_requests.lock().expect("recorder").len(),
                1,
                "{code:?}: the derive was attempted"
            );
            assert_eq!(
                session_connections, 0,
                "{code:?}: no thread or run write may follow a refused derive"
            );
            assert!(broker.revoke_requests.lock().expect("recorder").is_empty());
        }
    }

    // A start that fails after the derive revokes the grant it derived, and
    // each POST is its own run with its own key, so a retry derives anew.
    #[tokio::test]
    async fn a_start_that_fails_after_the_derive_revokes_the_derived_grant() {
        let broker = session_broker(
            &["example.com"],
            Ok(derived_run("run-1", "session-1", &["example.com"])),
        );
        for _ in 0..2 {
            let (result, session_connections) =
                start_with(broker.clone(), browser_request("session-1")).await;
            let error = result.expect_err("session-core is down");
            assert_eq!(error.0, StatusCode::BAD_GATEWAY);
            assert!(
                session_connections > 0,
                "the derive succeeded, so the start reached Session Core"
            );
        }
        let revokes = broker.revoke_requests.lock().expect("recorder").clone();
        assert_eq!(revokes.len(), 2, "one revoke per failed start");
        for revoke in &revokes {
            assert_eq!(revoke.grant_id, "run-1");
            assert_eq!(revoke.reason, REVOKE_REASON_START_FAILED);
        }
        let derives = broker.derive_requests.lock().expect("recorder").clone();
        assert_eq!(derives.len(), 2);
        assert!(derives.iter().all(|d| d.session_key.starts_with("browser-")));
        assert_ne!(
            derives[0].session_key, derives[1].session_key,
            "a retried POST is a new run and derives under a new key"
        );
    }

    // A run grant the caller presented is not ours to revoke.
    #[tokio::test]
    async fn a_failed_start_never_revokes_a_presented_run_grant() {
        let validation = ValidateGrantResponse {
            kind: "run".to_owned(),
            ..session_validation("run-0", &["example.com"])
        };
        let broker = TestBrowserBroker::validating(Ok(validation));
        let (result, session_connections) =
            start_with(broker.clone(), browser_request("run-0")).await;
        assert_eq!(result.expect_err("session-core is down").0, StatusCode::BAD_GATEWAY);
        assert!(session_connections > 0);
        assert!(broker.revoke_requests.lock().expect("recorder").is_empty());
    }

    // ---- Idempotency-Key -------------------------------------------------

    fn key_headers(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            // The literal, not the constant: the wire name is the contract.
            headers.append(
                "idempotency-key",
                axum::http::HeaderValue::from_bytes(value.as_bytes()).expect("header value"),
            );
        }
        headers
    }

    #[test]
    fn the_idempotency_key_wire_name_is_the_one_the_gateway_forwards() {
        // The Verevon gateway forwards the literal "idempotency-key" (its own
        // test pins it, and scripts/tests/browser-grant-contract.test.mjs
        // checks both spellings agree). A rename here would make every retry a
        // new run with no failing test.
        assert_eq!(IDEMPOTENCY_KEY_HEADER, "idempotency-key");
        let mut headers = HeaderMap::new();
        headers.insert(
            "idempotency-key",
            axum::http::HeaderValue::from_static("retry-1"),
        );
        assert_eq!(
            parse_idempotency_key(&headers).unwrap().as_deref(),
            Some("retry-1")
        );
    }

    #[test]
    fn the_idempotency_key_header_is_optional_and_strictly_shaped() {
        assert_eq!(parse_idempotency_key(&HeaderMap::new()).unwrap(), None);
        for ok in [
            "a",
            "retry-1",
            "01J00000000000000000000000",
            "2f1c0c0e-9d3a-4f7e-8d0b-5a1f7b0c9a11",
            "a.b_c:d-e",
            "  padded  ",
        ] {
            let parsed = parse_idempotency_key(&key_headers(&[ok])).expect(ok);
            assert_eq!(parsed.as_deref(), Some(ok.trim()), "{ok}");
        }
        let too_long = "k".repeat(IDEMPOTENCY_KEY_MAX_LEN + 1);
        let at_limit = "k".repeat(IDEMPOTENCY_KEY_MAX_LEN);
        assert!(parse_idempotency_key(&key_headers(&[&at_limit])).is_ok());
        for bad in [
            "",
            "   ",
            "has space",
            "semi;colon",
            "quote\"d",
            "slash/y",
            "new\tline",
            "ünicode",
            too_long.as_str(),
        ] {
            let error = parse_idempotency_key(&key_headers(&[bad]))
                .expect_err(&format!("{bad:?} must be refused"));
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
            assert_eq!(error.1 .0["error"], "idempotency_key_invalid");
        }
        // Two headers are ambiguous, not "the first one".
        assert!(parse_idempotency_key(&key_headers(&["one", "two"])).is_err());
    }

    #[test]
    fn a_keyed_run_key_is_stable_scoped_and_never_carries_the_raw_key() {
        let base = keyed_run_key("org-a", "user-a", "session-1", "retry-1");
        assert_eq!(base, keyed_run_key("org-a", "user-a", "session-1", "retry-1"));
        assert!(base.starts_with("browser-idem-"));
        assert_eq!(base.len(), "browser-idem-".len() + 32);
        assert!(!base.contains("retry-1") && !base.contains("session-1"));
        for other in [
            keyed_run_key("org-b", "user-a", "session-1", "retry-1"),
            keyed_run_key("org-a", "user-b", "session-1", "retry-1"),
            keyed_run_key("org-a", "user-a", "session-2", "retry-1"),
            keyed_run_key("org-a", "user-a", "session-1", "retry-2"),
        ] {
            assert_ne!(other, base, "each scope part changes the run key");
        }
        // Length-prefixed parts: moving a character across a boundary differs.
        assert_ne!(
            keyed_run_key("org", "a", "bc", "k"),
            keyed_run_key("org", "ab", "c", "k")
        );
        assert_eq!(
            keyed_plan_id(&base),
            format!("plan-idem-{}", &base["browser-idem-".len()..])
        );
        assert!(keyed_plan_id(&base).len() < 64);
    }

    mod keyed_start {
        use super::*;
        use std::collections::HashMap;
        use std::future::Future;
        use std::sync::atomic::{AtomicBool, AtomicU64};
        use std::time::Duration;

        use mp_contracts::model_plane::v1::{
            managed_run_lifecycle_client::ManagedRunLifecycleClient,
            managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
            run_service_client::RunServiceClient,
            session_core_client::SessionCoreClient,
            AppendMessageRequest, AppendMessageResponse, CompleteDirectRunWithMessageRequest,
            CompleteDirectRunWithMessageResponse, CreateThreadRequest, CreateThreadResponse,
            GetRunRequest, HeartbeatManagedRunRequest, HeartbeatManagedRunResponse, RecordRunOutputRequest,
            RecordRunOutputResponse, RecordTerminalOutcomeRequest, RecordTerminalOutcomeResponse,
            RunDetail, StartManagedRunRequest, StartManagedRunResponse,
        };
        use tonic::codegen::{http, Body, BoxFuture, Service, StdError};

        /// One unary call answered by `handler`, framed the way tonic's own
        /// generated servers do. Lets the fakes below serve just the calls a
        /// test needs instead of every method of `SessionCore`.
        async fn serve_unary<Req, Resp, F, Fut, B>(
            request: http::Request<B>,
            handler: F,
        ) -> http::Response<tonic::body::BoxBody>
        where
            Req: prost::Message + Default + Send + 'static,
            Resp: prost::Message + Send + 'static,
            F: FnOnce(tonic::Request<Req>) -> Fut + Send + 'static,
            Fut: Future<Output = Result<tonic::Response<Resp>, Status>> + Send + 'static,
            B: Body + Send + 'static,
            B::Error: Into<StdError> + Send + 'static,
        {
            struct Once<F>(Option<F>);
            impl<Req, Resp, F, Fut> tonic::server::UnaryService<Req> for Once<F>
            where
                Req: prost::Message + Default + Send + 'static,
                Resp: prost::Message + Send + 'static,
                F: FnOnce(tonic::Request<Req>) -> Fut + Send + 'static,
                Fut: Future<Output = Result<tonic::Response<Resp>, Status>> + Send + 'static,
            {
                type Response = Resp;
                type Future = BoxFuture<tonic::Response<Resp>, Status>;
                fn call(&mut self, request: tonic::Request<Req>) -> Self::Future {
                    let handler = self.0.take().expect("one handler call per request");
                    Box::pin(handler(request))
                }
            }
            let mut grpc = tonic::server::Grpc::new(tonic::codec::ProstCodec::default());
            grpc.unary(Once(Some(handler)), request).await
        }

        fn unimplemented_rpc() -> http::Response<tonic::body::BoxBody> {
            http::Response::builder()
                .status(200)
                .header("grpc-status", tonic::Code::Unimplemented as i32)
                .header("content-type", "application/grpc")
                .body(tonic::codegen::empty_body())
                .expect("unimplemented response")
        }

        #[derive(Clone, Default)]
        struct FakeSession {
            creates: Arc<Mutex<Vec<CreateThreadRequest>>>,
            appends: Arc<Mutex<Vec<AppendMessageRequest>>>,
            append_fails: Arc<AtomicBool>,
            /// Milliseconds AppendMessage waits before answering: holds a
            /// request inside the start sequence so a test can drop it there.
            append_delay_ms: Arc<AtomicU64>,
        }

        impl tonic::server::NamedService for FakeSession {
            const NAME: &'static str = "model_plane.v1.SessionCore";
        }

        impl<B> Service<http::Request<B>> for FakeSession
        where
            B: Body + Send + 'static,
            B::Error: Into<StdError> + Send + 'static,
        {
            type Response = http::Response<tonic::body::BoxBody>;
            type Error = std::convert::Infallible;
            type Future = BoxFuture<Self::Response, Self::Error>;

            fn poll_ready(
                &mut self,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), Self::Error>> {
                std::task::Poll::Ready(Ok(()))
            }

            fn call(&mut self, request: http::Request<B>) -> Self::Future {
                let this = self.clone();
                match request.uri().path() {
                    // Idempotent like Session Core's: the same session key is
                    // the same thread.
                    "/model_plane.v1.SessionCore/CreateThread" => Box::pin(async move {
                        Ok(serve_unary(request, move |r: tonic::Request<CreateThreadRequest>| async move {
                            let r = r.into_inner();
                            let thread_id = format!("thread-{}", r.session_key);
                            this.creates.lock().expect("creates").push(r);
                            Ok(tonic::Response::new(CreateThreadResponse {
                                thread_id,
                                created_at: None,
                            }))
                        })
                        .await)
                    }),
                    "/model_plane.v1.SessionCore/AppendMessage" => Box::pin(async move {
                        Ok(serve_unary(request, move |r: tonic::Request<AppendMessageRequest>| async move {
                            let delay = this.append_delay_ms.load(Ordering::SeqCst);
                            if delay > 0 {
                                tokio::time::sleep(Duration::from_millis(delay)).await;
                            }
                            if this.append_fails.load(Ordering::SeqCst) {
                                return Err(Status::unavailable("append is down"));
                            }
                            this.appends.lock().expect("appends").push(r.into_inner());
                            Ok(tonic::Response::new(AppendMessageResponse { sequence: 1, ..Default::default() }))
                        })
                        .await)
                    }),
                    _ => Box::pin(async move { Ok(unimplemented_rpc()) }),
                }
            }
        }

        #[derive(Clone, Default)]
        struct FakeRuns {
            cancels: Arc<Mutex<Vec<CancelRunRequest>>>,
            /// The status GetRun reports; empty means "running".
            status: Arc<Mutex<String>>,
            get_fails: Arc<AtomicBool>,
            gets: Arc<AtomicUsize>,
        }

        impl tonic::server::NamedService for FakeRuns {
            const NAME: &'static str = "model_plane.v1.RunService";
        }

        impl<B> Service<http::Request<B>> for FakeRuns
        where
            B: Body + Send + 'static,
            B::Error: Into<StdError> + Send + 'static,
        {
            type Response = http::Response<tonic::body::BoxBody>;
            type Error = std::convert::Infallible;
            type Future = BoxFuture<Self::Response, Self::Error>;

            fn poll_ready(
                &mut self,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), Self::Error>> {
                std::task::Poll::Ready(Ok(()))
            }

            fn call(&mut self, request: http::Request<B>) -> Self::Future {
                let this = self.clone();
                match request.uri().path() {
                    "/model_plane.v1.RunService/CancelRun" => Box::pin(async move {
                        Ok(serve_unary(request, move |r: tonic::Request<CancelRunRequest>| async move {
                            this.cancels.lock().expect("cancels").push(r.into_inner());
                            Ok(tonic::Response::new(CancelRunResponse {
                                cancelled: true,
                                ..Default::default()
                            }))
                        })
                        .await)
                    }),
                    "/model_plane.v1.RunService/GetRun" => Box::pin(async move {
                        Ok(serve_unary(request, move |r: tonic::Request<GetRunRequest>| async move {
                            this.gets.fetch_add(1, Ordering::SeqCst);
                            if this.get_fails.load(Ordering::SeqCst) {
                                return Err(Status::unavailable("get_run is down"));
                            }
                            let mut status = this.status.lock().expect("status").clone();
                            if status.is_empty() {
                                status = "running".to_owned();
                            }
                            Ok(tonic::Response::new(RunDetail {
                                run_id: r.into_inner().run_id,
                                status,
                                ..Default::default()
                            }))
                        })
                        .await)
                    }),
                    _ => Box::pin(async move { Ok(unimplemented_rpc()) }),
                }
            }
        }

        /// Session Core's managed-run contract as far as this route uses it:
        /// idempotent on `start_key`, `AlreadyExists` when the immutable input
        /// (thread, goal) differs, a heartbeat that reports a terminal run.
        #[derive(Clone, Default)]
        struct FakeLifecycle {
            runs: Arc<Mutex<HashMap<String, (String, String, String)>>>,
            starts: Arc<Mutex<Vec<StartManagedRunRequest>>>,
            terminal: Arc<AtomicBool>,
            heartbeats: Arc<AtomicUsize>,
            /// StartManagedRun answers unavailable (Session Core down).
            start_fails: Arc<AtomicBool>,
        }

        #[tonic::async_trait]
        impl ManagedRunLifecycle for FakeLifecycle {
            async fn start_managed_run(
                &self,
                request: tonic::Request<StartManagedRunRequest>,
            ) -> Result<Response<StartManagedRunResponse>, Status> {
                let start = request.into_inner();
                if self.start_fails.load(Ordering::SeqCst) {
                    return Err(Status::unavailable("start is down"));
                }
                self.starts.lock().expect("starts").push(start.clone());
                let mut runs = self.runs.lock().expect("runs");
                if let Some((run_id, thread_id, goal)) = runs.get(&start.start_key) {
                    if *goal != start.goal || *thread_id != start.thread_id {
                        return Err(Status::already_exists(
                            "managed start key is bound to different immutable input",
                        ));
                    }
                    return Ok(Response::new(StartManagedRunResponse {
                        run_id: run_id.clone(),
                        created_at: None,
                        terminal_step_id: "model-gateway-browser-final".to_owned(),
                        already_started: true,
                        thread_id: thread_id.clone(),
                    }));
                }
                let run_id = format!("managed-run-{}", runs.len() + 1);
                runs.insert(
                    start.start_key.clone(),
                    (run_id.clone(), start.thread_id.clone(), start.goal.clone()),
                );
                Ok(Response::new(StartManagedRunResponse {
                    run_id,
                    created_at: None,
                    terminal_step_id: "model-gateway-browser-final".to_owned(),
                    already_started: false,
                    thread_id: start.thread_id,
                }))
            }

            async fn record_terminal_outcome(
                &self,
                _: tonic::Request<RecordTerminalOutcomeRequest>,
            ) -> Result<Response<RecordTerminalOutcomeResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }

            async fn heartbeat_managed_run(
                &self,
                _: tonic::Request<HeartbeatManagedRunRequest>,
            ) -> Result<Response<HeartbeatManagedRunResponse>, Status> {
                self.heartbeats.fetch_add(1, Ordering::SeqCst);
                Ok(Response::new(HeartbeatManagedRunResponse {
                    renewed_until: None,
                    already_terminal: self.terminal.load(Ordering::SeqCst),
                }))
            }

            async fn record_run_output(
                &self,
                _: tonic::Request<RecordRunOutputRequest>,
            ) -> Result<Response<RecordRunOutputResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }

            async fn complete_direct_run_with_message(
                &self,
                _: tonic::Request<CompleteDirectRunWithMessageRequest>,
            ) -> Result<Response<CompleteDirectRunWithMessageResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }
        }

        /// Counts the dispatches that reach Execution Core.
        #[derive(Clone, Default)]
        struct FakeExecution {
            executes: Arc<Mutex<Vec<ExecuteStepRequest>>>,
            restricted_unimplemented: Arc<AtomicBool>,
            capabilities_unimplemented: Arc<AtomicBool>,
            capability_bearer: Arc<Mutex<Option<String>>>,
        }

        #[tonic::async_trait]
        impl ExecutionCore for FakeExecution {
    async fn execute_restricted_browser_step(
        &self,
        request: tonic::Request<mp_contracts::model_plane::v1::ExecuteStepRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ExecuteStepResponse>, tonic::Status> {
        if self.restricted_unimplemented.load(Ordering::SeqCst) {
            Err(Status::unimplemented("older execution deployment"))
        } else {
            self.execute_step(request).await
        }
    }

    async fn browser_agent_capabilities(
        &self,
        request: tonic::Request<mp_contracts::model_plane::v1::BrowserAgentCapabilitiesRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::BrowserAgentCapabilitiesResponse>, tonic::Status> {
        if self.capabilities_unimplemented.load(Ordering::SeqCst) {
            return Err(Status::unimplemented("older execution deployment"));
        }
        *self.capability_bearer.lock().unwrap() = request.metadata().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_owned);
        Ok(tonic::Response::new(mp_contracts::model_plane::v1::BrowserAgentCapabilitiesResponse { attached_policy_protocol: 1 }))
    }

            async fn execute_step(
                &self,
                request: tonic::Request<ExecuteStepRequest>,
            ) -> Result<Response<ExecuteStepResponse>, Status> {
                self.executes
                    .lock()
                    .expect("executes")
                    .push(request.into_inner());
                Err(Status::unavailable("the fake executes nothing"))
            }

            async fn execute_scheduled_step(
                &self,
                _: tonic::Request<mp_contracts::model_plane::v1::ExecuteScheduledStepRequest>,
            ) -> Result<
                Response<mp_contracts::model_plane::v1::ExecuteScheduledStepResponse>,
                Status,
            > {
                Err(Status::unimplemented("not used"))
            }

            async fn resume_run(
                &self,
                _: tonic::Request<ResumeRunRequest>,
            ) -> Result<Response<ResumeRunResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }

            async fn cancel_run(
                &self,
                _: tonic::Request<CancelRunRequest>,
            ) -> Result<Response<CancelRunResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }

            async fn pause_run(
                &self,
                _: tonic::Request<PauseRunRequest>,
            ) -> Result<Response<PauseRunResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }

            async fn run_agent(
                &self,
                _: tonic::Request<RunAgentRequest>,
            ) -> Result<Response<RunAgentResponse>, Status> {
                Err(Status::unimplemented("not used"))
            }
        }

        struct Harness {
            state: AppState,
            broker: TestBrowserBroker,
            session: FakeSession,
            runs: FakeRuns,
            lifecycle: FakeLifecycle,
            execution: FakeExecution,
            _tasks: Vec<tokio::task::JoinHandle<()>>,
            _auth: wiremock::MockServer,
        }

        impl Harness {
            /// `with_tokens`: the gateway's service credential for the managed
            /// heartbeat is configured (a fake Auth Core mints it).
            async fn new(with_tokens: bool) -> Self {
                let broker = session_broker(
                    &["example.com"],
                    Ok(derived_run("run-1", "session-1", &["example.com"])),
                );
                Self::with_broker(with_tokens, broker).await
            }

            async fn with_broker(with_tokens: bool, broker: TestBrowserBroker) -> Self {
                let (mut state, broker_task) = state_with_browser_broker(broker.clone()).await;

                let session = FakeSession::default();
                let runs = FakeRuns::default();
                let lifecycle = FakeLifecycle::default();
                let execution = FakeExecution::default();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind fake session core");
                let address = listener.local_addr().expect("fake session core address");
                let (s, r, l, e) = (
                    session.clone(),
                    runs.clone(),
                    lifecycle.clone(),
                    execution.clone(),
                );
                let server = tokio::spawn(async move {
                    let _ = tonic::transport::Server::builder()
                        .add_service(s)
                        .add_service(r)
                        .add_service(ManagedRunLifecycleServer::new(l))
                        .add_service(ExecutionCoreServer::new(e))
                        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(
                            listener,
                        ))
                        .await;
                });
                let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
                    .expect("fake endpoint")
                    .connect_lazy();
                state.session_client = SessionCoreClient::new(channel.clone());
                state.managed_run_client = ManagedRunLifecycleClient::new(channel.clone());
                state.run_client = RunServiceClient::new(channel.clone());
                state.execution_client = ExecutionCoreClient::new(channel);

                let auth = wiremock::MockServer::start().await;
                wiremock::Mock::given(wiremock::matchers::method("POST"))
                    .and(wiremock::matchers::path("/api/session-core/internal-token"))
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                        "token": "heartbeat-token",
                        "expiresInSeconds": 300,
                        "audience": "session-core",
                    })))
                    .mount(&auth)
                    .await;
                if with_tokens {
                    state.session_terminal_tokens = Some(Arc::new(
                        crate::session_terminal_auth::SessionTerminalTokenProvider::from_service_credential(
                            &auth.uri(),
                            "model-gateway".to_owned(),
                            "secret".to_owned(),
                        )
                        .expect("token provider"),
                    ));
                }
                Self {
                    state,
                    broker,
                    session,
                    runs,
                    lifecycle,
                    execution,
                    _tasks: vec![broker_task, server],
                    _auth: auth,
                }
            }

            /// The request as an owned future, so a test can spawn it and
            /// abort it the way a client disconnect drops the handler.
            fn start_future(
                &self,
                key: Option<&str>,
                request: BrowserRunStartRequest,
            ) -> impl Future<Output = Result<Json<Value>, ApiError>> + Send + 'static {
                let mut headers = HeaderMap::new();
                if let Some(key) = key {
                    // The literal wire name, not IDEMPOTENCY_KEY_HEADER.
                    headers.insert(
                        "idempotency-key",
                        axum::http::HeaderValue::from_str(key).expect("key header"),
                    );
                }
                browser_run_start(
                    State(self.state.clone()),
                    Extension(browser_claims(false)),
                    Some(Extension(VerifiedBearer::for_test("data-plane-token"))),
                    VerifiedExecutionBearer::for_test("execution-token"),
                    VerifiedSessionBearer::for_test("session-token"),
                    VerifiedInferenceBearer::for_test("inference-token"),
                    VerifiedBrowserBearer::for_test("browser-user-token"),
                    headers,
                    Json(request),
                )
            }

            async fn start(
                &self,
                key: Option<&str>,
                request: BrowserRunStartRequest,
            ) -> Result<Json<Value>, ApiError> {
                self.start_future(key, request).await
            }

            fn derive_keys(&self) -> Vec<String> {
                self.broker
                    .derive_requests
                    .lock()
                    .expect("derives")
                    .iter()
                    .map(|derive| derive.session_key.clone())
                    .collect()
            }

            fn dispatches(&self) -> Vec<ExecuteStepRequest> {
                self.execution.executes.lock().expect("executes").clone()
            }

            fn revokes(&self) -> usize {
                self.broker.revoke_requests.lock().expect("revokes").len()
            }
        }

        async fn eventually(mut condition: impl FnMut() -> bool) -> bool {
            for _ in 0..80 {
                if condition() {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            false
        }

        #[tokio::test]
        async fn restricted_dispatch_never_falls_back_to_legacy_rpc() {
            let h = Harness::new(true).await;
            h.execution.restricted_unimplemented.store(true, Ordering::SeqCst);
            let error = dispatch_browser_step(&mut h.state.execution_client.clone(), tonic::Request::new(ExecuteStepRequest::default()), true)
                .await.expect_err("an older execution instance must refuse the restricted RPC");
            assert_eq!(error.code(), tonic::Code::Unimplemented);
            assert!(h.dispatches().is_empty(), "the legacy execution RPC must not run");
        }

        #[tokio::test]
        async fn capability_probe_forwards_execution_credential_and_refuses_old_server() {
            let h = Harness::new(true).await;
            let bearer = VerifiedExecutionBearer::for_test("execution-token");
            verify_attached_policy_protocol(&h.state, "org-a", &bearer).await.expect("protocol supported");
            assert_eq!(h.execution.capability_bearer.lock().unwrap().as_deref(), Some("Bearer execution-token"));
            h.execution.capabilities_unimplemented.store(true, Ordering::SeqCst);
            let failure = verify_attached_policy_protocol(&h.state, "org-a", &bearer).await.expect_err("old server is unavailable");
            assert_eq!(failure.0, StatusCode::SERVICE_UNAVAILABLE);
        }

        fn run_key() -> String {
            keyed_run_key("org-a", "user-a", "session-1", "retry-1")
        }

        #[tokio::test]
        async fn a_keyed_start_names_the_grant_thread_and_run_by_one_key_and_dispatches_once() {
            let h = Harness::new(true).await;
            let Json(body) = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("keyed start");

            assert_eq!(body["run_id"], "managed-run-1");
            assert_eq!(body["thread_id"], format!("thread-{}", run_key()));
            assert_eq!(body["plan_id"], keyed_plan_id(&run_key()));
            assert!(body.get("idempotent_replay").is_none());

            // One key, three places: the broker's session_key, the thread's
            // session key and the managed run's start key.
            assert_eq!(h.derive_keys(), [run_key()]);
            let creates = h.session.creates.lock().expect("creates").clone();
            assert_eq!(creates.len(), 1);
            assert_eq!(creates[0].session_key, run_key());
            let starts = h.lifecycle.starts.lock().expect("starts").clone();
            assert_eq!(starts.len(), 1);
            assert_eq!(starts[0].start_key, run_key());
            assert!(!run_key().contains("retry-1"), "the raw key is never forwarded");
            assert_eq!(
                h.session.appends.lock().expect("appends").len(),
                1,
                "the prompt is appended once, after the run exists"
            );

            assert!(eventually(|| h.dispatches().len() == 1).await, "one dispatch");
            let dispatched = h.dispatches();
            let tool_input: Value = serde_json::from_str(&dispatched[0].tool_input).unwrap();
            assert_eq!(tool_input["grant_id"], "run-1");
            assert_eq!(tool_input["plan_id"], body["plan_id"]);
            assert_eq!(dispatched[0].run_id, "managed-run-1");
            assert_eq!(h.revokes(), 0);
        }

        /// WS6.E: with the claim on, the numbers in the dispatched tool input are
        /// the request's tightened by the grant's claim; with it off they are the
        /// request's own. (The unit test of `bounded_run_params` does not show the
        /// handler passes its answer on.)
        #[tokio::test]
        async fn the_claim_tightens_the_numbers_in_the_dispatched_tool_input() {
            let parent = ValidateGrantResponse {
                budget_micro_usd: 1_000_000,
                budget_max_steps: 10,
                budget_max_runtime_s: 120,
                budget_source: "org_default".to_owned(),
                budget_minted_at_unix: 1_760_000_000,
                ..session_validation("session-1", &["example.com"])
            };
            let broker = TestBrowserBroker {
                derive: Some(Ok(with_claim(
                    derived_run("run-1", "session-1", &["example.com"]),
                    1_000_000,
                    10,
                    120,
                ))),
                ..TestBrowserBroker::validating(Ok(parent))
            };
            for (flag, want) in [
                (true, (json!(10), json!(120), json!(1.0))),
                (false, (json!(50), json!(900), json!(5.0))),
            ] {
                let mut h = Harness::with_broker(true, broker.clone()).await;
                h.state.browser_budget_claim = Some(flag);
                let mut request = browser_request("session-1");
                request.max_steps = Some(50);
                request.max_runtime_s = Some(900);
                request.max_cost_usd = Some(5.0);
                let Json(_) = h.start(Some("retry-1"), request).await.expect("a start");
                assert!(eventually(|| h.dispatches().len() == 1).await, "one dispatch");
                let tool_input: Value =
                    serde_json::from_str(&h.dispatches()[0].tool_input).unwrap();
                assert_eq!(
                    (
                        tool_input["max_steps"].clone(),
                        tool_input["max_runtime_s"].clone(),
                        tool_input["max_cost_usd"].clone()
                    ),
                    want,
                    "claim flag {flag}"
                );
            }
        }

        /// A keyed retry whose claim moved (the request now names a different cost)
        /// still answers the same run: the derive replay is looked up, not refused.
        #[tokio::test]
        async fn a_keyed_retry_whose_claim_moved_still_answers_the_same_run() {
            let mut broker = session_broker(
                &["example.com"],
                Ok(derived_run("run-1", "session-1", &["example.com"])),
            );
            broker.stateful_derive = true;
            let mut h = Harness::with_broker(true, broker).await;
            h.state.browser_budget_claim = Some(true);
            let mut first = browser_request("session-1");
            first.max_cost_usd = Some(0.5);
            let Json(started) = h.start(Some("retry-1"), first).await.expect("first start");
            assert!(eventually(|| h.dispatches().len() == 1).await);

            let mut retry = browser_request("session-1");
            retry.max_cost_usd = Some(2.0);
            let Json(replay) = h
                .start(Some("retry-1"), retry)
                .await
                .expect("the retry must find its run, not 409 on the grant");
            assert_eq!(replay["run_id"], started["run_id"]);
            assert_eq!(replay["idempotent_replay"], true);
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(h.dispatches().len(), 1, "a replay never dispatches");
            assert_eq!(h.revokes(), 0);
            // first derive, the retry's derive (refused as a different claim), the lookup
            assert_eq!(h.derive_keys(), [run_key(), run_key(), run_key()]);
        }

        #[tokio::test]
        async fn a_retry_with_the_same_key_returns_the_same_run_and_grant_and_does_nothing_else() {
            let h = Harness::new(true).await;
            let Json(first) = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("first start");
            assert!(eventually(|| h.dispatches().len() == 1).await);

            let Json(retry) = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("the retry answers the same run");
            assert_eq!(retry["run_id"], first["run_id"]);
            assert_eq!(retry["thread_id"], first["thread_id"]);
            assert_eq!(retry["plan_id"], first["plan_id"]);
            assert_eq!(retry["idempotent_replay"], true);

            // The same derived grant: the broker saw the same key twice.
            assert_eq!(h.derive_keys(), [run_key(), run_key()]);
            // Nothing else happened: no second prompt, no second dispatch, and
            // the grant the live run uses was not revoked.
            assert_eq!(h.session.appends.lock().expect("appends").len(), 1);
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(h.dispatches().len(), 1, "a replay never dispatches");
            assert_eq!(h.revokes(), 0);
            assert_eq!(h.lifecycle.starts.lock().expect("starts").len(), 2);
            // The replay read the run and did not renew its lease: the only
            // heartbeat is the first start's own.
            assert_eq!(h.runs.gets.load(Ordering::SeqCst), 1);
            assert_eq!(
                h.lifecycle.heartbeats.load(Ordering::SeqCst),
                1,
                "a replay never renews the run's lease"
            );
        }

        #[tokio::test]
        async fn another_key_or_no_key_is_another_run() {
            let h = Harness::new(true).await;
            let Json(first) = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("first");
            let Json(second) = h
                .start(Some("retry-2"), browser_request("session-1"))
                .await
                .expect("second key");
            let Json(unkeyed) = h
                .start(None, browser_request("session-1"))
                .await
                .expect("no key");
            let ids = [&first, &second, &unkeyed].map(|body| body["run_id"].clone());
            assert_eq!(ids, ["managed-run-1", "managed-run-2", "managed-run-3"]);
            let keys = h.derive_keys();
            assert_eq!(keys.len(), 3);
            assert_eq!(
                keys.iter().collect::<std::collections::BTreeSet<_>>().len(),
                3,
                "three starts, three grant keys"
            );
            assert!(keys[2].starts_with("browser-") && !keys[2].starts_with("browser-idem-"));
            assert!(eventually(|| h.dispatches().len() == 3).await);
        }

        #[tokio::test]
        async fn the_same_key_with_a_different_goal_is_refused_and_leaves_the_live_run_alone() {
            let h = Harness::new(true).await;
            let _ = h.start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("first");
            let mut other = browser_request("session-1");
            other.goal = "a different goal".to_owned();
            let error = h
                .start(Some("retry-1"), other)
                .await
                .expect_err("a reused key");
            assert_eq!(error.0, StatusCode::CONFLICT);
            assert_eq!(error.1 .0["error"], "idempotency_key_reused");
            assert_eq!(h.revokes(), 0, "the first run's grant is untouched");
            assert_eq!(h.session.appends.lock().expect("appends").len(), 1);
            assert!(eventually(|| h.dispatches().len() == 1).await);
        }

        #[tokio::test]
        async fn a_replay_of_a_run_that_ended_is_409_not_a_working_run() {
            let h = Harness::new(true).await;
            let _ = h.start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("first");
            *h.runs.status.lock().expect("status") = "cancelled".to_owned();
            let error = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect_err("the run has ended");
            assert_eq!(error.0, StatusCode::CONFLICT);
            assert_eq!(error.1 .0["error"], "idempotency_key_run_ended");
            assert_eq!(error.1 .0["run_id"], "managed-run-1");
            assert_eq!(h.revokes(), 0);
        }

        #[tokio::test]
        async fn a_replay_whose_liveness_cannot_be_checked_is_503_and_dispatches_nothing() {
            let h = Harness::new(true).await;
            let _ = h.start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("first");
            assert!(eventually(|| h.dispatches().len() == 1).await);
            h.runs.get_fails.store(true, Ordering::SeqCst);
            let error = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect_err("liveness unknown");
            assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.1 .0["error"], "idempotency_replay_unverified");
            // A status this code does not know is not guessed either.
            h.runs.get_fails.store(false, Ordering::SeqCst);
            *h.runs.status.lock().expect("status") = "some_new_status".to_owned();
            let error = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect_err("unknown status");
            assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.1 .0["error"], "idempotency_replay_unverified");
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(h.dispatches().len(), 1);
            assert_eq!(h.revokes(), 0);
        }

        #[tokio::test]
        async fn a_keyed_start_that_cannot_be_dispatched_ends_its_own_run_and_grant() {
            // No service credential: the first liveness heartbeat fails.
            let h = Harness::new(false).await;
            let error = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect_err("no liveness");
            assert_eq!(error.0, StatusCode::BAD_GATEWAY);
            let cancels = h.runs.cancels.lock().expect("cancels").clone();
            assert_eq!(cancels.len(), 1, "the undispatched run is cancelled");
            assert_eq!(cancels[0].run_id, "managed-run-1");
            assert_eq!(cancels[0].reason, "browser_run_start_failed");
            assert_eq!(h.revokes(), 1, "and its grant is ended");
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(h.dispatches().is_empty());
        }

        #[tokio::test]
        async fn a_keyed_start_whose_prompt_cannot_be_saved_cancels_the_new_run_and_keeps_the_grant() {
            let h = Harness::new(true).await;
            h.session.append_fails.store(true, Ordering::SeqCst);
            let error = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect_err("append down");
            assert_eq!(error.0, StatusCode::BAD_GATEWAY);
            let cancels = h.runs.cancels.lock().expect("cancels").clone();
            assert_eq!(cancels.len(), 1);
            assert_eq!(cancels[0].run_id, "managed-run-1");
            // The grant stays: this could be a retry of a live run, and a
            // same-key retry needs it. The broker bounds it per parent.
            assert_eq!(h.revokes(), 0);
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(h.dispatches().is_empty());
        }

        #[tokio::test]
        async fn a_client_that_disconnects_mid_start_still_gets_its_run_dispatched() {
            let h = Harness::new(true).await;
            // Hold the start inside the prompt append, after the managed run exists.
            h.session.append_delay_ms.store(400, Ordering::SeqCst);
            let task = tokio::spawn(h.start_future(Some("retry-1"), browser_request("session-1")));
            assert!(
                eventually(|| !h.lifecycle.starts.lock().expect("starts").is_empty()).await,
                "the managed run was started"
            );
            // The client disconnects: axum drops the handler future here.
            task.abort();
            assert!(task.await.expect_err("aborted").is_cancelled());

            assert!(
                eventually(|| h.dispatches().len() == 1).await,
                "the detached start ran on to its dispatch"
            );
            assert_eq!(h.session.appends.lock().expect("appends").len(), 1);

            // The client's retry is a replay of a run that really is working.
            h.session.append_delay_ms.store(0, Ordering::SeqCst);
            let Json(retry) = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("retry");
            assert_eq!(retry["idempotent_replay"], true);
            assert_eq!(retry["run_id"], "managed-run-1");
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert_eq!(h.dispatches().len(), 1);
            assert_eq!(h.session.appends.lock().expect("appends").len(), 1);
            assert_eq!(h.revokes(), 0);
        }

        #[tokio::test]
        async fn a_keyed_start_session_core_cannot_begin_keeps_the_grant_and_the_retry_starts_the_run() {
            let h = Harness::new(true).await;
            h.lifecycle.start_fails.store(true, Ordering::SeqCst);
            let error = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect_err("session core is down");
            assert_eq!(error.0, StatusCode::BAD_GATEWAY);
            // The run is not known to exist, so nothing is cancelled or
            // appended, and the grant is kept for the same key's retry.
            assert_eq!(h.revokes(), 0);
            assert!(h.runs.cancels.lock().expect("cancels").is_empty());
            assert!(h.session.appends.lock().expect("appends").is_empty());
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(h.dispatches().is_empty());

            h.lifecycle.start_fails.store(false, Ordering::SeqCst);
            let Json(body) = h
                .start(Some("retry-1"), browser_request("session-1"))
                .await
                .expect("the retry starts the run");
            assert_eq!(body["run_id"], "managed-run-1");
            assert!(body.get("idempotent_replay").is_none(), "a new run, not a replay");
            assert_eq!(h.derive_keys(), [run_key(), run_key()], "the same derived grant");
            assert!(eventually(|| h.dispatches().len() == 1).await);
            assert_eq!(h.session.appends.lock().expect("appends").len(), 1);
            assert_eq!(h.revokes(), 0);
        }

        #[tokio::test]
        async fn a_malformed_key_or_a_thread_id_is_refused_before_any_broker_or_session_call() {
            let h = Harness::new(true).await;
            let error = h
                .start(Some("bad key!"), browser_request("session-1"))
                .await
                .expect_err("malformed key");
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
            assert_eq!(error.1 .0["error"], "idempotency_key_invalid");

            let mut with_thread = browser_request("session-1");
            with_thread.thread_id = Some("thread-existing".to_owned());
            let error = h
                .start(Some("retry-1"), with_thread)
                .await
                .expect_err("keyed start owns its thread");
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
            assert_eq!(error.1 .0["error"], "idempotency_key_requires_a_new_thread");

            assert!(h.derive_keys().is_empty(), "no broker call");
            assert!(h.session.creates.lock().expect("creates").is_empty());
            assert!(h.lifecycle.starts.lock().expect("starts").is_empty());
        }

        /// D1-019 limit c: a keyed retry that changes a limit, the stop criteria or
        /// the start page is not a replay of the first request; it is a 409 that
        /// reaches Session Core and the broker no more than the first start did.
        #[tokio::test]
        async fn a_keyed_retry_with_a_changed_body_is_a_409_and_the_same_body_still_replays() {
            let h = Harness::new(true).await;
            let mut first = browser_request("session-1");
            first.max_steps = Some(20);
            first.stop_criteria = Some("a receipt is shown".to_owned());
            h.start(Some("retry-1"), first.clone()).await.expect("first start");
            assert!(eventually(|| h.dispatches().len() == 1).await, "one dispatch");

            let changes: [fn(&mut BrowserRunStartRequest); 5] = [
                |r| r.max_steps = Some(200),
                |r| r.stop_criteria = Some("something else".to_owned()),
                |r| r.start_url = Some("https://example.com/other".to_owned()),
                |r| r.profile_id = Some("profile-1".to_owned()),
                |r| r.goal = "another goal".to_owned(),
            ];
            for change in changes {
                let mut retry = first.clone();
                change(&mut retry);
                let error = h
                    .start(Some("retry-1"), retry)
                    .await
                    .expect_err("a changed body under the same key");
                assert_eq!(error.0, StatusCode::CONFLICT);
                assert_eq!(error.1 .0["error"], "idempotency_key_reused");
            }
            assert_eq!(h.derive_keys().len(), 1, "no second derive");
            assert_eq!(h.lifecycle.starts.lock().expect("starts").len(), 1);
            assert_eq!(h.dispatches().len(), 1, "no second dispatch");

            let Json(replay) = h
                .start(Some("retry-1"), first)
                .await
                .expect("the identical body is a replay");
            assert_eq!(replay["idempotent_replay"], true);
        }

        /// D1-001: a run grant the caller presents backs one live run. The broker's
        /// record of it carries no session key, so the gateway holds the claim; a
        /// second run with the same grant is refused before anything is created.
        #[tokio::test]
        async fn a_second_run_with_the_same_presented_run_grant_is_refused() {
            let run_grant = ValidateGrantResponse {
                kind: "run".to_owned(),
                parent_grant_id: "session-1".to_owned(),
                ..session_validation("run-9", &["example.com"])
            };
            let h = Harness::with_broker(true, TestBrowserBroker::validating(Ok(run_grant))).await;
            h.start(None, browser_request("run-9")).await.expect("first run");
            assert!(eventually(|| h.dispatches().len() == 1).await, "one dispatch");

            let error = h
                .start(None, browser_request("run-9"))
                .await
                .expect_err("the grant already backs a live run");
            assert_eq!(error.0, StatusCode::CONFLICT);
            assert_eq!(error.1 .0["error"], "browser_grant_in_use");
            assert_eq!(h.lifecycle.starts.lock().expect("starts").len(), 1);
            assert_eq!(h.dispatches().len(), 1);
            assert_eq!(h.revokes(), 0, "a presented grant is never revoked here");
        }

        /// D1-001 durable: a run grant derived for an earlier run is refused by a start
        /// with an empty guard (a restart, another replica, the first run long over).
        #[tokio::test]
        async fn a_run_grant_derived_for_an_earlier_run_is_refused_with_an_empty_guard() {
            let run_grant = ValidateGrantResponse {
                kind: "run".to_owned(),
                parent_grant_id: "session-1".to_owned(),
                session_key: "browser-01J0EARLIERRUN".to_owned(),
                ..session_validation("run-9", &["example.com"])
            };
            let h = Harness::with_broker(true, TestBrowserBroker::validating(Ok(run_grant))).await;
            let error = h
                .start(None, browser_request("run-9"))
                .await
                .expect_err("the grant belongs to another run");
            assert_eq!(error.0, StatusCode::CONFLICT);
            assert_eq!(error.1 .0["error"], "browser_grant_in_use");
            assert!(h.lifecycle.starts.lock().expect("starts").is_empty());
            assert!(h.dispatches().is_empty());
            assert_eq!(h.revokes(), 0);
        }

        /// A keyed retry of the run that holds a presented run grant is the same
        /// run, not a second one.
        #[tokio::test]
        async fn a_keyed_retry_of_the_run_holding_a_presented_run_grant_is_not_refused() {
            let run_grant = ValidateGrantResponse {
                kind: "run".to_owned(),
                parent_grant_id: "session-1".to_owned(),
                ..session_validation("run-9", &["example.com"])
            };
            let h = Harness::with_broker(true, TestBrowserBroker::validating(Ok(run_grant))).await;
            h.start(Some("retry-1"), browser_request("run-9"))
                .await
                .expect("first keyed run");
            let Json(replay) = h
                .start(Some("retry-1"), browser_request("run-9"))
                .await
                .expect("the retry of the holder");
            assert_eq!(replay["idempotent_replay"], true);
        }

        /// WS8.G: an attach start derives the attach grant, then runs under it; what
        /// reaches execution-core is that grant and the session id, with no profile.
        #[tokio::test]
        async fn attach_tool_input_carries_attach_grant_and_session_id() {
            std::env::set_var("MODEL_GATEWAY_BROWSER_ATTACH", "1");
            let broker = session_broker(
                &["example.com"],
                Ok(DeriveRunGrantResponse {
                    kind: "attach".to_owned(),
                    resource_id: ATTACH_SESSION.to_owned(),
                    ..derived_run("att-1", "session-1", &["example.com"])
                }),
            );
            let h = Harness::with_broker(true, broker).await;
            let mut request = attach_request("session-1", &["example.com"]);
            request.profile_id = Some("profile-1".to_owned());
            request.start_url = Some("https://example.com/start".to_owned());
            let Json(body) = h.start(Some("retry-1"), request).await.expect("attach start");
            assert_eq!(body["run_id"], "managed-run-1");

            let derives = h.broker.derive_requests.lock().expect("derives").clone();
            assert_eq!(derives.len(), 1);
            assert_eq!(derives[0].purpose, "attach");
            assert_eq!(derives[0].resource_id, ATTACH_SESSION);

            assert!(eventually(|| h.dispatches().len() == 1).await, "one dispatch");
            let dispatched = h.dispatches();
            let tool_input: Value = serde_json::from_str(&dispatched[0].tool_input).unwrap();
            assert_eq!(tool_input["grant_id"], "att-1", "the derived attach grant");
            assert_eq!(tool_input["attach_session_id"], ATTACH_SESSION);
            assert_eq!(tool_input["profile_id"], Value::Null);
            assert_eq!(tool_input["allowed_domains"], json!(["example.com"]));
            assert_eq!(tool_input["run_id"], "managed-run-1");
            assert_eq!(dispatched[0].tool_name, "browser_agent");
        }
    }

    #[test]
    fn the_start_guard_claims_releases_and_expires() {
        let guard = RunStartGuard::default();
        let t0 = Instant::now();
        assert_eq!(guard.claim_grant_at("g", "run-a", t0), GrantClaim::Fresh);
        assert_eq!(guard.claim_grant_at("g", "run-a", t0), GrantClaim::Held);
        assert_eq!(guard.claim_grant_at("g", "run-b", t0), GrantClaim::Refused);
        guard.release_grant("g");
        assert_eq!(guard.claim_grant_at("g", "run-b", t0), GrantClaim::Fresh);
        let later = t0 + PRESENTED_GRANT_CLAIM_TTL + Duration::from_secs(1);
        assert_eq!(
            guard.claim_grant_at("g", "run-c", later),
            GrantClaim::Fresh,
            "an unobserved run's claim lapses"
        );
        assert_eq!(guard.check_fingerprint_at("k", "f1", t0), FingerprintCheck::Unknown);
        guard.remember_fingerprint_at("k", "f1", t0);
        assert_eq!(guard.check_fingerprint_at("k", "f1", t0), FingerprintCheck::Match);
        assert_eq!(guard.check_fingerprint_at("k", "f2", t0), FingerprintCheck::Mismatch);
        let expired = t0 + START_FINGERPRINT_TTL + Duration::from_secs(1);
        assert_eq!(guard.check_fingerprint_at("k", "f2", expired), FingerprintCheck::Unknown);
    }

    #[test]
    fn the_request_fingerprint_follows_every_field_that_shapes_the_run() {
        let base = browser_request("session-1");
        let same = browser_request("session-1");
        assert_eq!(run_request_fingerprint(&base), run_request_fingerprint(&same));
        let mut domains_a = base.clone();
        domains_a.allowed_domains = Some(vec!["b.example".to_owned(), "a.example".to_owned()]);
        let mut domains_b = base.clone();
        domains_b.allowed_domains = Some(vec!["a.example".to_owned(), "b.example".to_owned()]);
        assert_eq!(
            run_request_fingerprint(&domains_a),
            run_request_fingerprint(&domains_b),
            "host order is not a change"
        );
        let mut changed = base.clone();
        changed.max_steps = Some(3);
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&changed));
        let mut moved = base.clone();
        moved.max_cost_usd = Some(2.0);
        moved.max_runtime_s = Some(30);
        assert_eq!(
            run_request_fingerprint(&base),
            run_request_fingerprint(&moved),
            "the budget claim may move between a request and its retry"
        );
        let mut changed = base.clone();
        changed.require_approval = Some(true);
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&changed));
        let mut changed = base.clone();
        changed.profile_id = Some("profile-1".to_owned());
        assert_ne!(run_request_fingerprint(&base), run_request_fingerprint(&changed));
    }
}
