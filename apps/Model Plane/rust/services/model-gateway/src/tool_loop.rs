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

use mp_contracts::dataplane::retrieval_v2::RetrieveRequest;
use mp_contracts::model_plane::v1::{
    ChatMessage, IndexMemoryRequest, InferRequest, SearchMemoryRequest, ToolCall, ToolDefinition,
    WebSearchRequest,
};
use serde_json::Value;

use crate::{
    auth::VerifiedDataPlaneBearer as VerifiedBearer, sse_events::ChatEvent, state::AppState,
};

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

/// Inline chat tools execute without the execution-core approval workflow.
/// MCP tools therefore remain agentic-only until the same signed approval
/// contract is available on this path.
#[must_use]
pub(crate) fn inline_tool_allowed(name: &str) -> bool {
    !name.starts_with("mcp__")
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

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

fn clean_name_token(token: &str) -> &str {
    token.trim_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '`'
                    | ':'
                    | ';'
                    | ','
                    | '.'
                    | '!'
                    | '?'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
            )
    })
}

fn name_after_phrase(content: &str, phrase: &str) -> Option<String> {
    let lower = content.to_lowercase();
    let index = lower.find(phrase)?;
    let rest = content.get(index + phrase.len()..)?.trim();
    let mut parts = Vec::new();
    for token in rest.split_whitespace() {
        let clean = clean_name_token(token);
        if clean.is_empty() {
            continue;
        }
        parts.push(clean.to_owned());
        if parts.len() >= 3 || token.ends_with(['.', ',', '!', '?', ';']) {
            break;
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn extract_declared_user_name(content: &str) -> Option<String> {
    [
        "mitt navn er ",
        "navnet mitt er ",
        "jeg heter ",
        "my name is ",
        "i am ",
        "i'm ",
    ]
    .into_iter()
    .find_map(|phrase| name_after_phrase(content, phrase))
    .map(|name| capitalize_first(name.trim()))
    .filter(|name| !name.is_empty())
}

fn asks_for_own_name_meaning(query: &str) -> bool {
    let lower = query.to_lowercase();
    let own_name_reference = lower.contains("navnet mitt")
        || lower.contains("mitt navn")
        || lower.contains("my name")
        || lower.contains("name mean");
    let meaning_intent = lower.contains("betyr")
        || lower.contains("betyder")
        || lower.contains("meaning")
        || lower.contains("mean");
    own_name_reference && meaning_intent
}

pub fn asks_about_conversation_state(query: &str) -> bool {
    let lower = query.to_lowercase();
    lower.contains("hva snakket vi")
        || lower.contains("snakket vi om")
        || lower.contains("i denne samtalen")
        || lower.contains("denne samtalen")
        || lower.contains("lagde du et bilde")
        || lower.contains("laget du et bilde")
        || lower.contains("genererte du")
        || lower.contains("har du laget")
        || lower.contains("what did we talk")
        || lower.contains("did you make")
        || lower.contains("did you create")
        || lower.contains("did you generate")
        || lower.contains("this conversation")
}

/// Tokens that signal the query wants CURRENT or external information the
/// model cannot answer from its own knowledge. Matched as case-insensitive
/// substrings, so multi-word phrases ("right now", "as of") are fine.
const TIME_SENSITIVE_TOKENS: &[&str] = &[
    // English — recency / "now" signals
    "latest",
    "today",
    "tonight",
    "current",
    "currently",
    "right now",
    "as of",
    "this week",
    "this month",
    "this year",
    "recent",
    "recently",
    "breaking",
    "news",
    "headline",
    "just announced",
    "up to date",
    "up-to-date",
    // External live data the model can't know
    "weather",
    "forecast",
    "temperature",
    "price",
    "pricing",
    "cost of",
    "stock",
    "share price",
    "exchange rate",
    "score",
    "schedule",
    "release date",
    "who won",
    "election",
    // Norwegian — recency / "now" signals
    "i dag",
    "i kveld",
    "nyeste",
    "siste nytt",
    "akkurat nå",
    "akkurat naa",
    "denne uka",
    "denne uken",
    "denne måneden",
    "denne maaneden",
    "i år",
    "i aar",
    "nyheter",
    "værmelding",
    "vaermelding",
    "været",
    "vaeret",
    "pris",
    "kurs",
    "aksje",
];

/// Detect a standalone 4-digit year >= 2024 anywhere in the query (e.g. asking
/// about events in a recent/future year the model may not have full data for).
fn mentions_recent_year(query: &str) -> bool {
    let bytes = query.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 4 <= len {
        // A 4-digit run must not be flanked by other ASCII digits (so we match
        // "in 2025" but not "12025" or "20255").
        let is_four_digits = bytes[i..i + 4].iter().all(u8::is_ascii_digit);
        let left_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
        let right_ok = i + 4 == len || !bytes[i + 4].is_ascii_digit();
        if is_four_digits && left_ok && right_ok {
            // Safe: the slice is exactly 4 ASCII digits.
            if let Ok(year) = query[i..i + 4].parse::<u32>() {
                if year >= 2024 {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Whether a pre-loop forced web search is warranted for this query.
///
/// The gateway advertises `web_search` as a built-in tool (see
/// [`builtin_tool_defs`]), so the model can call it whenever it judges a query
/// needs the public web. Forcing a search up-front is therefore reserved for
/// queries that *clearly* need CURRENT or external live information (recency
/// tokens, live data like weather/price/stock, or a recent 4-digit year).
/// Ordinary or conversational queries return `false` and let the model decide.
///
/// Conversation-state questions (e.g. "what did we talk about?") are always
/// excluded — a web search cannot answer them.
#[must_use]
pub fn should_force_web_search(query: &str) -> bool {
    if asks_about_conversation_state(query) {
        return false;
    }
    let lower = query.to_lowercase();
    if TIME_SENSITIVE_TOKENS
        .iter()
        .any(|token| lower.contains(token))
    {
        return true;
    }
    mentions_recent_year(&lower)
}

fn latest_declared_user_name(messages: &[ChatMessage], current_query: &str) -> Option<String> {
    let current_query = current_query.trim();
    messages
        .iter()
        .rev()
        .filter(|message| matches!(message.role.as_str(), "user" | "system"))
        .filter(|message| !message.content.trim().eq_ignore_ascii_case(current_query))
        .find_map(|message| extract_declared_user_name(&message.content))
}

#[must_use]
fn resolve_forced_web_search_query(messages: &[ChatMessage], query: &str) -> String {
    let query = query.trim();
    if !asks_for_own_name_meaning(query) {
        return query.to_owned();
    }

    match latest_declared_user_name(messages, query) {
        Some(name) => format!("{name} name meaning"),
        None => query.to_owned(),
    }
}

fn err_outcome(call: &ToolCall, msg: impl Into<String>) -> ToolOutcome {
    ToolOutcome {
        call_id: call.id.clone(),
        name: call.name.clone(),
        output: String::new(),
        error: Some(msg.into()),
    }
}

fn brreg_base_url() -> String {
    std::env::var("BRREG_API_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://data.brreg.no/enhetsregisteret/api".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn is_brreg_org_number_query(query: &str) -> Option<String> {
    let digits: String = query.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 9
        && query
            .chars()
            .all(|value| value.is_ascii_digit() || value.is_ascii_whitespace())
    {
        Some(digits)
    } else {
        None
    }
}

fn normalize_brreg_entity(entity: &Value) -> Value {
    serde_json::json!({
        "organisasjonsnummer": entity.get("organisasjonsnummer").cloned().unwrap_or(Value::Null),
        "navn": entity.get("navn").cloned().unwrap_or(Value::Null),
        "organisasjonsform": entity.get("organisasjonsform").cloned().unwrap_or(Value::Null),
        "forretningsadresse": entity.get("forretningsadresse").cloned().unwrap_or(Value::Null),
        "postadresse": entity.get("postadresse").cloned().unwrap_or(Value::Null),
        "registreringsdatoEnhetsregisteret": entity.get("registreringsdatoEnhetsregisteret").cloned().unwrap_or(Value::Null),
        "naeringskode1": entity.get("naeringskode1").cloned().unwrap_or(Value::Null),
        "antallAnsatte": entity.get("antallAnsatte").cloned().unwrap_or(Value::Null),
        "hjemmeside": entity.get("hjemmeside").cloned().unwrap_or(Value::Null),
        "konkurs": entity.get("konkurs").cloned().unwrap_or(Value::Bool(false)),
        "underAvvikling": entity.get("underAvvikling").cloned().unwrap_or(Value::Bool(false)),
    })
}

async fn dispatch_brreg_lookup_tool(state: &AppState, call: &ToolCall) -> ToolOutcome {
    let query = {
        let q = arg_str(&call.arguments_json, "q");
        if q.trim().is_empty() {
            arg_str(&call.arguments_json, "query")
        } else {
            q
        }
    };
    let query = query.trim();
    if query.is_empty() {
        return err_outcome(
            call,
            "brreg_lookup_organization requires a non-empty 'q' argument",
        );
    }

    let base_url = brreg_base_url();
    let request = if let Some(org_number) = is_brreg_org_number_query(query) {
        let url = format!("{base_url}/enheter/{org_number}");
        match state
            .http_client
            .get(url)
            .header("accept", "application/json")
            .build()
        {
            Ok(request) => request,
            Err(err) => return err_outcome(call, format!("brreg lookup request failed: {err}")),
        }
    } else {
        let size = arg_i64(&call.arguments_json, "size")
            .unwrap_or(8)
            .clamp(1, 20);
        let url = format!("{base_url}/enheter");
        let size = size.to_string();
        match state
            .http_client
            .get(url)
            .query(&[("navn", query), ("size", size.as_str())])
            .header("accept", "application/json")
            .build()
        {
            Ok(request) => request,
            Err(err) => return err_outcome(call, format!("brreg search request failed: {err}")),
        }
    };

    let source_url = request.url().to_string();
    let response = match state.http_client.execute(request).await {
        Ok(response) => response,
        Err(err) => return err_outcome(call, format!("brreg lookup failed: {err}")),
    };

    if response.status().as_u16() == 404 || response.status().as_u16() == 410 {
        return ToolOutcome {
            call_id: call.id.clone(),
            name: call.name.clone(),
            output: serde_json::json!({
                "query": query,
                "count": 0,
                "source": "Brønnøysundregistrene Enhetsregisteret",
                "sourceUrl": source_url,
                "results": [],
            })
            .to_string(),
            error: None,
        };
    }

    if !response.status().is_success() {
        return err_outcome(
            call,
            format!("brreg lookup returned {}", response.status().as_u16()),
        );
    }

    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(err) => return err_outcome(call, format!("brreg response decode failed: {err}")),
    };

    let results = if let Some(items) = body.pointer("/_embedded/enheter").and_then(Value::as_array)
    {
        items.iter().map(normalize_brreg_entity).collect::<Vec<_>>()
    } else {
        vec![normalize_brreg_entity(&body)]
    };

    ToolOutcome {
        call_id: call.id.clone(),
        name: call.name.clone(),
        output: serde_json::json!({
            "query": query,
            "count": results.len(),
            "source": "Brønnøysundregistrene Enhetsregisteret",
            "sourceUrl": source_url,
            "results": results,
        })
        .to_string(),
        error: None,
    }
}

/// Execute a single model-requested tool call against the gateway's tool
/// handlers. Unknown tools / bad args return an error outcome (the model is
/// told, so it can recover). New tools plug in here (MCP proxy, etc.).
fn knowledge_search_request(org_id: &str, query: &str, top_k: i32, zdr: bool) -> RetrieveRequest {
    RetrieveRequest {
        org_id: org_id.to_owned(),
        query: query.to_owned(),
        top_k,
        // Data Plane derives the viewer from the verified bearer. Caller-supplied
        // identity in the message is deliberately absent.
        user_id: None,
        zdr_mode: crate::retrieval::data_plane_zdr_mode(zdr),
        ..Default::default()
    }
}

#[allow(clippy::too_many_lines)] // cohesive tool dispatcher — one arm per tool
pub async fn dispatch_tool(
    state: &AppState,
    org_id: &str,
    _user_id: &str,
    thread_id: &str,
    data_plane_bearer: Option<&VerifiedBearer>,
    zdr: bool,
    call: &ToolCall,
) -> ToolOutcome {
    if !inline_tool_allowed(&call.name) {
        return err_outcome(
            call,
            "MCP tools require governed agentic execution and approval",
        );
    }

    match call.name.as_str() {
        "web_search" => {
            let query = arg_str(&call.arguments_json, "query");
            if query.trim().is_empty() {
                return err_outcome(call, "web_search requires a non-empty 'query' argument");
            }
            let intent = arg_str(&call.arguments_json, "intent");
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
                    intent,
                    zdr,
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
            match state.quarry.scrape(&url, org_id, None, false, zdr).await {
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
        // Interactive browsing agent (chat-parity Phase 3) — runs a multi-step
        // browse objective via the agent service (known `AgentModeRequest`
        // contract). Operator points BROWSER_AGENT_URL at the service; disabled
        // (error outcome) when unset, so it never calls an unknown endpoint.
        "browser_agent" => {
            let objective = arg_str(&call.arguments_json, "objective");
            if objective.trim().is_empty() {
                return err_outcome(call, "browser_agent requires an 'objective' argument");
            }
            let base = std::env::var("BROWSER_AGENT_URL").unwrap_or_default();
            if base.trim().is_empty() {
                return err_outcome(
                    call,
                    "browser_agent is not configured (BROWSER_AGENT_URL unset)",
                );
            }
            let target_url = arg_str(&call.arguments_json, "target_url");
            let body = serde_json::json!({
                "user_id": org_id,
                "objective": objective,
                "target_url": if target_url.is_empty() { serde_json::Value::Null } else { serde_json::json!(target_url) },
                "max_steps": 8,
                "enable_web_search": true,
            });
            let endpoint = format!("{}/v1/agent/run", base.trim_end_matches('/'));
            let mut http = state
                .http_client
                .post(&endpoint)
                .json(&body)
                .timeout(std::time::Duration::from_secs(60));
            if let Ok(key) = std::env::var("BROWSER_AGENT_API_KEY") {
                if !key.is_empty() {
                    http = http.header("X-API-Key", key);
                }
            }
            match http.send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<serde_json::Value>().await {
                        Ok(v) => {
                            let content = v
                                .get("content")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("");
                            let out = serde_json::json!({
                                "content": truncate_chars(content, MAX_FETCH_CHARS),
                                "confidence": v.get("confidence").cloned().unwrap_or(serde_json::Value::Null),
                            });
                            ToolOutcome {
                                call_id: call.id.clone(),
                                name: call.name.clone(),
                                output: out.to_string(),
                                error: None,
                            }
                        }
                        Err(e) => err_outcome(call, format!("browser_agent decode failed: {e}")),
                    }
                }
                Ok(resp) => err_outcome(
                    call,
                    format!("browser_agent returned {}", resp.status().as_u16()),
                ),
                Err(e) => err_outcome(call, format!("browser_agent failed: {e}")),
            }
        }
        // Long-term memory (chat-parity §Phase 3 memory/projects) — reuses the
        // MemoryService (canonical owner). Thread+org scoped.
        "recall_memory" => {
            let query = arg_str(&call.arguments_json, "query");
            if query.trim().is_empty() {
                return err_outcome(call, "recall_memory requires a 'query' argument");
            }
            let mut client = state.memory_client.clone();
            match client
                .search_memory(tonic::Request::new(SearchMemoryRequest {
                    thread_id: thread_id.to_owned(),
                    query,
                    topic_filter: Vec::new(),
                    limit: 5,
                    org_id: org_id.to_owned(),
                    updated_after: None,
                }))
                .await
            {
                Ok(resp) => {
                    let items: Vec<serde_json::Value> = resp
                        .into_inner()
                        .entries
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "topic": e.topic,
                                "content": truncate_chars(&e.content, 600),
                                "score": e.score,
                            })
                        })
                        .collect();
                    ToolOutcome {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        output: serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_owned()),
                        error: None,
                    }
                }
                Err(e) => err_outcome(call, format!("recall_memory failed: {}", e.message())),
            }
        }
        "save_memory" => {
            if zdr {
                return err_outcome(
                    call,
                    "save_memory is unavailable in Zero Data Retention mode",
                );
            }
            let content = arg_str(&call.arguments_json, "content");
            if content.trim().is_empty() {
                return err_outcome(call, "save_memory requires a 'content' argument");
            }
            let topic = {
                let t = arg_str(&call.arguments_json, "topic");
                if t.is_empty() {
                    "MEMORY".to_owned()
                } else {
                    t
                }
            };
            let mut client = state.memory_client.clone();
            match client
                .index_memory(tonic::Request::new(IndexMemoryRequest {
                    thread_id: thread_id.to_owned(),
                    topic,
                    content,
                    org_id: org_id.to_owned(),
                }))
                .await
            {
                Ok(resp) => ToolOutcome {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    output: serde_json::json!({
                        "memory_id": resp.into_inner().memory_id,
                        "saved": true,
                    })
                    .to_string(),
                    error: None,
                },
                Err(e) => err_outcome(call, format!("save_memory failed: {}", e.message())),
            }
        }
        // Knowledge-base RAG — searches the org's OWN ingested documents via
        // Data Plane v2 retrieval (canonical owner). org-scoped: `org_id` is the
        // verified request org, never model input, so a tool call can't read
        // another tenant's knowledge.
        "knowledge_search" => {
            let query = arg_str(&call.arguments_json, "query");
            if query.trim().is_empty() {
                return err_outcome(call, "knowledge_search requires a 'query' argument");
            }
            let Some(bearer) = data_plane_bearer else {
                return err_outcome(call, "knowledge_search requires a verified user bearer");
            };
            let top_k = i32::try_from(arg_i64(&call.arguments_json, "top_k").unwrap_or(5))
                .unwrap_or(5)
                .clamp(1, 20);
            let request = knowledge_search_request(org_id, &query, top_k, zdr);
            let request = match crate::retrieval::authorize(tonic::Request::new(request), bearer) {
                Ok(request) => request,
                Err(error) => {
                    return err_outcome(
                        call,
                        format!(
                            "knowledge_search authentication failed: {}",
                            error.message()
                        ),
                    );
                }
            };
            match state.retrieval_client.clone().retrieve(request).await {
                Ok(resp) => {
                    let items: Vec<serde_json::Value> = resp
                        .into_inner()
                        .candidates
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "text": truncate_chars(c.text.trim(), 600),
                                "score": c.final_score,
                                "document_id": c.document_id,
                            })
                        })
                        .collect();
                    ToolOutcome {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        output: serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_owned()),
                        error: None,
                    }
                }
                Err(e) => err_outcome(call, format!("knowledge_search failed: {}", e.message())),
            }
        }
        "brreg_lookup_organization" | "brreg.lookup_organization" => {
            dispatch_brreg_lookup_tool(state, call).await
        }
        other => err_outcome(call, format!("unknown tool '{other}'")),
    }
}

