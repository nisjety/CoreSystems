//! ChatGPT subscription-backed inference through Integration Core.
//!
//! This adapter never accepts, stores, or forwards a ChatGPT OAuth token.
//! Integration Core owns the official Codex app-server's managed auth state;
//! Model Plane supplies only its service credential plus an opaque, scoped
//! connection id from the authenticated inference request.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::warn;

use chrono::{DateTime, Utc};

use super::{
    ChatMessage, InferChunk, InferRequest, InferResponse, ModelFamily, ModelInfo, PrivacyTier,
    ProviderCapabilities, ProviderError, ProviderRouter, ProviderTiming, Residency,
    SubscriptionRefusal,
};

const PROVIDER_ID: &str = "openai-codex-subscription";

/// The broker's reason codes this adapter types, spelled as Integration
/// Core's `codexsubscription.Failure*` constants. Any other code stays the
/// generic `Unavailable` on the wire: on the unary route a plain `Unavailable`
/// (retried by the chain, as before), and on a started stream a
/// `SubscriptionFailed` (one call, as before).
const USAGE_LIMIT: &str = "subscription_usage_limit";
const REAUTH_REQUIRED: &str = "subscription_reauth_required";
/// The broker's pre-flight code for a connection that is not active. It
/// means the same to the user as an expired sign-in: reconnect.
const CONNECTION_INACTIVE: &str = "subscription_connection_inactive";
const UPSTREAM_UNAVAILABLE: &str = "subscription_upstream_unavailable";
const CONTEXT_TOO_LONG: &str = "subscription_context_too_long";
const BAD_REQUEST: &str = "subscription_bad_request";
const RATE_LIMITED: &str = "rate_limited";

/// The retry-after assumed for a throttled broker that named none, as before.
const DEFAULT_RATE_LIMIT_RETRY_MS: u64 = 1_000;

/// The furthest ahead a reported reset time may lie and still be believed:
/// the longest Codex usage window (seven days) plus an hour of clock skew.
/// Anything later, or already past, is dropped as unknown rather than shown.
const MAX_RESET_AHEAD: chrono::TimeDelta = chrono::TimeDelta::hours(7 * 24 + 1);

/// The broker phases forwarded as upstream timings: Integration Core's
/// `codexsubscription.Phase*` names. Any other key the broker sends is
/// dropped, so the upstream map can only ever hold these durations.
const BROKER_PHASES: [&str; 6] = [
    "spawn",
    "initialize",
    "thread_start",
    "turn_start",
    "first_delta",
    "turn_total",
];

/// Upstream phase measured here rather than reported: from sending a stream
/// request until the broker's `ready` event, i.e. its validation and audit
/// before it starts a Codex process.
const BROKER_READY: &str = "broker_ready";

#[derive(Clone)]
pub struct CodexSubscriptionProvider {
    client: reqwest::Client,
    integration_core_url: Arc<str>,
    internal_api_key: Arc<str>,
    models: Vec<String>,
}

impl CodexSubscriptionProvider {
    pub fn new(
        integration_core_url: impl Into<String>,
        internal_api_key: impl Into<String>,
        models: Vec<String>,
    ) -> Result<Self, ProviderError> {
        let integration_core_url = integration_core_url.into();
        let internal_api_key = internal_api_key.into();
        if integration_core_url.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "CODEX_SUBSCRIPTION_INTEGRATION_CORE_URL is empty".to_owned(),
            ));
        }
        if internal_api_key.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "CODEX_SUBSCRIPTION_INTERNAL_API_KEY is empty".to_owned(),
            ));
        }
        if models.is_empty() {
            return Err(ProviderError::Unavailable(
                "CODEX_SUBSCRIPTION_MODELS must list at least one enabled model".to_owned(),
            ));
        }
        Ok(Self {
            client: super::provider_http_client(),
            integration_core_url: Arc::from(integration_core_url.trim_end_matches('/')),
            internal_api_key: Arc::from(internal_api_key),
            models,
        })
    }

    fn url(&self) -> String {
        format!(
            "{}/internal/model-subscriptions/openai-codex/infer",
            self.integration_core_url
        )
    }

    fn request_body<'a>(&self, req: &'a InferRequest) -> Result<BrokerRequest<'a>, ProviderError> {
        if req.subscription_connection_id.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "subscription_connection_id is required for openai-codex-subscription".to_owned(),
            ));
        }
        if req.org_id.trim().is_empty() || req.user_id.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "verified organization and user scope are required for a subscription connection"
                    .to_owned(),
            ));
        }
        if !self.models.iter().any(|model| model == &req.model) {
            return Err(ProviderError::UnsupportedModel(format!(
                "model `{}` is not enabled for openai-codex-subscription",
                req.model
            )));
        }
        let (messages, output_schema) = super::codex_subscription_tools::prepare(req)?;
        Ok(BrokerRequest {
            organization_id: &req.org_id,
            user_id: &req.user_id,
            connection_id: &req.subscription_connection_id,
            request_id: &req.request_id,
            model: &req.model,
            messages,
            output_schema,
            max_tokens: req.max_tokens,
            reasoning_effort: codex_reasoning_effort(req.thinking_budget_tokens),
            service_tier: codex_service_tier(&req.model, req.thinking_budget_tokens, req.prefer_priority_service_tier),
        })
    }

    fn stream_url(&self) -> String {
        format!("{}/stream", self.url())
    }
}

/// Codex models otherwise choose their own default reasoning effort (medium
/// for most subscription models). Normal and quick chat should favor latency;
/// only the gateway's explicit deep profile asks Codex for heavier reasoning.
fn codex_reasoning_effort(thinking_budget_tokens: i32) -> &'static str {
    if thinking_budget_tokens > 1_024 {
        "high"
    } else {
        "low"
    }
}

/// Quick opts into priority through its profile. A source reviewer may also
/// request priority explicitly while retaining high reasoning effort. Both
/// paths stay on the same subscription connection and allowed model.
fn codex_service_tier(model: &str, thinking_budget_tokens: i32, prefer_priority: bool) -> Option<&'static str> {
    if thinking_budget_tokens != 1_024 && !prefer_priority {
        return None;
    }
    match model {
        "gpt-6-astra" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-5.5" => {
            Some("priority")
        }
        _ => None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrokerRequest<'a> {
    organization_id: &'a str,
    user_id: &'a str,
    connection_id: &'a str,
    request_id: &'a str,
    model: &'a str,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_schema: Option<serde_json::Value>,
    max_tokens: i32,
    reasoning_effort: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_tier: Option<&'static str>,
}

