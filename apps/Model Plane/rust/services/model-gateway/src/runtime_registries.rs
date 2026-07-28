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
use tonic::Status;

use mp_contracts::model_plane::v1::{
    AppendThreadMessageRequest, AppendThreadMessageResponse, CheckPermissionRequest,
    CheckPermissionResponse, Command, CreateTaskRequest, CreateTaskResponse, ExecuteCommandRequest,
    ExecuteCommandResponse, GetAnalyticsRequest, GetAnalyticsResponse, GetPolicyRequest,
    GetPolicyResponse, Hook, ListCommandsRequest, ListCommandsResponse, ListHooksRequest,
    ListHooksResponse, ListMcpServersRequest, ListMcpServersResponse, ListPluginsRequest,
    ListPluginsResponse, ListTasksRequest, ListTasksResponse, ListThreadMessagesRequest,
    ListThreadMessagesResponse, McpServer, OrgPolicy, Plugin, ProxyMcpToolRequest,
    ProxyMcpToolResponse, RegisterHookRequest, RegisterHookResponse, RegisterMcpServerRequest,
    RegisterMcpServerResponse, RegisterPluginRequest, RegisterPluginResponse, SetPermissionRequest,
    SetPermissionResponse, SetPluginEnabledRequest, SetPluginEnabledResponse, SetPolicyRequest,
    SetPolicyResponse, TaskRecord, ThreadMessage, ToolCallCount, ToolDefinition,
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

    /// Whether a server is cached for (org, `server_id`). Test/observability helper.
    #[must_use]
    pub fn contains(&self, org_id: &str, server_id: &str) -> bool {
        self.inner
            .contains_key(&(org_id.to_owned(), server_id.to_owned()))
    }
}

/// Discover an MCP server's tools over the **HTTP** bridge: `POST {url}/tools/list`.
/// Accepts either a JSON-RPC envelope (`result.tools`) or a bare `{tools:[...]}`
/// (the bridge may unwrap), mirroring the lenient `tools/call` bridge shape.
async fn http_list_tools(url: &str, token: &str) -> Result<Vec<McpToolDef>, String> {
    let (http, endpoint) = safe_mcp_http_client(url).await?;
    let target = endpoint
        .join("tools/list")
        .map_err(|error| format!("invalid tools/list endpoint: {error}"))?;
    let mut req = http.post(target).json(&serde_json::json!({}));
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| format!("transport: {e}"))?;
    let status = resp.status();
    let body = bounded_mcp_response(resp).await?;
    if !status.is_success() {
        return Err(format!("mcp HTTP {}: {}", status, truncate(&body, 200)));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("invalid json: {e}"))?;
    let tools = v
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(serde_json::Value::as_array)
        .or_else(|| v.get("tools").and_then(serde_json::Value::as_array))
        .ok_or_else(|| "tools/list response missing tools array".to_owned())?;
    Ok(tools
        .iter()
        .filter_map(crate::mcp_jsonrpc::parse_one_tool)
        .collect())
}

