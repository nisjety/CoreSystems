//! App-initiated MCP tool calls (Wave 3 EP-4 S10).
//!
//! An MCP App drawn under a chat answer (S9) may ask its host to call a tool on
//! its own server, for example to refresh what it shows. This is a second entry
//! into tool execution beside the run loop, so it is held narrow (D13): only the
//! person who owns the run that drew the app, only a tool on the app's own
//! server that the server's allowlist names, and only a tool the registry
//! classes `read` with no approval floor, an approved review state and an
//! `allow` policy. Anything else answers `app_call_requires_chat`, and the app
//! may stage a message the person sends instead. Every call is bounded per frame
//! (a nonce the SPA mints per mounted frame) and per viewer.
//!
//! The call itself is execution-core's `ExecuteStep` with the viewer's delegated
//! bearers and `permission_mode = ask`, as `tools::handle_code_interpreter` calls
//! it for `code_interpreter`: execution-core authorises the caller and the run
//! owner again, asks capability-core for a signed decision, and dispatches the
//! `mcp__` name through `execute_mcp` with the signed class. The gateway never
//! decides the class; it only refuses earlier than execution-core would.
//!
//! Nothing is stored here. The output is scrubbed and capped before the app sees
//! it, and the audit line carries ids, the tool, the class and the outcome, never
//! the arguments or the output.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use mp_contracts::model_plane::v1::{
    ExecuteStepRequest, ExecuteStepResponse, PrivacyTier, ResolveToolBindingsRequest, ToolBinding,
};
use mp_contracts::tool_class::{effective_floor, ApprovalFloor, SideEffectClass};
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

/// The switch: only the literal `true` turns the route on.
pub const ENABLED_VAR: &str = "MCP_APP_TOOL_CALLS";
/// The catalogue feature a zero-retention or waived caller is refused with: the
/// call carries the app's arguments to the MCP server, as a tool call does.
pub const ZDR_FEATURE: &str = "mcp_execution";
/// The typed refusal for every call this route will not make (D13).
pub const REQUIRES_CHAT: &str = "app_call_requires_chat";
/// Calls one frame may make in [`RATE_WINDOW`].
pub const CALLS_PER_FRAME: usize = 10;
/// Calls one viewer may make in [`RATE_WINDOW`] across every frame, so minting
/// fresh frames cannot lift the per-frame bound.
pub const CALLS_PER_VIEWER: usize = 60;
/// The window both bounds count in.
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// The largest output an app is handed, in bytes of UTF-8.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// The largest arguments object accepted, serialised.
pub const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
/// Bound on the run id, server id and tool name a caller may send.
pub const MAX_NAME_BYTES: usize = 256;
/// Frame nonce bounds: long enough to be unguessable, short enough to key on.
const MIN_NONCE_BYTES: usize = 16;
const MAX_NONCE_BYTES: usize = 128;
/// Frames and viewers tracked per process; when full and nothing has expired,
/// a call is refused rather than tracked without bound.
const MAX_TRACKED_KEYS: usize = 4096;
/// How long one `ExecuteStep` may take before the app is told the call failed:
/// inside the BFF's 25 s upstream budget, so the typed answer reaches the app.
const EXECUTE_STEP_TIMEOUT: Duration = Duration::from_secs(20);
/// The capability ids the registry derives for MCP tool rows; a binding under
/// any other id is not an MCP tool's and is not trusted here.
const MCP_CAPABILITY_PREFIX: &str = "cap.mcp_tool.";
/// The registry review state of a definition a person approved.
const REVIEW_APPROVED: &str = "approved";
/// The registry's own policy answer that needs no person.
const POLICY_ALLOW: &str = "allow";

/// `MCP_APP_TOOL_CALLS`: only the literal `true` turns it on.
#[must_use]
pub fn enabled_from_env_value(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The process's `MCP_APP_TOOL_CALLS`, read once so it cannot change under a request.
#[must_use]
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| enabled_from_env_value(std::env::var(ENABLED_VAR).ok().as_deref()))
}

