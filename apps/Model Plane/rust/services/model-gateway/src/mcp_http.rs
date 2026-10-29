//! MCP **Streamable HTTP** transport — the real wire protocol remote MCP
//! servers speak, layered on the pure JSON-RPC framing in
//! [`crate::mcp_jsonrpc`].
//!
//! ## Why this exists
//!
//! The original `http` transport in [`crate::runtime_registries`] POSTed to
//! `{url}/tools/list` and `{url}/tools/call` with bespoke REST bodies,
//! documented as expecting a "bridge" sidecar that would translate REST into
//! real MCP. That bridge was never built, so the shape only ever matched a
//! hypothetical server: a genuine MCP endpoint 404s those sub-paths.
//! Confirmed against Visma Net's production server —
//! `POST {url}/tools/list` → 404, while `POST {url}` carrying a JSON-RPC
//! body → 401 + `WWW-Authenticate` (i.e. the right path, awaiting a token).
//!
//! ## The actual protocol
//!
//! One endpoint (the server URL itself) accepts JSON-RPC POSTs. A session is
//! opened with `initialize`, acknowledged with `notifications/initialized`,
//! and thereafter carries any server-assigned `Mcp-Session-Id` header back on
//! every request. Responses arrive either as a plain JSON body or as an SSE
//! (`text/event-stream`) frame sequence, so both are accepted — the client
//! advertises both in `Accept` and does not care which it gets.
//!
//! Transport concerns only. Every envelope built and every payload parsed
//! comes from [`crate::mcp_jsonrpc`], so the protocol semantics stay in one
//! place and remain unit-testable without IO.

use serde_json::Value;

use crate::mcp_jsonrpc::{
    build_initialize_request, build_initialized_notification, build_list_tools_request,
    build_tool_call_request, is_response_for, parse_list_tools_response, parse_tool_call_response,
    McpCallOutcome, McpToolDef, MCP_PROTOCOL_VERSION,
};

/// Defensive cap on `tools/list` pages followed per discovery call. A
/// well-behaved server needs one or two pages even for large catalogs; this
/// only guards against a misbehaving server returning a `nextCursor` forever.
const MAX_TOOLS_LIST_PAGES: u32 = 50;

/// Session id the server may assign at `initialize`, echoed on later calls.
const MCP_SESSION_HEADER: &str = "mcp-session-id";
/// Negotiated protocol revision, sent alongside every request.
const MCP_PROTOCOL_HEADER: &str = "mcp-protocol-version";

// Fixed request ids: each session issues at most one of each call, so a
// counter would add state without buying anything. `is_response_for` still
// pins every response to the id that asked for it.
const INITIALIZE_ID: i64 = 1;
const LIST_TOOLS_ID: i64 = 2;
const CALL_TOOL_ID: i64 = 3;

/// Pull the JSON-RPC response for `expected_id` out of a response body,
/// accepting either shape the spec permits: a bare JSON object, or SSE frames
/// whose `data:` lines each carry one JSON-RPC message. Returns the matching
/// message as a string ready for [`crate::mcp_jsonrpc`]'s parsers, or `None`
/// when the body holds no response for that id (e.g. only notifications).
///
/// Pure — no IO, fully unit-tested below.
#[must_use]
pub fn extract_jsonrpc_response(body: &str, expected_id: i64) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Plain `application/json` body.
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if is_response_for(&value, expected_id) {
            return Some(trimmed.to_owned());
        }
    }
    // SSE framing: interleaved `event:`/`id:`/`data:` lines. Only `data:`
    // payloads carry JSON-RPC, and a stream may deliver notifications before
    // the awaited response, so scan every frame for the matching id.
    trimmed
        .lines()
        .filter_map(|line| line.trim().strip_prefix("data:"))
        .map(str::trim)
        .filter(|data| !data.is_empty())
        .find_map(|data| {
            let value = serde_json::from_str::<Value>(data).ok()?;
            is_response_for(&value, expected_id).then(|| data.to_owned())
        })
}

/// An initialized MCP Streamable HTTP session against one server.
pub struct McpHttpSession {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    /// Bearer credential, or empty for an unauthenticated server. For
    /// OAuth-connected servers this is a live access token resolved from
    /// capability-core, never the (always-empty) cached `McpServer::token`.
    token: String,
    session_id: Option<String>,
}