#[derive(Deserialize)]
struct BrokerEnvelope {
    success: bool,
    data: Option<BrokerData>,
    error: Option<BrokerError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerData {
    response: BrokerResponse,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerResponse {
    request_id: String,
    content: String,
    model_used: String,
    /// `None` from a broker that predates usage reporting, and whenever
    /// Codex sent no usage before the turn completed.
    #[serde(default, deserialize_with = "lenient")]
    usage: Option<BrokerUsage>,
    #[serde(default, deserialize_with = "lenient")]
    phases_ms: BTreeMap<String, i64>,
}

/// The broker's `error` object. Its `message` is deliberately not read: the
/// reason travels as `code` alone, and only the content-free timing fields
/// beside it are kept.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerError {
    code: Option<String>,
    /// RFC 3339; usage limit only, and only when the broker knew the reset.
    #[serde(default, deserialize_with = "lenient")]
    resets_at: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    retry_after_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    model_used: String,
    #[serde(default)]
    code: String,
    /// Sent on an `error` event only, as on the unary rejection.
    #[serde(default, deserialize_with = "lenient")]
    resets_at: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    retry_after_ms: Option<u64>,
    /// Sent on the `done` event only, like the unary reply's fields.
    #[serde(default, deserialize_with = "lenient")]
    usage: Option<BrokerUsage>,
    #[serde(default, deserialize_with = "lenient")]
    phases_ms: BTreeMap<String, i64>,
}

/// A broker code reduced to what may be logged or carried in a message: its
/// lowercase ASCII letters, digits and underscores, at most 64 of them. The
/// broker only ever sends such tokens; this keeps anything else from riding
/// along if it ever did.
fn code_token(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .take(64)
        .collect()
}

/// A reported reset time, if it parses as RFC 3339 and lies in
/// `(now, now + MAX_RESET_AHEAD]`. Anything else is unknown, not shown.
fn plausible_reset(raw: Option<&str>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let at = DateTime::parse_from_rfc3339(raw?.trim()).ok()?.with_timezone(&Utc);
    (at > now && at <= now + MAX_RESET_AHEAD).then_some(at)
}

/// A `Retry-After` header in whole seconds, as milliseconds.
fn retry_after_header_ms(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1_000))
}

/// The typed error for a broker reason code, or `None` for a code this
/// adapter does not type (which the caller reports untyped).
///
/// Shared by the unary rejection, the stream's rejection before its 200, and
/// an `error` event that arrives before the stream's first token, so all
/// three read one vocabulary. Only the code and its content-free timing are
/// read; the broker's message never is.
fn broker_failure(
    code: &str,
    body_retry_ms: Option<u64>,
    header_retry_ms: Option<u64>,
    resets_at: Option<&str>,
    now: DateTime<Utc>,
) -> Option<ProviderError> {
    let refused = |reason, retry_after_ms, resets_at| ProviderError::SubscriptionRefused {
        reason,
        retry_after_ms,
        resets_at,
    };
    Some(match code.trim() {
        USAGE_LIMIT => refused(
            SubscriptionRefusal::UsageLimit,
            body_retry_ms.or(header_retry_ms),
            plausible_reset(resets_at, now),
        ),
        REAUTH_REQUIRED | CONNECTION_INACTIVE => {
            refused(SubscriptionRefusal::ReauthRequired, None, None)
        }
        UPSTREAM_UNAVAILABLE => refused(
            SubscriptionRefusal::UpstreamUnavailable,
            body_retry_ms.or(header_retry_ms),
            None,
        ),
        // The detail is the code alone: never a body, never provider prose.
        CONTEXT_TOO_LONG => ProviderError::TooLong {
            detail: CONTEXT_TOO_LONG.to_owned(),
        },
        BAD_REQUEST => refused(SubscriptionRefusal::BadRequest, None, None),
        RATE_LIMITED => ProviderError::RateLimited {
            retry_after_ms: header_retry_ms
                .or(body_retry_ms)
                .unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
        },
        _ => return None,
    })
}

/// The error for a broker rejection whose envelope was read. `what` names the
/// call ("request" or "stream") in the untyped message.
fn broker_rejection(
    status: reqwest::StatusCode,
    header_retry_ms: Option<u64>,
    error: Option<BrokerError>,
    what: &str,
) -> ProviderError {
    let (code, body_retry_ms, resets_at) = error.map_or((String::new(), None, None), |error| {
        (
            error.code.as_deref().map(code_token).unwrap_or_default(),
            error.retry_after_ms,
            error.resets_at,
        )
    });
    if let Some(typed) = broker_failure(
        &code,
        body_retry_ms,
        header_retry_ms,
        resets_at.as_deref(),
        Utc::now(),
    ) {
        return typed;
    }
    // A throttled broker whose code this adapter does not know is still
    // throttled: the pre-existing reading of every 429.
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return ProviderError::RateLimited {
            retry_after_ms: header_retry_ms.unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
        };
    }
    ProviderError::Unavailable(format!(
        "subscription broker rejected the {what}{}",
        if code.is_empty() {
            String::new()
        } else {
            format!(": {code}")
        }
    ))
}

/// The broker's SSE `data:` events, one at a time.
///
/// Raw bytes are kept until a complete line arrives, because an HTTP chunk
/// boundary may split a multi-byte UTF-8 character.
struct BrokerEvents<S> {
    bytes: S,
    buffer: Vec<u8>,
}

/// What reading the next broker event found.
enum NextEvent {
    Event(Box<BrokerStreamEvent>),
    /// The stream ended, or a read failed (already logged).
    Ended,
}

impl<S, B, E> BrokerEvents<S>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    fn new(bytes: S) -> Self {
        Self {
            bytes,
            buffer: Vec::new(),
        }
    }

    async fn next(&mut self) -> NextEvent {
        loop {
            while let Some(newline_pos) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line_bytes: Vec<u8> = self.buffer.drain(..=newline_pos).collect();
                let Ok(line) = std::str::from_utf8(&line_bytes[..newline_pos]) else {
                    warn!("subscription broker emitted an invalid UTF-8 SSE line");
                    continue;
                };
                let Some(data) = line.trim().strip_prefix("data: ") else {
                    continue;
                };
                if let Ok(event) = serde_json::from_str::<BrokerStreamEvent>(data) {
                    return NextEvent::Event(Box::new(event));
                }
            }
            match self.bytes.next().await {
                Some(Ok(bytes)) => self.buffer.extend_from_slice(bytes.as_ref()),
                Some(Err(error)) => {
                    warn!(error = %error, "subscription broker stream read failed");
                    return NextEvent::Ended;
                }
                None => return NextEvent::Ended,
            }
        }
    }
}

