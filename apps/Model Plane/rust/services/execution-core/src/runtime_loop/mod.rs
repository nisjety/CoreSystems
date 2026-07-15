//! Runtime loop orchestration for one execution step.

pub mod agent;

use mp_contracts::model_plane::v1::{
    self as pb, orchestration_core_service_client::OrchestrationCoreServiceClient,
};
use tonic::transport::Channel;

use crate::hook;
use crate::permission::{self, PermissionDecision, PermissionMode};
use crate::policy::{MpNetworkPolicy, MpSandboxPolicy};
use crate::subagent;
use crate::tool_bridge;

/// Tool name that routes to real sandboxed process execution (G1) instead of
/// the deterministic `tool_bridge` stub. Input is JSON `{"program","args"}`.
const SHELL_TOOL: &str = "shell";

/// Tool name that drives the live agentic browser loop through Quarry
/// (`/v1/agent/*`). Async, like `shell` — dispatched off the async path below.
const BROWSER_AGENT_TOOL: &str = "browser_agent";

/// Read-only research tools backed by the Quarry edge (`web_tools`). Async.
/// Not in `permission::is_risky_tool`, so they run under `ask` without a gate.
const WEB_SEARCH_TOOL: &str = "web_search";
const WEB_FETCH_TOOL: &str = "web_fetch";

/// RAG over the org's own ingested knowledge via Data Plane v2 retrieval
/// (`knowledge_tools`). Async, read-only. `org_id` comes from the run context.
const KNOWLEDGE_SEARCH_TOOL: &str = "knowledge_search";

/// Norwegian real-time read tools backed by the Application Plane
/// `information-core` service (`info_tools`) plus the public Brønnøysund
/// registry. All five are read-only (none match `permission::is_risky_tool`),
/// so they run under `ask` posture without an approval gate.
const YR_WEATHER_TOOL: &str = "yr_weather";
const TRAFFIC_TOOL: &str = "traffic";
const NEWS_TOOL: &str = "news";
const TRACK_SHIPMENT_TOOL: &str = "track_shipment";
const COMPANY_LOOKUP_TOOL: &str = "company_lookup";

/// Freight tools backed by the Ingestion Plane `shipping-core` aggregator
/// (`shipping_tools`): read-only quote comparison + fleet listing, and the
/// write-side `book_shipment` — which places a REAL freight order and is
/// therefore in `permission::is_risky_tool`, so under `ask` posture the run
/// pauses for explicit human approval before it executes. shipping-core's
/// own two-step token gate is chained inside the tool after that approval.
const GET_SHIPPING_QUOTES_TOOL: &str = "get_shipping_quotes";
const SHIPPING_CARRIERS_TOOL: &str = "shipping_carriers";
const BOOK_SHIPMENT_TOOL: &str = "book_shipment";

/// Social workspace tools backed by the Application Plane `social-core`
/// (`social_tools`): `list_social_accounts` is read-only discovery of the
/// org's connected social accounts; `publish_social_post` creates a REAL
/// workspace post and requests its publish — it is in
/// `permission::is_risky_tool`, so under `ask` posture the run pauses for
/// explicit human approval, AND the post is always created
/// approval-required so social-core's own org-visible `ApprovalState` gate
/// holds as defense in depth (eval case 07 closed this capability gap).
const LIST_SOCIAL_ACCOUNTS_TOOL: &str = "list_social_accounts";
const PUBLISH_SOCIAL_POST_TOOL: &str = "publish_social_post";

/// Provider-action tools backed by the Ingestion Plane `integration-corev2`
/// actions gateway (`integration_tools`): `list_provider_actions` is
/// read-only discovery (org's live connections × the operations catalog);
/// `execute_provider_action` runs one operation and is in
/// `permission::is_risky_tool`, so under `ask` posture the run pauses for
/// explicit human approval before any provider write. The approval reference
/// is forwarded to the gateway as `approvalId` to satisfy its own
/// write-approval check.
const LIST_PROVIDER_ACTIONS_TOOL: &str = "list_provider_actions";
const EXECUTE_PROVIDER_ACTION_TOOL: &str = "execute_provider_action";

