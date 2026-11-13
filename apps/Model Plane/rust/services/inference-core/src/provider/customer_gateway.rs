//! Customer-gateway chat inference through Integration Core (F-t3-065 phase 2, W4-BYO-5).
//!
//! An organisation admin registers an OpenAI-compatible endpoint of their own as a model connection in
//! Integration Core, which seals its key and never returns it. This adapter never accepts, stores or
//! forwards that key or the endpoint's address. Like `codex_subscription.rs`, it names the connection to
//! the broker by its opaque id, sends the conversation and the connection's model id, and relays the
//! answer; the broker makes the call and holds everything else.
//!
//! The adapter is reached only by a call that names a connection (`model_connection_id`) while
//! `INFERENCE_BYO_GATEWAY_ENABLED` is the literal `true`, and no platform provider ever serves such a
//! call. A customer gateway is never zero data retention and makes no residency commitment this product
//! can verify, so a ZDR turn, and any privacy or residency floor above what the route declares, is refused
//! before anything is sent. Its usage is customer-billed: the served-by id names the connection, and the
//! usage is logged under the `customer_connection` billing label, so no platform price is applied to it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{info, warn};

use super::fallback::is_unspecified_model;
use super::role::ROLE_MODEL_ID;
use super::{
    ChatMessage, InferChunk, InferRequest, InferResponse, ModelFamily, PrivacyTier,
    ProviderCapabilities, ProviderError, ProviderRouter, ProviderTiming, Residency,
};
use crate::config::InferenceConfig;

/// The registry id of this adapter. A served answer names `customer-gateway.<connection id>` in its
/// `provider_used`, so the served-by line names the connection and never the gateway's address.
pub const PROVIDER_ID: &str = "customer-gateway";

/// The billing label a customer-gateway call is recorded under. It is a label only: it marks the call as
/// customer-billed for whoever prices it, and nothing here sets a price.
pub const BILLING_LABEL: &str = "customer_connection";

/// The longest `provider_used` the served-by reader accepts: a registry-shaped token of at most 64
/// characters. The connection id takes what the registry id leaves.
const SERVED_BY_MAX_LEN: usize = 64;

/// Integration Core's broker routes for a customer-gateway connection.
const INFER_PATH: &str = "/internal/model-connections/customer-gateway/infer";
const STREAM_PATH: &str = "/internal/model-connections/customer-gateway/infer/stream";

/// The broker's own failure codes this adapter types. Any other code stays untyped.
const CODE_RATE_LIMITED: &str = "gateway_rate_limited";
const CODE_CONTEXT_TOO_LONG: &str = "gateway_context_too_long";
const CODE_REQUEST_REJECTED: &str = "gateway_request_rejected";
const CODE_ZDR_NOT_SUPPORTED: &str = "gateway_zdr_not_supported";
const CODE_MODEL_NOT_LISTED: &str = "gateway_model_not_listed";
/// The broker's codes for a body this adapter built wrongly: a refusal of the request, not of the gateway.
const CODES_MALFORMED: [&str; 3] = [
    "invalid_gateway_request",
    "invalid_gateway_param",
    "invalid_body",
];

/// The retry-after assumed for a throttled gateway that named none.
const DEFAULT_RATE_LIMIT_RETRY_MS: u64 = 1_000;

/// The broker's stream events: Integration Core's `gateway.*` event names.
const EVENT_READY: &str = "gateway.ready";
const EVENT_CHUNK: &str = "gateway.chunk";
const EVENT_ERROR: &str = "gateway.error";
const EVENT_DONE: &str = "gateway.done";

/// The upstream phase measured here: from sending a stream request until the broker's `ready` event.
const BROKER_READY: &str = "broker_ready";

/// The message roles the gateway contract carries. A tool or any other role is refused, since no tools run.
const ROLES: [&str; 3] = ["system", "user", "assistant"];

