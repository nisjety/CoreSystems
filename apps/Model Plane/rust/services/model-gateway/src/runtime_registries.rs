//! Wave 10g/h/i/j — in-memory registries and proxies.
//!
//! Each sub-module owns one or two stores + the matching handlers.
//! Everything here is gateway-scoped and ephemeral; durable state
//! should subscribe to NATS or live in capability-core / session-core.
//!
//! Modules:
//!   - mcp:        MCP server registry + tool proxy (Wave 10g)
//!   - plugins:    Plugin registry (Wave 10h)
//!   - commands:   Slash-command registry + execute pass-through (Wave 10i)
//!   - hooks:      Lifecycle hook registry (Wave 10i)
//!   - permissions: Per-org tool ACL (Wave 10i)
//!   - policy:     Per-org runtime guardrails (Wave 10i)
//!   - messages:   Thread message append/list (Wave 10j)
//!   - analytics:  Per-org counter rollups (Wave 10j)
//!   - voice:      TTS/STT — served by grpc.rs + /v1/ai/speech → inference-core
//!   - tasks:      Gateway-scoped task ingress (Wave 10j; durable state
//!     belongs in session-core `tasks` tables, cron in Temporal —
//!     the orphaned task-core service was retired, matrix §4.2)

// tonic::Status is the unavoidable large Err for gRPC handlers; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tonic::Status;

use mp_contracts::model_plane::v1::{
    AppendThreadMessageRequest, AppendThreadMessageResponse, Command, CreateTaskRequest,
    CreateTaskResponse, ExecuteCommandRequest, ExecuteCommandResponse, GetAnalyticsRequest,
    GetAnalyticsResponse, Hook, ListCommandsRequest, ListCommandsResponse, ListHooksRequest,
    ListHooksResponse, ListMcpServersRequest, ListMcpServersResponse, ListPluginsRequest,
    ListPluginsResponse, ListTasksRequest, ListTasksResponse, ListThreadMessagesRequest,
    ListThreadMessagesResponse, McpServer, Plugin, ProxyMcpToolRequest, ProxyMcpToolResponse,
    RegisterHookRequest, RegisterHookResponse, RegisterMcpServerRequest, RegisterMcpServerResponse,
    RegisterPluginRequest, RegisterPluginResponse, SetPluginEnabledRequest,
    SetPluginEnabledResponse, TaskRecord, ThreadMessage, ToolCallCount, ToolDefinition,
};

use crate::mcp_jsonrpc::McpToolDef;

/// Per-(org, `server_id`) discovered tool catalog with the instant it was
/// fetched. Aliased to keep the [`McpRegistry`] field readable (clippy
/// `type_complexity`).
type McpCatalog = Arc<DashMap<(String, String), (Instant, Vec<McpToolDef>)>>;

/// How long a discovered `tools/list` catalog is reused before the next
/// discovery. Keeps governed callers from making an HTTP round-trip on every
/// invocation while staying fresh enough to pick up newly added tools within a
/// minute. Process/stdio transports are not supported.
const MCP_CATALOG_TTL: Duration = Duration::from_secs(60);

/// Hard cap on how long tool discovery may block a turn. A registered-but-down
/// server must not stall a caller. Discovery fails closed: the stored allowlist
/// constrains discovered tools but never fabricates callable schemas.
const MCP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(6);
const MCP_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MCP_MAX_RESPONSE_BYTES: usize = 1_048_576;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ====================================================================
// Wave 10g — MCP server registry + tool proxy
// ====================================================================

/// An in-flight OAuth 2.1 + DCR connection attempt, keyed by the CSRF
/// `state` value. Lives only from `oauth/start` to `oauth/callback` (a few
/// minutes at most) — ephemeral by nature, so this is in-memory only, unlike
/// the tokens the flow produces (durably encrypted in capability-core).
/// No `Debug`/`Default`: `created_at` is a bare `Instant` (no `Default`), and
/// this struct carries a PKCE `code_verifier` that should not be casually
/// printable via `{:?}`.
#[derive(Clone)]
pub struct PendingMcpOAuth {
    pub org_id: String,
    pub user_id: String,
    pub server_name: String,
    pub server_url: String,
    pub tool_allowlist: Vec<String>,
    pub scope_wire: String,
    pub code_verifier: String,
    pub client_id: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
    pub oauth_scopes: Vec<String>,
    created_at: Instant,
}

impl PendingMcpOAuth {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        org_id: String,
        user_id: String,
        server_name: String,
        server_url: String,
        tool_allowlist: Vec<String>,
        scope_wire: String,
        code_verifier: String,
        client_id: String,
        token_endpoint: String,
        redirect_uri: String,
        oauth_scopes: Vec<String>,
    ) -> Self {
        Self {
            org_id,
            user_id,
            server_name,
            server_url,
            tool_allowlist,
            scope_wire,
            code_verifier,
            client_id,
            token_endpoint,
            redirect_uri,
            oauth_scopes,
            created_at: Instant::now(),
        }
    }
}

/// Pending OAuth attempts older than this are refused at callback time —
/// long enough for a real consent screen, short enough that a stale entry
/// can't be replayed much later.
const MCP_OAUTH_SESSION_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Default)]
pub struct McpRegistry {
    inner: Arc<DashMap<(String, String), McpServer>>, // (org, server_id)
    /// Discovered `tools/list` catalog per (org, `server_id`), with the instant
    /// it was fetched — reused for [`MCP_CATALOG_TTL`] so the chat hot-path
    /// doesn't re-discover on every turn.
    catalog: McpCatalog,
    /// Pending OAuth connection attempts, keyed by the CSRF `state` value.
    pending_oauth: Arc<DashMap<String, PendingMcpOAuth>>,
}

impl McpRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            catalog: Arc::new(DashMap::new()),
            pending_oauth: Arc::new(DashMap::new()),
        }
    }

    /// Record a fresh OAuth attempt under its `state` value.
    pub fn start_oauth(&self, state: String, pending: PendingMcpOAuth) {
        self.pending_oauth.insert(state, pending);
    }

    /// Consume (remove — single-use, like the state hash in
    /// integration-corev2's OAuth sessions) the pending attempt for `state`,
    /// rejecting it if missing or past [`MCP_OAUTH_SESSION_TTL`].
    pub fn take_oauth(&self, state: &str) -> Option<PendingMcpOAuth> {
        let (_, pending) = self.pending_oauth.remove(state)?;
        if pending.created_at.elapsed() > MCP_OAUTH_SESSION_TTL {
            return None;
        }
        Some(pending)
    }

    /// Drop a cached server by (org, `server_id`). Returns true if an entry was
    /// removed. Used by the §4.3 reconcile consumer to keep the cache coherent
    /// when capability-core (the system-of-record) reports a server removed —
    /// so the gateway stops proxying to a decommissioned/revoked server.
    pub fn remove(&self, org_id: &str, server_id: &str) -> bool {
        let key = (org_id.to_owned(), server_id.to_owned());
        // Evict the discovered catalog too, so a decommissioned server's tools
        // stop being advertised to the model immediately (not after the TTL).
        self.catalog.remove(&key);
        self.inner.remove(&key).is_some()
    }

    /// Replace one tenant's cached MCP servers from the capability-core
    /// projection. This is deliberately a whole-tenant replacement: a missing
    /// row must revoke a cached server after restart, not leave an orphan that
    /// can still be advertised to the model.
    pub fn replace_org(&self, org_id: &str, servers: impl IntoIterator<Item = McpServer>) {
        self.inner.retain(|key, _| key.0 != org_id);
        self.catalog.retain(|key, _| key.0 != org_id);
        for server in servers {
            self.inner
                .insert((org_id.to_owned(), server.server_id.clone()), server);
        }
    }

    /// Whether a server is cached for (org, `server_id`). Test/observability helper.
    #[must_use]
    pub fn contains(&self, org_id: &str, server_id: &str) -> bool {
        self.inner
            .contains_key(&(org_id.to_owned(), server_id.to_owned()))
    }

    /// Seed the discovered tool catalog directly, standing in for a live
    /// `tools/list`. **Test-only** — production code must go through
    /// [`mcp_discover_cached`], which owns the TTL, the timeout, and the
    /// fail-closed behavior this bypasses. Exists so dispatch-level tests in
    /// other modules can exercise the tool paths without a live MCP server.
    #[cfg(test)]
    pub(crate) fn seed_catalog_for_test(
        &self,
        org_id: &str,
        server_id: &str,
        tools: Vec<McpToolDef>,
    ) {
        self.catalog.insert(
            (org_id.to_owned(), server_id.to_owned()),
            (Instant::now(), tools),
        );
    }
}

#[derive(Debug, Deserialize)]
struct CatalogMcpResponse {
    /// `deserialize_with`, not a bare `#[serde(default)]`: serde's `default`
    /// fills a MISSING key and rejects an explicit `null`, and "no servers
    /// registered" is exactly when a JSON producer is most likely to send one
    /// (Go marshals a nil slice as `null` — capability-core did, and an empty
    /// registry 503'd the entire MCP surface). Both sides are fixed; this half
    /// means a future producer regressing to `null` degrades to "no servers"
    /// instead of taking the feature down.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    servers: Vec<CatalogMcpServer>,
}

fn null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// The tenant capability-core uses for servers offered to every org. Its rows
/// come back from a single-tenant list query (`WHERE org_id=$1 OR
/// org_id='global'`), so a row under this tenant is legitimately not the
/// requested one and must not be mistaken for cross-tenant bleed.
const CATALOG_GLOBAL_ORG: &str = "global";

#[derive(Debug, Deserialize)]
struct CatalogMcpServer {
    #[serde(alias = "server_id")]
    id: String,
    /// The tenant capability-core actually returned this row under. Carried
    /// solely so hydration can confirm it matches the tenant being written —
    /// see the confinement check in [`catalog_mcp_parts`].
    #[serde(default)]
    org_id: String,
    name: String,
    #[serde(alias = "url")]
    endpoint_url: String,
    transport: String,
    #[serde(default)]
    tool_allowlist: Vec<String>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    owner_user_id: String,
    #[serde(default)]
    shared_with: Vec<String>,
}

