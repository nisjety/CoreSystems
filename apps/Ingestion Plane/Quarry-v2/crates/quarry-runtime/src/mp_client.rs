//! Model Plane HTTP client + real Planner implementation.
//!
//! Calls Model Plane's gateway `POST /v1/invoke` to ask an LLM for the next
//! agent action(s) given a BrowserObservation. Replaces MockPlanner for
//! production use.
//!
//! Auth: Bearer JWT (Model Plane gateway requires it). Caller supplies token.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use quarry_core::contracts::{
    AgentAction, AgentActionRequest, AgentConstraints, BrowserObservation,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::LeaseKind;
use quarry_core::ids::Id;
use quarry_core::zdr::ZdrMode;

use crate::planner::{Planner, PlannerDecision};
use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const CHAT_TIMEOUT_SECS: u64 = 60;
const CHAT_RETRY_BACKOFF_MS: u64 = 250;

#[derive(Debug, Clone, Serialize)]
pub struct ModelPlaneInvokeRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelPlaneInvokeResponse {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
}

#[derive(Clone)]
pub struct ModelPlaneClient {
    http: Client,
    base_url: String,
    auth: ModelPlaneAuth,
    default_model: String,
    /// Development bypass: the second token of the chat lane.
    inference_token: Option<String>,
    chat_timeout: Duration,
    retry_backoff: Duration,
    /// W3-E3.6-7: the OCR and PII model lanes (both off by default).
    document_lanes: DocumentLaneSettings,
    document_timeout: Duration,
}

#[derive(Clone)]
enum ModelPlaneAuth {
    None,
    StaticDev(String),
    Dynamic(SharedServiceTokenProvider),
}

