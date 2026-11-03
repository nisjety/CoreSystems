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

use std::collections::VecDeque;
use std::time::Duration;

use serde_json::Value;

use crate::mcp_jsonrpc::{
    build_empty_result, build_initialize_request_with, build_initialized_notification,
    build_list_tools_request, build_method_not_found, build_tool_call_request, is_response_for,
    parse_list_tools_response, parse_server_request, parse_tool_call_response, McpCallOutcome,
    McpToolDef, ServerRequest, ELICITATION_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION,
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

/// Accumulates SSE bytes and yields the `data:` payload of every complete
/// event. Frames end at a blank line; `data:` lines of one event are joined
/// with a newline, as the SSE specification says. Carriage returns are
/// dropped so `\r\n` streams frame the same way.
#[derive(Default)]
struct SseFrames {
    pending: Vec<u8>,
}

impl SseFrames {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        self.pending
            .extend(chunk.iter().copied().filter(|byte| *byte != b'\r'));
        let mut payloads = Vec::new();
        while let Some(end) = self
            .pending
            .windows(2)
            .position(|window| window == b"\n\n")
        {
            let frame: Vec<u8> = self.pending.drain(..end + 2).collect();
            if let Some(payload) = frame_payload(&frame[..end])? {
                payloads.push(payload);
            }
        }
        Ok(payloads)
    }

    /// The last event of a stream that ended without a trailing blank line.
    fn finish(&mut self) -> Result<Option<String>, String> {
        let frame = std::mem::take(&mut self.pending);
        frame_payload(&frame)
    }
}

fn frame_payload(frame: &[u8]) -> Result<Option<String>, String> {
    let text = std::str::from_utf8(frame).map_err(|_| "MCP response is not valid UTF-8".to_owned())?;
    let data: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| data.strip_prefix(' ').unwrap_or(data))
        .collect();
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some(data.join("\n")))
}

/// Where a [`McpMessageStream`] reads its bytes from.
enum ChunkSource {
    Http(reqwest::Response),
    #[cfg(test)]
    Scripted(VecDeque<Vec<u8>>),
}

impl ChunkSource {
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Http(response) => response
                .chunk()
                .await
                .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
                .map_err(|error| format!("MCP response read failed: {error}")),
            #[cfg(test)]
            Self::Scripted(chunks) => Ok(chunks.pop_front()),
        }
    }
}

/// What the next message of a tool call's response stream is.
#[derive(Debug, PartialEq)]
pub enum StreamEvent {
    /// The response to the awaited request id.
    Response(McpCallOutcome),
    /// A request the server sent the client before answering.
    Request(ServerRequest),
    /// The stream ended without a response.
    End,
}

/// One response body read message by message (EP-4 S4), so a server that asks
/// the user a question inside its `tools/call` stream is seen as soon as it
/// asks instead of after the transport times out. The byte cap of
/// `bounded_mcp_response` applies to the whole stream, counted across frames,
/// so a server cannot stream without bound by splitting its output.
pub struct McpMessageStream {
    source: ChunkSource,
    sse: bool,
    frames: SseFrames,
    plain_body: Vec<u8>,
    queue: VecDeque<Value>,
    read: usize,
    limit: usize,
    finished: bool,
}

impl McpMessageStream {
    fn from_response(response: reqwest::Response) -> Self {
        let sse = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.trim_start().starts_with("text/event-stream"));
        Self::new(
            ChunkSource::Http(response),
            sse,
            crate::runtime_registries::MCP_MAX_RESPONSE_BYTES,
        )
    }

    fn new(source: ChunkSource, sse: bool, limit: usize) -> Self {
        Self {
            source,
            sse,
            frames: SseFrames::default(),
            plain_body: Vec::new(),
            queue: VecDeque::new(),
            read: 0,
            limit,
            finished: false,
        }
    }

    /// A malformed payload is skipped, as [`extract_jsonrpc_response`] skips it.
    fn enqueue(&mut self, payload: &str) {
        match serde_json::from_str::<Value>(payload.trim()) {
            Ok(Value::Array(batch)) => self.queue.extend(batch),
            Ok(value) => self.queue.push_back(value),
            Err(_) => {}
        }
    }

    async fn next_message(&mut self) -> Result<Option<Value>, String> {
        loop {
            if let Some(value) = self.queue.pop_front() {
                return Ok(Some(value));
            }
            if self.finished {
                return Ok(None);
            }
            match self.source.chunk().await? {
                Some(chunk) => {
                    self.read = self.read.saturating_add(chunk.len());
                    if self.read > self.limit {
                        return Err("MCP response exceeds size limit".to_owned());
                    }
                    if self.sse {
                        for payload in self.frames.push(&chunk)? {
                            self.enqueue(&payload);
                        }
                    } else {
                        self.plain_body.extend_from_slice(&chunk);
                    }
                }
                None => {
                    self.finished = true;
                    if self.sse {
                        if let Some(payload) = self.frames.finish()? {
                            self.enqueue(&payload);
                        }
                    } else {
                        let body = std::mem::take(&mut self.plain_body);
                        let text = String::from_utf8(body)
                            .map_err(|_| "MCP response is not valid UTF-8".to_owned())?;
                        self.enqueue(&text);
                    }
                }
            }
        }
    }

    /// Read on to the next message that matters for the request `call_id`:
    /// its response, or a request from the server. Notifications and
    /// responses for other ids are skipped.
    ///
    /// # Errors
    /// A read failure, invalid UTF-8, or the byte cap.
    pub async fn next_event(&mut self, call_id: i64) -> Result<StreamEvent, String> {
        while let Some(value) = self.next_message().await? {
            // A server request may carry a numeric id equal to ours, so the
            // method is checked before the id.
            if let Some(request) = parse_server_request(&value) {
                return Ok(StreamEvent::Request(request));
            }
            if is_response_for(&value, call_id) {
                return Ok(StreamEvent::Response(parse_tool_call_response(
                    call_id,
                    &value.to_string(),
                )));
            }
        }
        Ok(StreamEvent::End)
    }
}