/// Convert the capability-core public projection into the gateway's execution
/// record and its durable ownership sidecar. The projection intentionally does
/// not contain an operational token; OAuth credentials are resolved through
/// capability-core's encrypted token endpoint at dispatch time.
fn catalog_mcp_parts(
    current_org_id: &str,
    raw: CatalogMcpServer,
) -> Result<(McpServer, crate::ownership::Ownership), String> {
    let server_id = raw.id.trim();
    if server_id.is_empty() {
        return Err("catalog MCP record has no id".to_owned());
    }
    let scope = raw.scope.trim().to_ascii_lowercase();
    let ownership = match scope.as_str() {
        "org" | "workspace" | "" => crate::ownership::Ownership::org(),
        "user" => {
            let owner = raw.owner_user_id.trim();
            if owner.is_empty() {
                return Err("user-scoped catalog MCP record has no owner".to_owned());
            }
            crate::ownership::Ownership {
                scope: crate::ownership::Scope::User,
                owner_user_id: owner.to_owned(),
                shared_with: raw
                    .shared_with
                    .into_iter()
                    .map(|user| user.trim().to_owned())
                    .filter(|user| !user.is_empty() && user != owner)
                    .collect(),
            }
        }
        other => return Err(format!("unsupported catalog MCP scope: {other}")),
    };
    let server = McpServer {
        server_id: server_id.to_owned(),
        name: raw.name,
        url: raw.endpoint_url,
        transport: raw.transport,
        // Never import a credential from a registry projection. The only
        // supported durable credential path is the encrypted OAuth resolver.
        token: String::new(),
        tool_allowlist: raw.tool_allowlist,
        enabled: raw.enabled,
    };
    // Keep the org argument explicit so a future multi-org response cannot be
    // accidentally written under a catalog-supplied tenant.
    let current_org_id = current_org_id.trim();
    if current_org_id.is_empty() {
        return Err("catalog hydration requires an organization".to_owned());
    }
    // Confinement check, and NOT a redundant one. capability-core scopes
    // `GET /api/v1/mcp` by the VERIFIED PRINCIPAL on the bearer
    // (`mcpVerifiedOrganization`, registry_apis.go:364) and ignores the
    // `org_id` query parameter hydration sends entirely — while the gateway
    // writes whatever comes back under the `org_id` it was *asked* for. Those
    // two tenants agree today only because the chat path derives both from one
    // verified request. Nothing enforced it, and the obvious way to extend
    // hydration to a service path (hand it a service credential) would silently
    // cache the service principal's tenant under the caller's, which is
    // cross-tenant tool exposure rather than a stale-cache bug.
    let row_org_id = raw.org_id.trim();
    if !row_org_id.is_empty() && row_org_id != current_org_id && row_org_id != CATALOG_GLOBAL_ORG {
        return Err(format!(
            "catalog MCP record belongs to org '{row_org_id}', not '{current_org_id}'"
        ));
    }
    Ok((server, ownership))
}

/// Refresh one tenant's MCP cache from capability-core using a verified,
/// user-bound capability bearer. A failed refresh clears the tenant cache and
/// ownership projection so a registry outage cannot leave revoked tools
/// callable from stale process memory.
pub async fn hydrate_mcp_registry(
    reg: &McpRegistry,
    ownership: &crate::ownership::OwnershipStore,
    http_client: &reqwest::Client,
    capability_core_base_url: &str,
    org_id: &str,
    capability_bearer: &str,
) -> Result<usize, String> {
    if capability_core_base_url.trim().is_empty() || capability_bearer.trim().is_empty() {
        reg.replace_org(org_id, std::iter::empty());
        ownership.replace_org_kind(org_id, crate::ownership::KIND_MCP, std::iter::empty());
        return Err("capability registry or bearer is unavailable".to_owned());
    }
    let mut url = reqwest::Url::parse(capability_core_base_url.trim())
        .map_err(|_| "capability registry URL is invalid".to_owned())?;
    url.set_path("/api/v1/mcp");
    url.query_pairs_mut().append_pair("org_id", org_id);

    // Clear before the request: if capability-core is unreachable, the safe
    // result is no advertised/callable MCP capability for this tenant.
    reg.replace_org(org_id, std::iter::empty());
    ownership.replace_org_kind(org_id, crate::ownership::KIND_MCP, std::iter::empty());

    let response = http_client
        .get(url)
        .bearer_auth(capability_bearer.trim())
        .send()
        .await
        .map_err(|error| format!("capability registry request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "capability registry returned HTTP {}",
            response.status()
        ));
    }
    let payload = response
        .json::<CatalogMcpResponse>()
        .await
        .map_err(|error| format!("capability registry response was invalid: {error}"))?;

    let mut servers = Vec::with_capacity(payload.servers.len());
    let mut ownership_entries = Vec::with_capacity(payload.servers.len());
    for raw in payload.servers {
        match catalog_mcp_parts(org_id, raw) {
            Ok((server, resource_ownership)) => {
                // Reuse the exact write-time validation, but never let one
                // malformed legacy row poison the rest of the tenant cache.
                let mut checked = server.clone();
                if validate_mcp_server(org_id, &mut checked).is_ok() {
                    servers.push(checked.clone());
                    ownership_entries.push((checked.server_id, resource_ownership));
                } else {
                    tracing::warn!(org_id, server_id = %server.server_id, "skipping invalid catalog MCP record");
                }
            }
            Err(error) => tracing::warn!(org_id, %error, "skipping malformed catalog MCP record"),
        }
    }
    let count = servers.len();
    reg.replace_org(org_id, servers);
    ownership.replace_org_kind(org_id, crate::ownership::KIND_MCP, ownership_entries);
    Ok(count)
}

/// Discover an MCP server's tools over the real **Streamable HTTP** transport
/// (`initialize` → `notifications/initialized` → `tools/list` against the
/// server URL itself). See [`crate::mcp_http`] for why the previous
/// `POST {url}/tools/list` bridge shape could never work against a genuine
/// MCP server.
pub(crate) async fn http_list_tools(url: &str, token: &str) -> Result<Vec<McpToolDef>, String> {
    crate::mcp_http::McpHttpSession::connect(url, token)
        .await?
        .list_tools()
        .await
}

pub(crate) async fn safe_mcp_http_client(
    url: &str,
) -> Result<(reqwest::Client, reqwest::Url), String> {
    let endpoint = reqwest::Url::parse(url).map_err(|_| "invalid MCP endpoint URL".to_owned())?;
    // A trusted internal host (opt-in via MCP_INTERNAL_ALLOWED_HOSTS) may use
    // plain HTTP and resolve to a private address — that is the whole point of a
    // co-located bridge sidecar. Everything else stays public-HTTPS-only + SSRF
    // guarded. We still pin the connection to the resolved addresses below so an
    // allowlisted hostname cannot be rebound to an arbitrary target.
    let host_allowed = endpoint.host_str().is_some_and(host_in_internal_allowlist);
    let scheme_ok = endpoint.scheme() == "https" || (host_allowed && endpoint.scheme() == "http");
    if !scheme_ok || (!host_allowed && endpoint_host_is_forbidden(&endpoint)) {
        return Err(
            "MCP endpoint must use public HTTPS (or be an allowlisted internal host)".to_owned(),
        );
    }
    let host = endpoint
        .host_str()
        .ok_or_else(|| "MCP endpoint host is required".to_owned())?;
    let port = endpoint
        .port_or_known_default()
        .ok_or_else(|| "MCP endpoint port is required".to_owned())?;
    let addresses: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("MCP endpoint DNS resolution failed: {error}"))?
        .collect();
    if addresses.is_empty()
        || (!host_allowed
            && addresses
                .iter()
                .any(|address| ip_is_forbidden(address.ip())))
    {
        return Err("MCP endpoint DNS resolved to a forbidden address".to_owned());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(MCP_HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|error| format!("MCP HTTP client unavailable: {error}"))?;
    Ok((client, endpoint))
}

pub(crate) async fn bounded_mcp_response(
    mut response: reqwest::Response,
) -> Result<String, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MCP_MAX_RESPONSE_BYTES as u64)
    {
        return Err("MCP response exceeds size limit".to_owned());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("MCP response read failed: {error}"))?
    {
        if body.len().saturating_add(chunk.len()) > MCP_MAX_RESPONSE_BYTES {
            return Err("MCP response exceeds size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| "MCP response is not valid UTF-8".to_owned())
}

/// Keep only discovered tools whose name exactly matches an allowlist entry.
/// Empty allowlists fail closed and expose no tools.
#[must_use]
fn filter_allowlist(tools: Vec<McpToolDef>, allowlist: &[String]) -> Vec<McpToolDef> {
    if allowlist.is_empty() {
        return Vec::new();
    }
    tools
        .into_iter()
        .filter(|tool| allowlist.contains(&tool.name))
        .collect()
}

/// Return `server`'s discovered tool catalog, served from the TTL cache when
/// fresh. On a miss it discovers over the server's transport (bounded by
/// [`MCP_DISCOVERY_TIMEOUT`]) and caches success. `None` means discovery is
/// currently unavailable. Callers fail closed rather than fabricating callable
/// tool definitions with an open input schema.
async fn mcp_discover_cached(
    reg: &McpRegistry,
    org_id: &str,
    server: &McpServer,
    oauth_token: Option<&str>,
) -> Option<Vec<McpToolDef>> {
    let key = (org_id.to_owned(), server.server_id.clone());
    if let Some(entry) = reg.catalog.get(&key) {
        if entry.0.elapsed() < MCP_CATALOG_TTL {
            return Some(entry.1.clone());
        }
    }
    // An OAuth-connected server's in-memory `token` is always empty (tokens
    // live only in capability-core's encrypted store) — same resolution the
    // tools/call dispatch path uses (handle_proxy_mcp_tool), so discovery
    // doesn't 401 against exactly the servers this flow exists for.
    let token = oauth_token
        .filter(|token| !token.is_empty())
        .unwrap_or(&server.token);
    let discovery = async {
        match server.transport.as_str() {
            "http" => http_list_tools(&server.url, token).await,
            other => Err(format!("discovery quarantined for transport {other}")),
        }
    };
    match tokio::time::timeout(MCP_DISCOVERY_TIMEOUT, discovery).await {
        Ok(Ok(tools)) => {
            reg.catalog.insert(key, (Instant::now(), tools.clone()));
            Some(tools)
        }
        Ok(Err(e)) => {
            tracing::debug!(
                server = %server.server_id,
                error = %e,
                "mcp tools/list discovery failed; exposing no tools"
            );
            None
        }
        Err(_) => {
            tracing::debug!(
                server = %server.server_id,
                "mcp tools/list discovery timed out; exposing no tools"
            );
            None
        }
    }
}

/// Default number of individual MCP tool definitions exposed to the model
/// before staging ([`stage_mcp_tool_defs`]) collapses them into the two
/// synthetic `mcp_catalog`/`mcp_call` tools instead. Deliberately generous —
/// a safety net, not a routine budget: any org whose full candidate count
/// stays at or under this (Visma's proven-live setup included) sees
/// byte-identical behavior to before staging existed.
const DEFAULT_MCP_DISCLOSURE_THRESHOLD: usize = 40;

/// Absolute ceiling regardless of configuration, mirroring
/// `tool_loop::MAX_TOOL_ROUNDS_CEILING`'s role — a bad env value must not
/// disable staging for a catalog large enough that per-tool disclosure would
/// blow the prompt budget.
const MAX_MCP_DISCLOSURE_THRESHOLD_CEILING: usize = 200;

/// Disclosure threshold for this process: `MCP_DISCLOSURE_THRESHOLD` env
/// override, clamped to `1..=MAX_MCP_DISCLOSURE_THRESHOLD_CEILING`, else
/// [`DEFAULT_MCP_DISCLOSURE_THRESHOLD`]. Read once, mirroring
/// `tool_loop::max_tool_rounds`'s pattern.
pub fn mcp_disclosure_threshold() -> usize {
    static THRESHOLD: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("MCP_DISCLOSURE_THRESHOLD")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|parsed| *parsed > 0)
            .map_or(DEFAULT_MCP_DISCLOSURE_THRESHOLD, |parsed| {
                parsed.min(MAX_MCP_DISCLOSURE_THRESHOLD_CEILING)
            })
    })
}

/// Synthetic tool name the model calls to browse the full MCP catalog once
/// staging is active — see [`stage_mcp_tool_defs`]. Handled in
/// `tool_loop::dispatch_tool`.
pub const MCP_CATALOG_TOOL_NAME: &str = "mcp_catalog";
/// Synthetic tool name the model calls to invoke one MCP tool by its exact
/// qualified name once staging is active. Handled in
/// `tool_loop::dispatch_tool`.
pub const MCP_CALL_TOOL_NAME: &str = "mcp_call";