impl ModelPlaneClient {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("http client build failed: {}", e.without_url()),
                )
            })?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            auth: ModelPlaneAuth::None,
            default_model: DEFAULT_MODEL.to_string(),
            inference_token: None,
            chat_timeout: Duration::from_secs(CHAT_TIMEOUT_SECS),
            retry_backoff: Duration::from_millis(CHAT_RETRY_BACKOFF_MS),
            document_lanes: DocumentLaneSettings::default(),
            document_timeout: Duration::from_secs(DOCUMENT_LANE_TIMEOUT_SECS),
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth = ModelPlaneAuth::StaticDev(token.into());
        self
    }

    /// Production authentication. Tokens are minted lazily for the verified
    /// request org supplied to [`Self::invoke_for_org`].
    pub fn with_token_provider(mut self, provider: SharedServiceTokenProvider) -> Self {
        self.auth = ModelPlaneAuth::Dynamic(provider);
        self
    }

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    pub async fn invoke(
        &self,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        self.invoke_with_org(None, req).await
    }

    pub async fn invoke_for_org(
        &self,
        org_id: &str,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        self.invoke_with_org(Some(org_id), req).await
    }

    async fn invoke_with_org(
        &self,
        org_id: Option<&str>,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        let token_request = ServiceTokenRequest::from_manifest("model-invoke")?;
        let bearer = self.bearer_for_org(org_id, &token_request, false).await?;
        let mut resp = self.send_invoke(req, bearer.as_deref()).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            && matches!(self.auth, ModelPlaneAuth::Dynamic(_))
        {
            let refreshed = self.bearer_for_org(org_id, &token_request, true).await?;
            resp = self.send_invoke(req, refreshed.as_deref()).await?;
        }

        self.decode_invoke(resp).await
    }

    async fn bearer_for_org(
        &self,
        org_id: Option<&str>,
        request: &ServiceTokenRequest,
        force_refresh: bool,
    ) -> QuarryResult<Option<String>> {
        match &self.auth {
            ModelPlaneAuth::None => Ok(None),
            ModelPlaneAuth::StaticDev(token) => Ok(Some(token.clone())),
            ModelPlaneAuth::Dynamic(provider) => {
                let org_id = org_id.ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Forbidden,
                        "verified org_id is required for Model Plane authentication",
                    )
                })?;
                Ok(Some(
                    provider
                        .token_for_org(org_id, request, force_refresh)
                        .await?
                        .expose()
                        .to_owned(),
                ))
            }
        }
    }

    async fn send_invoke(
        &self,
        req: &ModelPlaneInvokeRequest,
        bearer: Option<&str>,
    ) -> QuarryResult<reqwest::Response> {
        let url = format!("{}/v1/invoke", self.base_url.trim_end_matches('/'));
        let mut builder = self.http.post(&url).json(req);
        if let Some(token) = bearer {
            builder = builder.bearer_auth(token);
        }
        builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke transport failure: {}", e.without_url()),
            )
        })
    }

    async fn decode_invoke(
        &self,
        resp: reqwest::Response,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        let status = resp.status();
        if !status.is_success() {
            return Err(QuarryError::new(
                if matches!(status.as_u16(), 401 | 403) {
                    ErrorCode::Forbidden
                } else {
                    ErrorCode::DriverFailed
                },
                format!("model-plane invoke returned {status}"),
            ));
        }

        resp.json::<ModelPlaneInvokeResponse>().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke decode failure: {}", e.without_url()),
            )
        })
    }

    /// Streaming variant of [`invoke`]. POSTs to `/v1/invoke/stream` and yields
    /// answer-text deltas as the gateway produces them. The gateway emits SSE
    /// frames (`event: …\ndata: {json}\n\n`); we surface the `delta` string from
    /// each `data:` JSON object. A transport/parse failure ends the stream with
    /// a single `Err` item. (Note: bounded by the client's 30s request timeout.)
    pub async fn invoke_stream(
        &self,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<impl futures::Stream<Item = QuarryResult<String>>> {
        self.invoke_stream_scoped(None, req).await
    }

    pub async fn invoke_stream_for_org(
        &self,
        org_id: &str,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<impl futures::Stream<Item = QuarryResult<String>>> {
        self.invoke_stream_scoped(Some(org_id), req).await
    }

    pub async fn invoke_stream_scoped(
        &self,
        org_id: Option<&str>,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<impl futures::Stream<Item = QuarryResult<String>>> {
        let token_request = ServiceTokenRequest::from_manifest("model-stream")?;
        let bearer = self.bearer_for_org(org_id, &token_request, false).await?;
        let mut resp = self.send_invoke_stream(req, bearer.as_deref()).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            && matches!(self.auth, ModelPlaneAuth::Dynamic(_))
        {
            let refreshed = self.bearer_for_org(org_id, &token_request, true).await?;
            resp = self.send_invoke_stream(req, refreshed.as_deref()).await?;
        }
        let status = resp.status();
        if !status.is_success() {
            return Err(QuarryError::new(
                if matches!(status.as_u16(), 401 | 403) {
                    ErrorCode::Forbidden
                } else {
                    ErrorCode::DriverFailed
                },
                format!("model-plane invoke/stream returned {status}"),
            ));
        }

        Ok(async_stream::stream! {
            use futures::StreamExt;
            let mut bytes = resp.bytes_stream();
            let mut buf = String::new();
            while let Some(chunk) = bytes.next().await {
                match chunk {
                    Ok(b) => buf.push_str(&String::from_utf8_lossy(&b)),
                    Err(e) => {
                        yield Err(QuarryError::new(
                            ErrorCode::DriverFailed,
                            format!("model-plane invoke/stream read failed: {e}"),
                        ));
                        return;
                    }
                }
                while let Some(idx) = buf.find("\n\n") {
                    let frame: String = buf.drain(..idx + 2).collect();
                    for line in frame.lines() {
                        let Some(data) = line.trim_start().strip_prefix("data:") else {
                            continue;
                        };
                        let data = data.trim();
                        if data.is_empty() || data == "[DONE]" {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                if !delta.is_empty() {
                                    yield Ok(delta.to_string());
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    async fn send_invoke_stream(
        &self,
        req: &ModelPlaneInvokeRequest,
        bearer: Option<&str>,
    ) -> QuarryResult<reqwest::Response> {
        let url = format!("{}/v1/invoke/stream", self.base_url.trim_end_matches('/'));
        let mut builder = self
            .http
            .post(&url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(req);
        if let Some(token) = bearer {
            builder = builder.bearer_auth(token);
        }
        builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke/stream transport failure: {}", e.without_url()),
            )
        })
    }
}

// ---- POST /v1/ai/chat (INNH-1 U2) ------------------------------------------
//
// The lane the AI scrape formats use. It is `/v1/ai/chat`, never `/v1/invoke`:
// the gateway refuses service principals on `/v1/invoke` and its non-ZDR path
// writes the prompt into a Session Core thread, while `/v1/ai/chat` persists
// nothing and takes `structured_output_schema` and `zdr`. The body carries
// exactly the fields the gateway reads: no `session_key`, no `thread_id`, and
// no `max_tokens` or `temperature` (the route fixes them and ignores ours, so
// sending them would suggest a cap that does not exist).

/// One chat message. `role` is `system` or `user`.
#[derive(Clone, Serialize)]
pub struct AiChatMessage {
    pub role: String,
    pub content: String,
}

impl AiChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
        }
    }
}

// A page rides in `content`, so Debug names the size and never the text.
impl std::fmt::Debug for AiChatMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiChatMessage")
            .field("role", &self.role)
            .field("chars", &self.content.chars().count())
            .finish()
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The body of `POST /v1/ai/chat`.
#[derive(Clone, Serialize)]
pub struct AiChatRequest {
    pub messages: Vec<AiChatMessage>,
    /// A routing alias (`verevon-balance`), never a vendor model id.
    pub model: String,
    /// The JSON Schema as a **string**: the gateway reads an `Option<String>`
    /// and answers 400 to an object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output_schema: Option<String>,
    /// Omitted when false.
    #[serde(skip_serializing_if = "is_false")]
    pub zdr: bool,
}

impl std::fmt::Debug for AiChatRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiChatRequest")
            .field("messages", &self.messages)
            .field("model", &self.model)
            .field("has_schema", &self.structured_output_schema.is_some())
            .field("zdr", &self.zdr)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct AiChatUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// The answer of `POST /v1/ai/chat`. `cost_usd` and `provider_used` are absent
/// until the gateway returns them (WS16.S1).
#[derive(Clone, Deserialize)]
pub struct AiChatResponse {
    #[serde(default)]
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub model_used: String,
    #[serde(default)]
    pub usage: AiChatUsage,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub provider_used: Option<String>,
}

impl std::fmt::Debug for AiChatResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiChatResponse")
            .field("id", &self.id)
            .field("chars", &self.content.chars().count())
            .field("model_used", &self.model_used)
            .field("usage", &self.usage)
            .finish()
    }
}

/// Why a `/v1/ai/chat` hop failed. One mapping, here: the engine turns these
/// into report codes and no other layer reads a status.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelHopError {
    /// Quarry holds no credential for this lane (the Dynamic path is not built
    /// until the grant exists, and an unauthenticated client has none). Nothing
    /// was sent and nothing was minted.
    #[error("the Model Plane lane is not granted to Quarry")]
    LaneNotGranted,
    /// 401: the gateway did not accept the token.
    #[error("the Model Plane did not accept the credential")]
    Unauthorized,
    /// 403: the token is valid but not allowed on this route.
    #[error("the Model Plane denied the route")]
    RouteDenied,
    /// 400 and any other 4xx: the gateway refused the request itself.
    #[error("the Model Plane rejected the request")]
    Rejected,
    /// 412: the gateway would not run the request under ZDR.
    #[error("the Model Plane refused the request under zero data retention")]
    ZdrRefused,
    /// 429, with the wait when the gateway said one.
    #[error("the Model Plane rate-limited the request")]
    RateLimited { retry_after_s: Option<u32> },
    /// Any other 5xx, after the one retry of 502 and 503.
    #[error("the Model Plane answered {status}")]
    Upstream { status: u16 },
    /// No answer in time. Never retried: the provider may have charged.
    #[error("the Model Plane did not answer in time")]
    Timeout,
    /// The connection failed (after one retry of a connect error).
    #[error("the Model Plane could not be reached")]
    Transport,
    /// A 2xx whose body was not a chat answer.
    #[error("the Model Plane answer could not be read")]
    Decode,
}

/// The longest answer body read.
const MAX_CHAT_BODY_BYTES: usize = 4 * 1024 * 1024;

impl ModelPlaneClient {
    /// Development bypass only: the token sent as `x-inference-authorization`.
    /// Without it the one dev token (`with_bearer_token`) is sent in both
    /// headers. The gateway wants two tokens: the user ingress token and one
    /// with `aud=inference-core`.
    pub fn with_inference_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.inference_token = Some(token.into());
        self
    }

    /// The bound on one chat call (default 60 s). A call that outlives it is
    /// [`ModelHopError::Timeout`] and is not retried.
    pub fn with_chat_timeout(mut self, timeout: Duration) -> Self {
        self.chat_timeout = timeout;
        self
    }

    /// The pause before the one retry of a connect error, 502 or 503.
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
        self
    }

    /// One chat call on behalf of `org_id`.
    ///
    /// Only the static development credential works today. The Dynamic path
    /// (Auth Core minted tokens) needs the grant and the manifest rows that are
    /// owner decision OD-1, so it returns [`ModelHopError::LaneNotGranted`]
    /// without minting anything; so does a client with no credential.
    ///
    /// A connect error, 502 or 503 is retried once; a read timeout never is.
    pub async fn chat_for_org(
        &self,
        org_id: &str,
        req: &AiChatRequest,
    ) -> Result<AiChatResponse, ModelHopError> {
        // The org will pick the minted token once the Dynamic path exists.
        let _ = org_id;
        let (bearer, inference) = match &self.auth {
            ModelPlaneAuth::StaticDev(token) => (
                token.clone(),
                self.inference_token.clone().unwrap_or_else(|| token.clone()),
            ),
            ModelPlaneAuth::Dynamic(_) | ModelPlaneAuth::None => {
                return Err(ModelHopError::LaneNotGranted)
            }
        };
        let url = format!("{}/v1/ai/chat", self.base_url.trim_end_matches('/'));
        let mut attempt = 0u8;
        loop {
            attempt += 1;
            let sent = self
                .http
                .post(&url)
                .timeout(self.chat_timeout)
                .bearer_auth(&bearer)
                .header("x-inference-authorization", format!("Bearer {inference}"))
                .json(req)
                .send()
                .await;
            let resp = match sent {
                Ok(resp) => resp,
                Err(e) if e.is_timeout() => return Err(ModelHopError::Timeout),
                Err(e) if e.is_connect() && attempt == 1 => {
                    tokio::time::sleep(self.retry_backoff).await;
                    continue;
                }
                Err(_) => return Err(ModelHopError::Transport),
            };
            let status = resp.status().as_u16();
            if matches!(status, 502 | 503) && attempt == 1 {
                tokio::time::sleep(self.retry_backoff).await;
                continue;
            }
            return Self::decode_chat(resp).await;
        }
    }

    async fn decode_chat(resp: reqwest::Response) -> Result<AiChatResponse, ModelHopError> {
        let status = resp.status();
        if !status.is_success() {
            return Err(hop_status_error(status.as_u16(), resp.headers()));
        }
        let bytes = match resp.bytes().await {
            Ok(bytes) => bytes,
            Err(e) if e.is_timeout() => return Err(ModelHopError::Timeout),
            Err(_) => return Err(ModelHopError::Decode),
        };
        if bytes.len() > MAX_CHAT_BODY_BYTES {
            return Err(ModelHopError::Decode);
        }
        serde_json::from_slice::<AiChatResponse>(&bytes).map_err(|_| ModelHopError::Decode)
    }
}