/// The most characters of a broker code kept for logs and messages.
const CODE_MAX_LEN: usize = 64;

/// The customer-gateway adapter: one OpenAI-compatible chat call through Integration Core, to the connection the
/// request names. It holds only the broker's URL and the service key; no connection, key or address is held here.
#[derive(Clone)]
pub struct CustomerGatewayProvider {
    client: reqwest::Client,
    integration_core_url: Arc<str>,
    internal_api_key: Arc<str>,
}

impl CustomerGatewayProvider {
    /// An adapter for the Integration Core broker at `integration_core_url`, authenticated with the service
    /// key. Both are the values the subscription route already uses.
    ///
    /// # Errors
    ///
    /// `Unavailable` when either is empty.
    pub fn new(
        integration_core_url: impl Into<String>,
        internal_api_key: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let integration_core_url = integration_core_url.into();
        let internal_api_key = internal_api_key.into();
        if integration_core_url.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "the Integration Core URL for customer gateways is empty".to_owned(),
            ));
        }
        if internal_api_key.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "the Integration Core service key for customer gateways is empty".to_owned(),
            ));
        }
        Ok(Self {
            client: super::provider_http_client(),
            integration_core_url: Arc::from(integration_core_url.trim().trim_end_matches('/')),
            internal_api_key: Arc::from(internal_api_key.trim()),
        })
    }

    /// The adapter this process runs, when `INFERENCE_BYO_GATEWAY_ENABLED` is the literal `true` and the
    /// broker is configured. It shares the Integration Core URL of the subscription route and takes its own
    /// service key. `None` when the switch is off; and when it is on but the broker is not configured, `None`
    /// with a warning.
    #[must_use]
    pub fn from_config(cfg: &InferenceConfig) -> Option<Self> {
        if !cfg.byo_gateway_enabled {
            return None;
        }
        let (Some(url), Some(key)) = (
            cfg.codex_subscription_integration_core_url.as_ref(),
            cfg.customer_gateway_internal_api_key.as_ref(),
        ) else {
            warn!(
                provider = PROVIDER_ID,
                "customer gateway connections are enabled but the Integration Core URL or service key is absent"
            );
            return None;
        };
        match Self::new(url.clone(), key.clone()) {
            Ok(provider) => {
                info!(provider = PROVIDER_ID, "customer gateway provider enabled");
                Some(provider)
            }
            Err(error) => {
                warn!(provider = PROVIDER_ID, error = %error, "customer gateway configuration rejected");
                None
            }
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.integration_core_url)
    }

    /// One call to the broker, authenticated with the service key. The body is the admitted request.
    async fn post(
        &self,
        path: &str,
        body: &BrokerRequest,
        what: &str,
    ) -> Result<reqwest::Response, ProviderError> {
        self.client
            .post(self.url(path))
            .header("X-Internal-API-Key", self.internal_api_key.as_ref())
            .json(body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(format!("customer gateway {what}: {error}")))
    }
}

/// What this route declares. Residency is `Global`, the weakest, because a customer endpoint makes no
/// residency commitment this product can verify, so the tier the route derives is `Global`. It is never
/// ZDR, and it offers no tools, vision or structured output.
fn declared() -> ProviderCapabilities {
    ProviderCapabilities {
        provider_id: PROVIDER_ID.to_owned(),
        aliases: Vec::new(),
        model_family: ModelFamily::OpenAiCompatible,
        residency: Residency::Global,
        exclusive_catalog: false,
        supports_tools: false,
        supports_vision: false,
        supports_thinking: false,
        supports_streaming: true,
        supports_embeddings: false,
        supports_zdr: false,
        modalities: vec!["chat".to_owned()],
        max_context_tokens: 0,
        max_output_tokens: 0,
    }
}