/// A content delta, in the chain's chunk shape.
fn delta_chunk(request_id: &str, delta: String) -> InferChunk {
    InferChunk {
        compaction_summary: String::new(),
        request_id: request_id.to_owned(),
        delta,
        done: false,
        model_used: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        stop_reason: String::new(),
        reasoning_delta: String::new(),
        provider_used: String::new(),
        residency: String::new(),
        token_confidence: None,
        // The broker reports usage on its `done` event only; a delta carries
        // none.
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        provider_timing: None,
    }
}

/// The final chunk for the broker's `done` event.
fn done_chunk(
    request_id: &str,
    model: &str,
    event: &BrokerStreamEvent,
    ready: Option<Duration>,
) -> InferChunk {
    let tokens = event.usage.map(BrokerUsage::counts).unwrap_or_default();
    InferChunk {
        compaction_summary: String::new(),
        request_id: if event.request_id.is_empty() {
            request_id.to_owned()
        } else {
            event.request_id.clone()
        },
        delta: String::new(),
        done: true,
        model_used: model.to_owned(),
        input_tokens: tokens.input,
        output_tokens: tokens.output,
        stop_reason: if event.model_used == model {
            "end_turn"
        } else {
            "subscription_model_mismatch"
        }
        .to_owned(),
        reasoning_delta: String::new(),
        provider_used: String::new(),
        residency: String::new(),
        token_confidence: None,
        cache_read_input_tokens: tokens.cache_read,
        cache_creation_input_tokens: tokens.cache_write,
        provider_timing: Some(broker_timing(event.usage, &event.phases_ms, ready)),
    }
}

/// A final chunk that ends a started stream without the broker's `done`.
fn failed_chunk(
    request_id: &str,
    model: &str,
    stop_reason: &str,
    ready: Option<Duration>,
) -> InferChunk {
    InferChunk {
        compaction_summary: String::new(),
        request_id: request_id.to_owned(),
        delta: String::new(),
        done: true,
        model_used: model.to_owned(),
        input_tokens: 0,
        output_tokens: 0,
        stop_reason: stop_reason.to_owned(),
        reasoning_delta: String::new(),
        provider_used: String::new(),
        residency: String::new(),
        token_confidence: None,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        provider_timing: Some(broker_timing(None, &BTreeMap::new(), ready)),
    }
}

/// Token accounting of one Codex turn: Integration Core's `TokenUsage`.
/// Its `reasoningOutputTokens` is not read, because `outputTokens` already
/// includes it.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[allow(clippy::struct_field_names)] // The broker's own JSON names.
struct BrokerUsage {
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_input_tokens: i64,
    output_tokens: i64,
}

/// Token counts in the shape `InferResponse` and `InferChunk` carry them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TokenCounts {
    input: i32,
    output: i32,
    cache_read: i32,
    cache_write: i32,
}

impl BrokerUsage {
    /// Both cache legs stay inside `input` (Codex relays the `OpenAI`
    /// accounting, which already counts them there, and the gateway prices
    /// them that way). Reads are capped at the input and writes at what the
    /// reads leave, so the uncached share can never go negative: the same
    /// rule as `openai_cache_tokens`.
    fn counts(self) -> TokenCounts {
        let input = self.input_tokens.max(0);
        let cache_read = self.cached_input_tokens.max(0).min(input);
        let cache_write = self.cache_write_input_tokens.max(0).min(input - cache_read);
        TokenCounts {
            input: to_i32_or_max(input),
            output: to_i32_or_max(self.output_tokens.max(0)),
            cache_read: to_i32_or_max(cache_read),
            cache_write: to_i32_or_max(cache_write),
        }
    }
}

fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Telemetry must never fail an answer: a malformed optional field reads as
/// absent rather than rejecting the whole broker reply.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// What this adapter knows about the broker's side of one call, for the
/// chain to carry forward: whether it relayed Codex's usage at all, its
/// known phase durations and, on a stream, how long it took to be ready.
fn broker_timing(
    usage: Option<BrokerUsage>,
    phases_ms: &BTreeMap<String, i64>,
    ready: Option<Duration>,
) -> ProviderTiming {
    let mut upstream: BTreeMap<String, Duration> = phases_ms
        .iter()
        .filter(|(phase, _)| BROKER_PHASES.contains(&phase.as_str()))
        .filter_map(|(phase, ms)| {
            let ms = u64::try_from(*ms).ok()?;
            Some((phase.clone(), Duration::from_millis(ms)))
        })
        .collect();
    if let Some(ready) = ready {
        upstream.insert(BROKER_READY.to_owned(), ready);
    }
    ProviderTiming {
        usage_reported: usage.is_some(),
        upstream,
        ..ProviderTiming::default()
    }
}