// ---- OCR route and PII model tier (W3-E3.6-7) -------------------------------
//
// Two document lanes on the WS16 service lane, both dark:
//
// - OCR: only the scanned pages of a PDF (the caller classifies them; WS5's
//   `document_has_no_text_layer` is the signal) go to
//   `POST /v1/ai/documents/layout` (prebuilt-layout). Behind
//   `QUARRY_EDGE__OCR`, default off.
// - PII model tier: an optional second pass after the deterministic
//   `quarry-transform` `pii.rs` stage, on text that stage already redacted.
//   `POST /v1/ai/language/pii` takes no entity parameters and its offsets are
//   unverified, so Quarry reads only `redacted_text`, never offsets. Behind
//   `QUARRY_EDGE__PII_MODEL`, default off.
//
// Both are refused under ZDR before anything is sent (OCR and the PII model
// are third-party processing; plan E3.6 "OCR and the PII model never run
// under ZDR"). Both fail closed: a disabled lane, a missing grant, an error
// status, a short or unreadable answer is a typed error, never the input text
// passed through and never empty text. The caller turns the error into a
// failed page (`pages_failed`), as the approved fail-closed PII decision says.
//
// Credentials follow `chat_for_org`: only the static development pair works
// today. The production grant (two service-route rows on model-gateway and the
// Auth Core manifest rows) is a Control Plane and Model Plane item (DR-15.1),
// so the Dynamic path answers `LaneNotGranted` without minting anything.

/// The switch for the OCR lane. Off unless set to 1, true, on or yes.
pub const OCR_FLAG_ENV: &str = "QUARRY_EDGE__OCR";
/// The switch for the PII model tier. Off unless set to 1, true, on or yes.
pub const PII_MODEL_FLAG_ENV: &str = "QUARRY_EDGE__PII_MODEL";
/// The Model Plane route the OCR lane calls.
pub const OCR_ROUTE: &str = "/v1/ai/documents/layout";
/// The Model Plane route the PII model tier calls.
pub const PII_MODEL_ROUTE: &str = "/v1/ai/language/pii";
/// The most scanned pages one OCR call carries. Per-org page caps are WS6.
pub const MAX_OCR_PAGES_PER_CALL: usize = 20;
/// The largest PDF one OCR call carries (before base64).
pub const MAX_OCR_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
/// The most texts one PII model call carries.
pub const MAX_PII_MODEL_TEXTS: usize = 25;
/// The longest text one PII model item carries, in characters (the provider's
/// per-document limit for synchronous PII calls).
pub const MAX_PII_MODEL_TEXT_CHARS: usize = 5_120;
const DOCUMENT_LANE_TIMEOUT_SECS: u64 = 120;

/// Which document lanes this process may use. Both default off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentLaneSettings {
    pub ocr: bool,
    pub pii_model: bool,
}

impl DocumentLaneSettings {
    /// Both lanes from their raw flag values (absent = off).
    pub fn from_values(ocr: Option<&str>, pii_model: Option<&str>) -> Self {
        Self {
            ocr: crate::org_erasure::flag_on(ocr),
            pii_model: crate::org_erasure::flag_on(pii_model),
        }
    }

    /// Both lanes from `QUARRY_EDGE__OCR` and `QUARRY_EDGE__PII_MODEL`.
    pub fn from_env() -> Self {
        let ocr = std::env::var(OCR_FLAG_ENV).ok();
        let pii_model = std::env::var(PII_MODEL_FLAG_ENV).ok();
        Self::from_values(ocr.as_deref(), pii_model.as_deref())
    }
}

/// Which document lane a refusal or result belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentLane {
    Ocr,
    PiiModel,
}

impl DocumentLane {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentLane::Ocr => "ocr",
            DocumentLane::PiiModel => "pii_model",
        }
    }

    fn route(self) -> &'static str {
        match self {
            DocumentLane::Ocr => OCR_ROUTE,
            DocumentLane::PiiModel => PII_MODEL_ROUTE,
        }
    }
}

impl std::fmt::Display for DocumentLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a document lane call did not produce usable text. Every variant is a
/// failed page for the caller: none carries text and none means "use the
/// input as it is".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentLaneError {
    /// The lane's flag is off. Nothing was sent.
    #[error("the {0} lane is switched off")]
    Disabled(DocumentLane),
    /// The run is under zero data retention. Nothing was sent.
    #[error("the {0} lane never runs under zero data retention")]
    ZdrRefused(DocumentLane),
    /// The input is outside the lane's bounds (no pages, a page number of 0,
    /// too many pages, too many bytes, an empty or too long text). Nothing
    /// was sent.
    #[error("the {lane} input is outside its bounds: {reason}")]
    InvalidInput {
        lane: DocumentLane,
        reason: &'static str,
    },
    /// The hop failed (grant, status, transport, timeout or body).
    #[error("the {lane} lane hop failed: {hop}")]
    Hop {
        lane: DocumentLane,
        hop: ModelHopError,
    },
    /// A 2xx whose content cannot be trusted as the whole answer (a status
    /// other than succeeded, fewer pages than asked, empty text, a missing or
    /// duplicated item).
    #[error("the {lane} answer is not usable: {reason}")]
    Unusable {
        lane: DocumentLane,
        reason: &'static str,
    },
}

/// The body of `POST /v1/ai/documents/layout` for the scanned pages of one
/// PDF. Built only through [`OcrPagesRequest::for_scanned_pages`].
#[derive(Clone, Serialize)]
pub struct OcrPagesRequest {
    content_base64: String,
    content_type: String,
    /// 1-based, ascending, comma separated (`"2,5,7"`).
    pages: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
    #[serde(skip)]
    page_count: usize,
}

// The document rides in `content_base64`, so Debug names the size only.
impl std::fmt::Debug for OcrPagesRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrPagesRequest")
            .field("base64_len", &self.content_base64.len())
            .field("content_type", &self.content_type)
            .field("pages", &self.pages)
            .field("locale", &self.locale)
            .finish()
    }
}

