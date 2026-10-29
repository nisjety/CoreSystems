use async_stream::stream;
use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::post,
    Json, Router,
};
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    domains::chat::shared,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
};

const DEFAULT_TOOL_PARAMETERS: &str = r#"{"type":"object","properties":{}}"#;
const DEFAULT_FEATURES: [&str; 6] = [
    "usage",
    "citations",
    "reasoning",
    "steps",
    "tools",
    "artifacts",
];
const MAX_TOOLS: usize = 32;
const MAX_TOOL_SCHEMA_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct SseParseState {
    event: Option<String>,
    id: Option<String>,
    data_lines: Vec<String>,
}

#[derive(Debug)]
struct SseFrame {
    event: Option<String>,
    id: Option<String>,
    data: String,
}

#[derive(Default)]
struct AgUiRunState {
    run_id: Option<String>,
    thread_id: Option<String>,
    text_started: bool,
}

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/ag-ui/stream", post(stream_agent_ui))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn stream_agent_ui(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let body = match normalize_agent_input(body) {
        Ok(body) => body,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_agent_input", message)),
            )
                .into_response();
        }
    };
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/invoke/stream", state.model_gateway_url);
    // Streaming client (no overall 25s timeout) so the agent run isn't severed mid-stream.
    let mut req = state.streaming_client.request(Method::POST, url);

    if let Some(token) = token {
        req = req.bearer_auth(token);
    }

    if shared::zdr_flag(&headers) || bool_field(&body, "zdr").unwrap_or(false) {
        req = req.header("x-zdr", "true");
    }

    let upstream = match req.json(&body).send().await {
        Ok(resp) => resp,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::upstream_unavailable()),
            )
                .into_response();
        }
    };

    let status_u16 = upstream.status().as_u16();
    if status_u16 >= 400 {
        return (
            StatusCode::from_u16(status_u16).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(error(
                if status_u16 == 401 {
                    "unauthorized"
                } else {
                    "model_error"
                },
                format!("Model gateway returned {status_u16}"),
            )),
        )
            .into_response();
    }

    let mut upstream_stream = upstream.bytes_stream();
    let output = stream! {
        let mut buffer = String::new();
        let mut parser = SseParseState::default();
        let mut run = AgUiRunState::default();

        while let Some(chunk) = upstream_stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    for event in map_error(&mut run, "The upstream service is unavailable.".to_owned()) {
                        yield Ok::<Event, std::convert::Infallible>(event);
                    }
                    break;
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));
            let mut lines: Vec<String> = buffer.split('\n').map(str::to_owned).collect();
            buffer = lines.pop().unwrap_or_default().to_owned();

            for raw in lines.drain(..) {
                if let Some(frame) = parse_sse_line(&mut parser, &raw) {
                    for event in map_upstream_frame(&mut run, frame) {
                        yield Ok::<Event, std::convert::Infallible>(event);
                    }
                }
            }
        }

        if !buffer.is_empty() {
            let tail = std::mem::take(&mut buffer);
            let _ = parse_sse_line(&mut parser, &tail);
        }

        if let Some(frame) = parse_sse_line(&mut parser, "") {
            for event in map_upstream_frame(&mut run, frame) {
                yield Ok::<Event, std::convert::Infallible>(event);
            }
        }
    };

    Sse::new(output)
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn normalize_agent_input(body: Value) -> Result<Value, String> {
    if string_field(&body, "content").is_some() {
        return normalize_legacy_invoke_body(body);
    }

    normalize_run_agent_input(body)
}

fn normalize_legacy_invoke_body(mut body: Value) -> Result<Value, String> {
    let content = string_field(&body, "content").ok_or_else(|| "content is required".to_owned())?;
    if content.trim().is_empty() {
        return Err("content cannot be empty".to_owned());
    }

    if let Some(obj) = body.as_object_mut() {
        if let Some(tools) = obj.get("tools").cloned() {
            obj.insert("tools".to_owned(), Value::Array(normalize_tools(&tools)));
        }
    }

    Ok(body)
}