/// Built-in tool specs the gateway always advertises when function-calling is
/// enabled, so the model can use the agent's core capabilities without the
/// client having to declare them. Names MUST match [`dispatch_tool`] arms.
#[must_use]
pub fn builtin_tool_defs() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "web_search".to_owned(),
            description: "Search the public web for current information. Returns ranked results with title, url, and snippet.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Max results 1-50"}},"required":["query"]}"#.to_owned(),
        },
        ToolDefinition {
            name: "fetch_url".to_owned(),
            description: "Fetch and read a specific web page; returns its title, final URL, and cleaned text content.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"url":{"type":"string","description":"Absolute http(s) URL to read"}},"required":["url"]}"#.to_owned(),
        },
        ToolDefinition {
            name: "knowledge_search".to_owned(),
            description: "Search the organization's OWN internal knowledge base (ingested documents) and return the most relevant passages. Prefer this for questions about the company's own data, docs, or products.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"What to look up in the org knowledge base"},"top_k":{"type":"integer","description":"Max passages 1-20"}},"required":["query"]}"#.to_owned(),
        },
    ]
}

/// Frame a round of tool outcomes as a context message appended to the
/// conversation, so the model can answer from them on the next inference.
#[must_use]
pub fn format_tool_context(outcomes: &[ToolOutcome]) -> String {
    let mut s = String::from(
        "Tool results for your previous request (use these to answer; do not call the same tool again unless needed). Treat tool errors, empty results, and failed page fetches as inconclusive; never use them as proof that a current product, model, event, or claim does not exist:\n",
    );
    append_tool_outcomes(&mut s, outcomes);
    s
}