/// The broker body of one call. Field names are the broker's own snake_case JSON.
#[derive(Debug, Serialize)]
struct BrokerRequest {
    connection_id: String,
    org_id: String,
    request_id: String,
    model: String,
    messages: Vec<BrokerMessage>,
    /// The OpenAI sampling parameters the broker forwards: `temperature` and `max_tokens`.
    params: serde_json::Map<String, serde_json::Value>,
    /// Always false: a call that asks for ZDR is refused before it is sent.
    zdr_required: bool,
}

#[derive(Debug, Serialize)]
struct BrokerMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
}

/// One admitted call: the broker body, and the served-by id its answer carries.
struct Admitted {
    body: BrokerRequest,
    served_by: String,
}

/// Every check a call must pass before a byte is sent to the broker, in the order a caller can act on them.
/// Each refusal is typed and content-free; nothing is sent when one fires.
fn admit(req: &InferRequest) -> Result<Admitted, ProviderError> {
    let connection_id = connection_id(&req.model_connection_id)?;
    if !req.provider_hint.trim().is_empty() && !names_this_provider(&req.provider_hint) {
        return Err(ProviderError::RequestRejected {
            detail: "provider_hint names a provider other than the customer gateway connection"
                .to_owned(),
        });
    }
    if req.org_id.trim().is_empty() {
        return Err(ProviderError::Unavailable(
            "verified organization scope is required for a customer gateway connection".to_owned(),
        ));
    }
    // A customer gateway is never zero data retention: its attestation is never made. A turn that asks for
    // ZDR, by its token or by its own flag, is refused, as a modality with no attested deployment is.
    if req.zdr {
        return Err(ProviderError::ZdrUnavailable(
            "customer gateway connections have no verified ZDR commitment".to_owned(),
        ));
    }
    // The route's own posture is the weakest tier, so an org floor above it refuses the call.
    let posture = PrivacyTier::classify(&declared());
    if req.min_privacy_tier != PrivacyTier::Unspecified && posture < req.min_privacy_tier {
        return Err(ProviderError::TierUnavailable(format!(
            "customer gateway connections do not satisfy privacy tier `{}`",
            req.min_privacy_tier.label()
        )));
    }
    residency_floor(req.min_residency.trim())?;
    if !req.image_parts.is_empty() {
        return Err(ProviderError::VisionUnsupported(
            "customer gateway connections do not carry image input".to_owned(),
        ));
    }
    if !req.tools.is_empty() {
        return Err(ProviderError::RequestRejected {
            detail: "customer gateway connections do not run tools".to_owned(),
        });
    }
    if req
        .structured_output_schema
        .as_deref()
        .is_some_and(|schema| !schema.is_empty())
    {
        return Err(ProviderError::RequestRejected {
            detail: "customer gateway connections do not return structured output".to_owned(),
        });
    }
    let model = req.model.trim();
    if is_unspecified_model(model) || model.eq_ignore_ascii_case(ROLE_MODEL_ID) {
        return Err(ProviderError::RequestRejected {
            detail: "a customer gateway call names the connection's own model id".to_owned(),
        });
    }
    if let Some(access) = req.access.as_deref() {
        if let Some(reason) = access.refuses(model, PROVIDER_ID) {
            return Err(access.refusal(model, reason));
        }
    }
    if req.messages.is_empty() {
        return Err(ProviderError::RequestRejected {
            detail: "a customer gateway call needs at least one message".to_owned(),
        });
    }
    let messages = req
        .messages
        .iter()
        .map(broker_message)
        .collect::<Result<Vec<_>, _>>()?;
    // Zero is the proto's unset value, so only a temperature the caller set is sent; an arbitrary endpoint keeps
    // its own default otherwise.
    let mut params = serde_json::Map::new();
    if req.temperature.is_finite() && req.temperature > 0.0 && req.temperature <= 2.0 {
        params.insert("temperature".to_owned(), serde_json::json!(req.temperature));
    }
    if req.max_tokens > 0 {
        params.insert("max_tokens".to_owned(), serde_json::json!(req.max_tokens));
    }
    Ok(Admitted {
        body: BrokerRequest {
            connection_id: connection_id.to_owned(),
            org_id: req.org_id.clone(),
            request_id: req.request_id.clone(),
            model: model.to_owned(),
            messages,
            params,
            zdr_required: false,
        },
        served_by: served_by_id(connection_id),
    })
}

