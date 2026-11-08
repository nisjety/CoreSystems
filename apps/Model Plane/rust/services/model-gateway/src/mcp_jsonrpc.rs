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
use sha2::{Digest, Sha256};

/// JSON-RPC protocol version string.
pub const JSONRPC_VERSION: &str = "2.0";
/// MCP protocol revision advertised in `initialize`.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
/// The first MCP revision with `elicitation/create` (EP-4 S4). Advertised only when the caller
/// allows the server to ask the user something; every other session keeps
/// [`MCP_PROTOCOL_VERSION`] byte for byte.
pub const ELICITATION_PROTOCOL_VERSION: &str = "2025-06-18";
/// JSON-RPC "method not found": the answer to a server request this client does not serve.
pub const METHOD_NOT_FOUND: i64 = -32601;

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
    build_initialize_request_with(id, false)
}

/// Build the `initialize` request, advertising elicitation only when `elicitation` is true
/// (EP-4 S4, D6). Without it the request is exactly today's: the old revision and empty
/// capabilities, so a server can never ask. `sampling` is never advertised (D15).
#[must_use]
pub fn build_initialize_request_with(id: i64, elicitation: bool) -> Value {
    let (revision, capabilities) = if elicitation {
        (ELICITATION_PROTOCOL_VERSION, json!({ "elicitation": {} }))
    } else {
        (MCP_PROTOCOL_VERSION, json!({}))
    };
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": revision,
            "capabilities": capabilities,
            "clientInfo": { "name": "model-gateway", "version": "1" }
        }
    })
}

/// A request the SERVER sent the client inside a response stream.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerRequest {
    /// `elicitation/create`: the server asks the user for a form. `params` is the request's
    /// `params` object (`message`, `requestedSchema`).
    Elicitation { id: Value, params: Value },
    /// `ping`: answered with an empty result.
    Ping { id: Value },
    /// Anything else, `sampling/createMessage` included: answered with method-not-found, so a
    /// server can never make this gateway run inference for it.
    Unsupported { id: Value, method: String },
}

impl ServerRequest {
    /// The JSON-RPC id the client's response must carry.
    #[must_use]
    pub fn id(&self) -> &Value {
        match self {
            Self::Elicitation { id, .. } | Self::Ping { id } | Self::Unsupported { id, .. } => id,
        }
    }
}

/// Read a server-to-client request out of one JSON-RPC message. A response (no `method`) and a
/// notification (no `id`) are not requests and yield `None`.
#[must_use]
pub fn parse_server_request(value: &Value) -> Option<ServerRequest> {
    let method = value.get("method").and_then(Value::as_str)?;
    let id = value.get("id").filter(|id| id.is_string() || id.is_number())?;
    Some(match method {
        "elicitation/create" => ServerRequest::Elicitation {
            id: id.clone(),
            params: value.get("params").cloned().unwrap_or(Value::Null),
        },
        "ping" => ServerRequest::Ping { id: id.clone() },
        other => ServerRequest::Unsupported {
            id: id.clone(),
            method: other.to_owned(),
        },
    })
}

/// The client's result for an `elicitation/create`: `accept` with `content`, or `decline` /
/// `cancel` without it.
#[must_use]
pub fn build_elicitation_result(id: &Value, action: &str, content: Option<&Value>) -> Value {
    let mut result = json!({ "action": action });
    if let Some(content) = content {
        result["content"] = content.clone();
    }
    json!({ "jsonrpc": JSONRPC_VERSION, "id": id.clone(), "result": result })
}

/// An empty result, the answer to a server's `ping`.
#[must_use]
pub fn build_empty_result(id: &Value) -> Value {
    json!({ "jsonrpc": JSONRPC_VERSION, "id": id.clone(), "result": {} })
}

/// The method-not-found error for a server request this client does not serve.
#[must_use]
pub fn build_method_not_found(id: &Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "error": { "code": METHOD_NOT_FOUND, "message": "Method not found" }
    })
}

