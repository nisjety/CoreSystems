//! The MCP server probe: what a server needs and offers, before anyone saves it.
//!
//! `POST /v1/mcp/servers/probe` takes `{url}` and answers `{data: {auth, tools}}`. `auth` is `oauth`
//! when the server publishes RFC 9728 protected-resource metadata, `bearer_required` when an
//! unauthenticated tool listing is refused with 401 or 403, and `none` when the server lists its tools
//! without a credential. Tools are listed only for `none`: up to [`MAX_TOOLS`], each with its name, a
//! description cut to [`MAX_DESCRIPTION_CHARS`] characters, and the server's own read-only and
//! destructive hints. Those hints are the server's claims, shown for a person to judge, never trusted.
//!
//! The probe saves nothing. It writes no registry row and no capability row, and it sends no credential:
//! the body takes `url` alone, and a body with any other key, a token among them, is refused. The URL
//! meets the registration rules (`runtime_registries::validate_mcp_url`), and every dial goes through the
//! SSRF-guarded client the tool calls use, so the probe reaches no address that registration could not.
//!
//! A zero-retention or waived caller is refused with the `mcp_execution` feature, as the MCP app routes
//! refuse it: the probe reaches an external server on the caller's behalf, as a tool call does. Only the
//! literal `true` in `MODEL_GATEWAY_MCP_PROBE` turns the route on; off, it is a 404. A user may make
//! [`PROBES_PER_USER`] probes a minute, and one probe may take at most [`PROBE_TIMEOUT`].

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::{body::Bytes, http::StatusCode, Extension, Json};
use mp_contracts::zdr_refusal::Refusal;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

use crate::auth::Claims;
use crate::http_routes::HttpJsonError;
use crate::mcp_app_calls::FrameRateLimiter;
use crate::mcp_jsonrpc::McpToolDef;
use crate::zdr_posture::Route;

/// The switch: only the literal `true` turns the route on.
pub const ENABLED_VAR: &str = "MODEL_GATEWAY_MCP_PROBE";
/// The catalogue feature a zero-retention or waived caller is refused with. The MCP app routes share it,
/// since each reaches an MCP server on the caller's behalf.
pub const ZDR_FEATURE: &str = "mcp_execution";
/// Probes one user may make in [`RATE_WINDOW`].
pub const PROBES_PER_USER: usize = 10;
/// The window the per-user bound counts in.
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// The longest one probe may take, discovery and tool listing together.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// The most tools one probe reports.
pub const MAX_TOOLS: usize = 64;
/// The longest description one probed tool reports, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// The longest tool name a probe reports. Registration takes an exact name no longer than this, so a
/// longer one could never be allowed, and it is not offered.
const MAX_TOOL_NAME_BYTES: usize = 128;
/// The frame key the per-user window is kept under. A probe has no frame, so every probe of one user
/// shares this key, and the window it counts in is the per-user bound.
const PROBE_KEY: &str = "mcp-probe";

/// `MODEL_GATEWAY_MCP_PROBE`: only the literal `true` turns it on.
#[must_use]
pub fn enabled_from_env_value(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The process's `MODEL_GATEWAY_MCP_PROBE`, read once so it cannot change under a request.
#[must_use]
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| enabled_from_env_value(std::env::var(ENABLED_VAR).ok().as_deref()))
}

/// The body of `POST /v1/mcp/servers/probe`. Only `url` is read; any other key is refused.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeBody {
    url: String,
}

/// What a server needs before it answers a tool listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeAuth {
    /// The server lists its tools without a credential.
    NoAuth,
    /// The server publishes OAuth protected-resource metadata, so it is connected by a login.
    OAuth,
    /// The server refuses an unauthenticated request with 401 or 403 and publishes no OAuth metadata,
    /// so it needs a bearer credential.
    BearerRequired,
}

impl ProbeAuth {
    /// The wire label the answer carries.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::NoAuth => "none",
            Self::OAuth => "oauth",
            Self::BearerRequired => "bearer_required",
        }
    }
}

/// One tool a probe reports. The hints are the server's own claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedTool {
    /// The tool's name as the server reports it.
    pub name: String,
    /// The description, cut to [`MAX_DESCRIPTION_CHARS`] characters.
    pub description: String,
    /// The server's `readOnlyHint`, when it gave one.
    pub read_only_hint: Option<bool>,
    /// The server's `destructiveHint`, when it gave one.
    pub destructive_hint: Option<bool>,
}

/// What one probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// The auth requirement the server states by answering.
    pub auth: ProbeAuth,
    /// The tools, empty unless `auth` is [`ProbeAuth::NoAuth`].
    pub tools: Vec<ProbedTool>,
}

/// `POST /v1/mcp/servers/probe`, see the module docs.
pub(crate) async fn post_mcp_probe(
    Extension(claims): Extension<Claims>,
    body: Bytes,
) -> Result<Json<Value>, HttpJsonError> {
    probe_request(&claims, &body, enabled()).await
}