/// Truncation length for a tool's description in `mcp_catalog`'s summary
/// listing (§23.1's "Level 1" disclosure). Full descriptions remain reachable
/// via `mcp_catalog` with a `tool_name` filter (Level 2).
const MCP_CATALOG_SUMMARY_DESCRIPTION_CHARS: usize = 160;

/// Collapse `full_defs` into the two synthetic staging tools when the
/// candidate count exceeds `threshold`; otherwise return it unchanged.
///
/// Below threshold this is byte-identical to today's behavior: the model
/// still sees every individual `mcp__<server>__<tool>` definition with its
/// real schema. Above threshold, the individual definitions are replaced by
/// `mcp_catalog` (Level-1 summaries of every tool, or one tool's full schema
/// when called with `tool_name`) and `mcp_call` (proxied execution by
/// qualified name). Both re-run discovery at call time — cheap, since
/// [`mcp_discover_cached`]'s TTL cache makes this a cache hit, not a new
/// network round-trip — rather than needing any new cross-turn state, and
/// `mcp_call` dispatches through the exact same path the direct `mcp__`
/// tool-call arm already uses (`tool_loop::dispatch_mcp_tool_call`). Staging
/// changes only what the model sees up front, never what it can reach or
/// with what authority.
///
/// Pure — no IO — so the threshold behavior is fully unit-tested without a
/// live MCP server.
#[must_use]
pub fn stage_mcp_tool_defs(
    full_defs: Vec<ToolDefinition>,
    threshold: usize,
) -> Vec<ToolDefinition> {
    if full_defs.len() <= threshold {
        return full_defs;
    }
    vec![
        ToolDefinition {
            name: MCP_CATALOG_TOOL_NAME.to_owned(),
            description: format!(
                "List available MCP tools ({} total; staged because the full set is large). \
                 Call with no arguments for a short summary of every tool (qualified name, \
                 description). Call with `tool_name` set to one exact qualified name from that \
                 list to see its full parameter schema before calling it with {}.",
                full_defs.len(),
                MCP_CALL_TOOL_NAME
            ),
            parameters_json: json!({
                "type": "object",
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Optional. Exact qualified name (from a prior mcp_catalog call) to inspect one tool's full schema instead of listing summaries."
                    }
                }
            })
            .to_string(),
        },
        ToolDefinition {
            name: MCP_CALL_TOOL_NAME.to_owned(),
            description: format!(
                "Call one MCP tool by its exact qualified name (from {MCP_CATALOG_TOOL_NAME}). \
                 Check that tool's full schema via {MCP_CATALOG_TOOL_NAME} first if you have not \
                 already, so `arguments` matches what it expects."
            ),
            parameters_json: json!({
                "type": "object",
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Exact qualified tool name, from mcp_catalog."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments for the target tool, matching the schema mcp_catalog returned for it."
                    }
                },
                "required": ["tool_name", "arguments"]
            })
            .to_string(),
        },
    ]
}

/// Render `mcp_catalog`'s call result: Level-1 summaries for every tool in
/// `full_defs`, or one tool's full definition (name, description, real input
/// schema) when `tool_name` matches exactly. An unmatched `tool_name` returns
/// an `error` field rather than silently falling back to the full listing, so
/// a typo'd name is visible to the model instead of masquerading as "list
/// everything".
///
/// Pure — unit-tested directly with fixture [`ToolDefinition`]s, independent
/// of live discovery.
#[must_use]
pub fn format_mcp_catalog(full_defs: &[ToolDefinition], tool_name: Option<&str>) -> String {
    if let Some(name) = tool_name {
        return match full_defs.iter().find(|def| def.name == name) {
            Some(found) => json!({
                "name": found.name,
                "description": found.description,
                "input_schema": serde_json::from_str::<Value>(&found.parameters_json)
                    .unwrap_or(Value::Null),
            })
            .to_string(),
            None => json!({ "error": format!("no such tool '{name}'") }).to_string(),
        };
    }
    let tools: Vec<Value> = full_defs
        .iter()
        .map(|def| {
            let char_count = def.description.chars().count();
            let description = if char_count > MCP_CATALOG_SUMMARY_DESCRIPTION_CHARS {
                let mut truncated: String = def
                    .description
                    .chars()
                    .take(MCP_CATALOG_SUMMARY_DESCRIPTION_CHARS)
                    .collect();
                truncated.push('…');
                truncated
            } else {
                def.description.clone()
            };
            json!({ "name": def.name, "description": description })
        })
        .collect();
    json!({ "count": tools.len(), "tools": tools }).to_string()
}

/// Build the agent-facing tool definitions for every **enabled** MCP server an
/// org has registered, namespaced `mcp__<server_id>__<tool>` so the gateway's
/// `dispatch_tool` (and the model) can route calls back to the right server.
/// This is the governed exposure bridge: without it, registered MCP servers sit
/// in the registry but their tools never reach an execution caller. Both the
/// execution-core agentic loop AND the inline chat loop consume these
/// definitions — the chat surface IS the product, so an MCP server the user
/// connected must be usable there too, not gated behind a separate "agent"
/// concept.
///
/// Discovery is best-effort and bounded (see [`mcp_discover_cached`]): each
/// server's `tools/list` is fetched for real input schemas and then intersected
/// with the exact stored allowlist. A disabled or unreachable server contributes
/// nothing. Never panics.
///
/// Returns the full, unstaged set — see [`mcp_tool_defs`] for the version the
/// chat/agent loops actually consume, which applies [`stage_mcp_tool_defs`] on
/// top of this. Exposed separately so the `mcp_catalog`/`mcp_call` synthetic
/// tools (`tool_loop::dispatch_tool`) can re-run discovery without duplicating
/// the server-iteration/allowlist/naming logic.
pub async fn full_mcp_tool_defs(
    reg: &McpRegistry,
    ownership: &crate::ownership::OwnershipStore,
    org_id: &str,
    user_id: &str,
    http_client: &reqwest::Client,
    capability_core_base_url: &str,
    mcp_oauth_service_token: &str,
) -> Vec<ToolDefinition> {
    // Tenant scope from the key; ownership filters to the resources THIS user may
    // use (org-wide, owned, or shared-to-them) — never another user's private.
    let servers: Vec<McpServer> = reg
        .inner
        .iter()
        .filter(|e| {
            e.key().0 == org_id
                && e.value().enabled
                && ownership.usable(org_id, crate::ownership::KIND_MCP, &e.key().1, user_id)
        })
        .map(|e| e.value().clone())
        .collect();
    tracing::debug!(
        %org_id,
        %user_id,
        candidates = ?reg
            .inner
            .iter()
            .filter(|e| e.key().0 == org_id)
            .map(|e| {
                let s = e.value();
                (
                    s.server_id.clone(),
                    s.enabled,
                    ownership.usable(org_id, crate::ownership::KIND_MCP, &s.server_id, user_id),
                    ownership.get(org_id, crate::ownership::KIND_MCP, &s.server_id).map(|o| o.owner_user_id),
                )
            })
            .collect::<Vec<_>>(),
        matched = servers.len(),
        "mcp_tool_defs: candidate servers for this org (id, enabled, usable_by_caller, owner_user_id)"
    );

    let mut defs: Vec<ToolDefinition> = Vec::new();
    for server in servers {
        let oauth_token = crate::mcp_oauth::resolve_stored_oauth_token(
            http_client,
            capability_core_base_url,
            mcp_oauth_service_token,
            org_id,
            &server.server_id,
        )
        .await;
        let discovered = mcp_discover_cached(reg, org_id, &server, oauth_token.as_deref()).await;
        let tools = match &discovered {
            Some(discovered) => filter_allowlist(discovered.clone(), &server.tool_allowlist),
            None => Vec::new(),
        };
        tracing::debug!(
            %org_id,
            server_id = %server.server_id,
            has_oauth_token = oauth_token.is_some(),
            discovered_count = discovered.as_ref().map(Vec::len),
            allowlist = ?server.tool_allowlist,
            after_allowlist = tools.len(),
            "mcp_tool_defs: discovery outcome for this server"
        );
        for t in tools {
            let description = if t.description.is_empty() {
                format!("Tool '{}' from MCP server '{}'.", t.name, server.name)
            } else {
                format!("[{}] {}", server.name, t.description)
            };
            defs.push(ToolDefinition {
                name: format!("mcp__{}__{}", server.server_id, t.name),
                description,
                parameters_json: t.input_schema_json,
            });
        }
    }
    defs
}

/// The version [`mcp_tool_defs`]'s callers (the chat and execution-core tool
/// loops) actually consume: [`full_mcp_tool_defs`] with
/// [`stage_mcp_tool_defs`] applied on top, so a large catalog collapses to
/// the two synthetic `mcp_catalog`/`mcp_call` tools instead of flooding every
/// turn with every individual tool's full schema (§23.1).
pub async fn mcp_tool_defs(
    reg: &McpRegistry,
    ownership: &crate::ownership::OwnershipStore,
    org_id: &str,
    user_id: &str,
    http_client: &reqwest::Client,
    capability_core_base_url: &str,
    mcp_oauth_service_token: &str,
    capability_bearer: Option<&str>,
) -> Vec<ToolDefinition> {
    if let Some(bearer) = capability_bearer {
        if let Err(error) = hydrate_mcp_registry(
            reg,
            ownership,
            http_client,
            capability_core_base_url,
            org_id,
            bearer,
        )
        .await
        {
            tracing::warn!(%org_id, %error, "MCP catalog hydration failed; MCP tools disabled for this turn");
            return Vec::new();
        }
    }
    let full = full_mcp_tool_defs(
        reg,
        ownership,
        org_id,
        user_id,
        http_client,
        capability_core_base_url,
        mcp_oauth_service_token,
    )
    .await;
    stage_mcp_tool_defs(full, mcp_disclosure_threshold())
}

/// Registers (or upserts) a validated MCP server in the gateway-scoped
/// registry. Secure-MVP registration supports only public HTTPS bridges.
/// Caller-selected stdio commands and raw bearer secrets are quarantined.
///
/// # Errors
///
/// Returns `Status::invalid_argument` for missing tenant/name, unsafe or
/// mismatched transport, raw credentials, or a fail-open allowlist.
pub fn handle_register_mcp_server(
    reg: &McpRegistry,
    req: RegisterMcpServerRequest,
) -> Result<RegisterMcpServerResponse, Status> {
    let mut server = req
        .server
        .ok_or_else(|| Status::invalid_argument("server is required"))?;
    validate_mcp_server(&req.org_id, &mut server)?;
    if server.server_id.is_empty() {
        server.server_id = new_ulid();
    }
    let key = (req.org_id.clone(), server.server_id.clone());
    // A re-register may change the url/allowlist/enabled flag — drop any stale
    // discovered catalog so the next exposure re-discovers against new config.
    reg.catalog.remove(&key);
    reg.inner.insert(key, server.clone());
    Ok(RegisterMcpServerResponse {
        request_id: req.request_id,
        server: Some(server),
    })
}