/// The outcome of reading a tool call until it either ends or waits on the user.
pub enum Interactive {
    /// The call finished (or failed).
    Done(McpCallOutcome),
    /// The server asked the user something (`elicitation/create`) and waits:
    /// answer `request_id` with [`McpHttpSession::respond`], then continue
    /// with [`McpHttpSession::read_on`] on `stream`.
    Elicit {
        request_id: Value,
        params: Value,
        stream: McpMessageStream,
    },
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
    /// The revision sent in the protocol header: [`MCP_PROTOCOL_VERSION`]
    /// unless this session advertised elicitation and the server agreed.
    protocol_version: &'static str,
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
        Self::connect_with(url, token, false).await
    }

    /// [`Self::connect`], advertising elicitation when `elicitation` is true
    /// (EP-4 S4, D6). With `false` every byte on the wire is today's.
    ///
    /// # Errors
    /// As [`Self::connect`].
    pub async fn connect_with(url: &str, token: &str, elicitation: bool) -> Result<Self, String> {
        let (client, endpoint) = crate::runtime_registries::safe_mcp_http_client(url).await?;
        let mut session = Self {
            client,
            endpoint,
            token: token.to_owned(),
            session_id: None,
            protocol_version: if elicitation {
                ELICITATION_PROTOCOL_VERSION
            } else {
                MCP_PROTOCOL_VERSION
            },
        };

        let (body, session_id) = session
            .post(&build_initialize_request_with(INITIALIZE_ID, elicitation))
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
        // A server that answers an older revision does not know elicitation
        // and never sends it; the session then speaks today's revision.
        if elicitation
            && value
                .pointer("/result/protocolVersion")
                .and_then(Value::as_str)
                != Some(ELICITATION_PROTOCOL_VERSION)
        {
            session.protocol_version = MCP_PROTOCOL_VERSION;
        }

        // Fire-and-forget per spec (no id, so no response to await). A server
        // that rejects it is still usable, so a failure here must not fail the
        // whole session.
        if let Err(error) = session.post(&build_initialized_notification()).await {
            tracing::debug!(%error, "mcp http: initialized notification not accepted");
        }
        Ok(session)
    }

    /// The POST of one JSON-RPC envelope with the session's headers.
    fn request(&self, payload: &Value) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(MCP_PROTOCOL_HEADER, self.protocol_version)
            .json(payload);
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        if let Some(session_id) = self.session_id.as_deref() {
            request = request.header(MCP_SESSION_HEADER, session_id);
        }
        request
    }

    /// POST one JSON-RPC envelope, returning `(body, server-assigned session id)`.
    async fn post(&self, payload: &Value) -> Result<(String, Option<String>), String> {
        let response = self
            .request(payload)
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

    /// Invoke one tool and read its response stream message by message
    /// (EP-4 S4), stopping when the call ends or the server asks the user a
    /// question. `stream_timeout` replaces the client's 10 s total timeout for
    /// this one request, because a held stream stays open while the user
    /// answers; it must outlast the hold window. [`Self::call_tool`] is
    /// unchanged.
    pub async fn call_tool_interactive(
        &self,
        tool_name: &str,
        arguments: &Value,
        stream_timeout: Duration,
    ) -> Interactive {
        let payload = build_tool_call_request(CALL_TOOL_ID, tool_name, arguments);
        let response = match self.request(&payload).timeout(stream_timeout).send().await {
            Ok(response) => response,
            Err(error) => return Interactive::Done(McpCallOutcome::Err(format!("transport: {error}"))),
        };
        let status = response.status();
        if !status.is_success() {
            let body = crate::runtime_registries::bounded_mcp_response(response)
                .await
                .unwrap_or_default();
            return Interactive::Done(McpCallOutcome::Err(format!(
                "mcp HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            )));
        }
        self.read_on(McpMessageStream::from_response(response)).await
    }

    /// Continue reading a tool call's stream after the user's answer was sent.
    /// A `ping` is answered and a request this client does not serve
    /// (`sampling/createMessage` among them, D15) gets method-not-found; the
    /// read then goes on.
    pub async fn read_on(&self, mut stream: McpMessageStream) -> Interactive {
        loop {
            let event = match stream.next_event(CALL_TOOL_ID).await {
                Ok(event) => event,
                Err(error) => return Interactive::Done(McpCallOutcome::Err(error)),
            };
            let reply = match event {
                StreamEvent::Response(outcome) => return Interactive::Done(outcome),
                StreamEvent::End => {
                    return Interactive::Done(McpCallOutcome::Err(
                        "tools/call returned no JSON-RPC response".to_owned(),
                    ))
                }
                StreamEvent::Request(ServerRequest::Elicitation { id, params }) => {
                    return Interactive::Elicit {
                        request_id: id,
                        params,
                        stream,
                    }
                }
                StreamEvent::Request(ServerRequest::Ping { id }) => build_empty_result(&id),
                StreamEvent::Request(ServerRequest::Unsupported { id, method }) => {
                    tracing::debug!(method_len = method.len(), "mcp http: refused a server request");
                    build_method_not_found(&id)
                }
            };
            if let Err(error) = self.respond(&reply).await {
                return Interactive::Done(McpCallOutcome::Err(error));
            }
        }
    }

    /// POST the client's JSON-RPC response to a server request, with the
    /// session header. The server acknowledges it (202) and answers on the
    /// stream that carried its request.
    ///
    /// # Errors
    /// Transport failure or a non-success status.
    pub async fn respond(&self, payload: &Value) -> Result<(), String> {
        self.post(payload).await.map(|_| ())
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

    fn scripted(chunks: &[&str], sse: bool, limit: usize) -> McpMessageStream {
        McpMessageStream::new(
            ChunkSource::Scripted(chunks.iter().map(|c| c.as_bytes().to_vec()).collect()),
            sse,
            limit,
        )
    }

    #[tokio::test]
    async fn interactive_returns_done_for_a_plain_result() {
        let mut stream = scripted(
            &[
                r#"{"jsonrpc":"2.0","id":3,"#,
                r#""result":{"content":[{"type":"text","text":"ok"}]}}"#,
            ],
            false,
            1024,
        );
        let event = stream.next_event(CALL_TOOL_ID).await.expect("read");
        let StreamEvent::Response(McpCallOutcome::Ok(output)) = event else {
            panic!("not a result: {event:?}");
        };
        assert!(output.contains("\"ok\""));
        assert_eq!(stream.next_event(CALL_TOOL_ID).await, Ok(StreamEvent::End));
    }

    #[tokio::test]
    async fn interactive_stops_at_an_elicitation_frame() {
        // The elicitation frame is split across chunks and uses CRLF framing;
        // the final result follows only after the client would have answered.
        let mut stream = scripted(
            &[
                "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\n",
                "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"elicitation/create\",",
                "\"params\":{\"message\":\"Hvilket prosjekt?\",\"requestedSchema\":{\"type\":\"object\"}}}\r\n\r\n",
                "data: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"content\":[]}}\n\n",
            ],
            true,
            4096,
        );
        // A server request whose numeric id equals the call's is still a request.
        match stream.next_event(CALL_TOOL_ID).await.expect("read") {
            StreamEvent::Request(ServerRequest::Elicitation { id, params }) => {
                assert_eq!(id, serde_json::json!(3));
                assert_eq!(params["message"], "Hvilket prosjekt?");
            }
            other => panic!("not an elicitation: {other:?}"),
        }
        assert!(matches!(
            stream.next_event(CALL_TOOL_ID).await,
            Ok(StreamEvent::Response(McpCallOutcome::Ok(_)))
        ));
    }

    #[tokio::test]
    async fn sampling_request_surfaces_for_refusal() {
        let mut stream = scripted(
            &["data: {\"jsonrpc\":\"2.0\",\"id\":\"s1\",\"method\":\"sampling/createMessage\"}\n\n"],
            true,
            1024,
        );
        assert!(matches!(
            stream.next_event(CALL_TOOL_ID).await,
            Ok(StreamEvent::Request(ServerRequest::Unsupported { .. }))
        ));
    }

    #[tokio::test]
    async fn byte_cap_spans_frames() {
        // Every frame is far under the cap; together they exceed it.
        let frame = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n";
        let chunks: Vec<&str> = std::iter::repeat(frame).take(10).collect();
        let mut stream = scripted(&chunks, true, frame.len() * 5);
        assert_eq!(
            stream.next_event(CALL_TOOL_ID).await,
            Err("MCP response exceeds size limit".to_owned())
        );
    }
}