/// `POST /v1/mcp-apps/tool-call` body. Every field defaults so a missing one is
/// answered by [`validate`], after the switch, not by the extractor. No org or
/// user is read from it: both come from the verified claims.
#[derive(Default, Deserialize)]
pub struct AppCallBody {
    /// The run of the turn that drew the app.
    #[serde(default)]
    pub run_id: String,
    /// The app's own server; the call can name no other.
    #[serde(default)]
    pub server_id: String,
    /// The tool on that server, as the server names it.
    #[serde(default)]
    pub tool: String,
    /// The tool's arguments: a JSON object, or absent for none.
    #[serde(default)]
    pub arguments: Value,
    /// The per-frame nonce the SPA minted when it mounted the frame.
    #[serde(default)]
    pub frame_nonce: String,
}

/// A call that passed [`validate`]. Its `Debug` leaves the arguments out, so a
/// stray debug print cannot log them.
#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedCall {
    pub run_id: String,
    pub server_id: String,
    pub tool: String,
    /// `mcp__<server_id>__<tool>`, the name execution-core dispatches.
    pub qualified_name: String,
    /// The arguments as the tool input execution-core forwards.
    pub tool_input: String,
    pub frame_nonce: String,
}

impl std::fmt::Debug for ValidatedCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedCall")
            .field("run_id", &self.run_id)
            .field("server_id", &self.server_id)
            .field("tool", &self.tool)
            .field("tool_input_bytes", &self.tool_input.len())
            .finish_non_exhaustive()
    }
}

/// What the app is handed for a completed call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCallOutput {
    pub output: String,
    pub truncated: bool,
}

/// Why a call was not made or did not complete. Each maps to one typed error
/// code; none carries text from the MCP server or execution-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCallError {
    /// The body is malformed or over a bound.
    Invalid(&'static str),
    /// No such run, server or tool for this viewer. One code for all, so a
    /// viewer learns nothing about runs or servers they cannot use.
    NotFound,
    /// The tool is not one an app may call on its own (D13).
    RequiresChat,
    /// The frame or the viewer is over its bound.
    RateLimited,
    /// The run owner or the MCP registry could not be read.
    Unavailable,
    /// execution-core could not be reached in time.
    ExecutionUnavailable,
    /// The call ran and did not complete.
    Failed,
}

impl AppCallError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_request",
            Self::NotFound => "not_found",
            Self::RequiresChat => REQUIRES_CHAT,
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "registry_unavailable",
            Self::ExecutionUnavailable => "execution_unavailable",
            Self::Failed => "tool_call_failed",
        }
    }

    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Invalid(message) => message,
            Self::NotFound => "no such run, server or tool for this viewer",
            Self::RequiresChat => {
                "an app may only call read tools on its own server; ask in the chat instead"
            }
            Self::RateLimited => "too many tool calls from this app; try again in a minute",
            Self::Unavailable => "the run or the tool registry is unavailable",
            Self::ExecutionUnavailable => "the tool could not be run right now",
            Self::Failed => "the tool call did not complete",
        }
    }

    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::RequiresChat => StatusCode::FORBIDDEN,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable | Self::ExecutionUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Failed => StatusCode::BAD_GATEWAY,
        }
    }
}

const NAMES_REQUIRED: &str = "run_id, server_id and tool are required";
const BAD_NAME: &str = "run_id, server_id or tool is malformed";
const BAD_NONCE: &str = "frame_nonce must be 16 to 128 characters of A-Z, a-z, 0-9, '_' or '-'";
const BAD_ARGUMENTS: &str = "arguments must be a JSON object of at most 16 KiB";

fn plain_name(value: &str) -> bool {
    !value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
}