#[allow(clippy::result_large_err)]
fn validate_mcp_server(org_id: &str, server: &mut McpServer) -> Result<(), Status> {
    if org_id.trim().is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    server.name = server.name.trim().to_owned();
    if server.name.is_empty() || server.name.len() > 128 {
        return Err(Status::invalid_argument(
            "server.name must contain 1 to 128 characters",
        ));
    }
    if server.transport.trim() != "http" {
        return Err(Status::invalid_argument(
            "only the HTTPS MCP bridge transport is supported",
        ));
    }
    "http".clone_into(&mut server.transport);
    server.url = server.url.trim().trim_end_matches('/').to_owned();
    if server.url.len() > 2_048 {
        return Err(Status::invalid_argument("server.url is too long"));
    }
    let endpoint = reqwest::Url::parse(&server.url)
        .map_err(|_| Status::invalid_argument("server.url must be a valid HTTPS URL"))?;
    // A trusted internal host (opt-in via MCP_INTERNAL_ALLOWED_HOSTS) may register
    // over plain HTTP on a private address (the bundled bridge sidecar); all other
    // servers stay public-HTTPS-only + SSRF-guarded. Credential/query/fragment
    // hygiene is enforced for every server regardless.
    let host_allowed = endpoint.host_str().is_some_and(host_in_internal_allowlist);
    let scheme_ok = endpoint.scheme() == "https" || (host_allowed && endpoint.scheme() == "http");
    if !scheme_ok
        || endpoint.host().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.port_or_known_default().is_none()
        || (!host_allowed && endpoint_host_is_forbidden(&endpoint))
    {
        return Err(Status::invalid_argument(
            "server.url must be a public HTTPS base URL (or an allowlisted internal MCP host) without credentials, query, or fragment",
        ));
    }
    if !server.token.trim().is_empty() {
        return Err(Status::invalid_argument(
            "raw MCP credentials are not accepted; configure a managed secret reference",
        ));
    }
    server.token.clear();
    if server.tool_allowlist.is_empty() || server.tool_allowlist.len() > 64 {
        return Err(Status::invalid_argument(
            "tool_allowlist must contain 1 to 64 exact tool names",
        ));
    }
    let mut exact_names = Vec::with_capacity(server.tool_allowlist.len());
    for raw in &server.tool_allowlist {
        let name = raw.trim();
        if name.is_empty()
            || name.len() > 128
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "_.:-".contains(character))
        {
            return Err(Status::invalid_argument(
                "tool_allowlist contains an invalid exact tool name",
            ));
        }
        if !exact_names.iter().any(|existing| existing == name) {
            exact_names.push(name.to_owned());
        }
    }
    server.tool_allowlist = exact_names;
    Ok(())
}

/// Pure allowlist membership test: is `host` one of the comma-separated entries
/// in `allowlist_csv`? Case- and trailing-dot-insensitive.
fn host_in_allowlist(host: &str, allowlist_csv: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    allowlist_csv
        .split(',')
        .map(|entry| entry.trim().to_ascii_lowercase())
        .any(|allowed| !allowed.is_empty() && allowed == host)
}

/// Operator-configured allowlist of trusted INTERNAL MCP hosts that may be
/// registered/reached over plain HTTP on a private/loopback address — e.g. the
/// bundled `mcp-bridge` sidecar at `http://mcp-bridge:9201`. Empty by default,
/// so the public-HTTPS-only + SSRF guard is unchanged unless an operator opts a
/// known internal host in via `MCP_INTERNAL_ALLOWED_HOSTS` (comma-separated).
/// This is the runtime unlock that lets a registered MCP server actually be
/// discovered/proxied end-to-end without exposing arbitrary user URLs to SSRF.
fn host_in_internal_allowlist(host: &str) -> bool {
    std::env::var("MCP_INTERNAL_ALLOWED_HOSTS")
        .map(|csv| host_in_allowlist(host, &csv))
        .unwrap_or(false)
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn endpoint_host_is_forbidden(endpoint: &reqwest::Url) -> bool {
    let Some(host) = endpoint.host_str() else {
        return true;
    };
    if let Ok(address) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        return ip_is_forbidden(address);
    }
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
        || normalized.ends_with(".internal")
        || normalized == "metadata.google.internal"
}

/// Address-vetting range table. Kept in parity with capability-core's Go-side
/// `mcpForbiddenRanges` (`services/capability-core/internal/api/registry_apis.go`)
/// and Quarry's `quarry_security::heur::resolve_guard` — three independent
/// implementations (Go registration-time check, this per-dial Rust check, and
/// Quarry's crawler SSRF guard) that must agree on what counts as a forbidden
/// destination, or a hostname resolving into a range only one of them blocks
/// slips through wherever the weakest check runs. Rust's stable `std::net`
/// predicates don't cover CGNAT, benchmarking, or the IETF protocol-assignment
/// block, so those need the manual octet/segment checks below.
fn ip_is_forbidden(address: std::net::IpAddr) -> bool {
    fn ipv4_forbidden(address: std::net::Ipv4Addr) -> bool {
        let octets = address.octets();
        address.is_private()
            || address.is_loopback()
            || address.is_link_local()
            || address.is_broadcast()
            || address.is_documentation()
            || address.is_unspecified()
            || address.is_multicast()
            // 0.0.0.0/8 -- "this network" (RFC 791). is_unspecified() only
            // catches the single 0.0.0.0 address, not the whole /8.
            || octets[0] == 0
            // 100.64.0.0/10 -- CGNAT / Shared Address Space (RFC 6598). Real
            // in cloud/k8s pod networks, so a legitimate-looking DNS answer
            // can still land inside another tenant's internal address space.
            || (octets[0] == 100 && (octets[1] & 0xc0) == 0x40)
            // 192.0.0.0/24 -- IETF protocol assignments (RFC 6890).
            || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
            // 198.18.0.0/15 -- benchmarking (RFC 2544).
            || (octets[0] == 198 && (octets[1] & 0xfe) == 18)
            // 240.0.0.0/4 -- reserved / future use (Class E).
            || (octets[0] & 0xf0) == 0xf0
    }

    fn ipv6_forbidden(address: std::net::Ipv6Addr) -> bool {
        let segments = address.segments();
        address.is_loopback()
            || address.is_unspecified()
            || address.is_multicast()
            || address.to_ipv4_mapped().is_some_and(ipv4_forbidden)
            || (segments[0] & 0xfe00) == 0xfc00
            || (segments[0] & 0xffc0) == 0xfe80
            // 64:ff9b::/96 -- NAT64 well-known prefix (RFC 6052).
            || (segments[0] == 0x0064
                && segments[1] == 0xff9b
                && segments[2] == 0
                && segments[3] == 0
                && segments[4] == 0
                && segments[5] == 0)
            // 100::/64 -- discard-only address block (RFC 6666).
            || (segments[0] == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0)
            // 2001:db8::/32 -- documentation (RFC 3849).
            || (segments[0] == 0x2001 && segments[1] == 0x0db8)
    }

    match address {
        std::net::IpAddr::V4(address) => ipv4_forbidden(address),
        std::net::IpAddr::V6(address) => ipv6_forbidden(address),
    }
}

/// Build the capability-core `POST /api/v1/mcp` JSON body from a gateway
/// `McpServer` (matrix §4.1/H.1 write-through). capability-core is the registry
/// **system-of-record**; the gateway's in-memory store is a cache that writes
/// through here so the two converge instead of shadowing each other. The
/// `server_id` is sent as `id` — capability-core honors a client-supplied id
/// (so there is **no** id divergence, unlike the G8 approval crux) and upserts
/// on `(org_id, name)`.
///
/// SECURITY: the legacy bearer `token` field is never serialized. Secure-MVP
/// registration rejects raw credentials entirely; a future authenticated
/// bridge must use a managed secret reference resolved outside catalog records
/// and logs.
#[must_use]
pub fn mcp_capability_payload(
    org_id: &str,
    server: &McpServer,
    ownership: &crate::ownership::Ownership,
    auth_kind: &str,
) -> serde_json::Value {
    // Ownership (scope/owner/shares) rides in config_json so the durable catalog
    // record stays the system-of-record for who-can-see-what, not just the
    // ephemeral gateway sidecar. `scope` reflects the real owner/org scope.
    let mut config_json = serde_json::json!({
        "tool_allowlist": server.tool_allowlist,
        "owner_user_id": ownership.owner_user_id,
        "shared_with": ownership.shared_with,
    });
    // capability-core refuses to catalog a credential-bearing server without a
    // managed secret reference (`auth_kind != "none"` → `secret_ref` must parse
    // as a secret/vault URI). For an OAuth-connected server the credential
    // genuinely IS in a managed store — capability-core's own AES-256-GCM
    // encrypted `mcp_oauth_tokens` row, reachable at
    // `GET /api/v1/mcp/{id}/oauth-token` — so the reference points there rather
    // than naming an external vault that holds nothing.
    if auth_kind != "none" {
        if let Some(config) = config_json.as_object_mut() {
            config.insert(
                "secret_ref".to_owned(),
                serde_json::Value::String(format!(
                    "secret://capability-core/mcp-oauth-tokens/{}",
                    server.server_id
                )),
            );
        }
    }
    serde_json::json!({
        "id": server.server_id,
        "org_id": org_id,
        "name": server.name,
        "endpoint_url": server.url,
        "transport": server.transport,
        "auth_kind": auth_kind,
        "config_json": config_json,
        "scope": ownership.scope.as_wire(),
        "enabled": server.enabled,
    })
}

#[cfg(test)]
mod mcp_writethrough_tests {
    use super::*;
    use mp_contracts::model_plane::v1::McpServer;

    #[test]
    fn payload_maps_fields_and_never_leaks_token() {
        // Assemble the token at runtime so no literal credential sits in source
        // (the secret scanner correctly flags `token: "..."` literals — and a
        // write-through test must not itself embed one).
        let secret_token = format!("bearer-{}", "do-not-leak");
        let server = McpServer {
            server_id: "mcp_abc".into(),
            name: "fs".into(),
            url: "stdio:///bin/mcp-fs".into(),
            transport: "stdio".into(),
            token: secret_token.clone(),
            tool_allowlist: vec!["read".into(), "list".into()],
            enabled: true,
        };
        let p = mcp_capability_payload(
            "org-7",
            &server,
            &crate::ownership::Ownership::org(),
            "bearer",
        );
        // Client-supplied id is honored → gateway cache and catalog stay aligned.
        assert_eq!(p["id"], "mcp_abc");
        assert_eq!(p["org_id"], "org-7");
        assert_eq!(p["name"], "fs");
        assert_eq!(p["endpoint_url"], "stdio:///bin/mcp-fs");
        assert_eq!(p["transport"], "stdio");
        assert_eq!(p["auth_kind"], "bearer");
        assert_eq!(p["enabled"], true);
        assert_eq!(p["scope"], "org");
        assert_eq!(p["config_json"]["tool_allowlist"][0], "read");
        // SECURITY: the secret token must never reach the durable catalog.
        let serialized = p.to_string();
        assert!(
            !serialized.contains(&secret_token),
            "token leaked into catalog payload: {serialized}"
        );
    }

    #[test]
    fn payload_auth_kind_none_without_token() {
        let server = McpServer {
            server_id: "mcp_1".into(),
            name: "x".into(),
            url: "http://x".into(),
            transport: "http".into(),
            token: String::new(),
            tool_allowlist: vec![],
            enabled: false,
        };
        let p = mcp_capability_payload(
            "o",
            &server,
            &crate::ownership::Ownership::user("alice"),
            "none",
        );
        assert_eq!(p["auth_kind"], "none");
        assert_eq!(p["enabled"], false);
        // "none" needs no managed secret, and must not claim one.
        assert!(p["config_json"].get("secret_ref").is_none());
    }

