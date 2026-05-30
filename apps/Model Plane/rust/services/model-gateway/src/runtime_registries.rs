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
//!   - voice:      TTS/STT passthrough to inference-core (Wave 10j; stub)
//!   - tasks:      Gateway-scoped task ingress (Wave 10j; durable state
//!                 belongs in session-core `tasks` tables, cron in Temporal —
//!                 the orphaned task-core service was retired, matrix §4.2)

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
    SetPolicyResponse, SpeechToTextRequest, SpeechToTextResponse, TaskRecord, TextToSpeechRequest,
    TextToSpeechResponse, ThreadMessage, ToolCallCount,
};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ====================================================================
// Wave 10g — MCP server registry + tool proxy
// ====================================================================

#[derive(Clone, Default, Debug)]
pub struct McpRegistry {
    inner: Arc<DashMap<(String, String), McpServer>>, // (org, server_id)
    http: reqwest::Client,
}

impl McpRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Drop a cached server by (org, server_id). Returns true if an entry was
    /// removed. Used by the §4.3 reconcile consumer to keep the cache coherent
    /// when capability-core (the system-of-record) reports a server removed —
    /// so the gateway stops proxying to a decommissioned/revoked server.
    pub fn remove(&self, org_id: &str, server_id: &str) -> bool {
        self.inner
            .remove(&(org_id.to_owned(), server_id.to_owned()))
            .is_some()
    }

    /// Whether a server is cached for (org, server_id). Test/observability helper.
    #[must_use]
    pub fn contains(&self, org_id: &str, server_id: &str) -> bool {
        self.inner
            .contains_key(&(org_id.to_owned(), server_id.to_owned()))
    }
}

/// Execute an MCP `tools/call` over the **stdio** transport (matrix §G2):
/// spawn the server subprocess, perform the `initialize` handshake, then issue
/// `tools/call` — all newline-delimited JSON-RPC 2.0 (framing lives in
/// `crate::mcp_jsonrpc`, unit-tested). Bounded by a 30s timeout; the child is
/// always killed before returning. Returns the serialized `result` JSON on
/// success, or an error message.
async fn stdio_tool_call(url: &str, tool_name: &str, input_json: &str) -> Result<String, String> {
    use crate::mcp_jsonrpc::{
        build_initialize_request, build_initialized_notification, build_tool_call_request,
        parse_tool_call_response, McpCallOutcome,
    };
    use tokio::io::AsyncBufReadExt as _; // for BufReader::lines()

    let (program, args) = crate::mcp_jsonrpc::parse_stdio_command(url)?;
    // Empty input ⟺ no arguments; anything else must be valid JSON. Silently
    // coercing malformed input to null hid client errors behind confusing
    // server-side failures, so reject it with a clear message instead.
    let arguments: serde_json::Value = if input_json.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(input_json).map_err(|e| format!("invalid tool input_json: {e}"))?
    };

    let mut child = tokio::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("mcp stdio spawn {program}: {e}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "mcp stdio: no child stdin".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "mcp stdio: no child stdout".to_owned())?;
    let mut reader = tokio::io::BufReader::new(stdout).lines();

    let interaction = async {
        send_jsonrpc(&mut stdin, &build_initialize_request(1)).await?;
        await_response(&mut reader, 1).await?;
        send_jsonrpc(&mut stdin, &build_initialized_notification()).await?;
        send_jsonrpc(
            &mut stdin,
            &build_tool_call_request(2, tool_name, arguments),
        )
        .await?;
        await_response(&mut reader, 2).await
    };

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(30), interaction).await;
    let _ = child.kill().await;

    let line = match outcome {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("mcp stdio call timed out".to_owned()),
    };
    match parse_tool_call_response(2, &line) {
        McpCallOutcome::Ok(out) => Ok(out),
        McpCallOutcome::Err(e) => Err(e),
    }
}

async fn send_jsonrpc(
    stdin: &mut tokio::process::ChildStdin,
    msg: &serde_json::Value,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt as _;
    let mut line = msg.to_string();
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("mcp stdio write: {e}"))?;
    stdin
        .flush()
        .await
        .map_err(|e| format!("mcp stdio flush: {e}"))
}

async fn await_response(
    reader: &mut tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
    id: i64,
) -> Result<String, String> {
    // Bound how many non-matching lines we'll skip. The outer 30s timeout caps
    // wall-clock, but a misbehaving/adversarial server could stream unbounded
    // short notification lines within that window; this caps the work per call.
    const MAX_SKIPPED: usize = 1024;
    let mut skipped = 0usize;
    loop {
        match reader
            .next_line()
            .await
            .map_err(|e| format!("mcp stdio read: {e}"))?
        {
            Some(line) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    if crate::mcp_jsonrpc::is_response_for(&v, id) {
                        return Ok(line);
                    }
                }
                // else: notification / log / other id — skip and keep reading.
                skipped += 1;
                if skipped >= MAX_SKIPPED {
                    return Err(format!(
                        "mcp server sent >{MAX_SKIPPED} non-response lines before id={id}"
                    ));
                }
            }
            None => return Err(format!("mcp server closed stream before response id={id}")),
        }
    }
}

pub fn handle_register_mcp_server(
    reg: &McpRegistry,
    req: RegisterMcpServerRequest,
) -> Result<RegisterMcpServerResponse, Status> {
    let mut server = req
        .server
        .ok_or_else(|| Status::invalid_argument("server is required"))?;
    if server.name.is_empty() {
        return Err(Status::invalid_argument("server.name is required"));
    }
    if server.server_id.is_empty() {
        server.server_id = new_ulid();
    }
    reg.inner.insert(
        (req.org_id.clone(), server.server_id.clone()),
        server.clone(),
    );
    Ok(RegisterMcpServerResponse {
        request_id: req.request_id,
        server: Some(server),
    })
}