/// Namespace prefix for tools proxied to a registered MCP server
/// (`mcp__<server_id>__<tool>`). Routed back through the gateway's
/// `ProxyMcpTool` (matrix §G2) — exec-core holds no MCP registry of its own.
/// Offered to the model via [`agent::run_agent`]'s merged tool defs, so an
/// `mcp__` call has already passed the purpose-lock by the time it lands here.
const MCP_TOOL_PREFIX: &str = "mcp__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub status: String,
    pub output: String,
    pub error: String,
    pub compaction_triggered: bool,
}

impl StepOutcome {
    fn completed(output: String) -> Self {
        Self {
            status: "completed".to_owned(),
            output,
            error: String::new(),
            compaction_triggered: false,
        }
    }

    fn failed(error: &str) -> Self {
        Self {
            status: "failed".to_owned(),
            output: String::new(),
            error: error.to_owned(),
            compaction_triggered: false,
        }
    }

    fn awaiting_approval() -> Self {
        Self {
            status: "awaiting_approval".to_owned(),
            output: String::new(),
            error: String::new(),
            compaction_triggered: false,
        }
    }

    fn permission_denied() -> Self {
        Self {
            status: "permission_denied".to_owned(),
            output: String::new(),
            error: "permission denied by policy".to_owned(),
            compaction_triggered: false,
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn execute_step(
    tool_name: &str,
    tool_input: &str,
    permission_mode: &str,
    hook_context: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    step_id: &str,
    session_channel: Option<Channel>,
    browser_event_sink: Option<&dyn crate::browser_agent::BrowserEventSink>,
    state: Option<&crate::state::StateStore>,
    zdr: bool,
    data_plane_bearer: Option<&str>,
    session_bearer: Option<&str>,
    inference_bearer: Option<&str>,
) -> StepOutcome {
    // PreToolUse hook policy runs BEFORE the permission engine: a rule may deny
    // the call outright or escalate it to human approval (HITL). Allow defers to
    // the permission gate below.
    match hook::evaluate(hook_context, hook::HookEvent::PreToolUse, tool_name) {
        hook::HookDecision::Deny { reason } => {
            return StepOutcome::failed(&format!("blocked by pre-tool hook: {reason}"));
        }
        hook::HookDecision::Ask { .. } => return StepOutcome::awaiting_approval(),
        hook::HookDecision::Allow => {}
    }

    let mode = PermissionMode::from_wire(permission_mode);
    // Operation-aware gate: `execute_provider_action` is classified by the
    // `operation` embedded in `tool_input` (a read proceeds, a write pauses)
    // rather than by tool name alone; `browser_agent` and other write-capable
    // tools are gated by name.
    match permission::evaluate_call(mode, tool_name, tool_input) {
        PermissionDecision::Deny => return StepOutcome::permission_denied(),
        PermissionDecision::AwaitApproval => return StepOutcome::awaiting_approval(),
        PermissionDecision::Allow => {}
    }

    let subagent_note = subagent::maybe_spawn(tool_name)
        .map(|entry| format!(" [{}]", entry.summary))
        .unwrap_or_default();

    // G1: the `shell` tool runs a REAL sandboxed process (executor primitive);
    // every other tool keeps the deterministic tool_bridge path. The permission
    // and hook gates above apply uniformly, so a shell call is still subject to
    // the same approval/deny policy.
    let exec = if tool_name == SHELL_TOOL {
        execute_shell(tool_input).await
    } else if tool_name == BROWSER_AGENT_TOOL {
        tool_bridge::execute_browser_agent(
            tool_input,
            org_id,
            zdr,
            browser_event_sink,
            state,
            inference_bearer,
        )
        .await
    } else if tool_name == WEB_SEARCH_TOOL {
        execute_web_search(tool_input, org_id).await
    } else if tool_name == WEB_FETCH_TOOL {
        execute_web_fetch(tool_input, org_id).await
    } else if tool_name == KNOWLEDGE_SEARCH_TOOL {
        execute_knowledge_search(tool_input, org_id, user_id, zdr, data_plane_bearer).await
    } else if tool_name == YR_WEATHER_TOOL {
        execute_yr_weather(tool_input).await
    } else if tool_name == TRAFFIC_TOOL {
        execute_traffic(tool_input).await
    } else if tool_name == NEWS_TOOL {
        execute_news(tool_input).await
    } else if tool_name == TRACK_SHIPMENT_TOOL {
        execute_track_shipment(tool_input).await
    } else if tool_name == COMPANY_LOOKUP_TOOL {
        execute_company_lookup(tool_input).await
    } else if tool_name == GET_SHIPPING_QUOTES_TOOL {
        execute_get_shipping_quotes(tool_input, org_id).await
    } else if tool_name == SHIPPING_CARRIERS_TOOL {
        execute_shipping_carriers(org_id).await
    } else if tool_name == BOOK_SHIPMENT_TOOL {
        execute_book_shipment(
            tool_input,
            org_id,
            user_id,
            run_id,
            step_id,
            permission_mode,
            session_channel.as_ref(),
            session_bearer,
        )
        .await
    } else if tool_name == LIST_SOCIAL_ACCOUNTS_TOOL {
        execute_list_social_accounts(org_id).await
    } else if tool_name == PUBLISH_SOCIAL_POST_TOOL {
        execute_publish_social_post(tool_input, org_id, user_id, run_id).await
    } else if tool_name == LIST_PROVIDER_ACTIONS_TOOL {
        execute_list_provider_actions(org_id).await
    } else if tool_name == EXECUTE_PROVIDER_ACTION_TOOL {
        execute_provider_action(
            tool_input,
            org_id,
            user_id,
            run_id,
            step_id,
            permission_mode,
            session_channel.clone(),
            session_bearer,
        )
        .await
    } else if tool_name.starts_with(MCP_TOOL_PREFIX) {
        execute_mcp(tool_name, tool_input, org_id, user_id).await
    } else {
        tool_bridge::execute(tool_name, tool_input)
    };
    // Phase 8 promote-on-use (default OFF — PROMOTE_ON_USE_ENABLED): count a
    // successful web_fetch as a grounded use of its URL; the trigger promotes
    // the live page into the durable KB past a threshold. Fire-and-forget —
    // never alters the tool result.
    if tool_name == WEB_FETCH_TOOL && exec.error.is_none() {
        if let Some(url) = serde_json::from_str::<serde_json::Value>(tool_input)
            .ok()
            .and_then(|v| {
                v.get("url")
                    .and_then(|u| u.as_str())
                    .or_else(|| {
                        v.get("urls")
                            .and_then(|a| a.get(0))
                            .and_then(|u| u.as_str())
                    })
                    .map(str::to_owned)
            })
        {
            crate::promote_on_use::maybe_promote(org_id, user_id, &url).await;
        }
    }
    if let Some(error) = exec.error {
        return StepOutcome::failed(&error);
    }

    // PostToolUse hook policy runs on the SUCCESS path: a rule may reject the
    // result before it is handed back to the model (e.g. an output-scanning
    // policy). Allow is the default, so this is a no-op unless a rule matches.
    if let hook::HookDecision::Deny { reason } =
        hook::evaluate(hook_context, hook::HookEvent::PostToolUse, tool_name)
    {
        return StepOutcome::failed(&format!("blocked by post-tool hook: {reason}"));
    }

    let output = format!("{}{}", exec.output, subagent_note);
    let mut outcome = StepOutcome::completed(output.clone());
    outcome.compaction_triggered = output.len() > 2048;
    outcome
}

/// Run a `shell` tool call as a real sandboxed process (G1). Input is JSON
/// `{"program": String, "args": [String]}`. Runs under a **safe default**
/// policy — read-only filesystem, no network — so a shell tool gets minimal
/// privileges; a broader per-call policy is a future extension (the input
/// contract can grow a `policy` field). Output is already secret-scrubbed by
/// the executor before it returns here.
async fn execute_shell(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct ShellInput {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    }
    let input: ShellInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => {
            return tool_bridge::ToolExecution {
                output: String::new(),
                error: Some(format!("invalid shell input: {e}")),
            };
        }
    };
    let policy = MpSandboxPolicy::ReadOnly {
        network: MpNetworkPolicy::Disabled,
    };
    match crate::executor::execute_sandboxed(&policy, &input.program, &input.args).await {
        Ok(o) => tool_bridge::ToolExecution {
            output: o.stdout,
            error: if o.exit_code == 0 {
                None
            } else if o.stderr.is_empty() {
                Some(format!("shell exited with code {}", o.exit_code))
            } else {
                Some(o.stderr)
            },
        },
        Err(e) => tool_bridge::ToolExecution {
            output: String::new(),
            error: Some(format!("shell exec failed: {e}")),
        },
    }
}

/// `web_search` tool — input JSON `{"query": String, "limit"?: u32}`. Returns a
/// ranked result list from the Quarry edge. Read-only (no approval gate).
async fn execute_web_search(tool_input: &str, org_id: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct SearchInput {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
    }
    let input: SearchInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid web_search input: {e}")),
    };
    let client = match crate::web_tools::WebToolsClient::from_env() {
        Ok(Some(client)) => client,
        Ok(None) => {
            return tool_error("web_search unavailable: QUARRY_EDGE_URL not configured".to_owned())
        }
        Err(error) => return tool_error(format!("web_search unavailable: {error}")),
    };
    match client
        .search(&input.query, input.limit.unwrap_or(8), org_id)
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `mcp__<server_id>__<tool>` — proxy a registered MCP server's tool through the
/// gateway's `ProxyMcpTool` (matrix §G2). `org_id` is the run-context tenant
/// (never model input); the server is selected by the namespaced name, and the
/// gateway enforces the server's enabled flag + tool allowlist. Best-effort: a
/// missing gateway or remote error returns a clear tool error to the model.
async fn execute_mcp(
    tool_name: &str,
    tool_input: &str,
    org_id: &str,
    user_id: &str,
) -> tool_bridge::ToolExecution {
    let Some((server_id, remote_tool)) = crate::mcp_gateway::parse_mcp_tool_name(tool_name) else {
        return tool_error(format!(
            "malformed MCP tool '{tool_name}' (expected mcp__<server>__<tool>)"
        ));
    };
    let Some(client) = crate::mcp_gateway::McpGatewayClient::from_env() else {
        return tool_error("MCP gateway not configured (MODEL_GATEWAY_ADDR)".to_owned());
    };
    match client
        .proxy_tool(org_id, user_id, server_id, remote_tool, tool_input)
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `web_fetch` tool — input JSON `{"url": String}`. Returns the page's cleaned
/// markdown from the Quarry edge. Read-only (no approval gate).
async fn execute_web_fetch(tool_input: &str, org_id: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct FetchInput {
        url: String,
    }
    let input: FetchInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid web_fetch input: {e}")),
    };
    let client = match crate::web_tools::WebToolsClient::from_env() {
        Ok(Some(client)) => client,
        Ok(None) => {
            return tool_error("web_fetch unavailable: QUARRY_EDGE_URL not configured".to_owned())
        }
        Err(error) => return tool_error(format!("web_fetch unavailable: {error}")),
    };
    match client.fetch(&input.url, org_id).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `knowledge_search` tool — input JSON `{"query": String, "top_k"?: i32}`.
/// Retrieves the org's own ingested knowledge (RAG) via Data Plane v2. `org_id`
/// comes from the run context (verified), never the model's input, so a tool
/// call cannot cross tenant boundaries. Read-only (no approval gate).
async fn execute_knowledge_search(
    tool_input: &str,
    org_id: &str,
    user_id: &str,
    zdr: bool,
    data_plane_bearer: Option<&str>,
) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct KnowledgeInput {
        query: String,
        #[serde(default)]
        top_k: Option<i32>,
    }
    let input: KnowledgeInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid knowledge_search input: {e}")),
    };
    if org_id.trim().is_empty() {
        return tool_error("knowledge_search requires a run org_id (tenant scope)".to_owned());
    }
    let Some(client) = crate::knowledge_tools::KnowledgeClient::from_env() else {
        return tool_error(
            "knowledge_search unavailable: DATAPLANE_RETRIEVAL_URL not configured".to_owned(),
        );
    };
    let Some(data_plane_bearer) = data_plane_bearer.filter(|value| !value.trim().is_empty()) else {
        return tool_error(
            "knowledge_search requires the originating verified user credential".to_owned(),
        );
    };
    // Per-User Data Ownership: ground AS the run's verified user so the
    // retrieval post-filter hides documents this user cannot see.
    match client
        .search(
            org_id,
            user_id,
            &input.query,
            input.top_k.unwrap_or(5),
            zdr,
            data_plane_bearer,
        )
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `yr_weather` tool — input JSON `{"lat": f64, "lon": f64}`. Returns a compact
/// Yr forecast for the coordinate via information-core. Read-only.
async fn execute_yr_weather(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct WeatherInput {
        lat: f64,
        lon: f64,
    }
    let input: WeatherInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid yr_weather input: {e}")),
    };
    let Some(client) = crate::info_tools::InfoToolsClient::from_env() else {
        return tool_error(
            "yr_weather unavailable: info tools client could not be built".to_owned(),
        );
    };
    match client.yr_weather(input.lat, input.lon).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `traffic` tool — input JSON `{"lat": f64, "lon": f64, "radius"?: u32}`.
/// Returns nearby traffic registration stations via information-core. Read-only.
async fn execute_traffic(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct TrafficInput {
        lat: f64,
        lon: f64,
        #[serde(default)]
        radius: Option<u32>,
    }
    let input: TrafficInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid traffic input: {e}")),
    };
    let Some(client) = crate::info_tools::InfoToolsClient::from_env() else {
        return tool_error("traffic unavailable: info tools client could not be built".to_owned());
    };
    match client
        .traffic(input.lat, input.lon, input.radius.unwrap_or(5000))
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `news` tool — input JSON `{"category"?: String, "limit"?: u32}`. Returns the
/// latest normalised news articles via information-core. Read-only.
async fn execute_news(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct NewsInput {
        #[serde(default)]
        category: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
    }
    let input: NewsInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid news input: {e}")),
    };
    let Some(client) = crate::info_tools::InfoToolsClient::from_env() else {
        return tool_error("news unavailable: info tools client could not be built".to_owned());
    };
    match client
        .news(
            input.category.as_deref().unwrap_or(""),
            input.limit.unwrap_or(10),
        )
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `track_shipment` tool — input JSON `{"tracking_number": String}`. Returns
/// carrier status + recent events via information-core (Bring). Read-only.
async fn execute_track_shipment(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct TrackInput {
        tracking_number: String,
    }
    let input: TrackInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid track_shipment input: {e}")),
    };
    let Some(client) = crate::info_tools::InfoToolsClient::from_env() else {
        return tool_error(
            "track_shipment unavailable: info tools client could not be built".to_owned(),
        );
    };
    match client.track_shipment(&input.tracking_number).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `get_shipping_quotes` tool — input mirrors shipping-core's `QuoteRequest`
/// (from/to addresses + package dims + segment). Fans out to the carrier
/// fleet via the Ingestion Plane aggregator; read-only (no booking exists).
async fn execute_get_shipping_quotes(tool_input: &str, org_id: &str) -> tool_bridge::ToolExecution {
    let input: crate::shipping_tools::QuoteInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid get_shipping_quotes input: {e}")),
    };
    let Some(client) = crate::shipping_tools::ShippingToolsClient::from_env() else {
        return tool_error(
            "get_shipping_quotes unavailable: shipping tools client could not be built".to_owned(),
        );
    };
    match client.get_quotes(&input, org_id).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `shipping_carriers` tool — no input. Lists the aggregator's registered
/// carrier fleet (and whether each runs on demo or live agreement prices).
async fn execute_shipping_carriers(org_id: &str) -> tool_bridge::ToolExecution {
    let Some(client) = crate::shipping_tools::ShippingToolsClient::from_env() else {
        return tool_error(
            "shipping_carriers unavailable: shipping tools client could not be built".to_owned(),
        );
    };
    match client.list_carriers(org_id).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `book_shipment` tool — WRITE side: places a real freight order. Reaches
/// here only after the HITL approval gate (`permission::is_risky_tool`
/// matches this name, so `ask` posture pauses the run for a human). The
/// acting user id from the run context is recorded as the booking actor.
#[allow(clippy::too_many_arguments)]
async fn execute_book_shipment(
    tool_input: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    step_id: &str,
    permission_mode: &str,
    session_channel: Option<&Channel>,
    session_bearer: Option<&str>,
) -> tool_bridge::ToolExecution {
    let mut input: crate::shipping_tools::BookInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid book_shipment input: {e}")),
    };
    if input.booked_by.trim().is_empty() {
        input.booked_by = user_id.to_owned();
    }
    let approval_id = match resolve_write_approval(
        session_channel,
        org_id,
        user_id,
        run_id,
        step_id,
        permission_mode,
        "book_shipment",
        session_bearer,
    )
    .await
    {
        Ok(approval_id) => approval_id,
        Err(error) => return tool_error(error),
    };
    let idempotency_key = format!("{run_id}:{step_id}");
    let Some(client) = crate::shipping_tools::ShippingToolsClient::from_env() else {
        return tool_error(
            "book_shipment unavailable: shipping tools client could not be built".to_owned(),
        );
    };
    match client
        .book_shipment(&input, org_id, &approval_id, &idempotency_key)
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `list_social_accounts` tool — no input. Read-only discovery of the org's
/// connected social accounts (provider, status, capabilities) via
/// social-core; the model's prerequisite step before `publish_social_post`.
async fn execute_list_social_accounts(org_id: &str) -> tool_bridge::ToolExecution {
    let Some(client) = crate::social_tools::SocialToolsClient::from_env() else {
        return tool_error(
            "list_social_accounts unavailable: social tools client could not be built \
             (INTERNAL_API_KEY unset?)"
                .to_owned(),
        );
    };
    match client.list_accounts(org_id).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `publish_social_post` tool — WRITE side: creates a real workspace post and
/// requests its publish. Reaches here only after the HITL approval gate
/// (`permission::is_risky_tool` matches this name, so `ask` posture pauses
/// the run for a human); the post is additionally created approval-required
/// so social-core's own workspace approval gates the actual publish.
async fn execute_publish_social_post(
    tool_input: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
) -> tool_bridge::ToolExecution {
    let input: crate::social_tools::PublishPostInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid publish_social_post input: {e}")),
    };
    let Some(client) = crate::social_tools::SocialToolsClient::from_env() else {
        return tool_error(
            "publish_social_post unavailable: social tools client could not be built \
             (INTERNAL_API_KEY unset?)"
                .to_owned(),
        );
    };
    match client.publish_post(org_id, user_id, run_id, &input).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `company_lookup` tool — input JSON `{"query": String}`. A 9-digit query is an
/// org number; otherwise a name search against the public Brønnøysund registry.
/// Read-only, public, non-personal data.
async fn execute_company_lookup(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct CompanyInput {
        query: String,
    }
    let input: CompanyInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid company_lookup input: {e}")),
    };
    let Some(client) = crate::info_tools::InfoToolsClient::from_env() else {
        return tool_error(
            "company_lookup unavailable: info tools client could not be built".to_owned(),
        );
    };
    match client.company_lookup(&input.query).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `list_provider_actions` tool — read-only discovery of the org's connected
/// providers and their callable operations. `org_id` comes from the run
/// context (tenant scope). No approval gate.
async fn execute_list_provider_actions(org_id: &str) -> tool_bridge::ToolExecution {
    let Some(client) = crate::integration_tools::IntegrationActionsClient::from_env() else {
        return tool_error(
            "list_provider_actions unavailable: integration client could not be built".to_owned(),
        );
    };
    match client.list_provider_actions(org_id).await {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// `execute_provider_action` tool — WRITE-capable: runs one operation on a
/// connected provider via integration-corev2.
///
/// Approval binding (audit fix, Phase 2): a WRITE operation forwards a REAL,
/// per-decision durable approval id (session-core `appr_…`) as `approvalId`,
/// NOT a shared constant. The id is resolved from the durable approval store
/// keyed on this run+step (`resolve_write_approval`):
///   - `ask` (deployed-agent) posture: the id is forwarded ONLY when the durable
///     record shows the human GRANTED it; otherwise the write is blocked. This
///     is real server-side enforcement of the "requires approval" promise.
///   - `auto` (chat) posture: the interactive user is the live approver, so the
///     decision is recorded durably (attributed to that user) and its real id is
///     forwarded — an auditable per-call record, never a shared constant.
///
/// Read operations forward no `approvalId` (integration-corev2 does not gate
/// reads).
///
/// NOTE (remaining work — Stream 2 / integration-corev2): the downstream gateway
/// check (`requireActionCapability` / `actionApprovalRef`, Ingestion Plane) is
/// still presence-only. It must be upgraded to LOOK UP the forwarded id against
/// the durable approvals table and verify `state = granted` for this org/action
/// to fully close the loop. That file is owned by another stream and is out of
/// scope here.
#[allow(clippy::too_many_arguments)]
async fn execute_provider_action(
    tool_input: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    step_id: &str,
    permission_mode: &str,
    session_channel: Option<Channel>,
    session_bearer: Option<&str>,
) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct ActionInput {
        connection_id: String,
        operation: String,
        #[serde(default)]
        params: serde_json::Value,
        #[serde(default)]
        body: serde_json::Value,
    }
    let input: ActionInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid execute_provider_action input: {e}")),
    };

    // Only WRITE operations require an approval reference. Unknown operations are
    // treated as writes (fail safe — see `integration_tools::operation_is_write`).
    let is_write = crate::integration_tools::operation_is_write(&input.operation).unwrap_or(true);
    let approval_ref = if is_write {
        match resolve_write_approval(
            session_channel.as_ref(),
            org_id,
            user_id,
            run_id,
            step_id,
            permission_mode,
            &input.operation,
            session_bearer,
        )
        .await
        {
            Ok(id) => Some(id),
            // Fail closed: never send a write to integration-corev2 without a
            // real, verifiable approval reference behind it.
            Err(e) => return tool_error(e),
        }
    } else {
        None
    };

    let Some(client) = crate::integration_tools::IntegrationActionsClient::from_env() else {
        return tool_error(
            "execute_provider_action unavailable: integration client could not be built".to_owned(),
        );
    };
    match client
        .execute_action(
            org_id,
            &input.connection_id,
            &input.operation,
            input.params,
            input.body,
            approval_ref.as_deref(),
        )
        .await
    {
        Ok(output) => tool_bridge::ToolExecution {
            output,
            error: None,
        },
        Err(e) => tool_error(e),
    }
}

/// Resolve the REAL durable approval id to forward as `approvalId` for a
/// provider WRITE. Never returns a shared constant.
///
/// Uses session-core's durable approval store (idempotent on
/// `(org_id, idempotency_key)`, key = `{run_id}:{step_id}` — the SAME key the
/// HITL pause path mints under). Under `ask` the record must already be human-
/// GRANTED; under `auto` the interactive user's live authorization is recorded
/// durably and its id returned. Any failure to establish a verifiable record is
/// an `Err`, so the caller blocks the write (fail closed).
#[allow(clippy::too_many_arguments)]
async fn resolve_write_approval(
    session_channel: Option<&Channel>,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    step_id: &str,
    permission_mode: &str,
    operation: &str,
    session_bearer: Option<&str>,
) -> Result<String, String> {
    let Some(channel) = session_channel else {
        return Err(
            "provider write blocked: no session-core channel to establish a verifiable approval \
             record"
                .to_owned(),
        );
    };
    if run_id.trim().is_empty() || step_id.trim().is_empty() {
        return Err(
            "provider write blocked: run/step context required to bind a durable approval"
                .to_owned(),
        );
    }

    let mut client = OrchestrationCoreServiceClient::new(channel.clone());
    let bearer = session_bearer.ok_or_else(|| {
        "provider write blocked: verified session credential is unavailable".to_owned()
    })?;
    // Idempotent: returns the existing durable approval for this (org, run:step)
    // when the HITL pause path already minted one, else creates a fresh record.
    let created = client
        .create_approval(authenticated_session_request(
            pb::CreateApprovalRequest {
                run_id: run_id.to_owned(),
                step_id: step_id.to_owned(),
                kind: pb::ApprovalKind::ToolCall as i32,
                requested_of: org_id.to_owned(),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                reason: format!("provider write '{operation}' requires approval"),
                expires_in_seconds: 3600,
                client_approval_id: String::new(),
                idempotency_key: format!("{run_id}:{step_id}"),
            },
            bearer,
        )?)
        .await
        .map_err(|e| format!("provider write blocked: could not reach approval store: {e}"))?
        .into_inner();

    let approval = created
        .approval
        .ok_or_else(|| "provider write blocked: approval store returned no record".to_owned())?;
    let granted = approval.state == pb::ApprovalState::Granted as i32;

    if PermissionMode::from_wire(permission_mode) == PermissionMode::Ask {
        // Deployed-agent posture: a human must have granted this exact record.
        if granted {
            Ok(approval.id)
        } else {
            Err(format!(
                "provider write blocked: approval {} is not granted (a human must approve \
                 this action under the deployed-agent posture)",
                approval.id
            ))
        }
    } else {
        // Chat/interactive posture: the interactive user is the live approver.
        // Record that decision durably (idempotent — a re-run finds it granted),
        // then forward the real id.
        if !granted {
            let decided_by = if user_id.trim().is_empty() {
                org_id.to_owned()
            } else {
                user_id.to_owned()
            };
            client
                .decide_approval(authenticated_session_request(
                    pb::DecideApprovalRequest {
                        approval_id: approval.id.clone(),
                        decision: pb::ApprovalState::Granted as i32,
                        decided_by,
                        decision_reason:
                            "auto (chat) posture: interactive user is the live approver".to_owned(),
                        // Cross-org IDOR fix (Phase 6): this approval was just
                        // created with this same org_id above, so asserting it
                        // here is a real ownership check, not a no-op.
                        org_id: org_id.to_owned(),
                    },
                    bearer,
                )?)
                .await
                .map_err(|e| {
                    format!("provider write blocked: could not record interactive approval: {e}")
                })?;
        }
        Ok(approval.id)
    }
}

fn authenticated_session_request<T>(value: T, bearer: &str) -> Result<tonic::Request<T>, String> {
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}")
            .parse()
            .map_err(|_| "verified session credential is not forwardable".to_owned())?,
    );
    Ok(request)
}

fn tool_error(message: String) -> tool_bridge::ToolExecution {
    tool_bridge::ToolExecution {
        output: String::new(),
        error: Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // "auto" mode -> Allow (even for the risky "shell" tool). The executor runs
    // as passthrough when bwrap is absent, so these exercise the real
    // dispatch->executor path cross-platform.

    #[tokio::test]
    async fn shell_tool_runs_a_real_process() {
        let out = execute_step(
            "shell",
            r#"{"program":"echo","args":["hi-there"]}"#,
            "auto",
            "",
            "org_test",
            "user_test",
            "run_test",
            "step_test",
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(out.status, "completed", "outcome: {out:?}");
        assert!(out.output.contains("hi-there"), "output: {}", out.output);
    }

    #[tokio::test]
    async fn shell_tool_invalid_json_fails() {
        let out = execute_step(
            "shell",
            "not json",
            "auto",
            "",
            "org_test",
            "user_test",
            "run_test",
            "step_test",
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(out.status, "failed");
    }

    #[tokio::test]
    async fn shell_tool_nonzero_exit_fails() {
        let out = execute_step(
            "shell",
            r#"{"program":"sh","args":["-c","exit 2"]}"#,
            "auto",
            "",
            "org_test",
            "user_test",
            "run_test",
            "step_test",
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(out.status, "failed");
    }

    #[tokio::test]
    async fn non_shell_tool_still_uses_the_deterministic_bridge() {
        let out = execute_step(
            "echo",
            "hello-bridge",
            "auto",
            "",
            "org_test",
            "user_test",
            "run_test",
            "step_test",
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(out.status, "completed");
        assert!(out.output.contains("hello-bridge"));
    }

    #[tokio::test]
    async fn deny_mode_blocks_shell_before_execution() {
        let out = execute_step(
            "shell",
            r#"{"program":"echo","args":["x"]}"#,
            "deny",
            "",
            "org_test",
            "user_test",
            "run_test",
            "step_test",
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(out.status, "permission_denied");
    }

    // ── Phase 2: provider-write approval binding (fail-closed guarantees) ──────
    // A provider WRITE must never be forwarded to integration-corev2 without a
    // real, verifiable durable approval reference behind it. These exercise the
    // fail-closed branches of `resolve_write_approval` that do not require the
    // approval store (the granted-forwarding / interactive-record happy paths
    // round-trip through session-core and are covered by the agent-loop tests).

    #[tokio::test]
    async fn provider_write_without_session_channel_is_blocked() {
        let r = resolve_write_approval(
            None,
            "org",
            "user",
            "run",
            "step",
            "auto",
            "pages.post",
            Some("bearer"),
        )
        .await;
        assert!(r.is_err(), "a write with no approval store must be blocked");
        assert!(r.unwrap_err().contains("session-core"));
    }

    #[tokio::test]
    async fn provider_write_without_run_context_is_blocked() {
        // Channel present (lazy — never dialed), but no run/step to bind to.
        let ch = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let r = resolve_write_approval(
            Some(&ch),
            "org",
            "user",
            "",
            "",
            "auto",
            "pages.post",
            Some("bearer"),
        )
        .await;
        assert!(
            r.is_err(),
            "a write with no run/step context must be blocked"
        );
        assert!(r.unwrap_err().contains("run/step"));
    }
}