/// Build the `notifications/initialized` notification (no id — fire-and-forget)
/// that the client sends after a successful `initialize` handshake.
#[must_use]
pub fn build_initialized_notification() -> Value {
    json!({ "jsonrpc": JSONRPC_VERSION, "method": "notifications/initialized" })
}

/// What a tool's side effects are, as far as the SERVER is willing to say.
///
/// These are MCP's optional `annotations` hints. Every field is `Option` on
/// purpose: "the server said false" and "the server said nothing" are
/// different facts, and collapsing them would silently invent a claim the
/// server never made. The spec's own interpretation defaults (`readOnlyHint`
/// false, `destructiveHint` true, …) are applied by
/// [`McpToolAnnotations::side_effect`], never at parse time.
///
/// **These are advisory claims by a third party, not guarantees.** A server
/// can omit them, get them wrong, or lie. Treat a `read_only_hint: Some(true)`
/// as evidence, never as authorization — the ceiling on what a tool may do
/// stays the operator's per-server `tool_allowlist`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpToolAnnotations {
    /// A human-facing title for the tool, distinct from its callable `name`.
    pub title: Option<String>,
    /// Server claims this tool does not modify its environment.
    pub read_only_hint: Option<bool>,
    /// Server claims this tool may perform destructive updates. Per spec this
    /// is meaningful only when the tool is NOT read-only.
    pub destructive_hint: Option<bool>,
    /// Server claims repeated calls with the same arguments have no additional
    /// effect.
    pub idempotent_hint: Option<bool>,
    /// Server claims this tool touches an open world (e.g. the public web)
    /// rather than a closed domain.
    pub open_world_hint: Option<bool>,
}

/// What the server's annotations amount to, once the spec's interpretation
/// defaults are applied. `Undeclared` is deliberately its own variant so a
/// caller must decide what to do about silence rather than being handed a
/// default that looks like a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSideEffect {
    /// `readOnlyHint: true` — the server claims this only reads.
    ReadOnly,
    /// Not read-only, and `destructiveHint` is true or absent (the spec's
    /// default is true), so it may destroy or overwrite.
    Destructive,
    /// Not read-only, but the server explicitly said `destructiveHint: false`
    /// — it writes, additively.
    NonDestructiveWrite,
    /// The server supplied no usable annotation at all.
    Undeclared,
}

impl McpSideEffect {
    /// The wire label capability-core stores as `declared_side_effect`. Rust
    /// interprets the MCP spec's hint defaults once, here; Go only decides how
    /// far to believe the result.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Destructive => "destructive",
            Self::NonDestructiveWrite => "non_destructive_write",
            Self::Undeclared => "undeclared",
        }
    }
}

impl McpToolAnnotations {
    /// Collapse the hints into a single classification, applying the MCP
    /// spec's interpretation rules: `destructiveHint` matters only when the
    /// tool is not read-only, and defaults to `true` when omitted.
    ///
    /// Returns [`McpSideEffect::Undeclared`] when the server said nothing —
    /// the caller decides whether silence means "ask a human" or "allow", and
    /// that decision belongs in policy, not here.
    #[must_use]
    pub fn side_effect(&self) -> McpSideEffect {
        match self.read_only_hint {
            Some(true) => McpSideEffect::ReadOnly,
            // Explicitly not read-only: destructive unless the server said
            // otherwise (spec default for destructiveHint is true).
            Some(false) => match self.destructive_hint {
                Some(false) => McpSideEffect::NonDestructiveWrite,
                _ => McpSideEffect::Destructive,
            },
            // Silence on read-only. A lone `destructiveHint: true` is still a
            // statement worth honouring; anything else is undeclared.
            None => match self.destructive_hint {
                Some(true) => McpSideEffect::Destructive,
                _ => McpSideEffect::Undeclared,
            },
        }
    }

    /// True when the server supplied no annotation fields whatsoever.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.read_only_hint.is_none()
            && self.destructive_hint.is_none()
            && self.idempotent_hint.is_none()
            && self.open_world_hint.is_none()
    }
}