impl OcrPagesRequest {
    /// The request for `pages` (1-based) of `pdf`. Duplicates are dropped and
    /// the pages sorted; an empty list, a page 0, more than
    /// [`MAX_OCR_PAGES_PER_CALL`] pages or a PDF over
    /// [`MAX_OCR_DOCUMENT_BYTES`] is refused.
    pub fn for_scanned_pages(
        pdf: &[u8],
        pages: &[u32],
        locale: Option<&str>,
    ) -> Result<Self, DocumentLaneError> {
        let invalid = |reason| DocumentLaneError::InvalidInput {
            lane: DocumentLane::Ocr,
            reason,
        };
        if pdf.is_empty() {
            return Err(invalid("empty document"));
        }
        if pdf.len() > MAX_OCR_DOCUMENT_BYTES {
            return Err(invalid("document too large"));
        }
        let mut sorted: Vec<u32> = pages.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.is_empty() {
            return Err(invalid("no pages"));
        }
        if sorted[0] == 0 {
            return Err(invalid("page numbers are 1-based"));
        }
        if sorted.len() > MAX_OCR_PAGES_PER_CALL {
            return Err(invalid("too many pages"));
        }
        use base64::Engine as _;
        let locale = locale
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Ok(Self {
            content_base64: base64::engine::general_purpose::STANDARD.encode(pdf),
            content_type: "application/pdf".to_owned(),
            pages: sorted
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
            locale,
            page_count: sorted.len(),
        })
    }

    /// How many distinct pages the request asks for.
    pub fn page_count(&self) -> usize {
        self.page_count
    }

    /// The pages field as sent.
    pub fn pages(&self) -> &str {
        &self.pages
    }
}

#[derive(Deserialize)]
struct OcrLayoutAnswer {
    #[serde(default)]
    status: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    pages_processed: u32,
    #[serde(default)]
    model_used: String,
    #[serde(default)]
    provider_used: String,
}

/// The text the OCR lane read from the scanned pages.
#[derive(Clone, PartialEq)]
pub struct OcrPagesResult {
    pub text: String,
    pub pages_processed: u32,
    pub model_used: String,
    pub provider_used: String,
}

impl std::fmt::Debug for OcrPagesResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrPagesResult")
            .field("chars", &self.text.chars().count())
            .field("pages_processed", &self.pages_processed)
            .field("model_used", &self.model_used)
            .field("provider_used", &self.provider_used)
            .finish()
    }
}

#[derive(Serialize)]
struct PiiModelBody<'a> {
    texts: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<&'a str>,
}

#[derive(Deserialize)]
struct PiiModelAnswer {
    #[serde(default)]
    results: Vec<PiiModelItem>,
    #[serde(default)]
    model_used: String,
    #[serde(default)]
    provider_used: String,
}

#[derive(Deserialize)]
struct PiiModelItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    redacted_text: Option<String>,
}

/// The PII model tier's redacted texts, in the order they were sent.
#[derive(Clone, PartialEq)]
pub struct PiiModelResult {
    pub redacted: Vec<String>,
    pub model_used: String,
    pub provider_used: String,
}

impl std::fmt::Debug for PiiModelResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PiiModelResult")
            .field("items", &self.redacted.len())
            .field("model_used", &self.model_used)
            .field("provider_used", &self.provider_used)
            .finish()
    }
}

/// The HTTP status of a failed hop as a [`ModelHopError`]. Shared by the chat
/// lane and the document lanes so one table maps every status.
fn hop_status_error(status: u16, headers: &reqwest::header::HeaderMap) -> ModelHopError {
    match status {
        401 => ModelHopError::Unauthorized,
        403 => ModelHopError::RouteDenied,
        412 => ModelHopError::ZdrRefused,
        429 => ModelHopError::RateLimited {
            retry_after_s: headers
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u32>().ok()),
        },
        400..=499 => ModelHopError::Rejected,
        _ => ModelHopError::Upstream { status },
    }
}

impl ModelPlaneClient {
    /// Which document lanes this client may use (default: none).
    pub fn with_document_lanes(mut self, settings: DocumentLaneSettings) -> Self {
        self.document_lanes = settings;
        self
    }

    /// The bound on one document lane call (default 120 s). Never retried
    /// after it fires.
    pub fn with_document_timeout(mut self, timeout: Duration) -> Self {
        self.document_timeout = timeout;
        self
    }

    pub fn document_lanes(&self) -> DocumentLaneSettings {
        self.document_lanes
    }

    /// OCR of the scanned pages of one PDF on behalf of `org_id`.
    ///
    /// Refused before anything is sent when `QUARRY_EDGE__OCR` is off or the
    /// run is under ZDR. The answer must say `succeeded` (or nothing), cover
    /// at least the pages asked for and carry text; anything else is
    /// [`DocumentLaneError::Unusable`].
    pub async fn ocr_pages_for_org(
        &self,
        org_id: &str,
        zdr: ZdrMode,
        req: &OcrPagesRequest,
    ) -> Result<OcrPagesResult, DocumentLaneError> {
        let lane = DocumentLane::Ocr;
        self.admit_document_lane(lane, zdr)?;
        let bytes = self
            .post_document_lane(org_id, lane, req)
            .await
            .map_err(|hop| DocumentLaneError::Hop { lane, hop })?;
        let answer: OcrLayoutAnswer =
            serde_json::from_slice(&bytes).map_err(|_| DocumentLaneError::Hop {
                lane,
                hop: ModelHopError::Decode,
            })?;
        let unusable = |reason| DocumentLaneError::Unusable { lane, reason };
        let status = answer.status.trim();
        if !status.is_empty() && !status.eq_ignore_ascii_case("succeeded") {
            return Err(unusable("status is not succeeded"));
        }
        if (answer.pages_processed as usize) < req.page_count() {
            return Err(unusable("fewer pages processed than asked"));
        }
        if answer.content.trim().is_empty() {
            return Err(unusable("no text"));
        }
        Ok(OcrPagesResult {
            text: answer.content,
            pages_processed: answer.pages_processed,
            model_used: answer.model_used,
            provider_used: answer.provider_used,
        })
    }

