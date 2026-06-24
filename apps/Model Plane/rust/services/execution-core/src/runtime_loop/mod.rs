//! Runtime loop orchestration for one execution step.

pub mod agent;

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

pub async fn execute_step(
    tool_name: &str,
    tool_input: &str,
    permission_mode: &str,
    hook_context: &str,
    org_id: &str,
    user_id: &str,
    browser_event_sink: Option<&dyn crate::browser_agent::BrowserEventSink>,
) -> StepOutcome {
    if hook::is_blocked(hook_context) {
        return StepOutcome::failed("blocked by pre-tool hook");
    }

    let mode = PermissionMode::from_wire(permission_mode);
    match permission::evaluate(mode, tool_name) {
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
        tool_bridge::execute_browser_agent(tool_input, browser_event_sink).await
    } else if tool_name == WEB_SEARCH_TOOL {
        execute_web_search(tool_input).await
    } else if tool_name == WEB_FETCH_TOOL {
        execute_web_fetch(tool_input).await
    } else if tool_name == KNOWLEDGE_SEARCH_TOOL {
        execute_knowledge_search(tool_input, org_id, user_id).await
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
    } else if tool_name.starts_with(MCP_TOOL_PREFIX) {
        execute_mcp(tool_name, tool_input, org_id).await
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
                    .or_else(|| v.get("urls").and_then(|a| a.get(0)).and_then(|u| u.as_str()))
                    .map(str::to_owned)
            })
        {
            crate::promote_on_use::maybe_promote(org_id, user_id, &url).await;
        }
    }
    if let Some(error) = exec.error {
        return StepOutcome::failed(&error);
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
async fn execute_web_search(tool_input: &str) -> tool_bridge::ToolExecution {
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
    let Some(client) = crate::web_tools::WebToolsClient::from_env() else {
        return tool_error("web_search unavailable: QUARRY_EDGE_URL not configured".to_owned());
    };
    match client.search(&input.query, input.limit.unwrap_or(8)).await {
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
async fn execute_mcp(tool_name: &str, tool_input: &str, org_id: &str) -> tool_bridge::ToolExecution {
    let Some((server_id, remote_tool)) = crate::mcp_gateway::parse_mcp_tool_name(tool_name) else {
        return tool_error(format!(
            "malformed MCP tool '{tool_name}' (expected mcp__<server>__<tool>)"
        ));
    };
    let Some(client) = crate::mcp_gateway::McpGatewayClient::from_env() else {
        return tool_error("MCP gateway not configured (MODEL_GATEWAY_ADDR)".to_owned());
    };
    match client
        .proxy_tool(org_id, server_id, remote_tool, tool_input)
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
async fn execute_web_fetch(tool_input: &str) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct FetchInput {
        url: String,
    }
    let input: FetchInput = match serde_json::from_str(tool_input) {
        Ok(i) => i,
        Err(e) => return tool_error(format!("invalid web_fetch input: {e}")),
    };
    let Some(client) = crate::web_tools::WebToolsClient::from_env() else {
        return tool_error("web_fetch unavailable: QUARRY_EDGE_URL not configured".to_owned());
    };
    match client.fetch(&input.url).await {
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
    // Per-User Data Ownership: ground AS the run's verified user so the
    // retrieval post-filter hides documents this user cannot see.
    match client
        .search(org_id, user_id, &input.query, input.top_k.unwrap_or(5))
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
            None,
        )
        .await;
        assert_eq!(out.status, "permission_denied");
    }
}