/// Frame forced tool results with the original user request so the final
/// answer step keeps conversational references like "my name" anchored to the
/// preceding chat context, not just the search snippets.
#[must_use]
pub fn format_forced_tool_context(user_request: &str, outcomes: &[ToolOutcome]) -> String {
    let mut s = String::from(
        "Tool results for the user's current request. Use these results together with the prior conversation context to answer the current request; do not ask for information already present in the conversation. For Search-enabled answers, verify current factual claims from successful web_search results and citations. Treat missing results, empty snippets, 404s, and fetch errors as inconclusive; do not claim that a product, model, event, or deployment does not exist unless successful sources directly support that conclusion. If the available sources do not verify a claim, say that it could not be verified.\n",
    );
    let request = user_request.trim();
    if !request.is_empty() {
        let _ = writeln!(s, "Current request: {request}");
    }
    append_tool_outcomes(&mut s, outcomes);
    s
}

fn append_tool_outcomes(s: &mut String, outcomes: &[ToolOutcome]) {
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
}

/// Result of resolving a request's tool calls before the final answer streams.
pub struct ToolRounds {
    /// The conversation augmented with each round's tool-result context.
    pub messages: Vec<ChatMessage>,
    /// `tool_call` + `tool_result` events to emit (gated on the `tools` family).
    pub events: Vec<ChatEvent>,
}