/// The connection id a call names: trimmed, non-empty, made of letters, digits, `-`, `_` and `.`, and short
/// enough that the served-by id it forms fits the reader's limit.
fn connection_id(raw: &str) -> Result<&str, ProviderError> {
    let id = raw.trim();
    let well_formed = !id.is_empty()
        && PROVIDER_ID.len() + 1 + id.len() <= SERVED_BY_MAX_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if well_formed {
        Ok(id)
    } else {
        Err(ProviderError::RequestRejected {
            detail: "model_connection_id is not a customer gateway connection id".to_owned(),
        })
    }
}

/// The served-by id of a connection: the registry id, a dot and the opaque connection id.
fn served_by_id(connection_id: &str) -> String {
    format!("{PROVIDER_ID}.{connection_id}")
}

/// Whether a provider hint names this adapter, in the spellings the chain accepts for a provider.
fn names_this_provider(hint: &str) -> bool {
    hint.trim().to_ascii_lowercase().replace('_', "-") == PROVIDER_ID
}

/// Refuse a residency floor this route cannot meet. The route declares no commitment, so any floor above
/// `Global` refuses, and a token that is not a residency at all refuses too.
fn residency_floor(floor: &str) -> Result<(), ProviderError> {
    if floor.is_empty() {
        return Ok(());
    }
    match Residency::parse(floor) {
        Some(required) if required <= Residency::Global => Ok(()),
        Some(_) => Err(ProviderError::ResidencyViolation(format!(
            "customer gateway connections do not meet the requested `{floor}` residency floor"
        ))),
        None => Err(ProviderError::ResidencyViolation(format!(
            "unrecognized min_residency token `{floor}`"
        ))),
    }
}

/// One message in the gateway contract: its role, lower-cased and checked, its content and its name.
fn broker_message(message: &ChatMessage) -> Result<BrokerMessage, ProviderError> {
    let role = message.role.trim().to_ascii_lowercase();
    if !ROLES.contains(&role.as_str()) {
        return Err(ProviderError::RequestRejected {
            detail: "a message role the customer gateway does not carry".to_owned(),
        });
    }
    Ok(BrokerMessage {
        role,
        content: message.content.clone(),
        name: message.name.clone(),
    })
}

/// The broker's JSON envelope for every answer. The gateway's own answer rides inside `data.response`.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    success: bool,
    #[serde(default, deserialize_with = "lenient")]
    data: Option<EnvelopeData>,
    #[serde(default, deserialize_with = "lenient")]
    error: Option<EnvelopeError>,
}

#[derive(Deserialize)]
struct EnvelopeData {
    #[serde(default)]
    response: serde_json::Value,
}

/// The broker's `error` object. Its `message` is deliberately not read: the reason travels as `code`
/// alone, with the broker's own retry timing beside it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeError {
    #[serde(default, deserialize_with = "lenient")]
    code: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    retry_after_seconds: Option<u64>,
}

/// Telemetry and optional fields must never fail an answer: a malformed one reads as absent.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// A broker code reduced to a bare token: lower-case letters, digits and underscores, at most 64 of them.
fn code_token(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .take(CODE_MAX_LEN)
        .collect()
}

/// A `Retry-After` header in whole seconds, as milliseconds.
fn retry_after_header_ms(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1_000))
}