/// Check a body and build the call. A server id with `__` is refused: the
/// qualified name splits on the first `__` after the prefix, so such an id
/// would name another server's tool.
///
/// # Errors
/// [`AppCallError::Invalid`].
pub fn validate(body: AppCallBody) -> Result<ValidatedCall, AppCallError> {
    let run_id = body.run_id.trim();
    let server_id = body.server_id.trim();
    let tool = body.tool.trim();
    if [run_id, server_id, tool]
        .iter()
        .any(|value| value.is_empty())
    {
        return Err(AppCallError::Invalid(NAMES_REQUIRED));
    }
    if [run_id, server_id, tool]
        .iter()
        .any(|value| value.len() > MAX_NAME_BYTES || !plain_name(value))
        || server_id.contains("__")
    {
        return Err(AppCallError::Invalid(BAD_NAME));
    }
    let nonce = body.frame_nonce.as_str();
    if !(MIN_NONCE_BYTES..=MAX_NONCE_BYTES).contains(&nonce.len())
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(AppCallError::Invalid(BAD_NONCE));
    }
    let arguments = match body.arguments {
        Value::Null => Value::Object(serde_json::Map::new()),
        Value::Object(map) => Value::Object(map),
        _ => return Err(AppCallError::Invalid(BAD_ARGUMENTS)),
    };
    let tool_input = arguments.to_string();
    if tool_input.len() > MAX_ARGUMENT_BYTES {
        return Err(AppCallError::Invalid(BAD_ARGUMENTS));
    }
    Ok(ValidatedCall {
        run_id: run_id.to_owned(),
        server_id: server_id.to_owned(),
        tool: tool.to_owned(),
        qualified_name: format!("mcp__{server_id}__{tool}"),
        tool_input,
        frame_nonce: nonce.to_owned(),
    })
}

/// Whether the registry's binding for `qualified_name` lets an app call it on
/// its own: exactly one MCP binding, approved, classed `read`, no approval floor
/// once the class's own floor is applied, and an `allow` policy. An empty or
/// unknown class reads as `unclassified`, so a mixed-version registry cannot make
/// a tool look like a read.
///
/// # Errors
/// [`AppCallError::RequiresChat`] for anything else.
pub fn admit_binding(
    qualified_name: &str,
    bindings: &[ToolBinding],
) -> Result<SideEffectClass, AppCallError> {
    let mut matches = bindings
        .iter()
        .filter(|binding| binding.tool_name == qualified_name);
    let (Some(binding), None) = (matches.next(), matches.next()) else {
        return Err(AppCallError::RequiresChat);
    };
    let class = SideEffectClass::from_wire(&binding.side_effect_class);
    let floor = effective_floor(class, ApprovalFloor::from_wire(&binding.approval_floor));
    let admitted = binding.capability_id.starts_with(MCP_CAPABILITY_PREFIX)
        && binding.review_state == REVIEW_APPROVED
        && binding.effective_policy == POLICY_ALLOW
        && class == SideEffectClass::Read
        && floor == ApprovalFloor::None;
    if admitted {
        Ok(class)
    } else {
        Err(AppCallError::RequiresChat)
    }
}

/// The app's view of a finished `ExecuteStep`. A step that paused for approval
/// is refused, not waited on: an app call never asks the person for anything.
///
/// # Errors
/// [`AppCallError::RequiresChat`] for a paused step, [`AppCallError::Failed`] otherwise.
pub fn step_output(response: &ExecuteStepResponse) -> Result<AppCallOutput, AppCallError> {
    match response.status.as_str() {
        "completed" => {
            let (output, truncated) = capped(&mp_scrub::scrub_string(&response.output));
            Ok(AppCallOutput { output, truncated })
        }
        "awaiting_approval" => Err(AppCallError::RequiresChat),
        _ => Err(AppCallError::Failed),
    }
}