    /// The PII model tier over `texts` (already through the deterministic
    /// stage) on behalf of `org_id`. Returns one redacted text per input, in
    /// order.
    ///
    /// Refused before anything is sent when `QUARRY_EDGE__PII_MODEL` is off,
    /// the run is under ZDR, `texts` is empty or over
    /// [`MAX_PII_MODEL_TEXTS`], or a text is empty or over
    /// [`MAX_PII_MODEL_TEXT_CHARS`]. The gateway numbers items `"1"`..`"n"`;
    /// a missing, duplicated, unknown or empty item is
    /// [`DocumentLaneError::Unusable`], never the input passed through.
    pub async fn pii_model_for_org(
        &self,
        org_id: &str,
        zdr: ZdrMode,
        texts: &[String],
        language: Option<&str>,
    ) -> Result<PiiModelResult, DocumentLaneError> {
        let lane = DocumentLane::PiiModel;
        self.admit_document_lane(lane, zdr)?;
        let invalid = |reason| DocumentLaneError::InvalidInput { lane, reason };
        if texts.is_empty() {
            return Err(invalid("no texts"));
        }
        if texts.len() > MAX_PII_MODEL_TEXTS {
            return Err(invalid("too many texts"));
        }
        for text in texts {
            if text.trim().is_empty() {
                return Err(invalid("empty text"));
            }
            if text.chars().count() > MAX_PII_MODEL_TEXT_CHARS {
                return Err(invalid("text too long"));
            }
        }
        let body = PiiModelBody {
            texts,
            language: language.map(str::trim).filter(|value| !value.is_empty()),
        };
        let bytes = self
            .post_document_lane(org_id, lane, &body)
            .await
            .map_err(|hop| DocumentLaneError::Hop { lane, hop })?;
        let answer: PiiModelAnswer =
            serde_json::from_slice(&bytes).map_err(|_| DocumentLaneError::Hop {
                lane,
                hop: ModelHopError::Decode,
            })?;
        let unusable = |reason| DocumentLaneError::Unusable { lane, reason };
        if answer.results.len() != texts.len() {
            return Err(unusable("item count differs from the texts sent"));
        }
        let mut redacted: Vec<Option<String>> = vec![None; texts.len()];
        for item in answer.results {
            let index = item
                .id
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=texts.len()).contains(n))
                .ok_or_else(|| unusable("unknown item id"))?;
            let slot = &mut redacted[index - 1];
            if slot.is_some() {
                return Err(unusable("duplicated item id"));
            }
            let text = item
                .redacted_text
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| unusable("item has no redacted text"))?;
            *slot = Some(text);
        }
        let redacted = redacted
            .into_iter()
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| unusable("missing item"))?;
        Ok(PiiModelResult {
            redacted,
            model_used: answer.model_used,
            provider_used: answer.provider_used,
        })
    }

    /// The local refusals, in order: the flag, then ZDR. Nothing is sent and
    /// nothing is minted when either refuses.
    fn admit_document_lane(
        &self,
        lane: DocumentLane,
        zdr: ZdrMode,
    ) -> Result<(), DocumentLaneError> {
        let enabled = match lane {
            DocumentLane::Ocr => self.document_lanes.ocr,
            DocumentLane::PiiModel => self.document_lanes.pii_model,
        };
        if !enabled {
            return Err(DocumentLaneError::Disabled(lane));
        }
        if zdr.is_active() {
            return Err(DocumentLaneError::ZdrRefused(lane));
        }
        Ok(())
    }

    /// One POST on a document lane with the chat lane's retry rule: a connect
    /// error, 502 or 503 once; a timeout never (the provider may have charged).
    async fn post_document_lane<B: Serialize + ?Sized>(
        &self,
        org_id: &str,
        lane: DocumentLane,
        body: &B,
    ) -> Result<bytes::Bytes, ModelHopError> {
        // The org will pick the minted token once the Dynamic path exists.
        let _ = org_id;
        let (bearer, inference) = match &self.auth {
            ModelPlaneAuth::StaticDev(token) => (
                token.clone(),
                self.inference_token.clone().unwrap_or_else(|| token.clone()),
            ),
            ModelPlaneAuth::Dynamic(_) | ModelPlaneAuth::None => {
                return Err(ModelHopError::LaneNotGranted)
            }
        };
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), lane.route());
        let mut attempt = 0u8;
        loop {
            attempt += 1;
            let sent = self
                .http
                .post(&url)
                .timeout(self.document_timeout)
                .bearer_auth(&bearer)
                .header("x-inference-authorization", format!("Bearer {inference}"))
                .json(body)
                .send()
                .await;
            let resp = match sent {
                Ok(resp) => resp,
                Err(e) if e.is_timeout() => return Err(ModelHopError::Timeout),
                Err(e) if e.is_connect() && attempt == 1 => {
                    tokio::time::sleep(self.retry_backoff).await;
                    continue;
                }
                Err(_) => return Err(ModelHopError::Transport),
            };
            let status = resp.status().as_u16();
            if matches!(status, 502 | 503) && attempt == 1 {
                tokio::time::sleep(self.retry_backoff).await;
                continue;
            }
            if !resp.status().is_success() {
                return Err(hop_status_error(status, resp.headers()));
            }
            let bytes = match resp.bytes().await {
                Ok(bytes) => bytes,
                Err(e) if e.is_timeout() => return Err(ModelHopError::Timeout),
                Err(_) => return Err(ModelHopError::Decode),
            };
            if bytes.len() > MAX_CHAT_BODY_BYTES {
                return Err(ModelHopError::Decode);
            }
            return Ok(bytes);
        }
    }
}

/// LLM-driven planner that calls Model Plane gateway to decide next actions.
pub struct ModelPlanePlanner {
    client: Arc<ModelPlaneClient>,
    constraints: AgentConstraints,
    zdr: ZdrMode,
    session_key: Option<String>,
    lease_id: LeaseKind,
}

impl ModelPlanePlanner {
    pub fn new(client: Arc<ModelPlaneClient>, constraints: AgentConstraints) -> Self {
        Self {
            client,
            constraints,
            zdr: ZdrMode::Off,
            session_key: None,
            lease_id: Id::new(),
        }
    }

    pub fn with_session_key(mut self, key: impl Into<String>) -> Self {
        self.session_key = Some(key.into());
        self
    }

    pub fn with_zdr(mut self, zdr: ZdrMode) -> Self {
        self.zdr = zdr;
        self
    }

    pub fn with_lease_id(mut self, lease_id: LeaseKind) -> Self {
        self.lease_id = lease_id;
        self
    }

    fn build_prompt(observation: &BrowserObservation) -> String {
        let observation_json = serde_json::to_string_pretty(&json!({
            "url": observation.url,
            "title": observation.title,
            "step": observation.step,
            "snapshot": observation.snapshot.as_ref(),
            "interactive_elements": observation
                .dom_summary
                .as_ref()
                .map(|d| d.interactive_elements.iter().take(20).collect::<Vec<_>>())
                .unwrap_or_default(),
            "text_snippet": observation
                .dom_summary
                .as_ref()
                .and_then(|d| d.text_snippet.clone()),
            "policy_denials": observation.policy_denials,
        }))
        .unwrap_or_default();

        format!(
            "You are a browser-automation planner. Given the current browser \
             observation, decide the next 1-3 actions for the agent.\n\n\
             OBSERVATION:\n{observation_json}\n\n\
             Respond with ONLY a JSON object of the form:\n\
             {{\"done\": false, \"actions\": [<action>, ...]}}\n\n\
             Each <action> must match one of these shapes:\n\
             {{\"type\": \"navigate\", \"url\": \"...\"}}\n\
             {{\"type\": \"click_ref\", \"snapshot_id\": \"...\", \"generation\": 0, \"ref_id\": \"@e1\"}}\n\
             {{\"type\": \"click_semantic\", \"snapshot_id\": \"...\", \"generation\": 0, \"locator\": {{\"kind\": \"role\", \"role\": \"button\", \"name\": \"...\", \"exact\": true}}}}\n\
             {{\"type\": \"type_ref\", \"snapshot_id\": \"...\", \"generation\": 0, \"ref_id\": \"@e1\", \"text\": \"...\"}}\n\
             {{\"type\": \"type\", \"selector\": \"...\", \"text\": \"...\"}}\n\
             {{\"type\": \"press\", \"key\": \"Enter\"}}\n\
             {{\"type\": \"scroll\", \"target\": \"...\"}}\n\
             {{\"type\": \"select\", \"selector\": \"...\", \"value\": \"...\"}}\n\
             {{\"type\": \"wait\", \"ms\": 1000}}\n\
             {{\"type\": \"wait_for\", \"selector\": \"...\", \"timeout_ms\": 5000}}\n\
             {{\"type\": \"screenshot\", \"full_page\": false}}\n\
             {{\"type\": \"back\"}}\n\
             {{\"type\": \"get_content\"}}\n\n\
             When `snapshot` is present, prefer its `@e…` refs for any effectful\n\
             action. Echo its exact `snapshot_id` and `generation`; never invent\n\
             a ref and never reuse one after a new observation. Semantic locators\n\
             are allowed only when they identify exactly one snapshot target.\n\
             CSS selectors are legacy fallback only.\n\n\
             Set \"done\": true with empty actions when the agent goal is reached \
             or no further productive action is possible."
        )
    }

