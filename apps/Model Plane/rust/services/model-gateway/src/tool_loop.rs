//! Gateway tool-execution loop support (chat-parity §2 function-calling).
//!
//! Reuses the gateway's existing tool handlers (`tools.rs` → Quarry, etc.) and
//! the MCP registry — no new tool runtime. The loop itself lives in `sse.rs`
//! (`tool_loop_stream`); this module holds the name→handler dispatcher and the
//! pure helpers (argument parsing, result framing) that are unit-tested here.
//!
//! Loop shape (inject-results-as-context, no structured tool messages needed):
//!   infer(messages + tools) → if `tool_calls`: execute each, append the results
//!   as a context message, re-infer → repeat (capped) → stream the final answer.

use std::fmt::Write as _;

use mp_contracts::model_plane::v1::{
    ChatMessage, InferRequest, ProxyMcpToolRequest, ToolCall, ToolDefinition, WebSearchRequest,
};

use crate::sse_events::ChatEvent;
use crate::state::AppState;

/// Max tool rounds before forcing a final, tool-free answer.
pub const MAX_TOOL_ROUNDS: usize = 3;
/// Cap on inlined page content from `fetch_url` (keeps the prompt bounded).
const MAX_FETCH_CHARS: usize = 4_000;

/// The result of executing one model-requested tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    pub call_id: String,
    pub name: String,
    pub output: String,
    pub error: Option<String>,
}

fn arg_str(args_json: &str, key: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|v| v.get(key).and_then(|x| x.as_str().map(str::to_owned)))
        .unwrap_or_default()
}

fn arg_i64(args_json: &str, key: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|v| v.get(key).and_then(serde_json::Value::as_i64))
}

/// Parse an MCP tool name `mcp__<server_id>__<tool_name>` into its parts.
/// Splits on the FIRST `__` after the prefix (tool names may contain `__`).
fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__"))
}

fn truncate_chars(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_owned();
    }
    let mut out: String = trimmed.chars().take(max).collect();
    out.push('…');
    out
}

fn err_outcome(call: &ToolCall, msg: impl Into<String>) -> ToolOutcome {
    ToolOutcome {
        call_id: call.id.clone(),
        name: call.name.clone(),
        output: String::new(),
        error: Some(msg.into()),
    }
}

/// Execute a single model-requested tool call against the gateway's tool
/// handlers. Unknown tools / bad args return an error outcome (the model is
/// told, so it can recover). New tools plug in here (MCP proxy, etc.).
pub async fn dispatch_tool(state: &AppState, org_id: &str, call: &ToolCall) -> ToolOutcome {
    match call.name.as_str() {
        "web_search" => {
            let query = arg_str(&call.arguments_json, "query");
            if query.trim().is_empty() {
                return err_outcome(call, "web_search requires a non-empty 'query' argument");
            }
            let limit = i32::try_from(arg_i64(&call.arguments_json, "limit").unwrap_or(5))
                .unwrap_or(5)
                .clamp(1, 50);
            match crate::tools::handle_web_search(
                state,
                WebSearchRequest {
                    request_id: String::new(),
                    org_id: org_id.to_owned(),
                    query,
                    limit,
                    intent: String::new(),
                },
            )
            .await
            {
                Ok(resp) => {
                    let items: Vec<serde_json::Value> = resp
                        .results
                        .iter()
                        .map(|r| {
                            serde_json::json!({ "url": r.url, "title": r.title, "snippet": r.snippet })
                        })
                        .collect();
                    ToolOutcome {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        output: serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_owned()),
                        error: None,
                    }
                }
                Err(e) => err_outcome(call, format!("web_search failed: {}", e.message())),
            }
        }
        // Read a specific web page (reuses Quarry scrape — the canonical web
        // fetch owner). Returns title + final URL + (truncated) page content.
        "fetch_url" => {
            let url = arg_str(&call.arguments_json, "url");
            if url.trim().is_empty() {
                return err_outcome(call, "fetch_url requires a 'url' argument");
            }
            match state.quarry.scrape(&url, org_id, None, false).await {
                Ok(r) => {
                    let body = if r.markdown.trim().is_empty() {
                        r.text
                    } else {
                        r.markdown
                    };
                    let out = serde_json::json!({
                        "final_url": r.final_url,
                        "title": r.title,
                        "content": truncate_chars(&body, MAX_FETCH_CHARS),
                    });
                    ToolOutcome {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        output: out.to_string(),
                        error: None,
                    }
                }
                Err(e) => err_outcome(call, format!("fetch_url failed: {e}")),
            }
        }
        // MCP proxy: `mcp__<server_id>__<tool_name>` routes to a registered MCP
        // server via the existing registry (matrix §G2) — no new transport.
        mcp if mcp.starts_with("mcp__") => {
            let Some((server_id, tool_name)) = parse_mcp_tool_name(mcp) else {
                return err_outcome(
                    call,
                    format!("malformed MCP tool '{mcp}' (expected mcp__<server>__<tool>)"),
                );
            };
            match crate::runtime_registries::handle_proxy_mcp_tool(
                &state.mcp,
                ProxyMcpToolRequest {
                    request_id: String::new(),
                    org_id: org_id.to_owned(),
                    server_id: server_id.to_owned(),
                    tool_name: tool_name.to_owned(),
                    input_json: call.arguments_json.clone(),
                },
            )
            .await
            {
                Ok(resp) if resp.error_message.is_empty() => ToolOutcome {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    output: resp.output_json,
                    error: None,
                },
                Ok(resp) => err_outcome(call, resp.error_message),
                Err(e) => err_outcome(call, format!("mcp proxy failed: {}", e.message())),
            }
        }
        other => err_outcome(call, format!("unknown tool '{other}'")),
    }
}