/// One tool advertised by an MCP server's `tools/list` response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpToolDef {
    /// The tool's name as the server reports it (NOT yet namespaced).
    pub name: String,
    /// Human/model-readable description (may be empty).
    pub description: String,
    /// The tool's `inputSchema` object, serialized as a JSON string. Defaults
    /// to an open object schema when the server omits it.
    pub input_schema_json: String,
    /// The server's own side-effect hints. Empty when it supplied none.
    pub annotations: McpToolAnnotations,
}

/// Version of the canonical form [`McpToolDef::digest`] hashes. A change to the
/// set of fields or to how they are rendered must bump it, so a digest produced
/// under one form is never compared with one produced under another.
const TOOL_DIGEST_VERSION: u32 = 1;

impl McpToolDef {
    /// The tool's `inputSchema` as a JSON value. A schema the server sent that
    /// does not parse still takes part in the digest, as the raw string, so a
    /// server cannot change it unnoticed by making it unparseable.
    fn input_schema_value(&self) -> Value {
        serde_json::from_str(&self.input_schema_json)
            .unwrap_or_else(|_| Value::String(self.input_schema_json.clone()))
    }

    /// Lowercase hex SHA-256 over the canonical form of everything the server
    /// said about this tool that a caller could act on: name, description,
    /// input schema and the five annotation hints (`null` when absent, so a
    /// hint going from silent to `false` is a change).
    ///
    /// The canonical form is rendered by [`canonical_json`], which sorts every
    /// object's keys itself instead of relying on `serde_json`'s map order: the
    /// `preserve_order` feature can be switched on by any crate in the build,
    /// and a digest that depends on it would flip with an unrelated dependency.
    #[must_use]
    pub fn digest(&self) -> String {
        let annotations = &self.annotations;
        let document = json!({
            "v": TOOL_DIGEST_VERSION,
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema_value(),
            "annotations": {
                "title": annotations.title,
                "readOnlyHint": annotations.read_only_hint,
                "destructiveHint": annotations.destructive_hint,
                "idempotentHint": annotations.idempotent_hint,
                "openWorldHint": annotations.open_world_hint,
            },
        });
        sha256_hex(canonical_json(&document).as_bytes())
    }

    /// Lowercase hex SHA-256 of the description text. Lets a reviewer tell a
    /// description edit from a schema edit without the registry ever storing
    /// either.
    #[must_use]
    pub fn description_sha256(&self) -> String {
        sha256_hex(self.description.as_bytes())
    }

    /// Lowercase hex SHA-256 of the canonical `inputSchema`.
    #[must_use]
    pub fn schema_sha256(&self) -> String {
        sha256_hex(canonical_json(&self.input_schema_value()).as_bytes())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble is < 16"));
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble is < 16"));
    }
    hex
}

/// Render `value` as JSON with no whitespace and every object's keys in sorted
/// order, recursively. Arrays keep their order (it is meaningful), strings and
/// numbers use `serde_json`'s own rendering.
fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
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
        annotations: parse_tool_annotations(tool.get("annotations")),
    })
}