fn normalize_run_agent_input(body: Value) -> Result<Value, String> {
    let forwarded = object_field(&body, "forwardedProps");
    let data = object_field(&body, "data");
    let content = latest_message_content(body.get("messages"))
        .ok_or_else(|| "messages must include at least one text message".to_owned())?;

    if content.trim().is_empty() {
        return Err("message content cannot be empty".to_owned());
    }

    let thread_id = lookup_string(&body, forwarded, data, &["threadId", "thread_id"]);
    let session_key = lookup_string(&body, forwarded, data, &["sessionKey", "session_key"])
        .or_else(|| thread_id.clone());
    let generate_image =
        lookup_bool(&body, forwarded, data, &["generateImage", "generate_image"]).unwrap_or(false);
    let zdr = lookup_bool(&body, forwarded, data, &["zdr"]).unwrap_or(false);
    let browse_web =
        lookup_bool(&body, forwarded, data, &["browseWeb", "browse_web"]).unwrap_or(false);
    let attachments = lookup_value(&body, forwarded, data, &["attachments"])
        .filter(|value| value.is_array())
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut tools = Vec::new();
    if browse_web {
        push_tool(&mut tools, web_search_tool());
    }
    if let Some(raw_tools) = body.get("tools") {
        for tool in normalize_tools(raw_tools) {
            push_tool(&mut tools, tool);
        }
    }
    if let Some(raw_tools) = lookup_value(&body, forwarded, data, &["tools"]) {
        for tool in normalize_tools(raw_tools) {
            push_tool(&mut tools, tool);
        }
    }

    let mut features =
        lookup_string_array(&body, forwarded, data, &["features"]).unwrap_or_else(|| {
            DEFAULT_FEATURES
                .iter()
                .map(|feature| feature.to_string())
                .collect()
        });
    if !tools.is_empty() && !features.iter().any(|feature| feature == "tools") {
        features.push("tools".to_owned());
    }

    let mut out = Map::new();
    out.insert("content".to_owned(), Value::String(content));
    out.insert(
        "profile".to_owned(),
        Value::String(
            lookup_string(&body, forwarded, data, &["profile"])
                .unwrap_or_else(|| "chat".to_owned()),
        ),
    );
    out.insert("generate_image".to_owned(), Value::Bool(generate_image));
    out.insert("attachments".to_owned(), attachments);
    out.insert(
        "features".to_owned(),
        Value::Array(features.into_iter().map(Value::String).collect()),
    );
    out.insert("tools".to_owned(), Value::Array(tools));
    out.insert("zdr".to_owned(), Value::Bool(zdr));

    if let Some(model) = lookup_string(&body, forwarded, data, &["model"]) {
        out.insert("model".to_owned(), Value::String(model));
    }
    // Response-style / verbosity profile (token-efficiency): forward it so the
    // model-gateway can inject the matching directive. `normal`/unset is a no-op.
    if let Some(verbosity) = lookup_string(
        &body,
        forwarded,
        data,
        &["verbosity", "responseStyle", "response_style"],
    ) {
        out.insert("verbosity".to_owned(), Value::String(verbosity));
    }
    if let Some(thread_id) = thread_id {
        out.insert("thread_id".to_owned(), Value::String(thread_id));
    }
    if let Some(session_key) = session_key {
        out.insert("session_key".to_owned(), Value::String(session_key));
    }

    Ok(Value::Object(out))
}

fn latest_message_content(messages: Option<&Value>) -> Option<String> {
    let messages = messages?.as_array()?;
    let mut fallback = None;

    for message in messages.iter().rev() {
        let Some(content) = message_content(message) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }

        if string_field(message, "role").as_deref() == Some("user") {
            return Some(content);
        }
        if fallback.is_none() {
            fallback = Some(content);
        }
    }

    fallback
}

fn message_content(message: &Value) -> Option<String> {
    if let Some(content) = string_field(message, "content") {
        return Some(content);
    }

    if let Some(content) = message.get("content").and_then(text_parts) {
        return Some(content);
    }

    message.get("parts").and_then(text_parts)
}

fn text_parts(value: &Value) -> Option<String> {
    let parts = value.as_array()?;
    let content = parts
        .iter()
        .filter_map(|part| {
            string_field(part, "text")
                .or_else(|| string_field(part, "content"))
                .filter(|text| !text.is_empty())
        })
        .collect::<Vec<_>>()
        .join("");

    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

fn object_field<'a>(body: &'a Value, field: &str) -> Option<&'a Map<String, Value>> {
    body.get(field)?.as_object()
}

fn lookup_value<'a>(
    body: &'a Value,
    forwarded: Option<&'a Map<String, Value>>,
    data: Option<&'a Map<String, Value>>,
    keys: &[&str],
) -> Option<&'a Value> {
    for key in keys {
        if let Some(value) = forwarded.and_then(|props| props.get(*key)) {
            return Some(value);
        }
    }
    for key in keys {
        if let Some(value) = data.and_then(|props| props.get(*key)) {
            return Some(value);
        }
    }
    for key in keys {
        if let Some(value) = body.get(*key) {
            return Some(value);
        }
    }
    None
}