/// The typed error for a broker code, or `None` for a code this adapter does not type.
fn typed_failure(code: &str, retry_ms: Option<u64>) -> Option<ProviderError> {
    Some(match code {
        CODE_RATE_LIMITED => ProviderError::RateLimited {
            retry_after_ms: retry_ms.unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
        },
        CODE_CONTEXT_TOO_LONG => ProviderError::TooLong {
            detail: CODE_CONTEXT_TOO_LONG.to_owned(),
        },
        CODE_REQUEST_REJECTED => ProviderError::RequestRejected {
            detail: CODE_REQUEST_REJECTED.to_owned(),
        },
        code if CODES_MALFORMED.contains(&code) => ProviderError::RequestRejected {
            detail: code.to_owned(),
        },
        CODE_ZDR_NOT_SUPPORTED => ProviderError::ZdrUnavailable(
            "customer gateway connections have no verified ZDR commitment".to_owned(),
        ),
        CODE_MODEL_NOT_LISTED => ProviderError::UnsupportedModel(
            "the model is not listed on this customer gateway connection".to_owned(),
        ),
        _ => return None,
    })
}

/// The error for a broker answer whose envelope was read. Untyped codes name themselves and nothing more.
fn rejection(
    status: reqwest::StatusCode,
    header_retry_ms: Option<u64>,
    error: Option<EnvelopeError>,
) -> ProviderError {
    let (code, body_retry_ms) = error.map_or((String::new(), None), |error| {
        (
            error.code.as_deref().map(code_token).unwrap_or_default(),
            error
                .retry_after_seconds
                .map(|seconds| seconds.saturating_mul(1_000)),
        )
    });
    let retry_ms = body_retry_ms.or(header_retry_ms);
    if let Some(typed) = typed_failure(&code, retry_ms) {
        return typed;
    }
    // A throttled broker whose code is unknown is still throttled.
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return ProviderError::RateLimited {
            retry_after_ms: retry_ms.unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
        };
    }
    ProviderError::Unavailable(format!(
        "customer gateway rejected the request{}",
        if code.is_empty() {
            String::new()
        } else {
            format!(": {code}")
        }
    ))
}

/// The typed error for a broker answer that is not a success.
async fn refusal(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    let header_retry_ms = retry_after_header_ms(response.headers());
    match response.json::<Envelope>().await {
        Ok(envelope) => rejection(status, header_retry_ms, envelope.error),
        Err(_) if status == reqwest::StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimited {
            retry_after_ms: header_retry_ms.unwrap_or(DEFAULT_RATE_LIMIT_RETRY_MS),
        },
        Err(error) => ProviderError::InvalidResponse(format!("customer gateway refusal: {error}")),
    }
}

/// The token counts one call reported, in the gateway's OpenAI usage shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Usage {
    input: i64,
    output: i64,
    cached: i64,
}

/// Token counts in the shape the response and chunks carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    input: i32,
    output: i32,
    cache_read: i32,
}

impl Usage {
    /// Both sides of the count, with the cached share kept inside the input, as OpenAI's accounting counts it.
    fn counts(self) -> Counts {
        let input = self.input.max(0);
        let cache_read = self.cached.max(0).min(input);
        Counts {
            input: to_i32_or_max(input),
            output: to_i32_or_max(self.output.max(0)),
            cache_read: to_i32_or_max(cache_read),
        }
    }
}

fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// A usage block, when the gateway sent one.
fn read_usage(value: &serde_json::Value) -> Option<Usage> {
    value.is_object().then(|| Usage {
        input: value["prompt_tokens"].as_i64().unwrap_or(0),
        output: value["completion_tokens"].as_i64().unwrap_or(0),
        cached: value["prompt_tokens_details"]["cached_tokens"]
            .as_i64()
            .unwrap_or(0),
    })
}

/// The text of a message's content: a plain string, or the text parts of a content array.
fn message_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"].as_str().is_none_or(|kind| kind == "text"))
            .filter_map(|part| part["text"].as_str())
            .collect(),
        _ => String::new(),
    }
}