/// The probe under an explicit switch value. Each step answers before the next is read: the switch, then
/// the posture, then the body, then the per-user limit, then the probe itself.
pub(crate) async fn probe_request(
    claims: &Claims,
    body: &[u8],
    enabled: bool,
) -> Result<Json<Value>, HttpJsonError> {
    if !enabled {
        return Err(typed(StatusCode::NOT_FOUND, "not_found", "no such route"));
    }
    if crate::zdr_posture::zdr_or_refuse(claims, &Route::plain(false))? {
        return Err((
            StatusCode::FORBIDDEN,
            Json(Refusal::feature_not_supported(ZDR_FEATURE).json_body()),
        ));
    }
    let request: ProbeBody = serde_json::from_slice(body).map_err(|_| {
        typed(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the body must be a JSON object holding only url; a credential is never accepted",
        )
    })?;
    let url = crate::runtime_registries::validate_mcp_url(&request.url)
        .map_err(|status| typed(StatusCode::BAD_REQUEST, "invalid_request", status.message()))?;
    if !admit(claims, Instant::now()) {
        return Err(typed(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many probes from this account; try again in a minute",
        ));
    }
    match tokio::time::timeout(PROBE_TIMEOUT, probe_server(&url)).await {
        Ok(Ok(report)) => Ok(Json(json!({ "data": report_body(&report) }))),
        Ok(Err(error)) => {
            debug!(%error, "mcp probe: the address did not answer as an MCP server");
            Err(typed(
                StatusCode::BAD_GATEWAY,
                "probe_failed",
                "the address did not answer as an MCP server",
            ))
        }
        Err(_elapsed) => Err(typed(
            StatusCode::GATEWAY_TIMEOUT,
            "probe_timeout",
            "the server did not answer in time",
        )),
    }
}

/// Count one probe of this user at `now`, or refuse it once the user has spent [`PROBES_PER_USER`] of
/// them in the window. The limiter's frame and viewer bounds are both [`PROBES_PER_USER`] under one key,
/// so together they are the per-user bound.
fn admit(claims: &Claims, now: Instant) -> bool {
    static LIMITER: OnceLock<FrameRateLimiter> = OnceLock::new();
    LIMITER
        .get_or_init(|| FrameRateLimiter::new(RATE_WINDOW, PROBES_PER_USER, PROBES_PER_USER))
        .admit(&claims.org_id, &claims.user_id, PROBE_KEY, now)
}

/// Find out what `url` needs, and list its tools when it needs no credential. The error, when there is
/// one, is for the debug log only: the route answers with a fixed sentence, never with the transport text.
async fn probe_server(url: &str) -> Result<ProbeReport, String> {
    let protected = crate::mcp_oauth::discover_protected_resource(url).await;
    if protected.is_ok() {
        return Ok(ProbeReport {
            auth: ProbeAuth::OAuth,
            tools: Vec::new(),
        });
    }
    match crate::runtime_registries::http_list_tools(url, "").await {
        Ok(tools) => Ok(ProbeReport {
            auth: ProbeAuth::NoAuth,
            tools: probed_tools(tools),
        }),
        Err(error) if is_auth_refusal(&error) => Ok(ProbeReport {
            auth: ProbeAuth::BearerRequired,
            tools: Vec::new(),
        }),
        Err(error) => Err(error),
    }
}

/// The tools a probe reports: names registration could allow, at most [`MAX_TOOLS`], each description cut
/// to [`MAX_DESCRIPTION_CHARS`] characters.
fn probed_tools(tools: Vec<McpToolDef>) -> Vec<ProbedTool> {
    tools
        .into_iter()
        .filter(|tool| !tool.name.is_empty() && tool.name.len() <= MAX_TOOL_NAME_BYTES)
        .take(MAX_TOOLS)
        .map(|tool| ProbedTool {
            name: tool.name,
            description: bounded_description(&tool.description),
            read_only_hint: tool.annotations.read_only_hint,
            destructive_hint: tool.annotations.destructive_hint,
        })
        .collect()
}

/// `text` cut to [`MAX_DESCRIPTION_CHARS`] characters.
fn bounded_description(text: &str) -> String {
    text.chars().take(MAX_DESCRIPTION_CHARS).collect()
}

/// Whether a transport error is the server refusing the request for want of a credential. The MCP
/// transport words its HTTP refusals as `mcp HTTP <status>: <body>` (see `mcp_http`).
fn is_auth_refusal(error: &str) -> bool {
    error.starts_with("mcp HTTP 401") || error.starts_with("mcp HTTP 403")
}

/// The `data` of a probe answer: `{auth, tools}`.
fn report_body(report: &ProbeReport) -> Value {
    let tools: Vec<Value> = report
        .tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "read_only_hint": tool.read_only_hint,
                "destructive_hint": tool.destructive_hint,
            })
        })
        .collect();
    json!({ "auth": report.auth.as_wire(), "tools": tools })
}

/// The typed error envelope every refusal of this route uses: `{error: {code, message}}`.
fn typed(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}