/// `text` cut to [`MAX_OUTPUT_BYTES`] on a character boundary.
fn capped(text: &str) -> (String, bool) {
    if text.len() <= MAX_OUTPUT_BYTES {
        return (text.to_owned(), false);
    }
    let mut end = MAX_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

type FrameKey = (String, String, String);
type ViewerKey = (String, String);

#[derive(Default)]
struct Windows {
    frames: HashMap<FrameKey, VecDeque<Instant>>,
    viewers: HashMap<ViewerKey, VecDeque<Instant>>,
}

/// Drop the calls older than the window; `true` when fewer than `limit` remain.
fn has_room(calls: &mut VecDeque<Instant>, now: Instant, window: Duration, limit: usize) -> bool {
    while calls
        .front()
        .is_some_and(|first| now.saturating_duration_since(*first) >= window)
    {
        calls.pop_front();
    }
    calls.len() < limit
}

/// Sliding-window bounds per (org, viewer, frame) and per (org, viewer). The
/// frame is keyed with its viewer, so one person's nonce never spends another's.
pub struct FrameRateLimiter {
    windows: Mutex<Windows>,
    window: Duration,
    per_frame: usize,
    per_viewer: usize,
}

impl FrameRateLimiter {
    #[must_use]
    pub fn new(window: Duration, per_frame: usize, per_viewer: usize) -> Self {
        Self {
            windows: Mutex::new(Windows::default()),
            window,
            per_frame,
            per_viewer,
        }
    }

    /// The process's limiter.
    pub fn global() -> &'static Self {
        static LIMITER: OnceLock<FrameRateLimiter> = OnceLock::new();
        LIMITER.get_or_init(|| Self::new(RATE_WINDOW, CALLS_PER_FRAME, CALLS_PER_VIEWER))
    }

    /// Count one call at `now` when both bounds have room; otherwise count
    /// nothing and refuse.
    pub fn admit(&self, org_id: &str, user_id: &str, frame_nonce: &str, now: Instant) -> bool {
        let mut windows = self.windows.lock().unwrap_or_else(PoisonError::into_inner);
        let window = self.window;
        if windows.frames.len() >= MAX_TRACKED_KEYS || windows.viewers.len() >= MAX_TRACKED_KEYS {
            let live = |calls: &VecDeque<Instant>| {
                calls
                    .back()
                    .is_some_and(|last| now.saturating_duration_since(*last) < window)
            };
            windows.frames.retain(|_, calls| live(calls));
            windows.viewers.retain(|_, calls| live(calls));
        }
        let frame_key = (
            org_id.to_owned(),
            user_id.to_owned(),
            frame_nonce.to_owned(),
        );
        let viewer_key = (org_id.to_owned(), user_id.to_owned());
        let frame_known = windows.frames.contains_key(&frame_key);
        let viewer_known = windows.viewers.contains_key(&viewer_key);
        if (!frame_known && windows.frames.len() >= MAX_TRACKED_KEYS)
            || (!viewer_known && windows.viewers.len() >= MAX_TRACKED_KEYS)
        {
            return false;
        }
        let frame_room = has_room(
            windows.frames.entry(frame_key.clone()).or_default(),
            now,
            window,
            self.per_frame,
        );
        let viewer_room = has_room(
            windows.viewers.entry(viewer_key.clone()).or_default(),
            now,
            window,
            self.per_viewer,
        );
        if !(frame_room && viewer_room) {
            return false;
        }
        windows.frames.entry(frame_key).or_default().push_back(now);
        windows
            .viewers
            .entry(viewer_key)
            .or_default()
            .push_back(now);
        true
    }
}

/// The audit line of one call: ids, the tool, the class and the outcome. It has
/// no field for the arguments or the output, so it cannot carry either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditLine<'a> {
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub run_id: &'a str,
    pub server_id: &'a str,
    pub tool: &'a str,
    pub class: &'static str,
    pub outcome: &'static str,
}

impl std::fmt::Display for AuditLine<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "org_id={} user_id={} run_id={} server_id={} tool={} class={} outcome={}",
            self.org_id,
            self.user_id,
            self.run_id,
            self.server_id,
            self.tool,
            self.class,
            self.outcome
        )
    }
}