/// The stop reason in the response vocabulary: a natural end reads `end_turn` and a token ceiling
/// `max_tokens`. Any other label the gateway sent survives only as its lower-case letters and underscores.
fn stop_label(raw: Option<&str>) -> String {
    let label: String = raw
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || *c == '_')
        .take(32)
        .collect();
    match label.as_str() {
        "" | "stop" => "end_turn".to_owned(),
        "length" => "max_tokens".to_owned(),
        _ => label,
    }
}

/// A completed answer, read from the gateway's chat completion.
struct Completion {
    content: String,
    stop_reason: String,
    usage: Option<Usage>,
}

fn parse_completion(response: &serde_json::Value) -> Result<Completion, ProviderError> {
    let choice = response["choices"].get(0).ok_or_else(|| {
        ProviderError::InvalidResponse("customer gateway answer carried no choice".to_owned())
    })?;
    Ok(Completion {
        content: message_text(&choice["message"]["content"]),
        stop_reason: stop_label(choice["finish_reason"].as_str()),
        usage: read_usage(&response["usage"]),
    })
}

/// What a stream has said so far beyond its text: its finish reason and its usage.
#[derive(Debug, Default)]
struct StreamState {
    stop_reason: String,
    usage: Option<Usage>,
}

impl StreamState {
    /// Read one chunk's finish reason and usage, when it carries them.
    fn observe(&mut self, chunk: &serde_json::Value) {
        if let Some(reason) = chunk["choices"][0]["finish_reason"].as_str() {
            self.stop_reason = stop_label(Some(reason));
        }
        if let Some(usage) = read_usage(&chunk["usage"]) {
            self.usage = Some(usage);
        }
    }

    fn stop_reason(&self) -> String {
        if self.stop_reason.is_empty() {
            "end_turn".to_owned()
        } else {
            self.stop_reason.clone()
        }
    }
}

/// The text a streamed chunk adds: `choices[0].delta.content`.
fn chunk_delta(chunk: &serde_json::Value) -> String {
    message_text(&chunk["choices"][0]["delta"]["content"])
}

/// The broker code of a `gateway.error` payload, reduced to a bare token.
fn error_code(payload: &serde_json::Value) -> String {
    code_token(payload["code"].as_str().unwrap_or_default())
}

/// The upstream timing of one call: whether usage was reported, and on a stream how long the broker took to
/// be ready. Only the adapter's own measurement is carried; the broker reports no phases here.
fn timing(usage_reported: bool, ready: Option<Duration>) -> ProviderTiming {
    let mut upstream = BTreeMap::new();
    if let Some(ready) = ready {
        upstream.insert(BROKER_READY.to_owned(), ready);
    }
    ProviderTiming {
        usage_reported,
        upstream,
        ..ProviderTiming::default()
    }
}

/// A chunk with every field at its neutral value, stamped with this route's provenance. Every chunk the
/// adapter emits carries it, as the chain stamps every chunk it forwards.
fn chunk(request_id: &str, served_by: &str) -> InferChunk {
    InferChunk {
        compaction_summary: String::new(),
        request_id: request_id.to_owned(),
        delta: String::new(),
        done: false,
        model_used: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        stop_reason: String::new(),
        reasoning_delta: String::new(),
        provider_used: served_by.to_owned(),
        residency: Residency::Global.as_str().to_owned(),
        token_confidence: None,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        provider_timing: None,
        service_tier_used: String::new(),
    }
}

fn delta_chunk(request_id: &str, served_by: &str, delta: String) -> InferChunk {
    InferChunk {
        delta,
        ..chunk(request_id, served_by)
    }
}

