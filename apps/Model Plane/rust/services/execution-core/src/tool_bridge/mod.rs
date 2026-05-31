//! Tool execution bridge.

use crate::{browser_agent, wiki_agent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecution {
    pub output: String,
    pub error: Option<String>,
}

/// Execute a tool by name.
///
/// This is intentionally deterministic while execution-core is being bootstrapped.
pub fn execute(tool_name: &str, tool_input: &str) -> ToolExecution {
    if tool_name.is_empty() {
        return ToolExecution {
            output: "reasoning_step_completed".to_owned(),
            error: None,
        };
    }

    match tool_name {
        "echo" => ToolExecution {
            output: tool_input.to_owned(),
            error: None,
        },
        "fail" => ToolExecution {
            output: String::new(),
            error: Some("tool execution failed".to_owned()),
        },
        "browser_agent" => execute_browser_agent(tool_input),
        "wiki_propose_edit" => execute_wiki_propose_edit(tool_input),
        "wiki_lint" => execute_wiki_lint(tool_input),
        _ => ToolExecution {
            output: format!("tool={tool_name} input={tool_input}"),
            error: None,
        },
    }
}

fn execute_wiki_propose_edit(tool_input: &str) -> ToolExecution {
    let parsed: Result<wiki_agent::ProposeEditInput, _> = serde_json::from_str(tool_input);
    match parsed {
        Ok(input) => match wiki_agent::propose_edit(input) {
            Ok(env) => match serde_json::to_string(&env) {
                Ok(out) => ToolExecution {
                    output: out,
                    error: None,
                },
                Err(e) => ToolExecution {
                    output: String::new(),
                    error: Some(format!("serialize proposal: {e}")),
                },
            },
            Err(e) => ToolExecution {
                output: String::new(),
                error: Some(e),
            },
        },
        Err(e) => ToolExecution {
            output: String::new(),
            error: Some(format!("invalid wiki_propose_edit input: {e}")),
        },
    }
}

fn execute_wiki_lint(tool_input: &str) -> ToolExecution {
    let parsed: Result<wiki_agent::LintInput, _> = serde_json::from_str(tool_input);
    match parsed {
        Ok(input) => {
            let report = wiki_agent::lint(input);
            match serde_json::to_string(&report) {
                Ok(out) => ToolExecution {
                    output: out,
                    error: None,
                },
                Err(e) => ToolExecution {
                    output: String::new(),
                    error: Some(format!("serialize lint report: {e}")),
                },
            }
        }
        Err(e) => ToolExecution {
            output: String::new(),
            error: Some(format!("invalid wiki_lint input: {e}")),
        },
    }
}

fn execute_browser_agent(tool_input: &str) -> ToolExecution {
    let config: Result<BrowserAgentInput, _> = serde_json::from_str(tool_input);
    match config {
        Ok(input) => {
            let plan_config = browser_agent::PlanConfig {
                plan_id: input.plan_id.unwrap_or_else(mp_ids::new_ulid),
                grant_id: input.grant_id,
                run_id: input.run_id.unwrap_or_default(),
                org_id: input.org_id.unwrap_or_default(),
                system_prompt: input.system_prompt.unwrap_or_default(),
                max_steps: input.max_steps.unwrap_or(20),
                max_runtime_s: input.max_runtime_s.unwrap_or(120),
                allowed_domains: input.allowed_domains.unwrap_or_default(),
                stop_criteria: input.stop_criteria.unwrap_or_default(),
                require_approval: input.require_approval.unwrap_or(false),
            };
            let (status, _observations, summary) =
                browser_agent::run_browser_agent_loop(plan_config);
            ToolExecution {
                output: format!("status={} summary={}", status.as_str(), summary),
                error: if status == browser_agent::PlanStatus::Failed {
                    Some(summary)
                } else {
                    None
                },
            }
        }
        Err(e) => ToolExecution {
            output: String::new(),
            error: Some(format!("invalid browser_agent input: {e}")),
        },
    }
}

#[derive(serde::Deserialize)]
struct BrowserAgentInput {
    grant_id: String,
    plan_id: Option<String>,
    run_id: Option<String>,
    org_id: Option<String>,
    system_prompt: Option<String>,
    max_steps: Option<i32>,
    max_runtime_s: Option<i32>,
    allowed_domains: Option<Vec<String>>,
    stop_criteria: Option<String>,
    require_approval: Option<bool>,
}