    #[test]
    fn oauth_payload_carries_a_secret_ref_capability_core_accepts() {
        // capability-core rejects any auth_kind != "none" whose config_json
        // lacks a `secret_ref` parsing as a secret/vault URI with a non-empty
        // host AND path and no user/query/fragment (validMCPSecretReference).
        // Pin that contract here — it is enforced in another language, in
        // another service, so nothing else in this crate would catch a drift.
        let server = McpServer {
            server_id: "mcp_oauth_1".into(),
            name: "visma-net".into(),
            url: "https://mcp.finance.visma.net/mcp".into(),
            transport: "http".into(),
            token: String::new(),
            tool_allowlist: vec!["execute_query".into()],
            enabled: true,
        };
        let p = mcp_capability_payload(
            "org-9",
            &server,
            &crate::ownership::Ownership::user("alice"),
            "oauth",
        );
        assert_eq!(p["auth_kind"], "oauth");
        let secret_ref = p["config_json"]["secret_ref"]
            .as_str()
            .expect("oauth payload must carry a secret_ref");
        let parsed = reqwest::Url::parse(secret_ref).expect("secret_ref must be a valid URI");
        assert_eq!(parsed.scheme(), "secret");
        assert!(parsed.host_str().is_some_and(|host| !host.is_empty()));
        assert!(parsed.path().len() > 1, "path must be non-empty");
        assert!(parsed.query().is_none() && parsed.fragment().is_none());
        assert!(secret_ref.is_ascii() && secret_ref.len() <= 512);
        // Must identify this exact server, so it resolves to the right row.
        assert!(secret_ref.contains("mcp_oauth_1"));
    }
}

#[cfg(test)]
mod mcp_hydration_tests {
    use super::*;

    #[test]
    fn catalog_projection_restores_owner_and_shares_without_credentials() {
        let (server, ownership) = catalog_mcp_parts(
            "org-1",
            CatalogMcpServer {
                id: "mcp-1".to_owned(),
                org_id: "org-1".to_owned(),
                name: "finance".to_owned(),
                endpoint_url: "https://mcp.example.test/mcp".to_owned(),
                transport: "http".to_owned(),
                tool_allowlist: vec!["records.read".to_owned()],
                enabled: true,
                scope: "user".to_owned(),
                owner_user_id: "alice".to_owned(),
                shared_with: vec!["bob".to_owned(), "alice".to_owned(), "".to_owned()],
            },
        )
        .expect("valid catalog projection");

        assert_eq!(server.server_id, "mcp-1");
        assert_eq!(server.url, "https://mcp.example.test/mcp");
        assert!(
            server.token.is_empty(),
            "catalog projections never carry tokens"
        );
        assert_eq!(ownership.scope, crate::ownership::Scope::User);
        assert_eq!(ownership.owner_user_id, "alice");
        assert_eq!(ownership.shared_with, vec!["bob"]);
    }

    /// Builds a catalog row owned by `row_org`, varying nothing else.
    fn catalog_row_for(row_org: &str) -> CatalogMcpServer {
        CatalogMcpServer {
            id: "mcp-1".to_owned(),
            org_id: row_org.to_owned(),
            name: "finance".to_owned(),
            endpoint_url: "https://mcp.example.test/mcp".to_owned(),
            transport: "http".to_owned(),
            tool_allowlist: vec![],
            enabled: true,
            scope: "org".to_owned(),
            owner_user_id: String::new(),
            shared_with: vec![],
        }
    }

    #[test]
    fn a_row_from_another_tenant_is_refused_not_cached_under_the_caller() {
        // capability-core scopes this list by the bearer's principal and
        // ignores the org_id query parameter, so a bearer/argument mismatch
        // would otherwise write another tenant's MCP servers into this
        // tenant's registry — and every one of their tools becomes callable.
        let error = catalog_mcp_parts("org-1", catalog_row_for("org-2"))
            .expect_err("a foreign tenant's row must not hydrate");
        assert!(
            error.contains("org-2"),
            "error should name the foreign org: {error}"
        );
        assert!(
            error.contains("org-1"),
            "error should name the expected org: {error}"
        );
    }

    #[test]
    fn the_caller_own_tenant_and_global_rows_both_hydrate() {
        // `global` is not bleed: capability-core's list query is
        // `WHERE org_id=$1 OR org_id='global'`, so refusing it would silently
        // drop every org-wide server from the catalog.
        assert!(catalog_mcp_parts("org-1", catalog_row_for("org-1")).is_ok());
        assert!(catalog_mcp_parts("org-1", catalog_row_for("global")).is_ok());
    }

    #[test]
    fn a_projection_without_an_org_field_still_hydrates() {
        // Tolerated for compatibility: an absent org_id is capability-core
        // not stating one, which is not evidence of a mismatch. A *stated*
        // and differing tenant is the case worth refusing.
        assert!(catalog_mcp_parts("org-1", catalog_row_for("")).is_ok());
    }

    #[test]
    fn replacing_a_tenant_drops_revoked_servers_and_shares() {
        let registry = McpRegistry::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let mut old = McpServer {
            server_id: "old".to_owned(),
            name: "old".to_owned(),
            url: "https://old.example.test".to_owned(),
            transport: "http".to_owned(),
            token: String::new(),
            tool_allowlist: vec!["read".to_owned()],
            enabled: true,
        };
        handle_register_mcp_server(
            &registry,
            RegisterMcpServerRequest {
                request_id: "r".to_owned(),
                org_id: "org-1".to_owned(),
                server: Some(old.clone()),
            },
        )
        .expect("seed old server");
        ownership.set(
            "org-1",
            crate::ownership::KIND_MCP,
            "old",
            crate::ownership::Ownership::user("alice"),
        );

        old.server_id = "new".to_owned();
        registry.replace_org("org-1", vec![old]);
        ownership.replace_org_kind(
            "org-1",
            crate::ownership::KIND_MCP,
            vec![("new".to_owned(), crate::ownership::Ownership::org())],
        );

        assert!(!registry.contains("org-1", "old"));
        assert!(registry.contains("org-1", "new"));
        assert!(ownership
            .get("org-1", crate::ownership::KIND_MCP, "old")
            .is_none());
        assert!(ownership
            .get("org-1", crate::ownership::KIND_MCP, "new")
            .is_some());
    }
}

#[cfg(test)]
mod mcp_secure_registration_tests {
    use super::*;
    use mp_contracts::model_plane::v1::McpServer;

    fn request(server: McpServer) -> RegisterMcpServerRequest {
        RegisterMcpServerRequest {
            request_id: "req-security".to_owned(),
            org_id: "org-security".to_owned(),
            server: Some(server),
        }
    }

    fn server() -> McpServer {
        McpServer {
            server_id: String::new(),
            name: "safe connector".to_owned(),
            url: "https://mcp.example.test".to_owned(),
            transport: "http".to_owned(),
            token: String::new(),
            tool_allowlist: vec!["search_records".to_owned()],
            enabled: true,
        }
    }

    #[test]
    fn registration_quarantines_user_supplied_process_execution() {
        let registry = McpRegistry::new();
        for (transport, url) in [
            ("stdio", "stdio:///usr/bin/curl https://attacker.test"),
            ("stdio", "https://mcp.example.test"),
        ] {
            let mut candidate = server();
            candidate.transport = transport.to_owned();
            candidate.url = url.to_owned();
            let error = handle_register_mcp_server(&registry, request(candidate))
                .expect_err("stdio registration must be quarantined");
            assert_eq!(error.code(), tonic::Code::InvalidArgument);
        }
    }

    #[test]
    fn internal_host_allowlist_is_case_and_dot_insensitive_and_fails_closed_when_empty() {
        assert!(host_in_allowlist("mcp-bridge", "mcp-bridge"));
        assert!(host_in_allowlist(
            "MCP-Bridge.",
            " mcp-bridge , other-host "
        ));
        assert!(!host_in_allowlist("evil.example", "mcp-bridge"));
        // Empty / whitespace-only allowlist opts nothing in (SSRF guard intact).
        assert!(!host_in_allowlist("mcp-bridge", ""));
        assert!(!host_in_allowlist("mcp-bridge", "  ,  "));
    }

    #[test]
    fn registration_rejects_insecure_hybrid_and_fail_open_records() {
        let registry = McpRegistry::new();
        let mut insecure = server();
        insecure.url = "http://mcp.example.test".to_owned();
        assert!(handle_register_mcp_server(&registry, request(insecure)).is_err());

        let mut empty_allowlist = server();
        empty_allowlist.tool_allowlist.clear();
        assert!(handle_register_mcp_server(&registry, request(empty_allowlist)).is_err());

        let mut raw_secret = server();
        raw_secret.token = "caller-supplied-secret".to_owned();
        assert!(handle_register_mcp_server(&registry, request(raw_secret)).is_err());

        let mut missing_org = request(server());
        missing_org.org_id.clear();
        assert!(handle_register_mcp_server(&registry, missing_org).is_err());
    }

    #[test]
    fn registration_accepts_https_with_an_exact_nonempty_allowlist() {
        let registry = McpRegistry::new();
        let response = handle_register_mcp_server(&registry, request(server()))
            .expect("valid HTTPS connector");
        assert!(response.server.is_some());
    }

    #[test]
    fn registration_rejects_private_metadata_and_encoded_loopback_hosts() {
        let registry = McpRegistry::new();
        for url in [
            "https://127.0.0.1",
            "https://10.0.0.1",
            "https://169.254.169.254",
            "https://[::1]",
            "https://2130706433",
            "https://metadata.google.internal",
            "https://service.local",
        ] {
            let mut candidate = server();
            candidate.url = url.to_owned();
            assert!(
                handle_register_mcp_server(&registry, request(candidate)).is_err(),
                "unsafe URL accepted: {url}"
            );
        }
    }