/// The viewer's delegated bearers, each verified by the gateway's auth layer.
/// `ExecuteStep` authenticates the data-plane and inference bearers for every
/// tool, so all five are needed even though an MCP call uses neither.
pub struct ViewerBearers<'a> {
    pub capability: &'a str,
    pub session: &'a str,
    pub execution: &'a str,
    pub data_plane: &'a str,
    pub inference: &'a str,
}

/// Make one app-initiated call for the viewer `user_id` in `org_id`, after the
/// route refused a switched-off, zero-retention or malformed request. In order:
/// the frame and viewer bounds, the run owner (session-core), the server and
/// its allowlist, the registry's binding (capability-core, with the viewer's
/// own capability bearer), then `ExecuteStep`. One audit line per call.
///
/// # Errors
/// See [`AppCallError`].
pub async fn call_app_tool(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    call: &ValidatedCall,
    bearers: &ViewerBearers<'_>,
) -> Result<AppCallOutput, AppCallError> {
    let mut class = "unchecked";
    let result = run_call(state, org_id, user_id, call, bearers, &mut class).await;
    let line = AuditLine {
        org_id,
        user_id,
        run_id: &call.run_id,
        server_id: &call.server_id,
        tool: &call.tool,
        class,
        outcome: match &result {
            Ok(_) => "completed",
            Err(error) => error.code(),
        },
    };
    tracing::info!(target: "audit", audit = %line, "mcp app tool call");
    result
}

async fn run_call(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    call: &ValidatedCall,
    bearers: &ViewerBearers<'_>,
    class: &mut &'static str,
) -> Result<AppCallOutput, AppCallError> {
    if !FrameRateLimiter::global().admit(org_id, user_id, &call.frame_nonce, Instant::now()) {
        return Err(AppCallError::RateLimited);
    }
    crate::session_flow::require_durable_run_owner_with_token(
        state,
        &call.run_id,
        org_id,
        user_id,
        bearers.session,
    )
    .await
    .map_err(|status| match status.code() {
        tonic::Code::PermissionDenied | tonic::Code::NotFound | tonic::Code::InvalidArgument => {
            AppCallError::NotFound
        }
        _ => AppCallError::Unavailable,
    })?;
    crate::runtime_registries::hydrate_mcp_registry(
        &state.mcp,
        &state.ownership,
        &state.http_client,
        &state.capability_core_base_url,
        org_id,
        bearers.capability,
    )
    .await
    .map_err(|error| {
        tracing::warn!(%org_id, error_len = error.len(), "mcp app call: registry hydration failed");
        AppCallError::Unavailable
    })?;
    crate::mcp_apps::authorize(
        &state.mcp,
        &state.ownership,
        org_id,
        user_id,
        &call.server_id,
        &call.tool,
    )
    .map_err(|_| AppCallError::NotFound)?;
    let bindings = resolve_bindings(state, org_id, user_id, call, bearers.capability).await?;
    *class = admit_binding(&call.qualified_name, &bindings)?.as_str();
    let response = execute_step(state, org_id, user_id, call, bearers).await?;
    step_output(&response)
}

/// The registry's binding for the call's tool, read as the viewer: a user
/// caller may only resolve bindings for themselves.
async fn resolve_bindings(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    call: &ValidatedCall,
    capability_bearer: &str,
) -> Result<Vec<ToolBinding>, AppCallError> {
    let mut request = tonic::Request::new(ResolveToolBindingsRequest {
        org_id: org_id.to_owned(),
        run_id: call.run_id.clone(),
        user_id: user_id.to_owned(),
        tool_names: vec![call.qualified_name.clone()],
    });
    insert_bearer(&mut request, "authorization", capability_bearer)?;
    state
        .capability_client
        .clone()
        .resolve_tool_bindings(request)
        .await
        .map(|response| response.into_inner().bindings)
        .map_err(|status| {
            tracing::warn!(code = ?status.code(), "mcp app call: tool binding lookup failed");
            AppCallError::Unavailable
        })
}