/// The final chunk of a completed stream.
fn done_chunk(
    request_id: &str,
    served_by: &str,
    model: &str,
    stop_reason: String,
    usage: Option<Usage>,
    ready: Option<Duration>,
) -> InferChunk {
    let counts = usage.map(Usage::counts).unwrap_or_default();
    InferChunk {
        done: true,
        model_used: model.to_owned(),
        input_tokens: counts.input,
        output_tokens: counts.output,
        stop_reason,
        cache_read_input_tokens: counts.cache_read,
        provider_timing: Some(timing(usage.is_some(), ready)),
        service_tier_used: String::new(),
        ..chunk(request_id, served_by)
    }
}

/// The final chunk of a stream that ended without the broker's `done`.
fn failed_chunk(
    request_id: &str,
    served_by: &str,
    model: &str,
    stop_reason: &str,
    ready: Option<Duration>,
) -> InferChunk {
    InferChunk {
        done: true,
        model_used: model.to_owned(),
        stop_reason: stop_reason.to_owned(),
        provider_timing: Some(timing(false, ready)),
        service_tier_used: String::new(),
        ..chunk(request_id, served_by)
    }
}

/// One broker stream event: its `event:` name and its `data:` payload.
struct GatewayEvent {
    name: String,
    data: String,
}

impl GatewayEvent {
    /// The payload as JSON, or null when it is not JSON.
    fn payload(&self) -> serde_json::Value {
        serde_json::from_str(&self.data).unwrap_or(serde_json::Value::Null)
    }
}

/// What reading the next broker event found.
enum NextEvent {
    Event(GatewayEvent),
    /// The stream ended, or a read failed (already logged).
    Ended,
}

/// The broker's SSE events, one at a time. The name is the last `event:` line of the block a `data:` line
/// belongs to. Raw bytes are kept until a complete line arrives, because an HTTP chunk boundary may split a
/// multi-byte UTF-8 character.
struct GatewayEvents<S> {
    bytes: S,
    buffer: Vec<u8>,
    name: String,
}

impl<S, B, E> GatewayEvents<S>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    fn new(bytes: S) -> Self {
        Self {
            bytes,
            buffer: Vec::new(),
            name: String::new(),
        }
    }

    async fn next(&mut self) -> NextEvent {
        loop {
            while let Some(newline_pos) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line_bytes: Vec<u8> = self.buffer.drain(..=newline_pos).collect();
                let Ok(line) = std::str::from_utf8(&line_bytes[..newline_pos]) else {
                    warn!("customer gateway emitted an invalid UTF-8 SSE line");
                    continue;
                };
                let line = line.trim_end_matches('\r');
                if line.is_empty() {
                    self.name.clear();
                } else if let Some(name) = line.strip_prefix("event:") {
                    self.name = name.trim().to_owned();
                } else if let Some(data) = line.strip_prefix("data:") {
                    return NextEvent::Event(GatewayEvent {
                        name: self.name.clone(),
                        data: data.trim().to_owned(),
                    });
                }
            }
            match self.bytes.next().await {
                Some(Ok(bytes)) => self.buffer.extend_from_slice(bytes.as_ref()),
                Some(Err(error)) => {
                    warn!(error = %error, "customer gateway stream read failed");
                    return NextEvent::Ended;
                }
                None => return NextEvent::Ended,
            }
        }
    }
}