/// Build the capability-core `POST /api/v1/mcp` JSON body from a gateway
/// `McpServer` (matrix §4.1/H.1 write-through). capability-core is the registry
/// **system-of-record**; the gateway's in-memory store is a cache that writes
/// through here so the two converge instead of shadowing each other. The
/// `server_id` is sent as `id` — capability-core honors a client-supplied id
/// (so there is **no** id divergence, unlike the G8 approval crux) and upserts
/// on `(org_id, name)`.
///
/// SECURITY: the bearer `token` is an operational secret and is deliberately
/// NOT sent to the catalog — only `auth_kind` ("bearer"/"none") records that
/// auth is required. The token stays in the gateway cache, which is what
/// actually proxies tool calls; the catalog holds metadata only.
#[must_use]
pub fn mcp_capability_payload(org_id: &str, server: &McpServer) -> serde_json::Value {
    let auth_kind = if server.token.is_empty() {
        "none"
    } else {
        "bearer"
    };
    serde_json::json!({
        "id": server.server_id,
        "org_id": org_id,
        "name": server.name,
        "endpoint_url": server.url,
        "transport": server.transport,
        "auth_kind": auth_kind,
        "config_json": { "tool_allowlist": server.tool_allowlist },
        "scope": "org",
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
        let p = mcp_capability_payload("org-7", &server);
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
        let p = mcp_capability_payload("o", &server);
        assert_eq!(p["auth_kind"], "none");
        assert_eq!(p["enabled"], false);
    }
}

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

pub async fn handle_proxy_mcp_tool(
    reg: &McpRegistry,
    req: ProxyMcpToolRequest,
) -> Result<ProxyMcpToolResponse, Status> {
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
    if !server.tool_allowlist.is_empty()
        && !server
            .tool_allowlist
            .iter()
            .any(|p| req.tool_name.starts_with(p))
    {
        return Ok(ProxyMcpToolResponse {
            request_id: req.request_id,
            output_json: String::new(),
            error_message: format!("tool not in allowlist: {}", req.tool_name),
        });
    }

    // Transport dispatch. HTTP falls through to the inline implementation
    // below; stdio is handled here (matrix §G2) by spawning the server
    // subprocess and speaking JSON-RPC over its stdio; sse is still pending.
    match server.transport.as_str() {
        "http" => {}
        "stdio" => {
            let (output_json, error_message) =
                match stdio_tool_call(&server.url, &req.tool_name, &req.input_json).await {
                    Ok(out) => (out, String::new()),
                    Err(e) => (String::new(), e),
                };
            return Ok(ProxyMcpToolResponse {
                request_id: req.request_id,
                output_json,
                error_message,
            });
        }
        other => {
            return Err(Status::unimplemented(format!(
                "mcp transport {other} not yet implemented (http, stdio supported)"
            )));
        }
    }

    // MCP JSON-RPC over HTTP. Bridge expects POST {url}/tools/call with
    // `{name, arguments}`. The bridge is responsible for translating to
    // the actual MCP transport when multi-step.
    let body = serde_json::json!({
        "name": req.tool_name,
        "arguments": serde_json::from_str::<serde_json::Value>(&req.input_json)
            .unwrap_or(serde_json::Value::Null),
    });

    let mut http_req = reg
        .http
        .post(format!("{}/tools/call", server.url))
        .json(&body);
    if !server.token.is_empty() {
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
    let body = resp.text().await.unwrap_or_default();
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
            "resolves_to": if !cmd.tool_name.is_empty() { cmd.tool_name } else { cmd.remote_url },
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

pub fn handle_list_thread_messages(
    store: &MessageStore,
    req: ListThreadMessagesRequest,
) -> Result<ListThreadMessagesResponse, Status> {
    let limit = if req.limit <= 0 {
        50
    } else {
        req.limit.min(500)
    } as usize;
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

// Voice handlers — stub returns Unimplemented until the inference-core
// speech provider is wired through. The actual implementation would
// route to crate::state::AppState::inference_client with a speech-
// specific InferRequest variant. Kept as RPCs so the API surface
// matches v2.

pub fn handle_text_to_speech(_req: TextToSpeechRequest) -> Result<TextToSpeechResponse, Status> {
    Err(Status::unimplemented(
        "TTS requires inference-core speech provider wiring; coming in a follow-up.",
    ))
}

pub fn handle_speech_to_text(_req: SpeechToTextRequest) -> Result<SpeechToTextResponse, Status> {
    Err(Status::unimplemented(
        "STT requires inference-core speech provider wiring; coming in a follow-up.",
    ))
}

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

pub fn handle_list_tasks(
    store: &TaskStore,
    req: ListTasksRequest,
) -> Result<ListTasksResponse, Status> {
    let limit = if req.limit <= 0 {
        100
    } else {
        req.limit.min(500)
    } as usize;
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
                    url: "http://localhost:9000".into(),
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
        assert_eq!(p.max_cost_per_run_usd, 0.0);
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
                parent_run_id: "".into(),
                cron: "".into(),
            },
        )
        .unwrap();
        let list = handle_list_tasks(
            &store,
            ListTasksRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                status_filter: "".into(),
                limit: 0,
            },
        )
        .unwrap();
        assert_eq!(list.tasks.len(), 1);
    }
}
