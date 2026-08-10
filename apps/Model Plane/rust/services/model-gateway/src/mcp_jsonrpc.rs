//! MCP JSON-RPC 2.0 framing — the transport-agnostic core shared by the MCP
//! transports (stdio today; sse later). **Pure**: it builds request envelopes
//! and parses responses with no IO, so it is fully unit-tested here. The
//! HTTP transport in `runtime_registries.rs` predates this; the stdio
//! transport (the dominant MCP transport for local servers) is built on top.
//!
//! Per `docs/capability-ownership-matrix.md` §G2. The gateway's MCP store is
//! an ephemeral cache (matrix §4.3) — this module is the wire protocol it
//! speaks, independent of where the registry's system-of-record eventually
//! lives (capability-core).

use serde_json::{json, Value};

/// JSON-RPC protocol version string.
pub const JSONRPC_VERSION: &str = "2.0";
/// MCP protocol revision advertised in `initialize`.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Outcome of parsing a `tools/call` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpCallOutcome {
    /// Success — the serialized JSON of the `result` field.
    Ok(String),
    /// Failure — a human-readable error message (from JSON-RPC `error` or a
    /// protocol violation).
    Err(String),
}

/// Build the `initialize` request that opens an MCP session.
#[must_use]
pub fn build_initialize_request(id: i64) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "model-gateway", "version": "1" }
        }
    })
}

/// Build the `notifications/initialized` notification (no id — fire-and-forget)
/// that the client sends after a successful `initialize` handshake.
#[must_use]
pub fn build_initialized_notification() -> Value {
    json!({ "jsonrpc": JSONRPC_VERSION, "method": "notifications/initialized" })
}

/// One tool advertised by an MCP server's `tools/list` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolDef {
    /// The tool's name as the server reports it (NOT yet namespaced).
    pub name: String,
    /// Human/model-readable description (may be empty).
    pub description: String,
    /// The tool's `inputSchema` object, serialized as a JSON string. Defaults
    /// to an open object schema when the server omits it.
    pub input_schema_json: String,
}

/// Build a `tools/list` request — discovers the tools an MCP server exposes,
/// with their real input schemas (matrix §G2). Issued after the `initialize`
/// handshake, same as `tools/call`. `cursor` is the opaque pagination token
/// from a previous page's [`McpToolsPage::next_cursor`]; `None` requests the
/// first page.
#[must_use]
pub fn build_list_tools_request(id: i64, cursor: Option<&str>) -> Value {
    let mut params = serde_json::Map::new();
    if let Some(cursor) = cursor {
        params.insert("cursor".to_owned(), Value::String(cursor.to_owned()));
    }
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": "tools/list",
        "params": params
    })
}

/// One page of a `tools/list` response: the tools it carried, plus the
/// server's pagination cursor for the next page (per the MCP pagination
/// spec). `next_cursor` is `None` when this was the last page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolsPage {
    pub tools: Vec<McpToolDef>,
    pub next_cursor: Option<String>,
}

/// Parse a `tools/list` response line for `expected_id` into one page of
/// advertised tool defs plus its pagination cursor. Mirrors
/// [`parse_tool_call_response`]'s validation; a server that reports no
/// `tools` array yields an `Err` so the caller can fall back.
///
/// # Errors
/// Returns `Err` on invalid JSON-RPC, id mismatch, a JSON-RPC `error`, or a
/// `result` missing the `tools` array.
pub fn parse_list_tools_response(expected_id: i64, line: &str) -> Result<McpToolsPage, String> {
    let value: Value =
        serde_json::from_str(line.trim()).map_err(|e| format!("invalid json-rpc: {e}"))?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC_VERSION) {
        return Err("missing or wrong jsonrpc version".to_owned());
    }
    if !is_response_for(&value, expected_id) {
        return Err(format!("response id mismatch (expected {expected_id})"));
    }
    if let Some(err) = value.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| err.to_string(), ToOwned::to_owned);
        return Err(msg);
    }
    let result = value
        .get("result")
        .ok_or_else(|| "tools/list result missing tools array".to_owned())?;
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| "tools/list result missing tools array".to_owned())?;
    let next_cursor = result
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Ok(McpToolsPage {
        tools: tools.iter().filter_map(parse_one_tool).collect(),
        next_cursor,
    })
}