#[async_trait::async_trait]
impl ProviderRouter for CustomerGatewayProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        declared()
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let Admitted { body, served_by } = admit(req)?;
        let response = self.post(INFER_PATH, &body, "request").await?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        let envelope: Envelope = response.json().await.map_err(|error| {
            ProviderError::InvalidResponse(format!("customer gateway response: {error}"))
        })?;
        if !envelope.success {
            return Err(rejection(reqwest::StatusCode::OK, None, envelope.error));
        }
        let data = envelope.data.ok_or_else(|| {
            ProviderError::InvalidResponse(
                "customer gateway success response omitted data".to_owned(),
            )
        })?;
        let completion = parse_completion(&data.response)?;
        let counts = completion.usage.map(Usage::counts).unwrap_or_default();
        info!(
            request_id = %req.request_id,
            served_by = %served_by,
            billing = BILLING_LABEL,
            input_tokens = counts.input,
            output_tokens = counts.output,
            usage_reported = completion.usage.is_some(),
            "customer gateway call completed"
        );
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content: completion.content,
            model_used: req.model.clone(),
            stop_reason: completion.stop_reason,
            input_tokens: counts.input,
            output_tokens: counts.output,
            cache_read_input_tokens: counts.cache_read,
            cache_creation_input_tokens: 0,
            provider_used: served_by,
            residency: Residency::Global.as_str().to_owned(),
            provider_timing: Some(timing(completion.usage.is_some(), None)),
            ..InferResponse::default()
        })
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let Admitted { body, served_by } = admit(req)?;
        let started = Instant::now();
        let response = self.post(STREAM_PATH, &body, "stream request").await?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }

        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let mut events = GatewayEvents::new(Box::pin(response.bytes_stream()));
        let mut ready: Option<Duration> = None;
        let mut state = StreamState::default();
        // Wait for the first token, or the end, before answering. A refusal the gateway makes before any token
        // is then a typed error at stream open, as it is on the unary route.
        let first = loop {
            let NextEvent::Event(event) = events.next().await else {
                return Err(ProviderError::Unavailable(if ready.is_some() {
                    "customer gateway stream ended before its first token".to_owned()
                } else {
                    "customer gateway stream ended before the first event".to_owned()
                }));
            };
            let payload = event.payload();
            match event.name.as_str() {
                EVENT_READY => {
                    ready.get_or_insert_with(|| started.elapsed());
                }
                EVENT_CHUNK => {
                    state.observe(&payload);
                    let delta = chunk_delta(&payload);
                    if !delta.is_empty() {
                        break delta_chunk(&request_id, &served_by, delta);
                    }
                }
                EVENT_DONE => {
                    let usage = read_usage(&payload["usage"]).or(state.usage);
                    break done_chunk(
                        &request_id,
                        &served_by,
                        &model,
                        state.stop_reason(),
                        usage,
                        ready,
                    );
                }
                EVENT_ERROR => {
                    let code = error_code(&payload);
                    warn!(code = %code, "customer gateway stream failed before its first token");
                    return Err(typed_failure(&code, None).unwrap_or_else(|| {
                        ProviderError::Unavailable(format!(
                            "customer gateway stream failed: {code}"
                        ))
                    }));
                }
                _ => {}
            }
        };

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let finished = first.done;
            if tx.send(first).await.is_err() || finished {
                return;
            }
            while let NextEvent::Event(event) = events.next().await {
                let payload = event.payload();
                match event.name.as_str() {
                    EVENT_CHUNK => {
                        state.observe(&payload);
                        let delta = chunk_delta(&payload);
                        if !delta.is_empty()
                            && tx
                                .send(delta_chunk(&request_id, &served_by, delta))
                                .await
                                .is_err()
                        {
                            return;
                        }
                    }
                    EVENT_DONE => {
                        let usage = read_usage(&payload["usage"]).or(state.usage);
                        let _ = tx
                            .send(done_chunk(
                                &request_id,
                                &served_by,
                                &model,
                                state.stop_reason(),
                                usage,
                                ready,
                            ))
                            .await;
                        return;
                    }
                    EVENT_ERROR => {
                        // Text has already streamed, so the stream can only end. The code is logged, never the answer.
                        warn!(code = %error_code(&payload), "customer gateway stream failed");
                        let _ = tx
                            .send(failed_chunk(
                                &request_id,
                                &served_by,
                                &model,
                                "stream_error",
                                ready,
                            ))
                            .await;
                        return;
                    }
                    _ => {}
                }
            }
            let _ = tx
                .send(failed_chunk(
                    &request_id,
                    &served_by,
                    &model,
                    "stream_incomplete",
                    ready,
                ))
                .await;
        });
        Ok(rx)
    }
}