    fn parse_response(content: &str) -> QuarryResult<PlannerResponse> {
        let trimmed = strip_code_fences(content.trim());
        serde_json::from_str::<PlannerResponse>(trimmed).map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("planner response was not valid JSON: {e} — content: {trimmed}"),
            )
        })
    }
}

#[derive(Debug, Deserialize)]
struct PlannerResponse {
    #[serde(default)]
    done: bool,
    #[serde(default)]
    actions: Vec<AgentAction>,
}

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

#[async_trait]
impl Planner for ModelPlanePlanner {
    async fn next_actions(
        &self,
        observation: &BrowserObservation,
    ) -> QuarryResult<PlannerDecision> {
        let prompt = Self::build_prompt(observation);
        let req = ModelPlaneInvokeRequest {
            content: prompt,
            model: Some(self.client.default_model.clone()),
            session_key: self.session_key.clone(),
            thread_id: None,
        };

        let resp = self.client.invoke(&req).await?;
        let parsed = Self::parse_response(&resp.content)?;

        if parsed.done || parsed.actions.is_empty() {
            return Ok(PlannerDecision::Done);
        }

        let requests: Vec<AgentActionRequest> = parsed
            .actions
            .into_iter()
            .map(|action| AgentActionRequest {
                run_id: observation.run_id.clone(),
                lease_id: self.lease_id.clone(),
                action,
                instruction: None,
                constraints: self.constraints.clone(),
                zdr: self.zdr,
                extraction_profile: None,
            })
            .collect();

        Ok(PlannerDecision::Continue(requests))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_tokens::{ServiceBearer, ServiceTokenProvider, ServiceTokenRequest};
    use async_trait::async_trait;
    use chrono::Utc;
    use quarry_core::ids::kinds::RunKind;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_observation() -> BrowserObservation {
        let run_id: RunKind = Id::new();
        BrowserObservation {
            run_id,
            step: 0,
            url: "https://example.com".into(),
            title: Some("Example".into()),
            snapshot: None,
            dom_summary: None,
            screenshot_artifact_id: None,
            visual_observation_artifact_id: None,
            evidence_delta_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            egress_receipts: vec![],
            dialogs: vec![],
            policy_denials: vec![],
            action_outcome: quarry_core::contracts::ActionOutcome::default(),
            observation_delta: None,
            challenge: None,
            extraction_profile: None,
            extraction_result: None,
            proof_bundle: None,
            target_resolution: None,
            telemetry: quarry_core::contracts::BrowserTelemetry::default(),
            redaction: None,
            observed_at: Utc::now(),
        }
    }

    fn default_constraints() -> AgentConstraints {
        AgentConstraints {
            max_steps: 10,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        }
    }

    struct RotatingModelTokenProvider(AtomicUsize);

    #[async_trait]
    impl ServiceTokenProvider for RotatingModelTokenProvider {
        async fn token_for_org(
            &self,
            org_id: &str,
            _request: &ServiceTokenRequest,
            force_refresh: bool,
        ) -> QuarryResult<ServiceBearer> {
            assert_eq!(org_id, "org_verified");
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            assert_eq!(force_refresh, call > 0);
            Ok(ServiceBearer::from_test_token(if call == 0 {
                "first.model.token"
            } else {
                "fresh.model.token"
            }))
        }

        async fn invalidate(&self, _org_id: &str, _request: &ServiceTokenRequest) {}
    }

    #[derive(Clone)]
    struct ModelRejectThenAccept(Arc<AtomicUsize>);

    impl wiremock::Respond for ModelRejectThenAccept {
        fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
            let attempt = self.0.fetch_add(1, Ordering::SeqCst);
            let authorization = request
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap();
            if attempt == 0 {
                assert_eq!(authorization, "Bearer first.model.token");
                ResponseTemplate::new(401)
            } else {
                assert_eq!(authorization, "Bearer fresh.model.token");
                ResponseTemplate::new(200).set_body_json(json!({
                    "request_id": "req_refreshed",
                    "content": "ok",
                    "model_used": "test-model"
                }))
            }
        }
    }

    #[tokio::test]
    async fn dynamic_model_client_refreshes_once_after_401() {
        let server = MockServer::start().await;
        let requests = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ModelRejectThenAccept(requests.clone()))
            .expect(2)
            .mount(&server)
            .await;
        let tokens = Arc::new(RotatingModelTokenProvider(AtomicUsize::new(0)));
        let client = ModelPlaneClient::new(server.uri())
            .unwrap()
            .with_token_provider(tokens.clone());
        let request = ModelPlaneInvokeRequest {
            content: "hello".into(),
            model: None,
            session_key: None,
            thread_id: None,
        };

        let response = client
            .invoke_for_org("org_verified", &request)
            .await
            .unwrap();

        assert_eq!(response.request_id, "req_refreshed");
        assert_eq!(tokens.0.load(Ordering::SeqCst), 2);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn strip_fences_removes_json_block() {
        assert_eq!(strip_code_fences("```json\n{\"a\": 1}\n```"), "{\"a\": 1}");
        assert_eq!(strip_code_fences("```\n{\"a\": 1}\n```"), "{\"a\": 1}");
        assert_eq!(strip_code_fences("{\"a\": 1}"), "{\"a\": 1}");
    }