/// Extract a single `{name, description, inputSchema}` entry from a `tools/list`
/// result. Entries without a name are skipped (return `None`).
#[must_use]
pub fn parse_one_tool(tool: &Value) -> Option<McpToolDef> {
    let name = tool.get("name").and_then(Value::as_str)?.to_owned();
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let input_schema_json = tool
        .get("inputSchema")
        .map_or_else(|| "{\"type\":\"object\"}".to_owned(), ToString::to_string);
    Some(McpToolDef {
        name,
        description,
        input_schema_json,
    })
}

/// Build a `tools/call` request. `arguments` is the already-parsed JSON value
/// of the tool input (use `Value::Null` / `{}` when there are none).
#[must_use]
pub fn build_tool_call_request(id: i64, tool_name: &str, arguments: &Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": "tools/call",
        "params": { "name": tool_name, "arguments": arguments.clone() }
    })
}

/// Does this parsed JSON value carry the response for `expected_id`?
/// Notifications (no `id`) and responses for other ids return `false`, so a
/// transport read-loop can skip them.
#[must_use]
pub fn is_response_for(value: &Value, expected_id: i64) -> bool {
    value.get("id").and_then(Value::as_i64) == Some(expected_id)
}

/// Parse a single JSON-RPC response line for `expected_id`, extracting the
/// result or error. A line that is not the awaited response (notification,
/// other id, unparseable) yields `Err` describing why — callers that read a
/// stream should pre-filter with [`is_response_for`] and only call this on the
/// matching line.
#[must_use]
pub fn parse_tool_call_response(expected_id: i64, line: &str) -> McpCallOutcome {
    let value: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => return McpCallOutcome::Err(format!("invalid json-rpc: {e}")),
    };
    if value.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC_VERSION) {
        return McpCallOutcome::Err("missing or wrong jsonrpc version".to_owned());
    }
    if !is_response_for(&value, expected_id) {
        return McpCallOutcome::Err(format!("response id mismatch (expected {expected_id})"));
    }
    if let Some(err) = value.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| err.to_string(), ToOwned::to_owned);
        return McpCallOutcome::Err(msg);
    }
    match value.get("result") {
        Some(result) => McpCallOutcome::Ok(result.to_string()),
        None => McpCallOutcome::Err("response has neither result nor error".to_owned()),
    }
}