/// Frame a round of tool outcomes as a context message appended to the
/// conversation, so the model can answer from them on the next inference.
#[must_use]
pub fn format_tool_context(outcomes: &[ToolOutcome]) -> String {
    let mut s = String::from(
        "Tool results for your previous request (use these to answer; do not call the same tool again unless needed):\n",
    );
    for o in outcomes {
        match &o.error {
            Some(e) => {
                let _ = writeln!(s, "- {} → ERROR: {e}", o.name);
            }
            None => {
                let _ = writeln!(s, "- {} → {}", o.name, o.output);
            }
        }
    }
    s
}

/// Result of resolving a request's tool calls before the final answer streams.
pub struct ToolRounds {
    /// The conversation augmented with each round's tool-result context.
    pub messages: Vec<ChatMessage>,
    /// `tool_call` + `tool_result` events to emit (gated on the `tools` family).
    pub events: Vec<ChatEvent>,
}

/// Run the function-calling loop to resolution: unary infer-with-tools →
/// execute requested tools → inject results as context → repeat (capped at
/// [`MAX_TOOL_ROUNDS`]). Stops as soon as the model stops requesting tools.
/// The returned `messages` are then handed to the streaming infer (with tools
/// withheld) to produce the final answer. Inference errors stop the loop
/// gracefully (the normal stream path then handles the request).
pub async fn run_tool_rounds(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    model: &str,
    base_messages: Vec<ChatMessage>,
    tools: Vec<ToolDefinition>,
    tool_choice: String,
) -> ToolRounds {
    let mut messages = base_messages;
    let mut events = Vec::new();

    for _round in 0..MAX_TOOL_ROUNDS {
        let mut client = state.inference_client.clone();
        let infer = client.infer(tonic::Request::new(InferRequest {
            request_id: request_id.to_owned(),
            org_id: org_id.to_owned(),
            model: model.to_owned(),
            provider_hint: String::new(),
            messages: messages.clone(),
            temperature: 0.7,
            max_tokens: 1024,
            structured_output_schema: String::new(),
            zdr: false,
            tools: tools.clone(),
            tool_choice: tool_choice.clone(),
        }));
        let resp = match infer.await {
            Ok(r) => r.into_inner(),
            Err(e) => {
                tracing::warn!(error = %e.message(), "tool-round infer failed; ending loop");
                break;
            }
        };

        if resp.tool_calls.is_empty() {
            break; // model is ready to answer
        }

        let mut outcomes = Vec::with_capacity(resp.tool_calls.len());
        for call in &resp.tool_calls {
            let args = serde_json::from_str::<serde_json::Value>(&call.arguments_json)
                .unwrap_or_else(|_| serde_json::json!({}));
            events.push(ChatEvent::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args,
            });
            let outcome = dispatch_tool(state, org_id, call).await;
            events.push(ChatEvent::ToolResult {
                id: outcome.call_id.clone(),
                status: if outcome.error.is_some() {
                    "error".to_owned()
                } else {
                    "ok".to_owned()
                },
                output: outcome.output.clone(),
                error: outcome.error.clone(),
            });
            outcomes.push(outcome);
        }

        if !resp.content.trim().is_empty() {
            messages.push(ChatMessage {
                role: "assistant".to_owned(),
                content: resp.content,
                name: String::new(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_owned(),
            content: format_tool_context(&outcomes),
            name: String::new(),
        });
    }

    ToolRounds { messages, events }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arg_str_and_i64_parse_json_arguments() {
        let args = r#"{"query":"rust async","limit":3}"#;
        assert_eq!(arg_str(args, "query"), "rust async");
        assert_eq!(arg_i64(args, "limit"), Some(3));
        assert_eq!(arg_str(args, "missing"), "");
        assert_eq!(arg_i64(args, "missing"), None);
    }

    #[test]
    fn truncate_chars_caps_and_ellipsizes() {
        assert_eq!(truncate_chars("  hi  ", 10), "hi");
        let long = "x".repeat(20);
        let out = truncate_chars(&long, 5);
        assert_eq!(out.chars().count(), 6); // 5 + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn parses_mcp_tool_names() {
        assert_eq!(
            parse_mcp_tool_name("mcp__github__create_issue"),
            Some(("github", "create_issue"))
        );
        // tool name may itself contain `__` — split on the FIRST separator only.
        assert_eq!(
            parse_mcp_tool_name("mcp__srv__a__b"),
            Some(("srv", "a__b"))
        );
        assert_eq!(parse_mcp_tool_name("web_search"), None);
        assert_eq!(parse_mcp_tool_name("mcp__noseparator"), None);
    }

    #[test]
    fn arg_parsers_tolerate_malformed_json() {
        assert_eq!(arg_str("not json", "query"), "");
        assert_eq!(arg_i64("not json", "limit"), None);
    }

    #[test]
    fn format_tool_context_renders_outputs_and_errors() {
        let outcomes = vec![
            ToolOutcome {
                call_id: "c1".into(),
                name: "web_search".into(),
                output: "[{\"url\":\"x\"}]".into(),
                error: None,
            },
            ToolOutcome {
                call_id: "c2".into(),
                name: "unknown".into(),
                output: String::new(),
                error: Some("unknown tool 'unknown'".into()),
            },
        ];
        let ctx = format_tool_context(&outcomes);
        assert!(ctx.contains("web_search → [{\"url\":\"x\"}]"));
        assert!(ctx.contains("unknown → ERROR: unknown tool 'unknown'"));
    }
}