    #[test]
    fn resolved_private_ranges_are_forbidden() {
        for address in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.1.2",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                ip_is_forbidden(address.parse().expect("IP")),
                "unsafe address accepted: {address}"
            );
        }
        assert!(!ip_is_forbidden("93.184.216.34".parse().expect("IP")));
    }

    /// Ranges that capability-core's Go-side `mcpForbiddenRanges` blocks but
    /// Rust's stable `std::net` predicates alone don't cover -- CGNAT and
    /// benchmarking are real cloud/k8s address spaces, not exotic edge cases.
    /// Kept in parity with `resolved_private_ranges_are_forbidden` in
    /// `quarry-security`'s `heur.rs` tests.
    #[test]
    fn resolved_cgnat_and_reserved_ranges_are_forbidden() {
        for address in [
            "0.1.2.3",           // 0.0.0.0/8, beyond the single unspecified address
            "100.64.0.1",        // CGNAT / Shared Address Space (RFC 6598)
            "100.127.255.254",   // top of the CGNAT range
            "192.0.0.8",         // IETF protocol assignments (RFC 6890)
            "198.18.0.1",        // benchmarking (RFC 2544)
            "198.19.255.254",    // top of the benchmarking range
            "255.0.0.1",         // 240.0.0.0/4 reserved (Class E)
            "64:ff9b::1",        // NAT64 well-known prefix (RFC 6052)
            "100::1",            // discard-only address block (RFC 6666)
            "2001:db8::1",       // documentation (RFC 3849)
            "::ffff:100.64.0.1", // CGNAT wrapped in an IPv4-mapped IPv6 literal
        ] {
            assert!(
                ip_is_forbidden(address.parse().expect("IP")),
                "unsafe address accepted: {address}"
            );
        }
        // 100.63.255.255 and 100.128.0.0 are just outside the CGNAT /10 and
        // must stay reachable -- otherwise the range check is off by one.
        assert!(!ip_is_forbidden("100.63.255.255".parse().expect("IP")));
        assert!(!ip_is_forbidden("100.128.0.0".parse().expect("IP")));
        assert!(!ip_is_forbidden("198.17.255.255".parse().expect("IP")));
        assert!(!ip_is_forbidden("198.20.0.0".parse().expect("IP")));
    }

    #[tokio::test]
    async fn dispatch_resolution_revalidates_forbidden_hosts() {
        assert!(safe_mcp_http_client("https://localhost").await.is_err());
        assert!(safe_mcp_http_client("https://127.0.0.1").await.is_err());
    }

    #[test]
    fn allowlist_matching_is_exact_not_prefix_based() {
        let tools = vec![
            McpToolDef {
                name: "search".to_owned(),
                description: String::new(),
                input_schema_json: "{}".to_owned(),
                annotations: Default::default(),
            },
            McpToolDef {
                name: "search_and_delete".to_owned(),
                description: String::new(),
                input_schema_json: "{}".to_owned(),
                annotations: Default::default(),
            },
        ];
        let filtered = filter_allowlist(tools, &["search".to_owned()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "search");
    }
}

#[cfg(test)]
mod mcp_exposure_tests {
    use super::*;
    use mp_contracts::model_plane::v1::McpServer;

    fn tool(name: &str) -> McpToolDef {
        McpToolDef {
            name: name.to_owned(),
            description: String::new(),
            input_schema_json: "{}".to_owned(),
            annotations: Default::default(),
        }
    }

    /// `mcp_tool_defs` with an empty OAuth-resolution config — every test
    /// server here has no capability-core-stored token anyway, and an empty
    /// `capability_core_base_url`/service token makes `resolve_stored_oauth_token`
    /// short-circuit before any network attempt (see mcp_oauth.rs's own tests
    /// for that short-circuit), so this is behaviorally identical to the
    /// pre-OAuth-resolution signature.
    async fn tool_defs(
        reg: &McpRegistry,
        ownership: &crate::ownership::OwnershipStore,
        org_id: &str,
        user_id: &str,
    ) -> Vec<ToolDefinition> {
        mcp_tool_defs(
            reg,
            ownership,
            org_id,
            user_id,
            &reqwest::Client::new(),
            "",
            "",
            None,
        )
        .await
    }

    #[test]
    fn filter_allowlist_empty_fails_closed() {
        let tools = vec![tool("read"), tool("write")];
        assert!(filter_allowlist(tools, &[]).is_empty());
    }

    #[test]
    fn filter_allowlist_matches_exact_names_only() {
        let tools = vec![tool("read_file"), tool("write_file"), tool("list_dir")];
        let kept = filter_allowlist(tools, &["read_file".to_owned(), "list_dir".to_owned()]);
        let names: Vec<&str> = kept.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["read_file", "list_dir"]);
    }

    fn register(reg: &McpRegistry, org: &str, server: McpServer) {
        handle_register_mcp_server(
            reg,
            RegisterMcpServerRequest {
                request_id: String::new(),
                org_id: org.to_owned(),
                server: Some(server),
            },
        )
        .expect("register ok");
    }

    #[tokio::test]
    async fn exposes_only_successfully_discovered_exact_allowlist_tools() {
        let reg = McpRegistry::new();
        register(
            &reg,
            "org-1",
            McpServer {
                server_id: "fs".into(),
                name: "Filesystem".into(),
                url: "https://mcp.example.test".into(),
                transport: "http".into(),
                token: String::new(),
                tool_allowlist: vec!["read_file".into(), "list_dir".into()],
                enabled: true,
            },
        );
        reg.catalog.insert(
            ("org-1".to_owned(), "fs".to_owned()),
            (
                Instant::now(),
                vec![tool("read_file"), tool("list_dir"), tool("delete_all")],
            ),
        );
        let ownership = crate::ownership::OwnershipStore::new();
        ownership.set(
            "org-1",
            crate::ownership::KIND_MCP,
            "fs",
            crate::ownership::Ownership::org(),
        );
        let defs = tool_defs(&reg, &ownership, "org-1", "u1").await;
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"mcp__fs__read_file"), "got {names:?}");
        assert!(names.contains(&"mcp__fs__list_dir"), "got {names:?}");
        // Description names the server so the model knows the provenance.
        assert!(defs[0].description.contains("Filesystem"));
    }

    #[tokio::test]
    async fn disabled_server_contributes_nothing() {
        let reg = McpRegistry::new();
        // Disabled server → skipped entirely.
        register(
            &reg,
            "org-2",
            McpServer {
                server_id: "off".into(),
                name: "Off".into(),
                url: "https://mcp.example.test".into(),
                transport: "http".into(),
                token: String::new(),
                tool_allowlist: vec!["x".into()],
                enabled: false,
            },
        );
        assert!(tool_defs(
            &reg,
            &crate::ownership::OwnershipStore::new(),
            "org-2",
            "u1"
        )
        .await
        .is_empty());
    }

    #[tokio::test]
    async fn org_scoped_exposure_does_not_leak_across_orgs() {
        let reg = McpRegistry::new();
        register(
            &reg,
            "org-a",
            McpServer {
                server_id: "s".into(),
                name: "S".into(),
                url: "https://mcp.example.test".into(),
                transport: "http".into(),
                token: String::new(),
                tool_allowlist: vec!["t".into()],
                enabled: true,
            },
        );
        reg.catalog.insert(
            ("org-a".to_owned(), "s".to_owned()),
            (Instant::now(), vec![tool("t")]),
        );
        let own = crate::ownership::OwnershipStore::new();
        own.set(
            "org-a",
            crate::ownership::KIND_MCP,
            "s",
            crate::ownership::Ownership::org(),
        );
        assert_eq!(tool_defs(&reg, &own, "org-a", "u1").await.len(), 1);
        assert!(tool_defs(&reg, &own, "org-b", "u1").await.is_empty());
    }
}

/// Lists all MCP servers registered for `req.org_id`.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_list_mcp_servers(
    reg: &McpRegistry,
    req: ListMcpServersRequest,
) -> Result<ListMcpServersResponse, Status> {
    let servers: Vec<McpServer> = reg
        .inner
        .iter()
        .filter(|e| e.key().0 == req.org_id)
        .map(|e| e.value().clone())
        .collect();
    Ok(ListMcpServersResponse {
        request_id: req.request_id,
        servers,
    })
}

/// Proxies a tool call to a registered MCP server over its transport.
///
/// # Errors
///
/// Returns `Status::not_found` if the server is unknown, or `Status::unimplemented`
/// for an unsupported transport. Per-call dispatch failures are reported in the
/// response's `error_message` rather than as an `Err`.
pub async fn handle_proxy_mcp_tool(
    reg: &McpRegistry,
    ownership: &crate::ownership::OwnershipStore,
    req: ProxyMcpToolRequest,
    oauth_token: Option<&str>,
) -> Result<ProxyMcpToolResponse, Status> {
    // Re-authorize per-user, mirroring the ListMcpTools discovery filter: a
    // client-declared mcp__<server>__<tool> name must not let a caller invoke a
    // server they cannot see (another user's private server in the same org).
    // Reported as not_found (not permission_denied) so server existence within
    // the org is not leaked.
    if !req.user_id.trim().is_empty()
        && !ownership.usable(
            &req.org_id,
            crate::ownership::KIND_MCP,
            &req.server_id,
            &req.user_id,
        )
    {
        return Err(Status::not_found(format!(
            "mcp server not found: {}",
            req.server_id
        )));
    }
    let server = reg
        .inner
        .get(&(req.org_id.clone(), req.server_id.clone()))
        .map(|e| e.value().clone())
        .ok_or_else(|| Status::not_found(format!("mcp server not found: {}", req.server_id)))?;
    if !server.enabled {
        return Ok(ProxyMcpToolResponse {
            request_id: req.request_id,
            output_json: String::new(),
            error_message: "server disabled".into(),
        });
    }
    if !server.tool_allowlist.contains(&req.tool_name) {
        return Ok(ProxyMcpToolResponse {
            request_id: req.request_id,
            output_json: String::new(),
            error_message: format!("tool not in allowlist: {}", req.tool_name),
        });
    }

    // Secure-MVP dispatch permits only the validated HTTPS transport. Legacy
    // stdio records are quarantined even if they predate write-time
    // validation; they can never reach the subprocess spawn path.
    match server.transport.as_str() {
        "http" => {}
        other => {
            return Err(Status::unimplemented(format!(
                "mcp transport {other} is quarantined; only HTTPS transport is supported"
            )));
        }
    }

    let arguments = if req.input_json.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str::<serde_json::Value>(&req.input_json)
            .map_err(|_| Status::invalid_argument("input_json must be valid JSON"))?
    };

    // An OAuth-connected server's in-memory `token` is always empty (tokens
    // live only in capability-core's encrypted store) — prefer a freshly
    // resolved OAuth token when the caller supplied one, and fall back to
    // the legacy static-token field otherwise.
    let token = oauth_token
        .filter(|token| !token.is_empty())
        .unwrap_or(&server.token);

    // Real MCP Streamable HTTP: one JSON-RPC session against the server URL
    // itself (see crate::mcp_http). A failed session is an outcome, not an
    // RPC-level error, so the caller sees it as a tool result like any other
    // dispatch failure.
    let session = match crate::mcp_http::McpHttpSession::connect(&server.url, token).await {
        Ok(session) => session,
        Err(error) => {
            return Ok(ProxyMcpToolResponse {
                request_id: req.request_id,
                output_json: String::new(),
                error_message: error,
            });
        }
    };
    match session.call_tool(&req.tool_name, &arguments).await {
        crate::mcp_jsonrpc::McpCallOutcome::Ok(output_json) => Ok(ProxyMcpToolResponse {
            request_id: req.request_id,
            output_json,
            error_message: String::new(),
        }),
        crate::mcp_jsonrpc::McpCallOutcome::Err(error_message) => Ok(ProxyMcpToolResponse {
            request_id: req.request_id,
            output_json: String::new(),
            error_message,
        }),
    }
}

// ====================================================================
// Wave 10h — plugins
// ====================================================================