/// One `ExecuteStep` for the call, carrying the viewer's bearers as
/// `tools::handle_code_interpreter` does. `ask`, never `auto`: a tool the run's
/// gate would pause on pauses, and the pause is answered as requires-chat.
async fn execute_step(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    call: &ValidatedCall,
    bearers: &ViewerBearers<'_>,
) -> Result<ExecuteStepResponse, AppCallError> {
    let mut request = tonic::Request::new(ExecuteStepRequest {
        run_id: call.run_id.clone(),
        step_id: format!("app-{}", mp_ids::new_ulid()),
        tool_name: call.qualified_name.clone(),
        tool_input: call.tool_input.clone(),
        permission_mode: "ask".to_owned(),
        hook_context: crate::runtime_registries::hook_context_json(&state.hooks, org_id),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        // The route refused zero-retention and waived callers before this point.
        zdr: false,
        min_privacy_tier: PrivacyTier::Unspecified as i32,
        // An app call is personal: no Space sandbox, no thread workspace, no files or skills.
        space_id: String::new(),
        thread_id: String::new(),
        attachments: Vec::new(),
        skill_mounts: Vec::new(),
        connector_scope: None,
    });
    insert_bearer(&mut request, "authorization", bearers.execution)?;
    insert_bearer(&mut request, "x-session-authorization", bearers.session)?;
    insert_bearer(
        &mut request,
        "x-data-plane-authorization",
        bearers.data_plane,
    )?;
    insert_bearer(&mut request, "x-inference-authorization", bearers.inference)?;
    let mut client = state.execution_client.clone();
    match tokio::time::timeout(EXECUTE_STEP_TIMEOUT, client.execute_step(request)).await {
        Ok(Ok(response)) => Ok(response.into_inner()),
        Ok(Err(status)) => {
            tracing::warn!(code = ?status.code(), "mcp app call: ExecuteStep refused or failed");
            Err(match status.code() {
                tonic::Code::PermissionDenied | tonic::Code::NotFound => AppCallError::NotFound,
                tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => {
                    AppCallError::ExecutionUnavailable
                }
                _ => AppCallError::Failed,
            })
        }
        Err(_) => Err(AppCallError::ExecutionUnavailable),
    }
}