#[async_trait::async_trait]
impl ProviderRouter for CodexSubscriptionProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: PROVIDER_ID.to_owned(),
            aliases: vec!["chatgpt-codex".to_owned(), "codex-subscription".to_owned()],
            model_family: ModelFamily::OpenAiCompatible,
            // A ChatGPT subscription does not make an EU, sovereign, or ZDR
            // commitment available to this product integration.
            residency: Residency::Global,
            exclusive_catalog: true,
            supports_tools: true,
            supports_vision: false,
            supports_thinking: true,
            supports_streaming: true,
            supports_embeddings: false,
            supports_zdr: false,
            modalities: vec!["chat".to_owned()],
            max_context_tokens: 0,
            max_output_tokens: 0,
        }
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        if req.zdr {
            return Err(ProviderError::ZdrUnavailable(
                "ChatGPT subscription execution has no verified ZDR commitment".to_owned(),
            ));
        }
        let body = self.request_body(req)?;
        let response = self
            .client
            .post(self.url())
            .header("X-Internal-API-Key", self.internal_api_key.as_ref())
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                ProviderError::Http(format!("subscription broker request: {error}"))
            })?;
        let status = response.status();
        let header_retry_ms = retry_after_header_ms(response.headers());
        let envelope: BrokerEnvelope = match response.json().await {
            Ok(envelope) => envelope,
            // A throttled broker whose body cannot be read is still throttled.
            Err(_) if status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                return Err(ProviderError::RateLimited {
                    retry_after_ms: header_retry_ms.unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
                });
            }
            Err(error) => {
                return Err(ProviderError::InvalidResponse(format!(
                    "subscription broker response: {error}"
                )));
            }
        };
        if !status.is_success() || !envelope.success {
            return Err(broker_rejection(
                status,
                header_retry_ms,
                envelope.error,
                "request",
            ));
        }
        let result = envelope
            .data
            .ok_or_else(|| {
                ProviderError::InvalidResponse(
                    "subscription broker success response omitted data".to_owned(),
                )
            })?
            .response;
        if result.model_used != req.model {
            return Err(ProviderError::InvalidResponse(
                "subscription broker changed the selected model".into(),
            ));
        }
        let (content, tool_calls) = super::codex_subscription_tools::parse(req, &result.content)?;
        let stop_reason = if tool_calls.is_empty() { "end_turn" } else { "tool_use" }.to_owned();
        // An older broker, or a turn Codex reported no usage for, leaves the
        // counts at 0 and `usage_reported` false: unknown, not free.
        let tokens = result.usage.map(BrokerUsage::counts).unwrap_or_default();
        Ok(InferResponse {
            tool_calls,
            compaction_summary: String::new(),
            request_id: if result.request_id.is_empty() {
                req.request_id.clone()
            } else {
                result.request_id
            },
            content,
            model_used: if result.model_used.is_empty() {
                req.model.clone()
            } else {
                result.model_used
            },
            stop_reason,
            input_tokens: tokens.input,
            output_tokens: tokens.output,
            cache_read_input_tokens: tokens.cache_read,
            cache_creation_input_tokens: tokens.cache_write,
            provider_timing: Some(broker_timing(result.usage, &result.phases_ms, None)),
            ..InferResponse::default()
        })
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        if req.zdr {
            return Err(ProviderError::ZdrUnavailable(
                "ChatGPT subscription execution has no verified ZDR commitment".to_owned(),
            ));
        }
        if !req.tools.is_empty() {
            return Err(ProviderError::UnsupportedModel(
                "subscription tool proposals require non-streaming inference".into(),
            ));
        }
        let body = self.request_body(req)?;
        let started = Instant::now();
        let response = self
            .client
            .post(self.stream_url())
            .header("X-Internal-API-Key", self.internal_api_key.as_ref())
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                ProviderError::Http(format!("subscription broker stream request: {error}"))
            })?;
        let status = response.status();
        if !status.is_success() {
            let header_retry_ms = retry_after_header_ms(response.headers());
            let envelope: BrokerEnvelope = match response.json().await {
                Ok(envelope) => envelope,
                Err(_) if status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                    return Err(ProviderError::RateLimited {
                        retry_after_ms: header_retry_ms.unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
                    });
                }
                Err(error) => {
                    return Err(ProviderError::InvalidResponse(format!(
                        "subscription broker stream rejection: {error}"
                    )));
                }
            };
            return Err(broker_rejection(
                status,
                header_retry_ms,
                envelope.error,
                "stream",
            ));
        }

        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let mut events = BrokerEvents::new(Box::pin(response.bytes_stream()));
        let mut ready: Option<Duration> = None;
        // Wait for the first real event before answering. Every refusal the
        // broker can make arrives before the first token, and read here it
        // becomes a typed `Err` at stream open, which the chain and gRPC
        // already carry; after `Ok(rx)` it could only be a `stream_error`
        // chunk with the reason lost.
        //
        // Only a stream that ends before the broker's `ready` is retryable:
        // the broker sends `ready` before it starts the Codex turn, so until
        // then nothing ran. From `ready` on, a failure is final for this call
        // (`SubscriptionFailed`, never retried by the chain): the turn may
        // have used the plan's quota, and the untyped failures mostly repeat.
        // Before this wait, the same failures were one `stream_error` chunk
        // and one call; they stay one call.
        let first = loop {
            match events.next().await {
                NextEvent::Event(event) => match event.event_type.as_str() {
                    "ready" => {
                        ready.get_or_insert_with(|| started.elapsed());
                    }
                    "delta" if !event.delta.is_empty() => {
                        break delta_chunk(&request_id, event.delta);
                    }
                    "done" => break done_chunk(&request_id, &model, &event, ready),
                    "error" => {
                        let code = code_token(&event.code);
                        warn!(code = %code, "subscription broker stream failed before its first token");
                        return Err(broker_failure(
                            &code,
                            event.retry_after_ms,
                            None,
                            event.resets_at.as_deref(),
                            Utc::now(),
                        )
                        .unwrap_or_else(|| {
                            ProviderError::SubscriptionFailed(format!(
                                "subscription broker stream failed: {code}"
                            ))
                        }));
                    }
                    _ => {}
                },
                NextEvent::Ended if ready.is_some() => {
                    return Err(ProviderError::SubscriptionFailed(
                        "subscription broker stream ended before its first token".to_owned(),
                    ));
                }
                NextEvent::Ended => {
                    return Err(ProviderError::Unavailable(
                        "subscription broker stream ended before the first event".to_owned(),
                    ));
                }
            }
        };

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let finished = first.done;
            if tx.send(first).await.is_err() || finished {
                return;
            }
            loop {
                let NextEvent::Event(event) = events.next().await else {
                    break;
                };
                match event.event_type.as_str() {
                    "ready" => {
                        ready.get_or_insert_with(|| started.elapsed());
                    }
                    "delta" if !event.delta.is_empty() => {
                        if tx.send(delta_chunk(&request_id, event.delta)).await.is_err() {
                            return;
                        }
                    }
                    "done" => {
                        let _ = tx.send(done_chunk(&request_id, &model, &event, ready)).await;
                        return;
                    }
                    "error" => {
                        // Text has already streamed, so the stream can only
                        // end; the reason is logged, never the answer.
                        warn!(code = %code_token(&event.code), "subscription broker stream failed");
                        let _ = tx
                            .send(failed_chunk(&request_id, &model, "stream_error", ready))
                            .await;
                        return;
                    }
                    _ => {}
                }
            }
            let _ = tx
                .send(failed_chunk(&request_id, &model, "stream_incomplete", ready))
                .await;
        });
        Ok(rx)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let caps = self.capabilities();
        self.models
            .iter()
            .map(|model| ModelInfo {
                id: model.clone(),
                provider: PROVIDER_ID.to_owned(),
                modality: "chat".to_owned(),
                streaming: true,
                features: caps.feature_flags(),
                cheap: false,
                privacy_tier: PrivacyTier::classify(&caps),
                residency_label: caps.residency.as_str().to_owned(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn requires_a_connection_and_explicit_model_catalog() {
        assert!(
            CodexSubscriptionProvider::new("http://integration", "service-key", Vec::new())
                .is_err()
        );
        let provider = CodexSubscriptionProvider::new(
            "http://integration",
            "service-key",
            vec!["gpt-codex".to_owned()],
        )
        .expect("provider");
        let request = InferRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            model: "gpt-codex".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        };
        let error = provider
            .request_body(&request)
            .expect_err("missing connection must fail");
        assert!(matches!(error, ProviderError::Unavailable(_)));

        let request = InferRequest {
            subscription_connection_id: "conn-1".to_owned(),
            model: "not-enabled".to_owned(),
            ..request
        };
        let error = provider
            .request_body(&request)
            .expect_err("the subscription catalog must be an allow-list");
        assert!(matches!(error, ProviderError::UnsupportedModel(_)));
    }

    #[test]
    fn maps_gateway_effort_to_codex_reasoning_effort() {
        assert_eq!(codex_reasoning_effort(0), "low");
        assert_eq!(codex_reasoning_effort(1_024), "low");
        assert_eq!(codex_reasoning_effort(4_096), "high");
    }

    #[test]
    fn enables_priority_only_when_requested_on_compatible_models() {
        assert_eq!(codex_service_tier("gpt-6-astra", 1_024, false), Some("priority"));
        assert_eq!(codex_service_tier("gpt-5.6-luna", 1_024, false), Some("priority"));
        assert_eq!(codex_service_tier("gpt-5.6-terra", 4_096, true), Some("priority"));
        assert_eq!(codex_service_tier("gpt-6-astra", 0, false), None);
        assert_eq!(codex_service_tier("gpt-6-astra", 4_096, false), None);
        assert_eq!(codex_service_tier("gpt-5.3-codex-spark", 1_024, true), None);
    }

    #[test]
    fn broker_request_includes_reasoning_effort() {
        let provider = CodexSubscriptionProvider::new(
            "http://integration",
            "service-key",
            vec!["gpt-6-astra".to_owned()],
        )
        .expect("provider");
        let request = InferRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            subscription_connection_id: "conn-1".to_owned(),
            model: "gpt-6-astra".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            thinking_budget_tokens: 4_096,
            ..InferRequest::default()
        };

        let body = serde_json::to_value(provider.request_body(&request).expect("request body"))
            .expect("serialize request body");
        assert_eq!(body["reasoningEffort"], "high");
        assert!(body.get("serviceTier").is_none());

        let priority_review = InferRequest {
            prefer_priority_service_tier: true,
            ..request.clone()
        };
        let priority_body = serde_json::to_value(
            provider.request_body(&priority_review).expect("priority review body")
        ).expect("serialize priority review body");
        assert_eq!(priority_body["reasoningEffort"], "high");
        assert_eq!(priority_body["serviceTier"], "priority");

        let quick_request = InferRequest {
            thinking_budget_tokens: 1_024,
            ..request
        };
        let quick_body = serde_json::to_value(
            provider
                .request_body(&quick_request)
                .expect("quick request body"),
        )
        .expect("serialize quick request body");
        assert_eq!(quick_body["reasoningEffort"], "low");
        assert_eq!(quick_body["serviceTier"], "priority");
    }

    #[tokio::test]
    async fn tool_proposals_preserve_subscription_scope_through_the_broker() {
        use serde_json::json;
        let server = MockServer::start().await;
        let proposal = json!({"content":"", "toolCalls":[{"name":"create_artifact", "arguments":"{\"content\":\"Hei\"}"}]}).to_string();
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"success":true,"data":{"response":{
                    "requestId":"tool-round","content":proposal,"modelUsed":"gpt-5.6-terra"
                }}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();
        let request = InferRequest {
            request_id: "tool-round".into(),
            model: "gpt-5.6-terra".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            subscription_connection_id: "connection".into(),
            tool_choice: "required".into(),
            tools: vec![super::super::ToolDefinition {
                name: "create_artifact".into(),
                parameters_json: r#"{"type":"object"}"#.into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let response = provider.infer(&request).await.unwrap();
        assert_eq!(response.model_used, "gpt-5.6-terra");
        assert_eq!(response.tool_calls[0].name, "create_artifact");
        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["organizationId"], "org");
        assert_eq!(body["userId"], "user");
        assert_eq!(body["connectionId"], "connection");
        assert_eq!(body["outputSchema"]["type"], "object");
        assert!(
            body.get("tools").is_none(),
            "the broker does not execute gateway tools"
        );
    }

    #[tokio::test]
    async fn changed_model_is_rejected() {
        use serde_json::json;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"success":true,"data":{"response":{
                    "requestId":"test","content":"hello","modelUsed":"another-model"
                }}}),
            ))
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();
        let request = InferRequest {
            model: "gpt-5.6-terra".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            provider_hint: PROVIDER_ID.into(),
            subscription_connection_id: "connection".into(),
            ..Default::default()
        };
        assert!(matches!(
            provider.infer(&request).await,
            Err(ProviderError::InvalidResponse(_))
        ));
    }

    #[tokio::test]
    async fn streams_broker_deltas_before_the_done_event() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/internal/model-subscriptions/openai-codex/infer/stream",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                concat!(
                    "event: subscription.ready\n",
                    "data: {\"type\":\"ready\",\"requestId\":\"req-1\"}\n\n",
                    "event: subscription.delta\n",
                    "data: {\"type\":\"delta\",\"delta\":\"hello \"}\n\n",
                    "event: subscription.delta\n",
                    "data: {\"type\":\"delta\",\"delta\":\"world\"}\n\n",
                    "event: subscription.done\n",
                    "data: {\"type\":\"done\",\"requestId\":\"req-1\",\"modelUsed\":\"gpt-6-astra\"}\n\n"
                ),
                "text/event-stream",
            ))
            .mount(&server)
            .await;

        let provider = CodexSubscriptionProvider::new(
            server.uri(),
            "service-key",
            vec!["gpt-6-astra".to_owned()],
        )
        .expect("provider");
        let request = InferRequest {
            request_id: "req-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            subscription_connection_id: "conn-1".to_owned(),
            model: "gpt-6-astra".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        };

        let mut stream = provider.infer_stream(&request).await.expect("stream");
        let first = stream.recv().await.expect("first delta");
        let second = stream.recv().await.expect("second delta");
        let done = stream.recv().await.expect("done");
        assert_eq!(first.delta, "hello ");
        assert_eq!(second.delta, "world");
        assert!(first.provider_timing.is_none() && second.provider_timing.is_none());
        assert!(done.done);
        assert_eq!(done.stop_reason, "end_turn");
        assert_eq!(done.model_used, "gpt-6-astra");
        // A broker that predates usage reporting: zeros, reported as unknown.
        assert_eq!((done.input_tokens, done.output_tokens), (0, 0));
        assert_eq!(
            (
                done.cache_read_input_tokens,
                done.cache_creation_input_tokens
            ),
            (0, 0)
        );
        let timing = done
            .provider_timing
            .expect("the final chunk carries timing");
        assert!(!timing.usage_reported);
        assert_eq!(
            timing.upstream.keys().collect::<Vec<_>>(),
            [BROKER_READY],
            "only the adapter's own measurement of the ready event"
        );
    }

    fn subscription_request(request_id: &str) -> InferRequest {
        InferRequest {
            request_id: request_id.to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            subscription_connection_id: "conn-1".to_owned(),
            model: "gpt-5.6-terra".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        }
    }

    const USAGE: &str = r#"{"inputTokens":1800,"cachedInputTokens":1024,"cacheWriteInputTokens":256,"outputTokens":120,"reasoningOutputTokens":40}"#;
    // Every broker phase, plus a key outside the contract that must be dropped.
    const PHASES: &str = r#"{"spawn":41,"initialize":120,"thread_start":30,"turn_start":12,"first_delta":8000,"turn_total":9000,"unexpected":5}"#;

    fn assert_broker_phases(timing: &ProviderTiming) {
        for (phase, ms) in [
            ("spawn", 41),
            ("initialize", 120),
            ("thread_start", 30),
            ("turn_start", 12),
            ("first_delta", 8_000),
            ("turn_total", 9_000),
        ] {
            assert_eq!(
                timing.upstream.get(phase),
                Some(&Duration::from_millis(ms)),
                "{phase}"
            );
        }
        assert!(!timing.upstream.contains_key("unexpected"));
    }

    #[tokio::test]
    async fn unary_reply_with_usage_reports_tokens_cache_legs_and_broker_phases() {
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"success":true,"data":{{"response":{{"requestId":"req-1","content":"Hei","modelUsed":"gpt-5.6-terra","usage":{USAGE},"phasesMs":{PHASES}}}}}}}"#
        );
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();

        let response = provider
            .infer(&subscription_request("req-1"))
            .await
            .unwrap();

        assert_eq!(response.content, "Hei");
        assert_eq!(
            (response.input_tokens, response.output_tokens),
            (1_800, 120)
        );
        assert_eq!(response.cache_read_input_tokens, 1_024);
        assert_eq!(response.cache_creation_input_tokens, 256);
        let timing = response.provider_timing.expect("broker timing");
        assert!(timing.usage_reported);
        assert_broker_phases(&timing);
        assert!(
            !timing.upstream.contains_key(BROKER_READY),
            "the unary route has no ready event"
        );
    }

    #[tokio::test]
    async fn old_unary_reply_without_usage_still_gives_zeros_and_unknown_usage() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"success":true,"data":{"response":{"requestId":"req-1","content":"Hei","modelUsed":"gpt-5.6-terra"}}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();

        let response = provider
            .infer(&subscription_request("req-1"))
            .await
            .unwrap();

        assert_eq!(response.content, "Hei");
        assert_eq!((response.input_tokens, response.output_tokens), (0, 0));
        assert_eq!(
            (
                response.cache_read_input_tokens,
                response.cache_creation_input_tokens
            ),
            (0, 0)
        );
        let timing = response.provider_timing.expect("broker timing");
        assert!(!timing.usage_reported);
        assert!(timing.upstream.is_empty());
    }

    #[tokio::test]
    async fn malformed_usage_or_phases_never_fail_the_answer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"success":true,"data":{"response":{"requestId":"req-1","content":"Hei","modelUsed":"gpt-5.6-terra","usage":"n/a","phasesMs":{"spawn":-4,"turn_total":"slow"}}}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();

        let response = provider
            .infer(&subscription_request("req-1"))
            .await
            .unwrap();

        assert_eq!(response.content, "Hei");
        assert_eq!((response.input_tokens, response.output_tokens), (0, 0));
        let timing = response.provider_timing.expect("broker timing");
        assert!(!timing.usage_reported);
        assert!(timing.upstream.is_empty());
    }

    #[tokio::test]
    async fn stream_done_with_usage_reports_tokens_phases_and_broker_ready() {
        let server = MockServer::start().await;
        let body = format!(
            concat!(
                "event: subscription.ready\n",
                "data: {{\"type\":\"ready\",\"requestId\":\"req-1\",\"modelUsed\":\"gpt-5.6-terra\"}}\n\n",
                "event: subscription.delta\n",
                "data: {{\"type\":\"delta\",\"delta\":\"Hei\"}}\n\n",
                "event: subscription.done\n",
                "data: {{\"type\":\"done\",\"requestId\":\"req-1\",\"modelUsed\":\"gpt-5.6-terra\",\"usage\":{usage},\"phasesMs\":{phases}}}\n\n"
            ),
            usage = USAGE,
            phases = PHASES,
        );
        Mock::given(method("POST"))
            .and(path(
                "/internal/model-subscriptions/openai-codex/infer/stream",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();

        let mut stream = provider
            .infer_stream(&subscription_request("req-1"))
            .await
            .expect("stream");
        let delta = stream.recv().await.expect("delta");
        let done = stream.recv().await.expect("done");

        assert_eq!(delta.delta, "Hei");
        assert_eq!((delta.input_tokens, delta.output_tokens), (0, 0));
        assert!(done.done);
        assert_eq!(done.stop_reason, "end_turn");
        assert_eq!((done.input_tokens, done.output_tokens), (1_800, 120));
        assert_eq!(done.cache_read_input_tokens, 1_024);
        assert_eq!(done.cache_creation_input_tokens, 256);
        let timing = done.provider_timing.expect("broker timing");
        assert!(timing.usage_reported);
        assert_broker_phases(&timing);
        assert!(timing.upstream.contains_key(BROKER_READY));
    }

    #[test]
    fn usage_cache_legs_are_capped_inside_the_input() {
        let usage = |input, cached, write, output| BrokerUsage {
            input_tokens: input,
            cached_input_tokens: cached,
            cache_write_input_tokens: write,
            output_tokens: output,
        };
        let counts = |input, output, cache_read, cache_write| TokenCounts {
            input,
            output,
            cache_read,
            cache_write,
        };
        assert_eq!(
            usage(1_800, 1_024, 256, 120).counts(),
            counts(1_800, 120, 1_024, 256)
        );
        // A read above the input is capped at it, and leaves no room to write.
        assert_eq!(usage(100, 150, 20, 5).counts(), counts(100, 5, 100, 0));
        // A write is capped at what the reads leave.
        assert_eq!(usage(100, 60, 70, 5).counts(), counts(100, 5, 60, 40));
        assert_eq!(usage(-1, -2, -3, -4).counts(), TokenCounts::default());
        assert_eq!(
            usage(i64::MAX, 0, 0, i64::MAX).counts(),
            counts(i32::MAX, i32::MAX, 0, 0)
        );
    }

    /// Text a broker must never relay, planted where a careless reader would
    /// pick it up: the rejection's message.
    const CANARY: &str = "Try again at 3:05 PM CANARY";
    const UNARY_PATH: &str = "/internal/model-subscriptions/openai-codex/infer";
    const STREAM_PATH: &str = "/internal/model-subscriptions/openai-codex/infer/stream";

    /// A reset `hours` from now, as the broker writes it: RFC 3339 UTC to
    /// whole seconds.
    fn reset_in(hours: i64) -> (String, DateTime<Utc>) {
        let at = Utc::now() + chrono::TimeDelta::hours(hours);
        let text = at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let parsed = DateTime::parse_from_rfc3339(&text)
            .unwrap()
            .with_timezone(&Utc);
        (text, parsed)
    }

    /// A broker that answers `route` with `status` and the error envelope
    /// `error`, plus a `Retry-After` header when given.
    async fn rejecting_broker(
        route: &str,
        status: u16,
        error: serde_json::Value,
        retry_after_secs: Option<u64>,
    ) -> MockServer {
        let server = MockServer::start().await;
        let mut reply = ResponseTemplate::new(status)
            .set_body_json(serde_json::json!({"success": false, "error": error}));
        if let Some(seconds) = retry_after_secs {
            reply = reply.insert_header("Retry-After", seconds.to_string().as_str());
        }
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(reply)
            .mount(&server)
            .await;
        server
    }

    fn broker_provider(server: &MockServer) -> CodexSubscriptionProvider {
        CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
            .unwrap()
    }

    fn assert_no_canary(error: &ProviderError) {
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("CANARY"), "broker text leaked: {text}");
        }
    }

    #[tokio::test]
    async fn a_usage_limit_rejection_carries_its_reset_and_retry_after() {
        let (resets_at, expected) = reset_in(5);
        let server = rejecting_broker(
            UNARY_PATH,
            429,
            serde_json::json!({
                "code": "subscription_usage_limit", "message": CANARY,
                "resetsAt": resets_at, "retryAfterMs": 18_000_000_u64,
                "windowMinutes": 300, "limitReachedType": "rate_limit_reached"
            }),
            Some(18_000),
        )
        .await;
        let error = broker_provider(&server)
            .infer(&subscription_request("usage"))
            .await
            .unwrap_err();
        assert_no_canary(&error);
        match error {
            ProviderError::SubscriptionRefused {
                reason,
                retry_after_ms,
                resets_at,
            } => {
                assert_eq!(reason, SubscriptionRefusal::UsageLimit);
                assert_eq!(retry_after_ms, Some(18_000_000), "the body wins over the header");
                assert_eq!(resets_at, Some(expected));
            }
            other => panic!("expected a usage-limit refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_usage_limit_without_a_known_reset_has_none_and_falls_back_to_the_header() {
        let server = rejecting_broker(
            UNARY_PATH,
            429,
            serde_json::json!({"code": "subscription_usage_limit", "message": CANARY}),
            Some(60),
        )
        .await;
        match broker_provider(&server)
            .infer(&subscription_request("usage"))
            .await
            .unwrap_err()
        {
            ProviderError::SubscriptionRefused {
                reason: SubscriptionRefusal::UsageLimit,
                retry_after_ms,
                resets_at,
            } => {
                assert_eq!(retry_after_ms, Some(60_000));
                assert_eq!(resets_at, None);
            }
            other => panic!("expected a usage-limit refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_implausible_reset_time_is_dropped_as_unknown() {
        for resets_at in [
            "tomorrow".to_owned(),
            reset_in(30 * 24).0,
            reset_in(-1).0,
            String::new(),
        ] {
            let server = rejecting_broker(
                UNARY_PATH,
                429,
                serde_json::json!({"code": "subscription_usage_limit", "resetsAt": resets_at}),
                None,
            )
            .await;
            match broker_provider(&server)
                .infer(&subscription_request("usage"))
                .await
                .unwrap_err()
            {
                ProviderError::SubscriptionRefused {
                    reason: SubscriptionRefusal::UsageLimit,
                    resets_at: reported,
                    retry_after_ms,
                } => {
                    assert_eq!(reported, None, "{resets_at:?} must not be shown");
                    assert_eq!(retry_after_ms, None);
                }
                other => panic!("{resets_at:?}: expected a usage-limit refusal, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_reset_is_believed_only_inside_the_longest_usage_window() {
        let now = DateTime::parse_from_rfc3339("2026-09-25T10:24:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let at = |text: &str| plausible_reset(Some(text), now);
        assert_eq!(
            at("2026-09-26T01:21:00Z").map(|t| t.to_rfc3339()),
            Some("2026-09-26T01:21:00+00:00".to_owned())
        );
        // An offset is converted, not rejected.
        assert!(at("2026-09-26T03:21:00+02:00").is_some());
        assert_eq!(at("2026-09-25T10:24:00Z"), None, "a reset now is not ahead");
        assert!(at("2026-10-02T11:24:00Z").is_some(), "seven days and an hour");
        assert_eq!(at("2026-10-02T11:24:01Z"), None);
        assert_eq!(at("26.09.2026 01:21"), None);
        assert_eq!(plausible_reset(None, now), None);
    }

    #[tokio::test]
    async fn every_typed_broker_code_maps_to_its_own_error() {
        let cases: [(u16, &str, fn(&ProviderError) -> bool); 7] = [
            (409, "subscription_reauth_required", |e| {
                matches!(e, ProviderError::SubscriptionRefused { reason: SubscriptionRefusal::ReauthRequired, retry_after_ms: None, resets_at: None })
            }),
            (409, "subscription_connection_inactive", |e| {
                matches!(e, ProviderError::SubscriptionRefused { reason: SubscriptionRefusal::ReauthRequired, .. })
            }),
            (503, "subscription_upstream_unavailable", |e| {
                matches!(e, ProviderError::SubscriptionRefused { reason: SubscriptionRefusal::UpstreamUnavailable, retry_after_ms: Some(2_000), resets_at: None })
            }),
            (413, "subscription_context_too_long", |e| {
                matches!(e, ProviderError::TooLong { detail } if detail == "subscription_context_too_long")
            }),
            (422, "subscription_bad_request", |e| {
                matches!(e, ProviderError::SubscriptionRefused { reason: SubscriptionRefusal::BadRequest, retry_after_ms: None, resets_at: None })
            }),
            (429, "rate_limited", |e| matches!(e, ProviderError::RateLimited { retry_after_ms: 2_000 })),
            (502, "subscription_broker_failed", |e| {
                matches!(e, ProviderError::Unavailable(message) if message == "subscription broker rejected the request: subscription_broker_failed")
            }),
        ];
        for (status, code, expected) in cases {
            let server = rejecting_broker(
                UNARY_PATH,
                status,
                serde_json::json!({"code": code, "message": CANARY}),
                Some(2),
            )
            .await;
            let error = broker_provider(&server)
                .infer(&subscription_request("typed"))
                .await
                .unwrap_err();
            assert_no_canary(&error);
            assert!(expected(&error), "{status} {code}: got {error:?}");
        }
    }

    #[tokio::test]
    async fn a_throttle_without_a_readable_body_or_known_code_stays_rate_limited() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(UNARY_PATH))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "3")
                    .set_body_string("slow down"),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            broker_provider(&server).infer(&subscription_request("r")).await,
            Err(ProviderError::RateLimited { retry_after_ms: 3_000 })
        ));
        let server =
            rejecting_broker(UNARY_PATH, 429, serde_json::json!({"code": "queue_full"}), None)
                .await;
        assert!(matches!(
            broker_provider(&server).infer(&subscription_request("r")).await,
            Err(ProviderError::RateLimited { retry_after_ms: 1_000 })
        ));
    }

    #[tokio::test]
    async fn a_stream_rejected_before_its_200_is_typed_like_the_unary_route() {
        let server = rejecting_broker(
            STREAM_PATH,
            409,
            serde_json::json!({"code": "subscription_reauth_required", "message": CANARY}),
            None,
        )
        .await;
        let error = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .unwrap_err();
        assert_no_canary(&error);
        assert!(
            matches!(
                error,
                ProviderError::SubscriptionRefused {
                    reason: SubscriptionRefusal::ReauthRequired,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    async fn streaming_broker(events: &[serde_json::Value]) -> MockServer {
        let server = MockServer::start().await;
        let body = events
            .iter()
            .map(|event| format!("event: subscription.{}\ndata: {event}\n\n", event["type"].as_str().unwrap()))
            .collect::<String>();
        Mock::given(method("POST"))
            .and(path(STREAM_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_refusal_before_the_first_token_fails_the_stream_open() {
        let (resets_at, expected) = reset_in(3);
        let server = streaming_broker(&[
            serde_json::json!({"type": "ready", "requestId": "stream"}),
            serde_json::json!({
                "type": "error", "code": "subscription_usage_limit", "message": CANARY,
                "resetsAt": resets_at, "retryAfterMs": 10_800_000_u64, "windowMinutes": 300
            }),
        ])
        .await;
        let error = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .expect_err("a refusal before any token is a typed error at open");
        assert_no_canary(&error);
        match error {
            ProviderError::SubscriptionRefused {
                reason: SubscriptionRefusal::UsageLimit,
                retry_after_ms,
                resets_at,
            } => {
                assert_eq!(retry_after_ms, Some(10_800_000));
                assert_eq!(resets_at, Some(expected));
            }
            other => panic!("expected a usage-limit refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unknown_stream_error_before_the_first_token_names_only_its_code() {
        let server = streaming_broker(&[
            serde_json::json!({"type": "ready"}),
            serde_json::json!({"type": "error", "code": "subscription_broker_failed", "message": CANARY}),
        ])
        .await;
        let error = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .unwrap_err();
        assert_no_canary(&error);
        assert!(
            matches!(&error, ProviderError::SubscriptionFailed(message)
                if message == "subscription broker stream failed: subscription_broker_failed"),
            "{error:?}"
        );
    }

    /// After `ready` the broker has started the Codex turn, so an early end is
    /// final for the call, like an untyped `error` event.
    #[tokio::test]
    async fn a_stream_that_closes_after_ready_but_before_a_token_is_a_failed_turn() {
        let server = streaming_broker(&[serde_json::json!({"type": "ready"})]).await;
        let error = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, ProviderError::SubscriptionFailed(message)
                if message == "subscription broker stream ended before its first token"),
            "{error:?}"
        );
    }

    /// With no event at all the broker never reached the turn: a transport
    /// fault, which stays the retryable `Unavailable`.
    #[tokio::test]
    async fn a_stream_that_closes_before_any_event_is_unavailable() {
        let server = streaming_broker(&[]).await;
        let error = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, ProviderError::Unavailable(message)
                if message == "subscription broker stream ended before the first event"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn an_error_after_the_first_token_still_ends_the_open_stream() {
        let server = streaming_broker(&[
            serde_json::json!({"type": "ready"}),
            serde_json::json!({"type": "delta", "delta": "Hei"}),
            serde_json::json!({"type": "error", "code": "subscription_upstream_unavailable", "message": CANARY}),
        ])
        .await;
        let mut stream = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .expect("a started stream opens");
        let delta = stream.recv().await.expect("the first delta");
        assert_eq!(delta.delta, "Hei");
        let done = stream.recv().await.expect("a final chunk");
        assert!(done.done);
        assert_eq!(done.stop_reason, "stream_error");
        assert!(done.provider_timing.unwrap().upstream.contains_key(BROKER_READY));
        assert!(stream.recv().await.is_none());
    }

    #[tokio::test]
    async fn a_done_first_stream_still_answers() {
        let server = streaming_broker(&[
            serde_json::json!({"type": "ready"}),
            serde_json::json!({"type": "done", "requestId": "stream", "modelUsed": "gpt-5.6-terra"}),
        ])
        .await;
        let mut stream = broker_provider(&server)
            .infer_stream(&subscription_request("stream"))
            .await
            .unwrap();
        let done = stream.recv().await.expect("the final chunk");
        assert!(done.done);
        assert_eq!(done.stop_reason, "end_turn");
        assert!(stream.recv().await.is_none());
    }

    #[test]
    fn a_broker_code_is_reduced_to_a_bare_token() {
        assert_eq!(code_token(" subscription_usage_limit "), "subscription_usage_limit");
        assert_eq!(
            code_token("rate_limited: Try again at 3:05 PM"),
            "rate_limitedryagainat305"
        );
        assert_eq!(code_token(&"a".repeat(100)).len(), 64);
    }
}