impl McpHttpSession {
    /// Open a session: SSRF-guarded client, `initialize` handshake, then the
    /// `notifications/initialized` acknowledgement.
    ///
    /// # Errors
    /// Returns `Err` when the URL fails the SSRF/scheme guard, the server is
    /// unreachable, the HTTP status is not success (401 for a missing or
    /// expired credential, for instance), or `initialize` returns a JSON-RPC
    /// error / no parseable response.
    pub async fn connect(url: &str, token: &str) -> Result<Self, String> {
        let (client, endpoint) = crate::runtime_registries::safe_mcp_http_client(url).await?;
        let mut session = Self {
            client,
            endpoint,
            token: token.to_owned(),
            session_id: None,
        };

        let (body, session_id) = session
            .post(&build_initialize_request(INITIALIZE_ID))
            .await?;
        session.session_id = session_id;
        let response = extract_jsonrpc_response(&body, INITIALIZE_ID)
            .ok_or_else(|| "initialize returned no JSON-RPC response".to_owned())?;
        let value: Value = serde_json::from_str(&response)
            .map_err(|error| format!("initialize response is not valid json: {error}"))?;
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| error.to_string(), ToOwned::to_owned);
            return Err(format!("initialize rejected: {message}"));
        }

        // Fire-and-forget per spec (no id, so no response to await). A server
        // that rejects it is still usable, so a failure here must not fail the
        // whole session.
        if let Err(error) = session.post(&build_initialized_notification()).await {
            tracing::debug!(%error, "mcp http: initialized notification not accepted");
        }
        Ok(session)
    }

    /// POST one JSON-RPC envelope, returning `(body, server-assigned session id)`.
    async fn post(&self, payload: &Value) -> Result<(String, Option<String>), String> {
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(MCP_PROTOCOL_HEADER, MCP_PROTOCOL_VERSION)
            .json(payload);
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        if let Some(session_id) = self.session_id.as_deref() {
            request = request.header(MCP_SESSION_HEADER, session_id);
        }

        let response = request
            .send()
            .await
            .map_err(|error| format!("transport: {error}"))?;
        let status = response.status();
        let session_id = response
            .headers()
            .get(MCP_SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = crate::runtime_registries::bounded_mcp_response(response).await?;
        if !status.is_success() {
            return Err(format!(
                "mcp HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            ));
        }
        Ok((body, session_id))
    }

    /// Discover every tool this server advertises, following `nextCursor`
    /// (MCP pagination spec) until the server reports no further page or
    /// [`MAX_TOOLS_LIST_PAGES`] is reached.
    ///
    /// # Errors
    /// Returns `Err` on transport failure, a non-success status, a JSON-RPC
    /// error, or a `result` without the `tools` array — on any page.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOLS_LIST_PAGES {
            let (body, _) = self
                .post(&build_list_tools_request(LIST_TOOLS_ID, cursor.as_deref()))
                .await?;
            let response = extract_jsonrpc_response(&body, LIST_TOOLS_ID)
                .ok_or_else(|| "tools/list returned no JSON-RPC response".to_owned())?;
            let page = parse_list_tools_response(LIST_TOOLS_ID, &response)?;
            tools.extend(page.tools);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(tools),
            }
        }
        Ok(tools)
    }

    /// Invoke one tool. Transport and protocol failures both surface as
    /// [`McpCallOutcome::Err`] so callers have a single failure shape.
    pub async fn call_tool(&self, tool_name: &str, arguments: &Value) -> McpCallOutcome {
        let payload = build_tool_call_request(CALL_TOOL_ID, tool_name, arguments);
        match self.post(&payload).await {
            Ok((body, _)) => extract_jsonrpc_response(&body, CALL_TOOL_ID).map_or_else(
                || McpCallOutcome::Err("tools/call returned no JSON-RPC response".to_owned()),
                |response| parse_tool_call_response(CALL_TOOL_ID, &response),
            ),
            Err(error) => McpCallOutcome::Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_plain_json_response() {
        let body = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#;
        let found = extract_jsonrpc_response(body, 2).expect("response found");
        assert!(found.contains("\"tools\""));
    }

    #[test]
    fn extracts_a_response_from_sse_frames() {
        // Real servers interleave event/id lines and may emit notifications
        // before the awaited response.
        let body = concat!(
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n",
            "\n",
            "event: message\n",
            "id: 42\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"content\":[]}}\n",
            "\n",
        );
        let found = extract_jsonrpc_response(body, 3).expect("response found");
        assert!(found.contains("\"result\""));
        assert!(!found.contains("notifications/message"));
    }

    #[test]
    fn ignores_responses_for_other_ids_and_empty_bodies() {
        let other = r#"{"jsonrpc":"2.0","id":9,"result":{}}"#;
        assert!(extract_jsonrpc_response(other, 2).is_none());
        assert!(extract_jsonrpc_response("", 2).is_none());
        assert!(extract_jsonrpc_response("   \n  ", 2).is_none());
        // SSE stream carrying only notifications — nothing for our id.
        let notifications_only =
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n";
        assert!(extract_jsonrpc_response(notifications_only, 2).is_none());
    }

    #[test]
    fn tolerates_malformed_frames_and_keeps_scanning() {
        let body = concat!(
            "data: not-json\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n",
        );
        let found = extract_jsonrpc_response(body, 2).expect("response found");
        assert!(found.contains("\"ok\":true"));
    }

    #[test]
    fn parsed_sse_response_round_trips_through_the_jsonrpc_parsers() {
        // The extracted string must be directly consumable by mcp_jsonrpc,
        // which is the whole point of returning a string rather than a Value.
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"execute_query\"}]}}\n";
        let response = extract_jsonrpc_response(body, 2).expect("response found");
        let page = parse_list_tools_response(2, &response).expect("parses");
        assert_eq!(page.tools.len(), 1);
        assert_eq!(page.tools[0].name, "execute_query");
        assert_eq!(page.next_cursor, None);
    }
}