async fn safe_mcp_http_client(url: &str) -> Result<(reqwest::Client, reqwest::Url), String> {
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

async fn bounded_mcp_response(mut response: reqwest::Response) -> Result<String, String> {
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
) -> Option<Vec<McpToolDef>> {
    let key = (org_id.to_owned(), server.server_id.clone());
    if let Some(entry) = reg.catalog.get(&key) {
        if entry.0.elapsed() < MCP_CATALOG_TTL {
            return Some(entry.1.clone());
        }
    }
    let discovery = async {
        match server.transport.as_str() {
            "http" => http_list_tools(&server.url, &server.token).await,
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

/// Build the agent-facing tool definitions for every **enabled** MCP server an
/// org has registered, namespaced `mcp__<server_id>__<tool>` so the gateway's
/// `dispatch_tool` (and the model) can route calls back to the right server.
/// This is the governed exposure bridge: without it, registered MCP servers sit
/// in the registry but their tools never reach an execution caller. Inline chat
/// deliberately does not consume these definitions because it lacks the
/// execution-core approval workflow.
///
/// Discovery is best-effort and bounded (see [`mcp_discover_cached`]): each
/// server's `tools/list` is fetched for real input schemas and then intersected
/// with the exact stored allowlist. A disabled or unreachable server contributes
/// nothing. Never panics.
pub async fn mcp_tool_defs(
    reg: &McpRegistry,
    ownership: &crate::ownership::OwnershipStore,
    org_id: &str,
    user_id: &str,
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

    let mut defs: Vec<ToolDefinition> = Vec::new();
    for server in servers {
        let tools = match mcp_discover_cached(reg, org_id, &server).await {
            Some(discovered) => filter_allowlist(discovered, &server.tool_allowlist),
            None => Vec::new(),
        };
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

fn ip_is_forbidden(address: std::net::IpAddr) -> bool {
    fn ipv4_forbidden(address: std::net::Ipv4Addr) -> bool {
        address.is_private()
            || address.is_loopback()
            || address.is_link_local()
            || address.is_broadcast()
            || address.is_documentation()
            || address.is_unspecified()
            || address.is_multicast()
    }

    fn ipv6_forbidden(address: std::net::Ipv6Addr) -> bool {
        address.is_loopback()
            || address.is_unspecified()
            || address.is_multicast()
            || address.to_ipv4_mapped().is_some_and(ipv4_forbidden)
            || (address.segments()[0] & 0xfe00) == 0xfc00
            || (address.segments()[0] & 0xffc0) == 0xfe80
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
    serde_json::json!({
        "id": server.server_id,
        "org_id": org_id,
        "name": server.name,
        "endpoint_url": server.url,
        "transport": server.transport,
        "auth_kind": auth_kind,
        "config_json": {
            "tool_allowlist": server.tool_allowlist,
            "owner_user_id": ownership.owner_user_id,
            "shared_with": ownership.shared_with,
        },
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
            },
            McpToolDef {
                name: "search_and_delete".to_owned(),
                description: String::new(),
                input_schema_json: "{}".to_owned(),
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
        }
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
        let defs = mcp_tool_defs(&reg, &ownership, "org-1", "u1").await;
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
        assert!(mcp_tool_defs(
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
        assert_eq!(mcp_tool_defs(&reg, &own, "org-a", "u1").await.len(), 1);
        assert!(mcp_tool_defs(&reg, &own, "org-b", "u1").await.is_empty());
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

    // Secure-MVP dispatch permits only the validated HTTPS bridge transport.
    // Legacy stdio records are quarantined even if they predate write-time
    // validation; they can never reach the subprocess spawn path.
    match server.transport.as_str() {
        "http" => {}
        other => {
            return Err(Status::unimplemented(format!(
                "mcp transport {other} is quarantined; only HTTPS bridge transport is supported"
            )));
        }
    }

    // MCP JSON-RPC over HTTP. Bridge expects POST {url}/tools/call with
    // `{name, arguments}`. The bridge is responsible for translating to
    // the actual MCP transport when multi-step.
    let arguments = if req.input_json.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str::<serde_json::Value>(&req.input_json)
            .map_err(|_| Status::invalid_argument("input_json must be valid JSON"))?
    };
    let body = serde_json::json!({
        "name": req.tool_name,
        "arguments": arguments,
    });

    let (http, endpoint) = safe_mcp_http_client(&server.url)
        .await
        .map_err(Status::failed_precondition)?;
    let target = endpoint.join("tools/call").map_err(|error| {
        Status::invalid_argument(format!("invalid tools/call endpoint: {error}"))
    })?;
    let mut http_req = http.post(target).json(&body);
    // An OAuth-connected server's in-memory `token` is always empty (tokens
    // live only in capability-core's encrypted store) — prefer a freshly
    // resolved OAuth token when the caller supplied one, and fall back to
    // the legacy static-token field otherwise.
    if let Some(token) = oauth_token.filter(|t| !t.is_empty()) {
        http_req = http_req.bearer_auth(token);
    } else if !server.token.is_empty() {
        http_req = http_req.bearer_auth(&server.token);
    }
    let resp = match http_req.send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(ProxyMcpToolResponse {
                request_id: req.request_id,
                output_json: String::new(),
                error_message: format!("transport: {e}"),
            });
        }
    };
    let status = resp.status();
    let body = match bounded_mcp_response(resp).await {
        Ok(body) => body,
        Err(error) => {
            return Ok(ProxyMcpToolResponse {
                request_id: req.request_id,
                output_json: String::new(),
                error_message: error,
            });
        }
    };
    if !status.is_success() {
        return Ok(ProxyMcpToolResponse {
            request_id: req.request_id,
            output_json: String::new(),
            error_message: format!("mcp HTTP {}: {}", status, truncate(&body, 200)),
        });
    }
    Ok(ProxyMcpToolResponse {
        request_id: req.request_id,
        output_json: body,
        error_message: String::new(),
    })
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

#[derive(Debug, Clone)]
struct PermEntry {
    verdict: String, // "allow" | "deny"
    reason: String,
}

#[derive(Clone, Default, Debug)]
pub struct PermissionRegistry {
    // (org, tool_name) → verdict. Missing = allow (default-open).
    inner: Arc<DashMap<(String, String), PermEntry>>,
}
impl PermissionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Resolves the tool-ACL verdict for `(org_id, tool_name)` (default-open).
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_check_permission(
    reg: &PermissionRegistry,
    req: CheckPermissionRequest,
) -> Result<CheckPermissionResponse, Status> {
    if let Some(e) = reg.inner.get(&(req.org_id.clone(), req.tool_name.clone())) {
        let allowed = e.verdict == "allow";
        return Ok(CheckPermissionResponse {
            request_id: req.request_id,
            allowed,
            reason: e.reason.clone(),
        });
    }
    Ok(CheckPermissionResponse {
        request_id: req.request_id,
        allowed: true,
        reason: "default allow".into(),
    })
}

/// Sets the tool-ACL verdict for `(org_id, tool_name)`.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.verdict` is not `allow` or `deny`.
pub fn handle_set_permission(
    reg: &PermissionRegistry,
    req: SetPermissionRequest,
) -> Result<SetPermissionResponse, Status> {
    if !matches!(req.verdict.as_str(), "allow" | "deny") {
        return Err(Status::invalid_argument(
            "verdict must be 'allow' or 'deny'",
        ));
    }
    reg.inner.insert(
        (req.org_id.clone(), req.tool_name.clone()),
        PermEntry {
            verdict: req.verdict,
            reason: req.reason,
        },
    );
    Ok(SetPermissionResponse {
        request_id: req.request_id,
    })
}

#[derive(Clone, Default, Debug)]
pub struct PolicyStore {
    inner: Arc<DashMap<String, OrgPolicy>>, // org → policy
}
impl PolicyStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Returns the runtime policy for `req.org_id`, or a default when unset.
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub fn handle_get_policy(
    store: &PolicyStore,
    req: GetPolicyRequest,
) -> Result<GetPolicyResponse, Status> {
    let p = store
        .inner
        .get(&req.org_id)
        .map(|e| e.value().clone())
        .unwrap_or(OrgPolicy {
            org_id: req.org_id.clone(),
            ..Default::default()
        });
    Ok(GetPolicyResponse {
        request_id: req.request_id,
        policy: Some(p),
    })
}

/// Stores the runtime policy for an org.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.policy` is absent or its `org_id` is empty.
pub fn handle_set_policy(
    store: &PolicyStore,
    req: SetPolicyRequest,
) -> Result<SetPolicyResponse, Status> {
    let p = req
        .policy
        .ok_or_else(|| Status::invalid_argument("policy is required"))?;
    if p.org_id.is_empty() {
        return Err(Status::invalid_argument("policy.org_id is required"));
    }
    store.inner.insert(p.org_id.clone(), p.clone());
    Ok(SetPolicyResponse {
        request_id: req.request_id,
        policy: Some(p),
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

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn permission_defaults_open() {
        let reg = PermissionRegistry::new();
        let r = handle_check_permission(
            &reg,
            CheckPermissionRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                tool_name: "bash".into(),
            },
        )
        .unwrap();
        assert!(r.allowed);
    }

    #[test]
    fn permission_deny_takes_effect() {
        let reg = PermissionRegistry::new();
        handle_set_permission(
            &reg,
            SetPermissionRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                tool_name: "bash".into(),
                verdict: "deny".into(),
                reason: "policy".into(),
            },
        )
        .unwrap();
        let r = handle_check_permission(
            &reg,
            CheckPermissionRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                tool_name: "bash".into(),
            },
        )
        .unwrap();
        assert!(!r.allowed);
    }

    #[test]
    fn policy_get_returns_default_when_unset() {
        let store = PolicyStore::new();
        let r = handle_get_policy(
            &store,
            GetPolicyRequest {
                request_id: "t".into(),
                org_id: "o".into(),
            },
        )
        .unwrap();
        let p = r.policy.unwrap();
        assert_eq!(p.org_id, "o");
        assert!(p.max_cost_per_run_usd.abs() < f64::EPSILON);
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