/// Extract MCP's optional `annotations` object. A missing object, a
/// non-object, or a field of the wrong JSON type all yield `None` for that
/// field rather than a guess — see [`McpToolAnnotations`] on why silence and
/// `false` must stay distinguishable.
#[must_use]
fn parse_tool_annotations(annotations: Option<&Value>) -> McpToolAnnotations {
    let Some(object) = annotations.and_then(Value::as_object) else {
        return McpToolAnnotations::default();
    };
    let flag = |key: &str| object.get(key).and_then(Value::as_bool);
    McpToolAnnotations {
        title: object
            .get("title")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        read_only_hint: flag("readOnlyHint"),
        destructive_hint: flag("destructiveHint"),
        idempotent_hint: flag("idempotentHint"),
        open_world_hint: flag("openWorldHint"),
    }
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
        // Neither tool annotated itself, so nothing is claimed on its behalf.
        assert!(page.tools[0].annotations.is_empty());
        assert_eq!(
            page.tools[0].annotations.side_effect(),
            McpSideEffect::Undeclared
        );
    }

    #[test]
    fn parse_extracts_every_annotation_hint_the_server_supplies() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[
            {"name":"search","annotations":{"title":"Search","readOnlyHint":true,"idempotentHint":true,"openWorldHint":true}}
        ]}}"#;
        let page = parse_list_tools_response(3, line).expect("parse ok");
        let a = &page.tools[0].annotations;
        assert_eq!(a.title.as_deref(), Some("Search"));
        assert_eq!(a.read_only_hint, Some(true));
        assert_eq!(a.idempotent_hint, Some(true));
        assert_eq!(a.open_world_hint, Some(true));
        // Absent stays absent — never coerced to a value the server never sent.
        assert_eq!(a.destructive_hint, None);
        assert_eq!(a.side_effect(), McpSideEffect::ReadOnly);
    }

    #[test]
    fn side_effect_applies_the_specs_defaults_without_inventing_claims() {
        let annotated = |json: &str| -> McpToolAnnotations {
            let line = format!(
                r#"{{"jsonrpc":"2.0","id":3,"result":{{"tools":[{{"name":"t","annotations":{json}}}]}}}}"#
            );
            parse_list_tools_response(3, &line).expect("parse ok").tools[0]
                .annotations
                .clone()
        };

        // readOnlyHint wins outright.
        assert_eq!(
            annotated(r#"{"readOnlyHint":true,"destructiveHint":true}"#).side_effect(),
            McpSideEffect::ReadOnly
        );
        // Explicitly not read-only, destructiveHint omitted ⇒ spec default is
        // destructive. Assuming otherwise would understate a real write.
        assert_eq!(
            annotated(r#"{"readOnlyHint":false}"#).side_effect(),
            McpSideEffect::Destructive
        );
        // Not read-only but explicitly non-destructive ⇒ an additive write.
        assert_eq!(
            annotated(r#"{"readOnlyHint":false,"destructiveHint":false}"#).side_effect(),
            McpSideEffect::NonDestructiveWrite
        );
        // A lone destructiveHint:true is still a statement worth honouring.
        assert_eq!(
            annotated(r#"{"destructiveHint":true}"#).side_effect(),
            McpSideEffect::Destructive
        );
        // Silence is Undeclared, NOT ReadOnly — the caller must decide what to
        // do about a server that says nothing, and must not be handed a
        // default that reads like a safety claim.
        assert_eq!(
            annotated(r#"{"idempotentHint":true}"#).side_effect(),
            McpSideEffect::Undeclared
        );
    }

    #[test]
    fn malformed_annotations_yield_no_claims_rather_than_a_guess() {
        // Wrong JSON types and a non-object annotations value must not be
        // coerced into booleans.
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[
            {"name":"a","annotations":{"readOnlyHint":"yes","destructiveHint":1}},
            {"name":"b","annotations":"nonsense"}
        ]}}"#;
        let page = parse_list_tools_response(3, line).expect("parse ok");
        assert_eq!(page.tools[0].annotations.read_only_hint, None);
        assert_eq!(page.tools[0].annotations.destructive_hint, None);
        assert_eq!(
            page.tools[0].annotations.side_effect(),
            McpSideEffect::Undeclared
        );
        assert!(page.tools[1].annotations.is_empty());
    }

    /// The tool the digest vectors below are computed over. Mirrored by the
    /// registry's own tests, which feed the same digest string through the
    /// observation route.
    fn echo_read() -> McpToolDef {
        McpToolDef {
            name: "echo_read".to_owned(),
            description: "Echo the text back.".to_owned(),
            input_schema_json:
                r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}"#
                    .to_owned(),
            annotations: McpToolAnnotations {
                read_only_hint: Some(true),
                ..McpToolAnnotations::default()
            },
        }
    }

    // CROSS-LANGUAGE KNOWN-ANSWER VECTOR. The same digest is accepted by
    // capability-core's observation route in
    // go/services/capability-core/internal/api/mcp_tools_test.go
    // (knownAnswerToolDigest); the value was also reproduced independently by
    // hashing the canonical form below with another implementation.
    // canonical form:
    // {"annotations":{"destructiveHint":null,"idempotentHint":null,"openWorldHint":null,"readOnlyHint":true,"title":null},"description":"Echo the text back.","inputSchema":{"properties":{"text":{"type":"string"}},"required":["text"],"type":"object"},"name":"echo_read","v":1}
    const ECHO_READ_DIGEST: &str =
        "aad57097d17d7540d459e94756b10a2b703a9281b3988441933a3ac682bc8f95";
    const ECHO_READ_DESCRIPTION_SHA256: &str =
        "154d65d3a5b2b5e71b01b1e62a7417b232c56cc2261e14bd1cdd4b8861ac43ac";
    const ECHO_READ_SCHEMA_SHA256: &str =
        "e3dce8d1afb8d03604af3bc6729219a5cd7df05558807b6e44e391d56fdf75d9";

    #[test]
    fn tool_digest_matches_the_known_answer_vector() {
        let tool = echo_read();
        assert_eq!(tool.digest(), ECHO_READ_DIGEST);
        assert_eq!(tool.description_sha256(), ECHO_READ_DESCRIPTION_SHA256);
        assert_eq!(tool.schema_sha256(), ECHO_READ_SCHEMA_SHA256);
    }

    #[test]
    fn tool_digest_is_stable_under_schema_key_order() {
        // The same schema with every object's keys in another order is the same
        // definition, so it must not read as drift.
        let mut reordered = echo_read();
        reordered.input_schema_json =
            r#"{"required":["text"],"properties":{"text":{"type":"string"}},"type":"object"}"#
                .to_owned();
        assert_eq!(reordered.digest(), ECHO_READ_DIGEST);
        assert_eq!(reordered.schema_sha256(), ECHO_READ_SCHEMA_SHA256);
    }

    #[test]
    fn a_description_only_change_changes_the_digest() {
        let mut changed = echo_read();
        changed.description = "Echo the text back, then email it.".to_owned();
        assert_ne!(changed.digest(), ECHO_READ_DIGEST);
        assert_ne!(changed.description_sha256(), ECHO_READ_DESCRIPTION_SHA256);
        // The schema did not move, so a reviewer can see which half changed.
        assert_eq!(changed.schema_sha256(), ECHO_READ_SCHEMA_SHA256);
    }

    #[test]
    fn a_schema_change_changes_the_digest() {
        let mut changed = echo_read();
        changed.input_schema_json =
            r#"{"type":"object","properties":{"text":{"type":"string"},"to":{"type":"string"}},"required":["text"]}"#
                .to_owned();
        assert_ne!(changed.digest(), ECHO_READ_DIGEST);
        assert_ne!(changed.schema_sha256(), ECHO_READ_SCHEMA_SHA256);
        assert_eq!(changed.description_sha256(), ECHO_READ_DESCRIPTION_SHA256);
    }

    #[test]
    fn an_array_order_change_changes_the_digest() {
        // Unlike object keys, array order is meaningful (`required`, `enum`).
        let mut changed = echo_read();
        changed.input_schema_json =
            r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text","to"]}"#
                .to_owned();
        let mut swapped = changed.clone();
        swapped.input_schema_json =
            r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["to","text"]}"#
                .to_owned();
        assert_ne!(changed.digest(), swapped.digest());
    }

    #[test]
    fn an_annotation_change_changes_the_digest_and_silence_differs_from_false() {
        let mut flipped = echo_read();
        flipped.annotations.read_only_hint = Some(false);
        assert_ne!(flipped.digest(), ECHO_READ_DIGEST);

        let mut silent = echo_read();
        silent.annotations.read_only_hint = None;
        assert_ne!(silent.digest(), ECHO_READ_DIGEST);
        assert_ne!(silent.digest(), flipped.digest());
    }

    #[test]
    fn an_unparseable_schema_still_takes_part_in_the_digest() {
        let mut broken = echo_read();
        broken.input_schema_json = "{not json".to_owned();
        let mut other = echo_read();
        other.input_schema_json = "{also not json".to_owned();
        assert_ne!(broken.digest(), ECHO_READ_DIGEST);
        assert_ne!(broken.digest(), other.digest());
    }

    #[test]
    fn side_effect_wire_labels_are_the_registry_vocabulary() {
        assert_eq!(McpSideEffect::ReadOnly.as_wire(), "read_only");
        assert_eq!(McpSideEffect::Destructive.as_wire(), "destructive");
        assert_eq!(
            McpSideEffect::NonDestructiveWrite.as_wire(),
            "non_destructive_write"
        );
        assert_eq!(McpSideEffect::Undeclared.as_wire(), "undeclared");
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

    #[test]
    fn initialize_without_elicitation_is_unchanged() {
        // The exact bytes every session sent before EP-4: the switch-off path must keep them.
        let expected = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "model-gateway", "version": "1" }
            }
        });
        assert_eq!(build_initialize_request(1), expected);
        assert_eq!(build_initialize_request_with(1, false), expected);
        assert_eq!(
            build_initialize_request(1).to_string(),
            expected.to_string()
        );
    }

    #[test]
    fn initialize_with_elicitation_advertises_it() {
        let r = build_initialize_request_with(1, true);
        assert_eq!(r["params"]["protocolVersion"], ELICITATION_PROTOCOL_VERSION);
        assert_eq!(r["params"]["capabilities"], json!({ "elicitation": {} }));
        assert!(r["params"]["capabilities"].get("sampling").is_none());
    }

    #[test]
    fn server_request_is_parsed() {
        let elicit = json!({
            "jsonrpc": "2.0",
            "id": "s-1",
            "method": "elicitation/create",
            "params": { "message": "Hvilket prosjekt?", "requestedSchema": { "type": "object" } }
        });
        let parsed = parse_server_request(&elicit).expect("a request");
        assert_eq!(parsed.id(), &json!("s-1"));
        match parsed {
            ServerRequest::Elicitation { params, .. } => {
                assert_eq!(params["message"], "Hvilket prosjekt?");
            }
            other => panic!("not an elicitation: {other:?}"),
        }
        assert_eq!(
            parse_server_request(&json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" })),
            Some(ServerRequest::Ping { id: json!(4) })
        );
        // A response and a notification are not requests.
        assert!(parse_server_request(&json!({ "jsonrpc": "2.0", "id": 3, "result": {} })).is_none());
        assert!(parse_server_request(
            &json!({ "jsonrpc": "2.0", "method": "notifications/progress" })
        )
        .is_none());
    }

    #[test]
    fn sampling_is_refused_with_method_not_found() {
        let sampling = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "sampling/createMessage",
            "params": { "messages": [] }
        });
        let parsed = parse_server_request(&sampling).expect("a request");
        assert!(matches!(parsed, ServerRequest::Unsupported { ref method, .. } if method == "sampling/createMessage"));
        let reply = build_method_not_found(parsed.id());
        assert_eq!(reply["id"], 9);
        assert_eq!(reply["error"]["code"], METHOD_NOT_FOUND);
        assert!(reply.get("result").is_none());
    }

    #[test]
    fn elicitation_result_carries_content_only_on_accept() {
        let accept = build_elicitation_result(&json!(2), "accept", Some(&json!({ "a": 1 })));
        assert_eq!(accept["result"], json!({ "action": "accept", "content": { "a": 1 } }));
        let cancel = build_elicitation_result(&json!("x"), "cancel", None);
        assert_eq!(cancel["result"], json!({ "action": "cancel" }));
        assert_eq!(cancel["id"], "x");
    }
}