fn lookup_string(
    body: &Value,
    forwarded: Option<&Map<String, Value>>,
    data: Option<&Map<String, Value>>,
    keys: &[&str],
) -> Option<String> {
    lookup_value(body, forwarded, data, keys)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn lookup_bool(
    body: &Value,
    forwarded: Option<&Map<String, Value>>,
    data: Option<&Map<String, Value>>,
    keys: &[&str],
) -> Option<bool> {
    value_bool(lookup_value(body, forwarded, data, keys)?)
}

fn lookup_string_array(
    body: &Value,
    forwarded: Option<&Map<String, Value>>,
    data: Option<&Map<String, Value>>,
    keys: &[&str],
) -> Option<Vec<String>> {
    let values = lookup_value(body, forwarded, data, keys)?.as_array()?;
    let out = values
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn normalize_tools(raw_tools: &Value) -> Vec<Value> {
    let Some(tools) = raw_tools.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for raw in tools.iter().take(MAX_TOOLS) {
        if let Some(tool) = normalize_tool(raw) {
            push_tool(&mut out, tool);
        }
    }
    out
}

fn normalize_tool(raw: &Value) -> Option<Value> {
    let name = string_field(raw, "name")
        .or_else(|| string_field(raw, "id"))
        .and_then(safe_tool_name)?;
    let description = string_field(raw, "description").unwrap_or_default();
    let parameters_json = tool_parameters_json(raw);

    Some(json!({
        "name": name,
        "description": description,
        "parameters_json": parameters_json,
    }))
}

fn push_tool(out: &mut Vec<Value>, tool: Value) {
    if out.len() >= MAX_TOOLS {
        return;
    }

    let Some(name) = string_field(&tool, "name") else {
        return;
    };
    if out
        .iter()
        .any(|existing| string_field(existing, "name").as_deref() == Some(name.as_str()))
    {
        return;
    }
    out.push(tool);
}

fn safe_tool_name(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return None;
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn tool_parameters_json(raw: &Value) -> String {
    for field in ["parameters_json", "parametersJson"] {
        if let Some(value) = string_field(raw, field) {
            if !value.trim().is_empty()
                && value.len() <= MAX_TOOL_SCHEMA_BYTES
                && serde_json::from_str::<Value>(&value)
                    .map(|parsed| parsed.is_object())
                    .unwrap_or(false)
            {
                return value;
            }
        }
    }

    for field in ["parameters", "inputSchema", "input_schema"] {
        if let Some(value) = raw.get(field) {
            if value.is_object() {
                if let Ok(serialized) = serde_json::to_string(value) {
                    if serialized.len() <= MAX_TOOL_SCHEMA_BYTES {
                        return serialized;
                    }
                }
            }
        }
    }

    DEFAULT_TOOL_PARAMETERS.to_owned()
}

fn web_search_tool() -> Value {
    json!({
        "name": "web_search",
        "description": "Search the public web for current, factual information and return relevant results.",
        "parameters_json": r#"{"type":"object","properties":{"query":{"type":"string","description":"The search query."}},"required":["query"]}"#,
    })
}

fn parse_sse_line(state: &mut SseParseState, raw: &str) -> Option<SseFrame> {
    let line = raw.strip_suffix('\r').unwrap_or(raw);

    if line.is_empty() {
        if state.data_lines.is_empty() {
            state.event = None;
            state.id = None;
            return None;
        }

        return Some(SseFrame {
            event: state.event.take(),
            id: state.id.take(),
            data: std::mem::take(&mut state.data_lines).join("\n"),
        });
    }

    if line.starts_with(':') {
        return None;
    }

    let (field, value) = match line.split_once(':') {
        Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
        None => (line, ""),
    };

    match field {
        "event" => state.event = Some(value.to_owned()),
        "id" => state.id = Some(value.to_owned()),
        "data" => state.data_lines.push(value.to_owned()),
        _ => {}
    }

    None
}

fn map_upstream_frame(run: &mut AgUiRunState, frame: SseFrame) -> Vec<Event> {
    let payload = serde_json::from_str::<Value>(&frame.data).unwrap_or(Value::Null);
    let event = frame.event.as_deref().unwrap_or_default();
    if event != "connected" && run.run_id.is_none() {
        run.run_id = frame
            .id
            .clone()
            .or_else(|| Some(format!("run-{}", chrono::Utc::now().timestamp_millis())));
    }

    match event {
        "connected" => {
            run.run_id = string_field(&payload, "request_id").or(frame.id);
            run.thread_id = string_field(&payload, "thread_id");

            vec![ag_ui_event(json!({
                "type": "RUN_STARTED",
                "runId": run_id(run),
                "threadId": run.thread_id.clone(),
                "input": {
                    "model": string_field(&payload, "model_used").or_else(|| string_field(&payload, "model")),
                },
            }))]
        }
        "chunk" => {
            let delta = string_field(&payload, "delta")
                .or_else(|| string_field(&payload, "content"))
                .unwrap_or_default();

            if delta.is_empty() {
                return Vec::new();
            }

            let mut events = Vec::new();
            if !run.text_started {
                run.text_started = true;
                events.push(ag_ui_event(json!({
                    "type": "TEXT_MESSAGE_START",
                    "messageId": message_id(run),
                    "role": "assistant",
                    "runId": run_id(run),
                })));
            }

            events.push(ag_ui_event(json!({
                "type": "TEXT_MESSAGE_CONTENT",
                "messageId": message_id(run),
                "delta": delta,
                "runId": run_id(run),
            })));
            events
        }
        "done" | "stopped" => {
            let mut events = Vec::new();
            if run.text_started {
                events.push(ag_ui_event(json!({
                    "type": "TEXT_MESSAGE_END",
                    "messageId": message_id(run),
                    "runId": run_id(run),
                })));
            }
            run.text_started = false;
            events.push(ag_ui_event(json!({
                "type": "RUN_FINISHED",
                "runId": string_field(&payload, "request_id").unwrap_or_else(|| run_id(run)),
                "threadId": run.thread_id.clone(),
                "outcome": { "status": if event == "stopped" { "stopped" } else { "completed" } },
                "modelUsed": string_field(&payload, "model_used").or_else(|| string_field(&payload, "modelUsed")),
                "outputTokens": number_field(&payload, "output_tokens").or_else(|| number_field(&payload, "outputTokens")),
            })));
            events
        }
        "error" => map_error(
            run,
            string_field(&payload, "message").unwrap_or_else(|| "Stream error".to_owned()),
        ),
        "tool_call" => {
            let tool_call_id =
                string_field(&payload, "id").unwrap_or_else(|| format!("tool-{}", run_id(run)));
            let tool_name = string_field(&payload, "name").unwrap_or_else(|| "tool".to_owned());
            vec![
                ag_ui_event(json!({
                    "type": "TOOL_CALL_START",
                    "toolCallId": tool_call_id,
                    "toolCallName": tool_name,
                    "runId": run_id(run),
                })),
                ag_ui_event(json!({
                    "type": "TOOL_CALL_ARGS",
                    "toolCallId": string_field(&payload, "id").unwrap_or_else(|| format!("tool-{}", run_id(run))),
                    "args": payload.get("args").cloned().unwrap_or(Value::Null),
                    "runId": run_id(run),
                })),
                ag_ui_event(json!({
                    "type": "TOOL_CALL_END",
                    "toolCallId": string_field(&payload, "id").unwrap_or_else(|| format!("tool-{}", run_id(run))),
                    "runId": run_id(run),
                })),
            ]
        }
        "tool_result" => vec![ag_ui_event(json!({
            "type": "TOOL_CALL_RESULT",
            "toolCallId": string_field(&payload, "id"),
            "content": payload.get("result").cloned().unwrap_or(payload),
            "runId": run_id(run),
        }))],
        "artifact" | "attachment" | "reasoning_delta" | "usage" | "citation" | "grounding"
        | "step_update" => {
            vec![ag_ui_event(json!({
                "type": "CUSTOM",
                "name": event,
                "value": payload,
                "runId": run_id(run),
            }))]
        }
        _ => vec![ag_ui_event(json!({
            "type": "CUSTOM",
            "name": if event.is_empty() { "upstream" } else { event },
            "value": payload,
            "runId": run_id(run),
        }))],
    }
}

fn map_error(run: &mut AgUiRunState, message: String) -> Vec<Event> {
    let mut events = Vec::new();
    if run.text_started {
        events.push(ag_ui_event(json!({
            "type": "TEXT_MESSAGE_END",
            "messageId": message_id(run),
            "runId": run_id(run),
        })));
    }
    run.text_started = false;
    events.push(ag_ui_event(json!({
        "type": "RUN_ERROR",
        "runId": run_id(run),
        "message": message,
    })));
    events
}

fn ag_ui_event(data: Value) -> Event {
    Event::default().data(data.to_string())
}

fn string_field(payload: &Value, field: &str) -> Option<String> {
    payload.get(field)?.as_str().map(str::to_owned)
}

fn bool_field(payload: &Value, field: &str) -> Option<bool> {
    payload.get(field).and_then(value_bool)
}

fn value_bool(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        value
            .as_str()
            .map(|raw| raw.eq_ignore_ascii_case("true") || raw == "1")
    })
}

fn number_field(payload: &Value, field: &str) -> Option<f64> {
    payload.get(field)?.as_f64()
}

fn run_id(run: &AgUiRunState) -> String {
    run.run_id
        .clone()
        .unwrap_or_else(|| format!("run-{}", chrono::Utc::now().timestamp_millis()))
}

fn message_id(run: &AgUiRunState) -> String {
    format!("msg-{}", run_id(run))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{map_upstream_frame, normalize_agent_input, AgUiRunState, SseFrame};

    #[test]
    fn normalizes_tanstack_run_agent_input_to_model_invoke_body() {
        let body = json!({
            "threadId": "thread_1",
            "runId": "run_1",
            "messages": [
                { "role": "user", "content": "Old question" },
                { "role": "assistant", "content": "Old answer" },
                {
                    "role": "user",
                    "parts": [
                        { "type": "text", "text": "Refresh " },
                        { "type": "text", "text": "this source" }
                    ]
                }
            ],
            "tools": [
                {
                    "name": "knowledge.recrawl_source",
                    "description": "Refresh evidence",
                    "parameters": {
                        "type": "object",
                        "properties": { "sourceId": { "type": "string" } },
                        "required": ["sourceId"]
                    }
                }
            ],
            "forwardedProps": {
                "model": "verevon-default",
                "profile": "chat",
                "sessionKey": "thread_1",
                "features": ["tools", "artifacts"],
                "generateImage": true,
                "browseWeb": true,
                "zdr": true
            },
            "data": { "model": "legacy-model" }
        });

        let normalized = normalize_agent_input(body).expect("valid run input");

        assert_eq!(normalized["content"], "Refresh this source");
        assert_eq!(normalized["model"], "verevon-default");
        assert_eq!(normalized["profile"], "chat");
        assert_eq!(normalized["thread_id"], "thread_1");
        assert_eq!(normalized["session_key"], "thread_1");
        assert_eq!(normalized["generate_image"], true);
        assert_eq!(normalized["zdr"], true);

        let tool_names: Vec<&str> = normalized["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(tool_names, vec!["web_search", "knowledge.recrawl_source"]);
        assert!(normalized["tools"][1]["parameters_json"]
            .as_str()
            .unwrap()
            .contains("sourceId"));
    }

    #[test]
    fn leaves_legacy_chat_invoke_body_compatible() {
        let body = json!({
            "content": "Hello",
            "thread_id": "thread_1",
            "tools": [
                { "name": "web_search", "description": "Search", "parameters_json": "{\"type\":\"object\"}" }
            ]
        });

        let normalized = normalize_agent_input(body).expect("valid legacy input");

        assert_eq!(normalized["content"], "Hello");
        assert_eq!(normalized["thread_id"], "thread_1");
        assert_eq!(normalized["tools"][0]["name"], "web_search");
    }

    #[test]
    fn maps_chat_stream_to_ag_ui_lifecycle_and_text_events() {
        let mut run = AgUiRunState::default();

        let connected = map_upstream_frame(
            &mut run,
            SseFrame {
                event: Some("connected".into()),
                id: None,
                data: r#"{"request_id":"run_1","thread_id":"thread_1"}"#.into(),
            },
        );
        assert_eq!(connected.len(), 1);

        let chunk = map_upstream_frame(
            &mut run,
            SseFrame {
                event: Some("chunk".into()),
                id: None,
                data: r#"{"delta":"Hello"}"#.into(),
            },
        );
        assert_eq!(chunk.len(), 2);

        let done = map_upstream_frame(
            &mut run,
            SseFrame {
                event: Some("done".into()),
                id: None,
                data: r#"{"request_id":"run_1"}"#.into(),
            },
        );
        assert_eq!(done.len(), 2);
    }

    #[test]
    fn maps_rich_chat_events_to_ag_ui_custom_events() {
        let mut run = AgUiRunState::default();

        let attachment = map_upstream_frame(
            &mut run,
            SseFrame {
                event: Some("attachment".into()),
                id: None,
                data: r#"{"id":"file_1","name":"generated-image.png","type":"image/png","url":"blob:1","size":12}"#.into(),
            },
        );
        assert_eq!(attachment.len(), 1);

        let grounding = map_upstream_frame(
            &mut run,
            SseFrame {
                event: Some("grounding".into()),
                id: None,
                data: r#"{"mode":"hybrid","sources":[]}"#.into(),
            },
        );
        assert_eq!(grounding.len(), 1);
    }
}
