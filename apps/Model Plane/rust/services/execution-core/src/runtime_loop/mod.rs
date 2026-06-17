//! Runtime loop orchestration for one execution step.

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
        execute_knowledge_search(tool_input, org_id).await
    } else {
        tool_bridge::execute(tool_name, tool_input)
    };
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
async fn execute_knowledge_search(tool_input: &str, org_id: &str) -> tool_bridge::ToolExecution {
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
    match client.search(org_id, &input.query, input.top_k.unwrap_or(5)).await {
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
            None,
        )
        .await;
        assert_eq!(out.status, "completed", "outcome: {out:?}");
        assert!(out.output.contains("hi-there"), "output: {}", out.output);
    }

    #[tokio::test]
    async fn shell_tool_invalid_json_fails() {
        let out = execute_step("shell", "not json", "auto", "", "org_test", None).await;
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
            None,
        )
        .await;
        assert_eq!(out.status, "failed");
    }

    #[tokio::test]
    async fn non_shell_tool_still_uses_the_deterministic_bridge() {
        let out = execute_step("echo", "hello-bridge", "auto", "", "org_test", None).await;
        assert_eq!(out.status, "completed");
        assert!(out.output.contains("hello-bridge"));
    }

    #[tokio::test]
    async fn deny_mode_blocks_shell_before_execution() {
        let out = execute_step("shell", r#"{"program":"echo","args":["x"]}"#, "deny", "", "org_test", None).await;
        assert_eq!(out.status, "permission_denied");
    }
}