#[derive(Clone, Default, Debug)]
pub struct PluginRegistry {
    inner: Arc<DashMap<(String, String), Plugin>>, // (org, plugin_id)
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Registers (or upserts) a plugin in the gateway-scoped registry.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.plugin` is absent or its `name` is empty.
pub fn handle_register_plugin(
    reg: &PluginRegistry,
    req: RegisterPluginRequest,
) -> Result<RegisterPluginResponse, Status> {
    let mut p = req
        .plugin
        .ok_or_else(|| Status::invalid_argument("plugin is required"))?;
    if p.name.is_empty() {
        return Err(Status::invalid_argument("plugin.name is required"));
    }
    if p.plugin_id.is_empty() {
        p.plugin_id = new_ulid();
    }
    if p.installed_at_unix == 0 {
        p.installed_at_unix = now_unix();
    }
    if p.status.is_empty() {
        p.status = "active".into();
    }
    reg.inner
        .insert((req.org_id, p.plugin_id.clone()), p.clone());
    Ok(RegisterPluginResponse {
        request_id: req.request_id,
        plugin: Some(p),
    })
}

/// Lists plugins for `req.org_id`, optionally filtered by `kind_filter`.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_list_plugins(
    reg: &PluginRegistry,
    req: ListPluginsRequest,
) -> Result<ListPluginsResponse, Status> {
    let plugins: Vec<Plugin> = reg
        .inner
        .iter()
        .filter(|e| e.key().0 == req.org_id)
        .filter(|e| req.kind_filter.is_empty() || e.value().kind == req.kind_filter)
        .map(|e| e.value().clone())
        .collect();
    Ok(ListPluginsResponse {
        request_id: req.request_id,
        plugins,
    })
}

/// Toggles the `enabled` flag of a registered plugin.
///
/// # Errors
///
/// Returns `Status::not_found` if no plugin matches `(org_id, plugin_id)`.
pub fn handle_set_plugin_enabled(
    reg: &PluginRegistry,
    req: SetPluginEnabledRequest,
) -> Result<SetPluginEnabledResponse, Status> {
    let mut entry = reg
        .inner
        .get_mut(&(req.org_id.clone(), req.plugin_id.clone()))
        .ok_or_else(|| Status::not_found(format!("plugin not found: {}", req.plugin_id)))?;
    entry.enabled = req.enabled;
    Ok(SetPluginEnabledResponse {
        request_id: req.request_id,
        plugin: Some(entry.clone()),
    })
}

// ====================================================================
// Wave 10i — commands / hooks / permissions / policy
// ====================================================================

#[derive(Clone, Default, Debug)]
pub struct CommandRegistry {
    inner: Arc<DashMap<(String, String), Command>>, // (org, name)
}
impl CommandRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn upsert(&self, org: &str, mut c: Command) {
        if c.command_id.is_empty() {
            c.command_id = new_ulid();
        }
        self.inner.insert((org.to_string(), c.name.clone()), c);
    }
}

/// Lists slash-commands registered for `req.org_id`.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_list_commands(
    reg: &CommandRegistry,
    req: ListCommandsRequest,
) -> Result<ListCommandsResponse, Status> {
    let commands: Vec<Command> = reg
        .inner
        .iter()
        .filter(|e| e.key().0 == req.org_id)
        .map(|e| e.value().clone())
        .collect();
    Ok(ListCommandsResponse {
        request_id: req.request_id,
        commands,
    })
}

/// Resolves a slash-command to its target (tool or remote URL) and acknowledges it.
///
/// # Errors
///
/// Returns `Status::not_found` if no command matches `(org_id, command_name)`.
pub fn handle_execute_command(
    reg: &CommandRegistry,
    req: ExecuteCommandRequest,
) -> Result<ExecuteCommandResponse, Status> {
    let cmd = reg
        .inner
        .get(&(req.org_id.clone(), req.command_name.clone()))
        .map(|e| e.value().clone())
        .ok_or_else(|| Status::not_found(format!("command not found: {}", req.command_name)))?;
    // The actual dispatch (tool_name → execution-core, remote_url →
    // HTTP POST) is the bridge's job. Here we just acknowledge.
    Ok(ExecuteCommandResponse {
        request_id: req.request_id,
        output_json: serde_json::json!({
            "command": cmd.name,
            "resolves_to": if cmd.tool_name.is_empty() { cmd.remote_url } else { cmd.tool_name },
            "default_payload": cmd.default_payload_json,
            "args": serde_json::from_str::<serde_json::Value>(&req.args_json)
                .unwrap_or(serde_json::Value::Null),
        })
        .to_string(),
        error_message: String::new(),
    })
}

#[derive(Clone, Default, Debug)]
pub struct HookRegistry {
    inner: Arc<DashMap<(String, String), Hook>>, // (org, hook_id)
}
impl HookRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Serialize an org's enabled hooks into the `hook_context` JSON that
/// execution-core's hook engine parses (`execution-core/src/hook/mod.rs`).
///
/// This is the wire that makes a registered hook actually do something. The
/// gateway used to hold hooks in memory and send `hook_context: ""` on every
/// dispatch, so execution-core — which has a working hook engine — never
/// received a single rule. An operator could register a `deny` on a dangerous
/// tool, list it back, and have it enforce nothing.
///
/// Two deliberate narrowings:
/// - Only `pre_tool`/`post_tool` are emitted. execution-core evaluates exactly
///   `PreToolUse` and `PostToolUse`; `on_error`/`on_complete` have no evaluator
///   there, so forwarding them would imply an enforcement that does not exist.
/// - Disabled hooks are omitted entirely rather than emitted as `allow`, so a
///   disabled rule cannot out-rank an enabled one during matching.
///
/// Returns an empty string when the org has no applicable rules — identical on
/// the wire to today's behaviour, so orgs without hooks are unaffected.
#[must_use]
pub fn hook_context_json(reg: &HookRegistry, org_id: &str) -> String {
    let rules: Vec<Value> = reg
        .inner
        .iter()
        .filter(|entry| entry.key().0 == org_id && entry.value().enabled)
        .filter_map(|entry| {
            let hook = entry.value();
            // Gateway spells the event `pre_tool`; execution-core expects
            // `pre_tool_use`. Translate rather than relying on its normalizer,
            // which folds case and separators but would not add the suffix.
            let event = match hook.event.as_str() {
                "pre_tool" => "pre_tool_use",
                "post_tool" => "post_tool_use",
                _ => return None,
            };
            Some(json!({
                "event": event,
                // Empty scope means "any tool" on both sides.
                "tools": if hook.tool_scope.trim().is_empty() { "*" } else { hook.tool_scope.trim() },
                "decision": hook.decision,
                "reason": hook.reason,
            }))
        })
        .collect();
    if rules.is_empty() {
        return String::new();
    }
    json!({ "rules": rules }).to_string()
}

/// Registers (or upserts) a lifecycle hook in the gateway-scoped registry.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.hook` is absent or its `event`
/// is not one of `pre_tool`, `post_tool`, `on_error`, `on_complete`.
pub fn handle_register_hook(
    reg: &HookRegistry,
    req: RegisterHookRequest,
) -> Result<RegisterHookResponse, Status> {
    let mut h = req
        .hook
        .ok_or_else(|| Status::invalid_argument("hook is required"))?;
    if !matches!(
        h.event.as_str(),
        "pre_tool" | "post_tool" | "on_error" | "on_complete"
    ) {
        return Err(Status::invalid_argument(format!(
            "unknown event: {}",
            h.event
        )));
    }
    if h.hook_id.is_empty() {
        h.hook_id = new_ulid();
    }
    reg.inner.insert((req.org_id, h.hook_id.clone()), h.clone());
    Ok(RegisterHookResponse {
        request_id: req.request_id,
        hook: Some(h),
    })
}

/// Lists hooks for `req.org_id`, optionally filtered by `event_filter`.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_list_hooks(
    reg: &HookRegistry,
    req: ListHooksRequest,
) -> Result<ListHooksResponse, Status> {
    let hooks: Vec<Hook> = reg
        .inner
        .iter()
        .filter(|e| e.key().0 == req.org_id)
        .filter(|e| req.event_filter.is_empty() || e.value().event == req.event_filter)
        .map(|e| e.value().clone())
        .collect();
    Ok(ListHooksResponse {
        request_id: req.request_id,
        hooks,
    })
}

// ====================================================================
// Wave 10j — messages / analytics / voice / tasks
// ====================================================================

#[derive(Clone, Default, Debug)]
pub struct MessageStore {
    inner: Arc<DashMap<String, Vec<ThreadMessage>>>, // thread_id → messages
}
impl MessageStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Appends a message to a thread and returns the stored record.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.thread_id` is empty.
pub fn handle_append_thread_message(
    store: &MessageStore,
    req: AppendThreadMessageRequest,
) -> Result<AppendThreadMessageResponse, Status> {
    if req.thread_id.is_empty() {
        return Err(Status::invalid_argument("thread_id is required"));
    }
    let msg = ThreadMessage {
        message_id: new_ulid(),
        thread_id: req.thread_id.clone(),
        role: req.role,
        content: req.content,
        created_at_unix: now_unix(),
    };
    let mut entry = store.inner.entry(req.thread_id.clone()).or_default();
    entry.push(msg.clone());
    Ok(AppendThreadMessageResponse {
        request_id: req.request_id,
        message: Some(msg),
    })
}

/// Lists the most recent messages of a thread (oldest-first within the limit).
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_list_thread_messages(
    store: &MessageStore,
    req: ListThreadMessagesRequest,
) -> Result<ListThreadMessagesResponse, Status> {
    let limit = usize::try_from(if req.limit <= 0 {
        50
    } else {
        req.limit.min(500)
    })
    .unwrap_or(50);
    let messages = store
        .inner
        .get(&req.thread_id)
        .map(|e| {
            let v = e.value();
            v.iter()
                .rev()
                .take(limit)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(ListThreadMessagesResponse {
        request_id: req.request_id,
        messages,
    })
}

#[derive(Clone, Default, Debug)]
pub struct AnalyticsStore {
    inner: Arc<DashMap<String, OrgCounters>>, // org → counters
}

#[derive(Clone, Default, Debug)]
struct OrgCounters {
    total_runs: i32,
    total_invocations: i32,
    total_cost_usd: f64,
    total_input_tokens: i64,
    total_output_tokens: i64,
    tool_calls: std::collections::HashMap<String, i32>,
}

impl AnalyticsStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment counters for a finished invocation. Called by the
    /// existing usage-envelope publish path; safe to call hot-loop.
    pub fn record(
        &self,
        org_id: &str,
        tool_name: &str,
        cost_usd: f64,
        input_tokens: i64,
        output_tokens: i64,
    ) {
        let mut entry = self.inner.entry(org_id.to_string()).or_default();
        entry.total_invocations += 1;
        entry.total_cost_usd += cost_usd;
        entry.total_input_tokens += input_tokens;
        entry.total_output_tokens += output_tokens;
        if !tool_name.is_empty() {
            *entry.tool_calls.entry(tool_name.to_string()).or_insert(0) += 1;
        }
    }
}

/// Returns the lifetime counter rollup for `req.org_id`.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_get_analytics(
    store: &AnalyticsStore,
    req: GetAnalyticsRequest,
) -> Result<GetAnalyticsResponse, Status> {
    let _ = req.window_secs; // window filtering is future-work (counters are lifetime)
    let counters = store
        .inner
        .get(&req.org_id)
        .map(|e| e.value().clone())
        .unwrap_or_default();
    let tool_calls = counters
        .tool_calls
        .into_iter()
        .map(|(tool_name, count)| ToolCallCount { tool_name, count })
        .collect();
    Ok(GetAnalyticsResponse {
        request_id: req.request_id,
        total_runs: counters.total_runs,
        total_invocations: counters.total_invocations,
        total_cost_usd: counters.total_cost_usd,
        total_input_tokens: counters.total_input_tokens,
        total_output_tokens: counters.total_output_tokens,
        tool_calls,
    })
}

// Voice (TTS/STT) is served for real — there is NO stub here. The gRPC
// `text_to_speech`/`speech_to_text` handlers (grpc.rs) and the HTTP
// `/v1/ai/speech` route (http_routes.rs) both proxy to inference-core's
// `SynthesizeSpeech`/`TranscribeSpeech`, which route through the real
// OpenAI/Azure `SpeechChain` (inference-core `provider/speech.rs`). The old
// dead `handle_text_to_speech`/`handle_speech_to_text` stubs here had no
// callers and were removed.

#[derive(Clone, Default, Debug)]
pub struct TaskStore {
    inner: Arc<DashMap<String, TaskRecord>>, // task_id → record
}
impl TaskStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Creates a gateway-scoped task record.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.description` is empty.
pub fn handle_create_task(
    store: &TaskStore,
    req: CreateTaskRequest,
) -> Result<CreateTaskResponse, Status> {
    if req.description.is_empty() {
        return Err(Status::invalid_argument("description is required"));
    }
    let task = TaskRecord {
        task_id: new_ulid(),
        org_id: req.org_id.clone(),
        description: req.description,
        status: "pending".into(),
        parent_run_id: req.parent_run_id,
        cron: req.cron,
        created_at_unix: now_unix(),
    };
    store.inner.insert(task.task_id.clone(), task.clone());
    Ok(CreateTaskResponse {
        request_id: req.request_id,
        task: Some(task),
    })
}