    #[test]
    fn parse_response_continue_with_navigate() {
        let json =
            r#"{"done": false, "actions": [{"type": "navigate", "url": "https://example.com"}]}"#;
        let parsed = ModelPlanePlanner::parse_response(json).unwrap();
        assert!(!parsed.done);
        assert_eq!(parsed.actions.len(), 1);
        match &parsed.actions[0] {
            AgentAction::Navigate { url } => assert_eq!(url, "https://example.com"),
            other => panic!("expected Navigate, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_done_no_actions() {
        let json = r#"{"done": true, "actions": []}"#;
        let parsed = ModelPlanePlanner::parse_response(json).unwrap();
        assert!(parsed.done);
        assert!(parsed.actions.is_empty());
    }

    #[test]
    fn parse_response_strips_fences() {
        let json = "```json\n{\"done\": true, \"actions\": []}\n```";
        let parsed = ModelPlanePlanner::parse_response(json).unwrap();
        assert!(parsed.done);
    }

    #[test]
    fn parse_response_rejects_garbage() {
        let result = ModelPlanePlanner::parse_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn build_prompt_contains_observation_url() {
        let obs = mock_observation();
        let prompt = ModelPlanePlanner::build_prompt(&obs);
        assert!(prompt.contains("https://example.com"));
        assert!(prompt.contains("\"done\""));
        assert!(prompt.contains("navigate"));
    }

    #[tokio::test]
    async fn planner_calls_gateway_and_continues() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req_test",
                "content": "{\"done\": false, \"actions\": [{\"type\": \"navigate\", \"url\": \"https://example.com\"}]}",
                "model_used": "claude-sonnet-4-6"
            })))
            .mount(&server)
            .await;

        let client = Arc::new(
            ModelPlaneClient::new(server.uri())
                .unwrap()
                .with_bearer_token("test-token"),
        );
        let planner = ModelPlanePlanner::new(client, default_constraints());
        let obs = mock_observation();

        let decision = planner.next_actions(&obs).await.unwrap();
        match decision {
            PlannerDecision::Continue(reqs) => {
                assert_eq!(reqs.len(), 1);
                match &reqs[0].action {
                    AgentAction::Navigate { url } => assert_eq!(url, "https://example.com"),
                    other => panic!("expected Navigate, got {other:?}"),
                }
            }
            PlannerDecision::Done => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn planner_returns_done_when_llm_signals_done() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req_test",
                "content": "{\"done\": true, \"actions\": []}",
                "model_used": "claude-sonnet-4-6"
            })))
            .mount(&server)
            .await;

        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let planner = ModelPlanePlanner::new(client, default_constraints());
        let obs = mock_observation();

        let decision = planner.next_actions(&obs).await.unwrap();
        assert!(matches!(decision, PlannerDecision::Done));
    }

    #[tokio::test]
    async fn planner_propagates_5xx_as_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server down"))
            .mount(&server)
            .await;

        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let planner = ModelPlanePlanner::new(client, default_constraints());
        let obs = mock_observation();

        let result = planner.next_actions(&obs).await;
        assert!(result.is_err());
    }

    // ---- W3-E3.6-7: OCR route and PII model tier on a fake gateway ---------

    const BOTH_LANES: DocumentLaneSettings = DocumentLaneSettings {
        ocr: true,
        pii_model: true,
    };
    const FAKE_PDF: &[u8] = b"%PDF-1.7 scanned fixture";

    fn lane_client(uri: String, settings: DocumentLaneSettings) -> ModelPlaneClient {
        ModelPlaneClient::new(uri)
            .unwrap()
            .with_bearer_token("dev-ingress")
            .with_inference_bearer_token("dev-inference")
            .with_document_lanes(settings)
            .with_retry_backoff(Duration::from_millis(1))
            .with_document_timeout(Duration::from_secs(5))
    }

    fn ocr_request(pages: &[u32]) -> OcrPagesRequest {
        OcrPagesRequest::for_scanned_pages(FAKE_PDF, pages, Some("nb-NO")).unwrap()
    }

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// A gateway that must not be called: `expect(0)` is checked on drop.
    async fn silent_gateway() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn document_lane_flags_default_off() {
        assert_eq!(DocumentLaneSettings::default(), DocumentLaneSettings {
            ocr: false,
            pii_model: false,
        });
        assert_eq!(
            DocumentLaneSettings::from_values(None, None),
            DocumentLaneSettings::default()
        );
        assert_eq!(
            DocumentLaneSettings::from_values(Some("0"), Some("off")),
            DocumentLaneSettings::default()
        );
        assert_eq!(
            DocumentLaneSettings::from_values(Some("1"), None),
            DocumentLaneSettings {
                ocr: true,
                pii_model: false,
            }
        );
        assert_eq!(
            DocumentLaneSettings::from_values(Some("off"), Some("true")),
            DocumentLaneSettings {
                ocr: false,
                pii_model: true,
            }
        );
        assert_eq!(OCR_FLAG_ENV, "QUARRY_EDGE__OCR");
        assert_eq!(PII_MODEL_FLAG_ENV, "QUARRY_EDGE__PII_MODEL");
        let client = ModelPlaneClient::new("http://127.0.0.1:9").unwrap();
        assert_eq!(client.document_lanes(), DocumentLaneSettings::default());
    }

    #[test]
    fn ocr_request_sorts_pages_and_refuses_out_of_bounds_input() {
        let req = OcrPagesRequest::for_scanned_pages(FAKE_PDF, &[7, 2, 5, 2], None).unwrap();
        assert_eq!(req.pages(), "2,5,7");
        assert_eq!(req.page_count(), 3);
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(body["content_type"], "application/pdf");
        assert_eq!(body["pages"], "2,5,7");
        assert!(body.get("locale").is_none());
        assert!(body.get("page_count").is_none());
        let debug = format!("{req:?}");
        assert!(!debug.contains(body["content_base64"].as_str().unwrap()));

        let invalid = |pdf: &[u8], pages: &[u32]| {
            match OcrPagesRequest::for_scanned_pages(pdf, pages, None) {
                Err(DocumentLaneError::InvalidInput { lane, reason }) => {
                    assert_eq!(lane, DocumentLane::Ocr);
                    reason
                }
                other => panic!("expected InvalidInput, got {other:?}"),
            }
        };
        assert_eq!(invalid(b"", &[1]), "empty document");
        assert_eq!(invalid(FAKE_PDF, &[]), "no pages");
        assert_eq!(invalid(FAKE_PDF, &[0, 1]), "page numbers are 1-based");
        let many: Vec<u32> = (1..=(MAX_OCR_PAGES_PER_CALL as u32 + 1)).collect();
        assert_eq!(invalid(FAKE_PDF, &many), "too many pages");
        let big = vec![0u8; MAX_OCR_DOCUMENT_BYTES + 1];
        assert_eq!(invalid(&big, &[1]), "document too large");
    }

    #[tokio::test]
    async fn ocr_lane_off_by_default_sends_nothing() {
        let server = silent_gateway().await;
        let client = lane_client(server.uri(), DocumentLaneSettings::default());
        let err = client
            .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[1]))
            .await
            .unwrap_err();
        assert_eq!(err, DocumentLaneError::Disabled(DocumentLane::Ocr));
    }

    #[tokio::test]
    async fn ocr_lane_refused_under_zdr_sends_nothing() {
        let server = silent_gateway().await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let err = client
            .ocr_pages_for_org("org-1", ZdrMode::On, &ocr_request(&[1]))
            .await
            .unwrap_err();
        assert_eq!(err, DocumentLaneError::ZdrRefused(DocumentLane::Ocr));
    }

    #[tokio::test]
    async fn ocr_lane_without_a_credential_is_not_granted() {
        let server = silent_gateway().await;
        let client = ModelPlaneClient::new(server.uri())
            .unwrap()
            .with_document_lanes(BOTH_LANES);
        let err = client
            .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[1]))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            DocumentLaneError::Hop {
                lane: DocumentLane::Ocr,
                hop: ModelHopError::LaneNotGranted,
            }
        );
    }

    #[tokio::test]
    async fn ocr_lane_sends_only_the_scanned_pages_and_reads_the_text() {
        use wiremock::matchers::{body_partial_json, header};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OCR_ROUTE))
            .and(header("authorization", "Bearer dev-ingress"))
            .and(header("x-inference-authorization", "Bearer dev-inference"))
            .and(body_partial_json(json!({
                "content_type": "application/pdf",
                "pages": "2,5",
                "locale": "nb-NO",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "01J",
                "object": "document.analysis",
                "status": "succeeded",
                "content": "Skannet side to\nSkannet side fem",
                "pages_processed": 2,
                "model_used": "prebuilt-layout",
                "provider_used": "azure",
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let result = client
            .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[5, 2]))
            .await
            .unwrap();
        assert_eq!(result.text, "Skannet side to\nSkannet side fem");
        assert_eq!(result.pages_processed, 2);
        assert_eq!(result.model_used, "prebuilt-layout");
        assert_eq!(result.provider_used, "azure");
        assert!(!format!("{result:?}").contains("Skannet"));
    }

    #[tokio::test]
    async fn ocr_lane_fails_closed_on_an_incomplete_answer() {
        let cases = [
            (
                json!({ "status": "succeeded", "content": "one page", "pages_processed": 1 }),
                "fewer pages processed than asked",
            ),
            (
                json!({ "status": "succeeded", "content": "  ", "pages_processed": 2 }),
                "no text",
            ),
            (
                json!({ "status": "failed", "content": "partial", "pages_processed": 2 }),
                "status is not succeeded",
            ),
        ];
        for (body, want) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(OCR_ROUTE))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            let client = lane_client(server.uri(), BOTH_LANES);
            let err = client
                .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[1, 2]))
                .await
                .unwrap_err();
            assert_eq!(
                err,
                DocumentLaneError::Unusable {
                    lane: DocumentLane::Ocr,
                    reason: want,
                }
            );
        }
    }

    #[tokio::test]
    async fn ocr_lane_maps_statuses_and_retries_a_503_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OCR_ROUTE))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let err = client
            .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[1]))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            DocumentLaneError::Hop {
                lane: DocumentLane::Ocr,
                hop: ModelHopError::RouteDenied,
            }
        );

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OCR_ROUTE))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&server)
            .await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let err = client
            .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[1]))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            DocumentLaneError::Hop {
                lane: DocumentLane::Ocr,
                hop: ModelHopError::Upstream { status: 503 },
            }
        );

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OCR_ROUTE))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let err = client
            .ocr_pages_for_org("org-1", ZdrMode::Off, &ocr_request(&[1]))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            DocumentLaneError::Hop {
                lane: DocumentLane::Ocr,
                hop: ModelHopError::Decode,
            }
        );
    }

    #[tokio::test]
    async fn pii_model_lane_off_or_zdr_or_bad_input_sends_nothing() {
        let server = silent_gateway().await;
        let off = lane_client(server.uri(), DocumentLaneSettings::default());
        let on = lane_client(server.uri(), BOTH_LANES);
        let lane = DocumentLane::PiiModel;
        assert_eq!(
            off.pii_model_for_org("org-1", ZdrMode::Off, &texts(&["a"]), None)
                .await
                .unwrap_err(),
            DocumentLaneError::Disabled(lane)
        );
        assert_eq!(
            on.pii_model_for_org("org-1", ZdrMode::On, &texts(&["a"]), None)
                .await
                .unwrap_err(),
            DocumentLaneError::ZdrRefused(lane)
        );
        let invalid = |reason| DocumentLaneError::InvalidInput { lane, reason };
        assert_eq!(
            on.pii_model_for_org("org-1", ZdrMode::Off, &[], None)
                .await
                .unwrap_err(),
            invalid("no texts")
        );
        assert_eq!(
            on.pii_model_for_org("org-1", ZdrMode::Off, &texts(&["ok", " "]), None)
                .await
                .unwrap_err(),
            invalid("empty text")
        );
        let long = vec!["x".repeat(MAX_PII_MODEL_TEXT_CHARS + 1)];
        assert_eq!(
            on.pii_model_for_org("org-1", ZdrMode::Off, &long, None)
                .await
                .unwrap_err(),
            invalid("text too long")
        );
        let many = vec!["x".to_owned(); MAX_PII_MODEL_TEXTS + 1];
        assert_eq!(
            on.pii_model_for_org("org-1", ZdrMode::Off, &many, None)
                .await
                .unwrap_err(),
            invalid("too many texts")
        );
    }

    #[tokio::test]
    async fn pii_model_lane_returns_redacted_texts_in_input_order() {
        use wiremock::matchers::body_partial_json;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PII_MODEL_ROUTE))
            .and(body_partial_json(json!({
                "texts": ["Ring Kari", "Mail Ola"],
                "language": "no",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "01J",
                "object": "language.pii",
                "results": [
                    { "id": "2", "redacted_text": "Mail ***", "entities": [] },
                    { "id": "1", "redacted_text": "Ring ****", "entities": [] },
                ],
                "model_used": "pii-2025",
                "provider_used": "azure",
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let result = client
            .pii_model_for_org(
                "org-1",
                ZdrMode::Off,
                &texts(&["Ring Kari", "Mail Ola"]),
                Some("no"),
            )
            .await
            .unwrap();
        assert_eq!(result.redacted, texts(&["Ring ****", "Mail ***"]));
        assert_eq!(result.model_used, "pii-2025");
        assert_eq!(result.provider_used, "azure");
    }

    #[tokio::test]
    async fn pii_model_lane_fails_closed_on_a_partial_answer() {
        let cases = [
            (
                json!({ "results": [ { "id": "1", "redacted_text": "a" } ] }),
                "item count differs from the texts sent",
            ),
            (
                json!({ "results": [
                    { "id": "1", "redacted_text": "a" },
                    { "id": "3", "redacted_text": "b" },
                ] }),
                "unknown item id",
            ),
            (
                json!({ "results": [
                    { "id": "1", "redacted_text": "a" },
                    { "id": "1", "redacted_text": "b" },
                ] }),
                "duplicated item id",
            ),
            (
                json!({ "results": [
                    { "id": "1", "redacted_text": "a" },
                    { "id": "2", "redacted_text": "" },
                ] }),
                "item has no redacted text",
            ),
            (
                json!({ "results": [
                    { "id": "1", "redacted_text": "a" },
                    { "id": "2" },
                ] }),
                "item has no redacted text",
            ),
        ];
        for (body, want) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(PII_MODEL_ROUTE))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            let client = lane_client(server.uri(), BOTH_LANES);
            let err = client
                .pii_model_for_org("org-1", ZdrMode::Off, &texts(&["one", "two"]), None)
                .await
                .unwrap_err();
            assert_eq!(
                err,
                DocumentLaneError::Unusable {
                    lane: DocumentLane::PiiModel,
                    reason: want,
                }
            );
        }
    }

    #[tokio::test]
    async fn pii_model_lane_maps_a_gateway_zdr_refusal() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PII_MODEL_ROUTE))
            .respond_with(ResponseTemplate::new(412))
            .mount(&server)
            .await;
        let client = lane_client(server.uri(), BOTH_LANES);
        let err = client
            .pii_model_for_org("org-1", ZdrMode::Off, &texts(&["one"]), None)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            DocumentLaneError::Hop {
                lane: DocumentLane::PiiModel,
                hop: ModelHopError::ZdrRefused,
            }
        );
    }
}