fn insert_bearer<T>(
    request: &mut tonic::Request<T>,
    header: &'static str,
    bearer: &str,
) -> Result<(), AppCallError> {
    let value = format!("Bearer {bearer}")
        .parse()
        .map_err(|_| AppCallError::Unavailable)?;
    request.metadata_mut().insert(header, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NONCE: &str = "frame-0123456789abcdef";

    fn body(server_id: &str, tool: &str, arguments: Value) -> AppCallBody {
        AppCallBody {
            run_id: "run-1".to_owned(),
            server_id: server_id.to_owned(),
            tool: tool.to_owned(),
            arguments,
            frame_nonce: NONCE.to_owned(),
        }
    }

    fn binding(name: &str, class: &str, floor: &str) -> ToolBinding {
        ToolBinding {
            tool_name: name.to_owned(),
            capability_id: "cap.mcp_tool.abc".to_owned(),
            capability_version: "v1".to_owned(),
            side_effect_class: class.to_owned(),
            approval_floor: floor.to_owned(),
            tool_digest: "d".repeat(64),
            review_state: REVIEW_APPROVED.to_owned(),
            effective_policy: POLICY_ALLOW.to_owned(),
        }
    }

    #[test]
    fn switch_parses_strictly() {
        assert!(enabled_from_env_value(Some("true")));
        for value in [
            None,
            Some(""),
            Some("TRUE"),
            Some("1"),
            Some(" true"),
            Some("yes"),
        ] {
            assert!(!enabled_from_env_value(value), "{value:?}");
        }
    }

    #[test]
    fn the_call_names_only_the_apps_own_server() {
        let call = validate(body("srv-1", "echo_read", json!({ "q": "x" }))).expect("valid");
        assert_eq!(call.qualified_name, "mcp__srv-1__echo_read");
        assert_eq!(call.tool_input, r#"{"q":"x"}"#);
        // No arguments is an empty object, never a null the server must guess at.
        let call = validate(body("srv-1", "echo_read", Value::Null)).expect("valid");
        assert_eq!(call.tool_input, "{}");
    }

    #[test]
    fn other_server_tool_is_refused() {
        // A server id with `__` would make the qualified name split onto another server.
        for server in ["srv__other", "srv-1__x"] {
            assert_eq!(
                validate(body(server, "echo_read", Value::Null)),
                Err(AppCallError::Invalid(BAD_NAME)),
                "{server}"
            );
        }
        // A binding the registry returns for another server's name is not this call's.
        let other = binding("mcp__srv-2__echo_read", "read", "none");
        assert_eq!(
            admit_binding("mcp__srv-1__echo_read", &[other]),
            Err(AppCallError::RequiresChat)
        );
    }

    #[test]
    fn malformed_bodies_are_refused() {
        assert_eq!(
            validate(body("", "echo_read", Value::Null)),
            Err(AppCallError::Invalid(NAMES_REQUIRED))
        );
        assert_eq!(
            validate(body("srv-1", "echo read", Value::Null)),
            Err(AppCallError::Invalid(BAD_NAME))
        );
        for arguments in [json!([1, 2]), json!("x"), json!(3)] {
            assert_eq!(
                validate(body("srv-1", "echo_read", arguments)),
                Err(AppCallError::Invalid(BAD_ARGUMENTS))
            );
        }
        let big = json!({ "q": "x".repeat(MAX_ARGUMENT_BYTES) });
        assert_eq!(
            validate(body("srv-1", "echo_read", big)),
            Err(AppCallError::Invalid(BAD_ARGUMENTS))
        );
        for nonce in [
            "short",
            "has space in the nonce!",
            &"n".repeat(MAX_NONCE_BYTES + 1),
        ] {
            let mut bad = body("srv-1", "echo_read", Value::Null);
            bad.frame_nonce = nonce.to_owned();
            assert_eq!(
                validate(bad),
                Err(AppCallError::Invalid(BAD_NONCE)),
                "{nonce}"
            );
        }
    }

    #[test]
    fn write_class_is_refused_with_requires_chat() {
        let name = "mcp__srv-1__tool";
        assert_eq!(
            admit_binding(name, &[binding(name, "read", "none")]),
            Ok(SideEffectClass::Read)
        );
        for class in [
            "external_write",
            "destructive",
            "owner_effect",
            "context_write",
            "sandbox_compute",
            "unclassified",
            "",
            "mystery",
        ] {
            assert_eq!(
                admit_binding(name, &[binding(name, class, "none")]),
                Err(AppCallError::RequiresChat),
                "{class:?}"
            );
        }
        // A read the registry floors at ask, or an unknown floor (read as the strictest), asks a person.
        for floor in ["ask", "durable_owner", "odd"] {
            assert_eq!(
                admit_binding(name, &[binding(name, "read", floor)]),
                Err(AppCallError::RequiresChat),
                "{floor:?}"
            );
        }
        let mut pending = binding(name, "read", "none");
        pending.review_state = "drifted".to_owned();
        let mut asks = binding(name, "read", "none");
        asks.effective_policy = "ask".to_owned();
        let mut builtin = binding(name, "read", "none");
        builtin.capability_id = "cap.command.sandbox".to_owned();
        for refused in [pending, asks, builtin] {
            assert_eq!(
                admit_binding(name, &[refused]),
                Err(AppCallError::RequiresChat)
            );
        }
        // No binding, or an ambiguous one, is not a read.
        assert_eq!(admit_binding(name, &[]), Err(AppCallError::RequiresChat));
        let twice = [binding(name, "read", "none"), binding(name, "read", "none")];
        assert_eq!(admit_binding(name, &twice), Err(AppCallError::RequiresChat));
    }

    #[test]
    fn rate_limit_per_frame() {
        let limiter = FrameRateLimiter::new(RATE_WINDOW, CALLS_PER_FRAME, CALLS_PER_VIEWER);
        let start = Instant::now();
        for _ in 0..CALLS_PER_FRAME {
            assert!(limiter.admit("org-a", "user-a", NONCE, start));
        }
        assert!(!limiter.admit("org-a", "user-a", NONCE, start));
        // Another frame, and another viewer with the same nonce, have their own bound.
        assert!(limiter.admit("org-a", "user-a", "frame-fedcba9876543210", start));
        assert!(limiter.admit("org-a", "user-b", NONCE, start));
        // The window slides.
        assert!(limiter.admit("org-a", "user-a", NONCE, start + RATE_WINDOW));
    }

    #[test]
    fn fresh_frames_cannot_lift_the_viewer_bound() {
        let limiter = FrameRateLimiter::new(RATE_WINDOW, CALLS_PER_FRAME, 3);
        let start = Instant::now();
        for frame in 0..3 {
            assert!(limiter.admit(
                "org-a",
                "user-a",
                &format!("frame-nonce-00000{frame}"),
                start
            ));
        }
        assert!(!limiter.admit("org-a", "user-a", "frame-nonce-000009", start));
    }

    #[test]
    fn arguments_never_logged() {
        let secret = "sekret-argument-value";
        let call = validate(body("srv-1", "echo_read", json!({ "q": secret }))).expect("valid");
        assert!(!format!("{call:?}").contains(secret));
        let line = AuditLine {
            org_id: "org-a",
            user_id: "user-a",
            run_id: &call.run_id,
            server_id: &call.server_id,
            tool: &call.tool,
            class: "read",
            outcome: "completed",
        };
        let rendered = line.to_string();
        assert!(!rendered.contains(secret));
        assert!(rendered.contains("tool=echo_read"));
        assert!(rendered.contains("outcome=completed"));
    }

    #[test]
    fn output_is_scrubbed_and_capped() {
        let completed = |output: String| ExecuteStepResponse {
            step_id: "app-1".to_owned(),
            status: "completed".to_owned(),
            output,
            ..ExecuteStepResponse::default()
        };
        let secret = "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.c2lnbmF0dXJlLXNpZ25hdHVyZQ";
        let out = step_output(&completed(format!("token {secret}"))).expect("completed");
        assert!(!out.output.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!out.truncated);
        // A multi-byte character at the cut is never split.
        let big = "\u{e6}".repeat(MAX_OUTPUT_BYTES);
        let out = step_output(&completed(big)).expect("completed");
        assert!(out.truncated);
        assert!(out.output.len() <= MAX_OUTPUT_BYTES);
        assert!(out.output.chars().all(|ch| ch == '\u{e6}'));
    }

    #[test]
    fn a_paused_step_requires_chat_and_a_failure_is_typed() {
        let with_status = |status: &str| ExecuteStepResponse {
            status: status.to_owned(),
            error: "upstream said something".to_owned(),
            ..ExecuteStepResponse::default()
        };
        assert_eq!(
            step_output(&with_status("awaiting_approval")),
            Err(AppCallError::RequiresChat)
        );
        for status in ["failed", "cancelled", "aborted", ""] {
            assert_eq!(
                step_output(&with_status(status)),
                Err(AppCallError::Failed),
                "{status}"
            );
        }
    }

    #[test]
    fn errors_map_to_typed_codes() {
        assert_eq!(AppCallError::RequiresChat.code(), "app_call_requires_chat");
        assert_eq!(AppCallError::RequiresChat.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            AppCallError::RateLimited.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(AppCallError::NotFound.code(), "not_found");
        assert_eq!(
            AppCallError::Invalid(BAD_NAME).status(),
            StatusCode::BAD_REQUEST
        );
    }
}