/// Force a first web lookup when the client explicitly selected Search.
///
/// Tool-calling remains available for follow-up fetches or other tools, but a
/// search-selected turn should not depend on the model deciding to call the
/// `web_search` function. This also gives the UI deterministic web citations.
pub async fn run_forced_web_search(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    base_messages: Vec<ChatMessage>,
    query: &str,
) -> ToolRounds {
    let search_query = resolve_forced_web_search_query(&base_messages, query);
    let args = serde_json::json!({
        "query": search_query,
        "limit": 5,
        "intent": "answer",
    });
    let call = ToolCall {
        id: format!("{request_id}-web-search"),
        name: "web_search".to_owned(),
        arguments_json: args.to_string(),
    };
    let outcome = dispatch_tool(state, org_id, user_id, thread_id, None, false, &call).await;
    let mut events = vec![
        ChatEvent::ToolCall {
            id: call.id,
            name: call.name,
            args,
        },
        ChatEvent::ToolResult {
            id: outcome.call_id.clone(),
            status: if outcome.error.is_some() {
                "error".to_owned()
            } else {
                "ok".to_owned()
            },
            output: outcome.output.clone(),
            error: outcome.error.clone(),
        },
    ];
    events.extend(web_search_citations(&outcome));

    let mut messages = base_messages;
    messages.push(ChatMessage {
        role: "user".to_owned(),
        content: format_forced_tool_context(query, &[outcome]),
        name: String::new(),
    });

    ToolRounds { messages, events }
}