/// Lists tasks for `req.org_id`, optionally filtered by `status_filter`.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_list_tasks(
    store: &TaskStore,
    req: ListTasksRequest,
) -> Result<ListTasksResponse, Status> {
    let limit = usize::try_from(if req.limit <= 0 {
        100
    } else {
        req.limit.min(500)
    })
    .unwrap_or(100);
    let mut tasks: Vec<TaskRecord> = store
        .inner
        .iter()
        .filter(|e| e.value().org_id == req.org_id)
        .filter(|e| req.status_filter.is_empty() || e.value().status == req.status_filter)
        .map(|e| e.value().clone())
        .collect();
    tasks.sort_by(|a, b| b.created_at_unix.cmp(&a.created_at_unix));
    tasks.truncate(limit);
    Ok(ListTasksResponse {
        request_id: req.request_id,
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_tool_def(name: &str, description: &str, parameters_json: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters_json: parameters_json.to_owned(),
        }
    }

    #[test]
    fn stage_mcp_tool_defs_returns_unchanged_at_or_under_threshold() {
        let defs = vec![
            fixture_tool_def("mcp__s1__a", "tool a", "{}"),
            fixture_tool_def("mcp__s1__b", "tool b", "{}"),
            fixture_tool_def("mcp__s2__c", "tool c", "{}"),
        ];
        let staged = stage_mcp_tool_defs(defs.clone(), 3);
        assert_eq!(staged, defs);
    }

    #[test]
    fn stage_mcp_tool_defs_collapses_to_two_synthetic_tools_over_threshold() {
        let defs = (0..5)
            .map(|i| fixture_tool_def(&format!("mcp__s1__t{i}"), "a tool", "{}"))
            .collect::<Vec<_>>();
        let staged = stage_mcp_tool_defs(defs, 3);
        assert_eq!(staged.len(), 2);
        assert_eq!(staged[0].name, MCP_CATALOG_TOOL_NAME);
        assert_eq!(staged[1].name, MCP_CALL_TOOL_NAME);
        // The catalog tool's own description surfaces the real count so the
        // model knows how much it is not seeing directly.
        assert!(staged[0].description.contains('5'));
        // mcp_call's schema requires both tool_name and arguments.
        let call_schema: Value = serde_json::from_str(&staged[1].parameters_json).unwrap();
        let required = call_schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("tool_name")));
        assert!(required.contains(&json!("arguments")));
    }

    #[test]
    fn format_mcp_catalog_lists_all_with_truncated_descriptions() {
        let long_description = "x".repeat(MCP_CATALOG_SUMMARY_DESCRIPTION_CHARS + 20);
        let defs = vec![
            fixture_tool_def("mcp__s1__short", "short desc", "{}"),
            fixture_tool_def("mcp__s1__long", &long_description, "{}"),
        ];
        let output = format_mcp_catalog(&defs, None);
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["count"], 2);
        let tools = parsed["tools"].as_array().unwrap();
        assert_eq!(tools[0]["description"], "short desc");
        let truncated = tools[1]["description"].as_str().unwrap();
        assert!(truncated.chars().count() <= MCP_CATALOG_SUMMARY_DESCRIPTION_CHARS + 1);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn format_mcp_catalog_inspects_one_tool_by_name() {
        let schema = r#"{"type":"object","required":["path"]}"#;
        let defs = vec![fixture_tool_def("mcp__s1__read", "reads a file", schema)];
        let output = format_mcp_catalog(&defs, Some("mcp__s1__read"));
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["name"], "mcp__s1__read");
        assert_eq!(parsed["description"], "reads a file");
        assert_eq!(parsed["input_schema"]["required"][0], "path");
    }

    #[test]
    fn format_mcp_catalog_reports_error_for_unknown_tool_name() {
        let defs = vec![fixture_tool_def("mcp__s1__read", "reads a file", "{}")];
        let output = format_mcp_catalog(&defs, Some("mcp__s1__does_not_exist"));
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("no such tool"));
    }

    // `callback_url` is deprecated (hooks are policy rules, not webhooks) but
    // the field still exists on the wire, so a literal must set it.
    #[allow(deprecated)]
    fn hook(event: &str, scope: &str, decision: &str, enabled: bool) -> Hook {
        Hook {
            hook_id: format!("h_{event}_{scope}_{decision}"),
            event: event.to_owned(),
            tool_scope: scope.to_owned(),
            callback_url: String::new(),
            enabled,
            decision: decision.to_owned(),
            reason: "policy".to_owned(),
        }
    }

    fn register(reg: &HookRegistry, org: &str, h: Hook) {
        handle_register_hook(
            reg,
            RegisterHookRequest {
                request_id: "t".into(),
                org_id: org.into(),
                hook: Some(h),
            },
        )
        .expect("registers");
    }

    #[test]
    fn hook_rules_reach_execution_core_in_the_shape_its_engine_parses() {
        let reg = HookRegistry::new();
        register(&reg, "org", hook("pre_tool", "shell", "deny", true));

        let ctx: Value = serde_json::from_str(&hook_context_json(&reg, "org")).expect("valid json");
        let rule = &ctx["rules"][0];
        // execution-core spells the event `pre_tool_use`; the gateway spells it
        // `pre_tool`. A missed translation here silently disables every rule.
        assert_eq!(rule["event"], "pre_tool_use");
        assert_eq!(rule["tools"], "shell");
        assert_eq!(rule["decision"], "deny");
        assert_eq!(rule["reason"], "policy");
    }

    #[test]
    fn an_empty_tool_scope_becomes_the_any_tool_matcher() {
        let reg = HookRegistry::new();
        register(&reg, "org", hook("post_tool", "", "ask", true));
        let ctx: Value = serde_json::from_str(&hook_context_json(&reg, "org")).expect("json");
        assert_eq!(ctx["rules"][0]["event"], "post_tool_use");
        assert_eq!(ctx["rules"][0]["tools"], "*");
    }

    #[test]
    fn disabled_hooks_and_unevaluated_events_are_omitted_entirely() {
        let reg = HookRegistry::new();
        // Disabled: emitting it as `allow` could out-rank an enabled deny.
        register(&reg, "org", hook("pre_tool", "shell", "deny", false));
        // execution-core has no evaluator for these two, so forwarding them
        // would imply enforcement that does not exist.
        register(&reg, "org", hook("on_error", "*", "deny", true));
        register(&reg, "org", hook("on_complete", "*", "deny", true));

        // No applicable rules ⇒ empty string, byte-identical to the behaviour
        // before hooks were wired, so an org without usable hooks is unaffected.
        assert_eq!(hook_context_json(&reg, "org"), "");
    }

    #[test]
    fn one_orgs_hook_rules_never_travel_with_another_orgs_dispatch() {
        let reg = HookRegistry::new();
        register(&reg, "org_a", hook("pre_tool", "shell", "deny", true));
        assert!(hook_context_json(&reg, "org_a").contains("shell"));
        assert_eq!(hook_context_json(&reg, "org_b"), "");
    }

    #[test]
    fn mcp_register_assigns_id() {
        let reg = McpRegistry::new();
        let resp = handle_register_mcp_server(
            &reg,
            RegisterMcpServerRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                server: Some(McpServer {
                    name: "test".into(),
                    transport: "http".into(),
                    url: "https://mcp.example.test".into(),
                    tool_allowlist: vec!["read_file".into()],
                    enabled: true,
                    ..Default::default()
                }),
            },
        )
        .unwrap();
        assert!(!resp.server.unwrap().server_id.is_empty());
    }

    #[test]
    fn plugin_register_and_toggle() {
        let reg = PluginRegistry::new();
        let p = handle_register_plugin(
            &reg,
            RegisterPluginRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                plugin: Some(Plugin {
                    name: "foo".into(),
                    kind: "tool".into(),
                    enabled: true,
                    installed_at_unix: 0,
                    ..Default::default()
                }),
            },
        )
        .unwrap();
        let id = p.plugin.unwrap().plugin_id;
        let toggled = handle_set_plugin_enabled(
            &reg,
            SetPluginEnabledRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                plugin_id: id.clone(),
                enabled: false,
            },
        )
        .unwrap();
        assert!(!toggled.plugin.unwrap().enabled);
    }

    #[test]
    fn messages_append_and_list_in_order() {
        let store = MessageStore::new();
        for i in 0..3 {
            handle_append_thread_message(
                &store,
                AppendThreadMessageRequest {
                    request_id: "t".into(),
                    org_id: "o".into(),
                    thread_id: "th1".into(),
                    role: "user".into(),
                    content: format!("msg {i}"),
                },
            )
            .unwrap();
        }
        let list = handle_list_thread_messages(
            &store,
            ListThreadMessagesRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                thread_id: "th1".into(),
                limit: 0,
            },
        )
        .unwrap();
        assert_eq!(list.messages.len(), 3);
        assert_eq!(list.messages[0].content, "msg 0");
        assert_eq!(list.messages[2].content, "msg 2");
    }

    #[test]
    fn analytics_records_per_org() {
        let store = AnalyticsStore::new();
        store.record("o", "bash", 0.01, 100, 50);
        store.record("o", "bash", 0.02, 200, 100);
        store.record("o", "fetch", 0.005, 50, 10);
        let r = handle_get_analytics(
            &store,
            GetAnalyticsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                window_secs: 0,
            },
        )
        .unwrap();
        assert_eq!(r.total_invocations, 3);
        assert!((r.total_cost_usd - 0.035).abs() < 1e-9);
        assert_eq!(r.total_input_tokens, 350);
        // Tool calls are unordered but counts must sum right.
        let total_tool_calls: i32 = r.tool_calls.iter().map(|tc| tc.count).sum();
        assert_eq!(total_tool_calls, 3);
    }

    #[test]
    fn tasks_create_and_list() {
        let store = TaskStore::new();
        handle_create_task(
            &store,
            CreateTaskRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                description: "first".into(),
                parent_run_id: String::new(),
                cron: String::new(),
            },
        )
        .unwrap();
        let list = handle_list_tasks(
            &store,
            ListTasksRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                status_filter: String::new(),
                limit: 0,
            },
        )
        .unwrap();
        assert_eq!(list.tasks.len(), 1);
    }
}