/// Parse a `stdio://` MCP server URL into `(program, args)`.
///
/// The proto documents stdio servers as `stdio:///path/to/exe --flag value`.
/// The `stdio://` scheme is stripped and the remainder split on whitespace;
/// the first token is the executable, the rest are arguments.
///
/// # Errors
/// Returns `Err` if the scheme is not `stdio://`, no executable is present, or
/// the command contains quote characters (whitespace-splitting cannot honor
/// quoted arguments, so rather than silently mis-split them we reject — the
/// caller must supply a quote-free `program arg arg` form).
pub fn parse_stdio_command(url: &str) -> Result<(String, Vec<String>), String> {
    let rest = url
        .strip_prefix("stdio://")
        .ok_or_else(|| format!("not a stdio url: {url}"))?;
    if rest.contains('"') || rest.contains('\'') {
        return Err("stdio url: quoted arguments are not supported".to_owned());
    }
    let mut parts = rest.split_whitespace();
    let program = parts
        .next()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| "stdio url has no executable".to_owned())?;
    let args: Vec<String> = parts.map(ToOwned::to_owned).collect();
    Ok((program.to_owned(), args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_request_is_wellformed() {
        let r = build_initialize_request(1);
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], 1);
        assert_eq!(r["method"], "initialize");
        assert_eq!(r["params"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    }

    #[test]
    fn tool_call_request_carries_name_and_arguments() {
        let r = build_tool_call_request(7, "read_file", &json!({"path": "/x"}));
        assert_eq!(r["method"], "tools/call");
        assert_eq!(r["id"], 7);
        assert_eq!(r["params"]["name"], "read_file");
        assert_eq!(r["params"]["arguments"]["path"], "/x");
    }

    #[test]
    fn parse_result_response_ok() {
        let line = r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"hi"}]}}"#;
        match parse_tool_call_response(2, line) {
            McpCallOutcome::Ok(out) => assert!(out.contains("\"text\":\"hi\"")),
            McpCallOutcome::Err(e) => panic!("expected Ok, got Err({e:?})"),
        }
    }

    #[test]
    fn parse_error_response_extracts_message() {
        let line =
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"method not found"}}"#;
        assert_eq!(
            parse_tool_call_response(2, line),
            McpCallOutcome::Err("method not found".to_owned())
        );
    }

    #[test]
    fn parse_rejects_id_mismatch_and_bad_version() {
        assert!(matches!(
            parse_tool_call_response(2, r#"{"jsonrpc":"2.0","id":9,"result":{}}"#),
            McpCallOutcome::Err(_)
        ));
        assert!(matches!(
            parse_tool_call_response(2, r#"{"id":2,"result":{}}"#),
            McpCallOutcome::Err(_)
        ));
    }

    #[test]
    fn is_response_for_skips_notifications_and_other_ids() {
        let notif = json!({"jsonrpc":"2.0","method":"notifications/message"});
        assert!(!is_response_for(&notif, 1));
        let other = json!({"jsonrpc":"2.0","id":5,"result":{}});
        assert!(!is_response_for(&other, 1));
        let matching = json!({"jsonrpc":"2.0","id":1,"result":{}});
        assert!(is_response_for(&matching, 1));
    }

    #[test]
    fn parse_stdio_command_splits_program_and_args() {
        let (prog, args) =
            parse_stdio_command("stdio:///usr/local/bin/mcp-fs --root /tmp --ro").expect("ok");
        assert_eq!(prog, "/usr/local/bin/mcp-fs");
        assert_eq!(args, vec!["--root", "/tmp", "--ro"]);
    }

    #[test]
    fn parse_stdio_command_rejects_non_stdio_and_empty() {
        assert!(parse_stdio_command("http://x").is_err());
        assert!(parse_stdio_command("stdio://").is_err());
    }

    #[test]
    fn parse_stdio_command_rejects_quoted_args() {
        // Quotes would be mis-split by split_whitespace; reject loudly instead
        // of silently producing broken argv tokens like `"a` / `b"`.
        assert!(parse_stdio_command("stdio:///bin/mcp --msg \"a b\"").is_err());
        assert!(parse_stdio_command("stdio:///bin/mcp --msg 'a b'").is_err());
    }

    #[test]
    fn build_list_tools_request_has_method_and_id() {
        let req = build_list_tools_request(7, None);
        assert_eq!(req["method"], "tools/list");
        assert_eq!(req["id"], 7);
        assert_eq!(req["jsonrpc"], JSONRPC_VERSION);
        assert!(req["params"].get("cursor").is_none());
    }

    #[test]
    fn build_list_tools_request_includes_cursor_when_given() {
        let req = build_list_tools_request(7, Some("page-2"));
        assert_eq!(req["params"]["cursor"], "page-2");
    }

    #[test]
    fn parse_list_tools_response_extracts_name_desc_and_schema() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[
            {"name":"read_file","description":"Read a file","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
            {"name":"list_dir"}
        ]}}"#;
        let page = parse_list_tools_response(3, line).expect("parse ok");
        assert_eq!(page.tools.len(), 2);
        assert_eq!(page.tools[0].name, "read_file");
        assert_eq!(page.tools[0].description, "Read a file");
        assert!(page.tools[0].input_schema_json.contains("\"path\""));
        // A tool with no inputSchema gets an open object schema, not empty.
        assert_eq!(page.tools[1].name, "list_dir");
        assert_eq!(page.tools[1].input_schema_json, "{\"type\":\"object\"}");
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn parse_list_tools_response_extracts_next_cursor() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[],"nextCursor":"page-2-token"}}"#;
        let page = parse_list_tools_response(3, line).expect("parse ok");
        assert_eq!(page.next_cursor, Some("page-2-token".to_owned()));
    }

    #[test]
    fn parse_list_tools_response_rejects_error_and_id_mismatch() {
        let err_line =
            r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"no such method"}}"#;
        assert!(parse_list_tools_response(3, err_line)
            .unwrap_err()
            .contains("no such method"));
        let other_id = r#"{"jsonrpc":"2.0","id":9,"result":{"tools":[]}}"#;
        assert!(parse_list_tools_response(3, other_id)
            .unwrap_err()
            .contains("id mismatch"));
        let no_tools = r#"{"jsonrpc":"2.0","id":3,"result":{}}"#;
        assert!(parse_list_tools_response(3, no_tools)
            .unwrap_err()
            .contains("missing tools array"));
    }
}