fn web_search_citations(outcome: &ToolOutcome) -> Vec<ChatEvent> {
    if outcome.name != "web_search" || outcome.error.is_some() {
        return Vec::new();
    }
    let Ok(items) = serde_json::from_str::<Vec<Value>>(&outcome.output) else {
        return Vec::new();
    };

    items
        .into_iter()
        .take(5)
        .enumerate()
        .filter_map(|(index, item)| {
            let url = item.get("url")?.as_str()?.trim().to_owned();
            if url.is_empty() {
                return None;
            }
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(url.as_str())
                .to_owned();
            let snippet = item
                .get("snippet")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Some(ChatEvent::Citation {
                id: format!("web-{}-{}", index + 1, url),
                title,
                url,
                snippet,
            })
        })
        .collect()
}

/// Run the function-calling loop to resolution: unary infer-with-tools →
/// execute requested tools → inject results as context → repeat (capped at
/// [`MAX_TOOL_ROUNDS`]). Stops as soon as the model stops requesting tools.
/// The returned `messages` are then handed to the streaming infer (with tools
/// withheld) to produce the final answer. Inference errors stop the loop
/// gracefully (the normal stream path then handles the request).
#[allow(clippy::too_many_arguments)] // cohesive loop entry — all are request context
pub async fn run_tool_rounds(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    data_plane_bearer: Option<&VerifiedBearer>,
    zdr: bool,
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
            zdr,
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
            let outcome = dispatch_tool(
                state,
                org_id,
                user_id,
                thread_id,
                data_plane_bearer,
                zdr,
                call,
            )
            .await;
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
    fn knowledge_search_is_claim_scoped_and_zdr_aware() {
        let request = knowledge_search_request("org-from-claims", "query", 7, true);

        assert_eq!(request.org_id, "org-from-claims");
        assert_eq!(request.query, "query");
        assert_eq!(request.top_k, 7);
        assert_eq!(request.user_id, None);
        assert_eq!(request.zdr_mode.as_deref(), Some("ephemeral"));
    }

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
    fn inline_loop_rejects_mcp_tools_that_require_governed_agentic_approval() {
        assert!(!inline_tool_allowed("mcp__github__create_issue"));
        assert!(!inline_tool_allowed("mcp__srv__a__b"));
        assert!(inline_tool_allowed("web_search"));
        assert!(inline_tool_allowed("knowledge_search"));
    }

    #[test]
    fn arg_parsers_tolerate_malformed_json() {
        assert_eq!(arg_str("not json", "query"), "");
        assert_eq!(arg_i64("not json", "limit"), None);
    }

    #[test]
    fn brreg_org_number_query_accepts_plain_or_spaced_digits() {
        assert_eq!(
            is_brreg_org_number_query("983515827"),
            Some("983515827".to_owned())
        );
        assert_eq!(
            is_brreg_org_number_query("983 515 827"),
            Some("983515827".to_owned())
        );
        assert_eq!(is_brreg_org_number_query("Aquatiq 983515827"), None);
        assert_eq!(is_brreg_org_number_query("1234"), None);
    }

    #[test]
    fn normalizes_brreg_entity_fields_for_tool_output() {
        let entity = serde_json::json!({
            "organisasjonsnummer": "983515827",
            "navn": "AQUATIQ AS",
            "organisasjonsform": { "kode": "AS", "beskrivelse": "Aksjeselskap" },
            "antallAnsatte": 42
        });

        let normalized = normalize_brreg_entity(&entity);

        assert_eq!(normalized["organisasjonsnummer"], "983515827");
        assert_eq!(normalized["navn"], "AQUATIQ AS");
        assert_eq!(normalized["organisasjonsform"]["kode"], "AS");
        assert_eq!(normalized["antallAnsatte"], 42);
        assert_eq!(normalized["konkurs"], false);
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
        assert!(ctx.contains("failed page fetches as inconclusive"));
    }

    #[test]
    fn forced_tool_context_requires_inconclusive_fetches_to_stay_unverified() {
        let outcomes = vec![ToolOutcome {
            call_id: "c1".into(),
            name: "fetch_url".into(),
            output: String::new(),
            error: Some("fetch_url failed: 404".into()),
        }];

        let ctx = format_forced_tool_context("does this current model exist?", &outcomes);
        assert!(ctx.contains("404s, and fetch errors as inconclusive"));
        assert!(ctx.contains("could not be verified"));
        assert!(ctx.contains("fetch_url → ERROR: fetch_url failed: 404"));
    }

    #[test]
    fn forced_web_search_query_uses_declared_name_for_name_meaning_followup() {
        let messages = vec![
            ChatMessage {
                role: "user".to_owned(),
                content: "mitt navn er ima".to_owned(),
                name: String::new(),
            },
            ChatMessage {
                role: "assistant".to_owned(),
                content: "Hei Ima!".to_owned(),
                name: String::new(),
            },
            ChatMessage {
                role: "user".to_owned(),
                content: "kan du finne ut hva navnet mitt betyr?".to_owned(),
                name: String::new(),
            },
        ];

        assert_eq!(
            resolve_forced_web_search_query(&messages, "kan du finne ut hva navnet mitt betyr?"),
            "Ima name meaning"
        );
    }

    #[test]
    fn forced_web_search_query_uses_name_from_context_assembly() {
        let messages = vec![
            ChatMessage {
                role: "system".to_owned(),
                content: "Velion context assembly.\n\n[thread]\nuser: mitt navn er ima".to_owned(),
                name: String::new(),
            },
            ChatMessage {
                role: "user".to_owned(),
                content: "kan du finne ut hva navnet mitt betyr?".to_owned(),
                name: String::new(),
            },
        ];

        assert_eq!(
            resolve_forced_web_search_query(&messages, "kan du finne ut hva navnet mitt betyr?"),
            "Ima name meaning"
        );
    }

    #[test]
    fn forced_web_search_query_leaves_unrelated_queries_unchanged() {
        let messages = vec![ChatMessage {
            role: "user".to_owned(),
            content: "mitt navn er ima".to_owned(),
            name: String::new(),
        }];

        assert_eq!(
            resolve_forced_web_search_query(&messages, "hva er model plane?"),
            "hva er model plane?"
        );
    }

    #[test]
    fn forced_web_search_is_skipped_for_conversation_state_questions() {
        assert!(!should_force_web_search(
            "hva snakket vi om, og lagde du et bilde?"
        ));
        assert!(!should_force_web_search(
            "sjekk konteksten: lagde du et bilde i denne samtalen?"
        ));
        // Even a query carrying a time-sensitive token is suppressed when it is
        // a conversation-state question — the web cannot answer those.
        assert!(!should_force_web_search(
            "hva snakket vi om i dag i denne samtalen?"
        ));
    }

    #[test]
    fn forces_web_search_for_time_sensitive_queries() {
        // English recency / live-data signals.
        assert!(should_force_web_search("what is the latest news on AI?"));
        assert!(should_force_web_search("what's the weather today in Oslo"));
        assert!(should_force_web_search("current price of bitcoin"));
        assert!(should_force_web_search("AAPL stock right now"));
        assert!(should_force_web_search("who won the election this week"));
        // Norwegian recency / live-data signals.
        assert!(should_force_web_search("hva er nyeste nytt om Velion"));
        assert!(should_force_web_search("hva er været i dag"));
        assert!(should_force_web_search("aksjekurs for Equinor akkurat nå"));
        // A recent 4-digit year.
        assert!(should_force_web_search("biggest tech releases in 2025"));
        assert!(should_force_web_search("what happened in 2024"));
    }

    #[test]
    fn does_not_force_web_search_for_ordinary_or_conversational_queries() {
        // Ordinary knowledge / how-to queries — let the model decide.
        assert!(!should_force_web_search("hva er model plane?"));
        assert!(!should_force_web_search("explain how rust ownership works"));
        assert!(!should_force_web_search(
            "kan du finne ut hva navnet mitt betyr?"
        ));
        assert!(!should_force_web_search("write me a poem about the sea"));
        assert!(!should_force_web_search("summarize this document for me"));
        // Old years must NOT trip the recent-year heuristic.
        assert!(!should_force_web_search("what happened in 1999"));
        assert!(!should_force_web_search("tell me about the year 2010"));
        // Digit runs that are not standalone years must not match.
        assert!(!should_force_web_search("call extension 20240 please"));
    }

    #[test]
    fn web_search_outputs_become_citation_events() {
        let outcome = ToolOutcome {
            call_id: "c1".into(),
            name: "web_search".into(),
            output: r#"[{"url":"https://example.com/model-plane","title":"Model Plane","snippet":"Overview"}]"#.into(),
            error: None,
        };

        let citations = web_search_citations(&outcome);
        assert_eq!(citations.len(), 1);
        match &citations[0] {
            ChatEvent::Citation {
                title,
                url,
                snippet,
                ..
            } => {
                assert_eq!(title, "Model Plane");
                assert_eq!(url, "https://example.com/model-plane");
                assert_eq!(snippet, "Overview");
            }
            other => panic!("expected citation, got {other:?}"),
        }
    }
}
