//! Speech provider routing: text-to-speech synthesis and speech-to-text transcription.

#![allow(dead_code)] // placeholder provider + DTO fields wired up incrementally

use std::sync::Arc;

use reqwest::multipart;
use serde::Deserialize;
use tracing::{info, warn};

use super::{ArtifactRef, ModelInfo, ProviderError};

const DEFAULT_OPENAI_SPEECH_BASE: &str = "https://api.openai.com";
const DEFAULT_OPENAI_TTS_MODEL: &str = "tts-1";
const DEFAULT_OPENAI_STT_MODEL: &str = "whisper-1";
const DEFAULT_OPENAI_VOICE: &str = "alloy";
const DEFAULT_AZURE_OPENAI_API_VERSION: &str = "2024-08-01-preview";
const DEFAULT_AZURE_TTS_DEPLOYMENT: &str = "tts";
const DEFAULT_AZURE_STT_DEPLOYMENT: &str = "whisper";
const DEFAULT_AZURE_SPEECH_VOICE: &str = "en-US-JennyNeural";
const REQUEST_TIMEOUT_SECS: u64 = 60;

/// Supported audio output formats for speech synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Ogg,
    Pcm,
}

impl AudioFormat {
    /// Parse from the wire string used by clients.
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "" | "mp3" => Self::Mp3,
            "wav" => Self::Wav,
            "ogg" | "opus" => Self::Ogg,
            "pcm" => Self::Pcm,
            _ => Self::Mp3,
        }
    }

    /// String for use as OpenAI's `response_format` field.
    fn openai_format(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Ogg => "opus",
            Self::Pcm => "pcm",
        }
    }

    /// String for Azure Speech's `X-Microsoft-OutputFormat` header.
    fn azure_speech_format(self) -> &'static str {
        match self {
            Self::Wav => "riff-24khz-16bit-mono-pcm",
            Self::Mp3 => "audio-24khz-96kbitrate-mono-mp3",
            Self::Ogg => "ogg-24khz-16bit-mono-opus",
            Self::Pcm => "raw-24khz-16bit-mono-pcm",
        }
    }

    /// Wire-format label for client consumption.
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Pcm => "pcm",
        }
    }
}

/// Internal TTS request.
#[derive(Debug, Clone)]
pub struct SpeechSynthesisRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub text: String,
    pub voice: String,
    pub format: AudioFormat,
    pub model: String,
    pub language: String,
}

/// Internal STT request.
#[derive(Debug, Clone)]
pub struct SpeechTranscriptionRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub audio_bytes: Vec<u8>,
    pub format: String,
    pub model: String,
    pub language: String,
}

/// Result of a speech synthesis request.
#[derive(Debug, Clone)]
pub struct AudioResult {
    pub audio_bytes: Vec<u8>,
    pub format: AudioFormat,
    pub duration_ms: u64,
    pub artifact_ref: Option<ArtifactRef>,
    pub model_used: String,
    pub provider_used: String,
}

/// Result of a speech transcription request.
#[derive(Debug, Clone)]
pub struct TranscriptResult {
    pub text: String,
    pub language: String,
    pub confidence: f32,
    pub model_used: String,
    pub provider_used: String,
}

/// Provider voice metadata surfaced to clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub language: String,
    pub gender: String,
    pub provider: String,
}

/// Trait for speech providers that support synthesis and/or transcription.
#[async_trait::async_trait]
pub trait SpeechProvider: Send + Sync {
    /// Stable provider identifier.
    fn provider_name(&self) -> &'static str;

    /// Synthesize speech from text.
    async fn synthesize(&self, req: &SpeechSynthesisRequest) -> Result<AudioResult, ProviderError>;

    /// Transcribe audio bytes to text.
    async fn transcribe(
        &self,
        req: &SpeechTranscriptionRequest,
    ) -> Result<TranscriptResult, ProviderError>;

    /// Models/deployments known at startup.
    fn list_models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    /// TTS voices known at startup.
    fn list_voices(&self) -> Vec<VoiceInfo> {
        Vec::new()
    }
}

/// Sequential speech fallback chain with provider-hint routing.
#[derive(Clone, Default)]
pub struct SpeechChain {
    providers: Vec<(String, Arc<dyn SpeechProvider>)>,
}

impl SpeechChain {
    /// Build from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, Arc<dyn SpeechProvider>)> = Vec::new();

        if let Some(provider) = AzureSpeechProvider::from_env() {
            providers.push(("azure-speech".to_owned(), Arc::new(provider)));
            info!(provider = "azure-speech", "speech provider registered");
        }

        if let Some(provider) = OpenAiSpeechProvider::from_azure_env() {
            providers.push(("azure-openai".to_owned(), Arc::new(provider)));
            info!(provider = "azure-openai", "speech provider registered");
        }

        if let Some(provider) = OpenAiSpeechProvider::from_openai_env() {
            providers.push(("openai".to_owned(), Arc::new(provider)));
            info!(provider = "openai", "speech provider registered");
        }

        Self { providers }
    }

    /// Create a chain for testing with explicit providers.
    #[allow(dead_code)]
    #[must_use]
    pub fn new_with_providers(providers: Vec<(String, Arc<dyn SpeechProvider>)>) -> Self {
        Self { providers }
    }

    /// Number of registered providers.
    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    fn provider_matches(name: &str, hint: &str) -> bool {
        let hint = hint.trim().to_ascii_lowercase();
        hint.is_empty()
            || hint == name
            || (hint == "azure" && (name == "azure-openai" || name == "azure-speech"))
            || (hint == "openai" && name == "azure-openai")
    }

    /// Synthesize speech with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every matching provider fails.
    pub async fn synthesize(
        &self,
        req: &SpeechSynthesisRequest,
    ) -> Result<AudioResult, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.synthesize(req).await {
                Ok(result) => {
                    info!(provider = %name, request_id = %req.request_id, "speech synthesis succeeded");
                    return Ok(result);
                }
                Err(ProviderError::UnsupportedModel(message)) => {
                    warn!(provider = %name, error = %message, "provider does not support speech synthesis request");
                }
                Err(err) => {
                    warn!(provider = %name, error = %err, "speech synthesis provider failed");
                }
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Transcribe speech with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every matching provider fails.
    pub async fn transcribe(
        &self,
        req: &SpeechTranscriptionRequest,
    ) -> Result<TranscriptResult, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.transcribe(req).await {
                Ok(result) => {
                    info!(provider = %name, request_id = %req.request_id, "speech transcription succeeded");
                    return Ok(result);
                }
                Err(ProviderError::UnsupportedModel(message)) => {
                    warn!(provider = %name, error = %message, "provider does not support speech transcription request");
                }
                Err(err) => {
                    warn!(provider = %name, error = %err, "speech transcription provider failed");
                }
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Return speech models from matching providers.
    #[must_use]
    pub fn list_models(&self, modality: &str, provider: &str) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .filter(|(name, _)| Self::provider_matches(name, provider))
            .flat_map(|(_, provider)| provider.list_models())
            .filter(|model| {
                modality.is_empty()
                    || model.modality == modality
                    || (modality == "speech" && matches!(model.modality.as_str(), "tts" | "stt"))
            })
            .collect()
    }

    /// Return voices from matching providers.
    #[must_use]
    pub fn list_voices(&self, provider: &str, language: &str) -> Vec<VoiceInfo> {
        self.providers
            .iter()
            .filter(|(name, _)| Self::provider_matches(name, provider))
            .flat_map(|(_, provider)| provider.list_voices())
            .filter(|voice| {
                language.is_empty()
                    || voice.language == "*"
                    || voice
                        .language
                        .to_ascii_lowercase()
                        .starts_with(&language.to_ascii_lowercase())
            })
            .collect()
    }
}

/// Placeholder speech provider that returns errors until connected to an upstream service.
#[derive(Debug, Clone)]
pub struct NoopSpeechProvider;

impl NoopSpeechProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopSpeechProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SpeechProvider for NoopSpeechProvider {
    fn provider_name(&self) -> &'static str {
        "noop"
    }

    async fn synthesize(
        &self,
        _req: &SpeechSynthesisRequest,
    ) -> Result<AudioResult, ProviderError> {
        Err(ProviderError::Unavailable(
            "speech provider not connected to upstream".to_owned(),
        ))
    }

    async fn transcribe(
        &self,
        _req: &SpeechTranscriptionRequest,
    ) -> Result<TranscriptResult, ProviderError> {
        Err(ProviderError::Unavailable(
            "speech provider not connected to upstream".to_owned(),
        ))
    }
}

#[derive(Debug, Clone)]
struct AzureOpenAiOp {
    endpoint: String,
    api_key: String,
    api_version: String,
    deployment: String,
}

#[derive(Debug, Clone)]
enum OpenAiSpeechFlavor {
    OpenAi {
        api_key: String,
        base: String,
    },
    AzureOpenAi {
        tts: AzureOpenAiOp,
        stt: AzureOpenAiOp,
    },
}

/// OpenAI-compatible speech provider.
#[derive(Debug, Clone)]
pub struct OpenAiSpeechProvider {
    flavor: OpenAiSpeechFlavor,
    tts_model: String,
    stt_model: String,
    http: reqwest::Client,
}

#[derive(Deserialize, Default)]
struct OpenAiTranscriptionResponse {
    #[serde(default)]
    text: String,
    #[serde(default)]
    language: String,
}

impl OpenAiSpeechProvider {
    /// Construct a public OpenAI-compatible provider from explicit credentials.
    ///
    /// `base` may be either `https://api.openai.com` or
    /// `https://api.openai.com/v1`; it is normalized internally.
    #[must_use]
    pub fn new(api_key: String, base: String, tts_model: String, stt_model: String) -> Self {
        Self {
            flavor: OpenAiSpeechFlavor::OpenAi {
                api_key,
                base: normalize_openai_speech_base(&base),
            },
            tts_model: nonempty_or(tts_model, DEFAULT_OPENAI_TTS_MODEL),
            stt_model: nonempty_or(stt_model, DEFAULT_OPENAI_STT_MODEL),
            http: speech_http_client(),
        }
    }

    /// Build a public OpenAI-compatible provider from env.
    #[must_use]
    pub fn from_openai_env() -> Option<Self> {
        let key = env_nonempty("OPENAI_API_KEY")?;
        let base = env_nonempty("OPENAI_BASE_URL")
            .or_else(|| env_nonempty("OPENAI_API_BASE"))
            .unwrap_or_default();
        Some(Self::new(
            key,
            base,
            env_nonempty("OPENAI_TTS_MODEL").unwrap_or_default(),
            env_nonempty("OPENAI_STT_MODEL").unwrap_or_default(),
        ))
    }

    /// Build an Azure OpenAI audio provider from env.
    #[must_use]
    pub fn from_azure_env() -> Option<Self> {
        let endpoint_default = env_nonempty("AZURE_OPENAI_ENDPOINT")?;
        let key_default = env_nonempty("AZURE_OPENAI_API_KEY")?;
        let api_version_default = env_nonempty("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|| DEFAULT_AZURE_OPENAI_API_VERSION.to_owned());

        let tts = AzureOpenAiOp {
            endpoint: env_nonempty("AZURE_OPENAI_TTS_ENDPOINT")
                .unwrap_or_else(|| endpoint_default.clone())
                .trim_end_matches('/')
                .to_owned(),
            api_key: env_nonempty("AZURE_OPENAI_TTS_API_KEY")
                .unwrap_or_else(|| key_default.clone()),
            api_version: env_nonempty("AZURE_OPENAI_TTS_API_VERSION")
                .unwrap_or_else(|| api_version_default.clone()),
            deployment: env_nonempty("AZURE_OPENAI_TTS_DEPLOYMENT")
                .unwrap_or_else(|| DEFAULT_AZURE_TTS_DEPLOYMENT.to_owned()),
        };
        let stt = AzureOpenAiOp {
            endpoint: env_nonempty("AZURE_OPENAI_STT_ENDPOINT")
                .unwrap_or(endpoint_default)
                .trim_end_matches('/')
                .to_owned(),
            api_key: env_nonempty("AZURE_OPENAI_STT_API_KEY").unwrap_or(key_default),
            api_version: env_nonempty("AZURE_OPENAI_STT_API_VERSION")
                .unwrap_or(api_version_default),
            deployment: env_nonempty("AZURE_OPENAI_STT_DEPLOYMENT")
                .unwrap_or_else(|| DEFAULT_AZURE_STT_DEPLOYMENT.to_owned()),
        };

        Some(Self {
            flavor: OpenAiSpeechFlavor::AzureOpenAi { tts, stt },
            tts_model: DEFAULT_OPENAI_TTS_MODEL.to_owned(),
            stt_model: DEFAULT_OPENAI_STT_MODEL.to_owned(),
            http: speech_http_client(),
        })
    }

    fn provider_name_str(&self) -> &'static str {
        match self.flavor {
            OpenAiSpeechFlavor::OpenAi { .. } => "openai",
            OpenAiSpeechFlavor::AzureOpenAi { .. } => "azure-openai",
        }
    }

    fn tts_model_for(&self, req: &SpeechSynthesisRequest) -> String {
        if !req.model.trim().is_empty() {
            return req.model.clone();
        }
        match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { .. } => self.tts_model.clone(),
            OpenAiSpeechFlavor::AzureOpenAi { tts, .. } => tts.deployment.clone(),
        }
    }

    fn stt_model_for(&self, req: &SpeechTranscriptionRequest) -> String {
        if !req.model.trim().is_empty() {
            return req.model.clone();
        }
        match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { .. } => self.stt_model.clone(),
            OpenAiSpeechFlavor::AzureOpenAi { stt, .. } => stt.deployment.clone(),
        }
    }

    fn tts_endpoint(&self, model: &str) -> String {
        match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { base, .. } => format!("{base}/v1/audio/speech"),
            OpenAiSpeechFlavor::AzureOpenAi { tts, .. } => format!(
                "{}/openai/deployments/{}/audio/speech?api-version={}",
                tts.endpoint, model, tts.api_version
            ),
        }
    }

    fn stt_endpoint(&self, model: &str) -> String {
        match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { base, .. } => {
                format!("{base}/v1/audio/transcriptions")
            }
            OpenAiSpeechFlavor::AzureOpenAi { stt, .. } => format!(
                "{}/openai/deployments/{}/audio/transcriptions?api-version={}",
                stt.endpoint, model, stt.api_version
            ),
        }
    }

    fn apply_tts_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { api_key, .. } => request.bearer_auth(api_key),
            OpenAiSpeechFlavor::AzureOpenAi { tts, .. } => request.header("api-key", &tts.api_key),
        }
    }

    fn apply_stt_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { api_key, .. } => request.bearer_auth(api_key),
            OpenAiSpeechFlavor::AzureOpenAi { stt, .. } => request.header("api-key", &stt.api_key),
        }
    }
}

#[async_trait::async_trait]
impl SpeechProvider for OpenAiSpeechProvider {
    fn provider_name(&self) -> &'static str {
        self.provider_name_str()
    }

    async fn synthesize(&self, req: &SpeechSynthesisRequest) -> Result<AudioResult, ProviderError> {
        if req.text.trim().is_empty() {
            return Err(ProviderError::InvalidResponse("text is required".into()));
        }
        let voice_name = if req.voice.trim().is_empty() {
            DEFAULT_OPENAI_VOICE
        } else {
            req.voice.as_str()
        };
        let model = self.tts_model_for(req);
        let mut body = serde_json::json!({
            "model": model,
            "voice": voice_name,
            "input": req.text,
            "response_format": req.format.openai_format(),
        });
        if matches!(self.flavor, OpenAiSpeechFlavor::AzureOpenAi { .. }) {
            body["model"] = serde_json::Value::String(model.clone());
        }

        let resp = self
            .apply_tts_auth(self.http.post(self.tts_endpoint(&model)).json(&body))
            .send()
            .await
            .map_err(|e| ProviderError::Http(format!("{} tts: {e}", self.provider_name())))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "{} tts HTTP {status}: {}",
                self.provider_name(),
                truncate(&body, 300)
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Http(format!("{} tts body: {e}", self.provider_name())))?
            .to_vec();
        Ok(AudioResult {
            audio_bytes: bytes,
            format: req.format,
            duration_ms: 0,
            artifact_ref: None,
            model_used: model,
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn transcribe(
        &self,
        req: &SpeechTranscriptionRequest,
    ) -> Result<TranscriptResult, ProviderError> {
        if req.audio_bytes.is_empty() {
            return Err(ProviderError::InvalidResponse("audio is required".into()));
        }
        let model = self.stt_model_for(req);
        let part = audio_part(req.audio_bytes.clone(), &req.format)?;
        let mut form = multipart::Form::new().part("file", part);
        if matches!(self.flavor, OpenAiSpeechFlavor::OpenAi { .. }) {
            form = form.text("model", model.clone());
        }
        if !req.language.trim().is_empty() {
            form = form.text("language", req.language.clone());
        }

        let resp = self
            .apply_stt_auth(self.http.post(self.stt_endpoint(&model)).multipart(form))
            .send()
            .await
            .map_err(|e| ProviderError::Http(format!("{} stt: {e}", self.provider_name())))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "{} stt HTTP {status}: {}",
                self.provider_name(),
                truncate(&body, 300)
            )));
        }
        let parsed: OpenAiTranscriptionResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Http(format!("{} stt json: {e}", self.provider_name())))?;
        let has_text = !parsed.text.is_empty();
        Ok(TranscriptResult {
            text: parsed.text,
            language: if parsed.language.is_empty() {
                req.language.clone()
            } else {
                parsed.language
            },
            confidence: if has_text { 1.0 } else { 0.0 },
            model_used: model,
            provider_used: self.provider_name().to_owned(),
        })
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let provider = self.provider_name().to_owned();
        let (tts_model, stt_model) = match &self.flavor {
            OpenAiSpeechFlavor::OpenAi { .. } => (self.tts_model.clone(), self.stt_model.clone()),
            OpenAiSpeechFlavor::AzureOpenAi { tts, stt } => {
                (tts.deployment.clone(), stt.deployment.clone())
            }
        };
        vec![
            ModelInfo {
                id: tts_model,
                provider: provider.clone(),
                modality: "tts".to_owned(),
                streaming: false,
            },
            ModelInfo {
                id: stt_model,
                provider,
                modality: "stt".to_owned(),
                streaming: false,
            },
        ]
    }

    fn list_voices(&self) -> Vec<VoiceInfo> {
        openai_voices(self.provider_name())
    }
}

/// Azure Speech Service provider. It currently owns neural TTS; STT falls
/// through to Azure OpenAI/OpenAI providers in the chain.
#[derive(Debug, Clone)]
pub struct AzureSpeechProvider {
    endpoint: String,
    api_key: String,
    default_voice: String,
    http: reqwest::Client,
}

impl AzureSpeechProvider {
    /// Build from `AZURE_SPEECH_ENDPOINT` or `AZURE_SPEECH_REGION` plus key.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_SPEECH_ENDPOINT")
            .map(|value| value.trim_end_matches('/').to_owned())
            .or_else(|| {
                env_nonempty("AZURE_SPEECH_REGION").map(|region| {
                    format!(
                        "https://{}.tts.speech.microsoft.com",
                        region.trim().trim_end_matches('/')
                    )
                })
            })?;
        let api_key =
            env_nonempty("AZURE_SPEECH_KEY").or_else(|| env_nonempty("AZURE_OPENAI_API_KEY"))?;
        Some(Self {
            endpoint,
            api_key,
            default_voice: env_nonempty("AZURE_SPEECH_DEFAULT_VOICE")
                .unwrap_or_else(|| DEFAULT_AZURE_SPEECH_VOICE.to_owned()),
            http: speech_http_client(),
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/cognitiveservices/v1", self.endpoint)
    }
}

#[async_trait::async_trait]
impl SpeechProvider for AzureSpeechProvider {
    fn provider_name(&self) -> &'static str {
        "azure-speech"
    }

    async fn synthesize(&self, req: &SpeechSynthesisRequest) -> Result<AudioResult, ProviderError> {
        if req.text.trim().is_empty() {
            return Err(ProviderError::InvalidResponse("text is required".into()));
        }
        let voice = if req.voice.trim().is_empty() {
            self.default_voice.as_str()
        } else {
            req.voice.as_str()
        };
        let language = if req.language.trim().is_empty() {
            language_from_voice(voice)
        } else {
            req.language.clone()
        };
        let ssml = format!(
            r#"<speak version="1.0" xml:lang="{}"><voice name="{}">{}</voice></speak>"#,
            escape_xml(&language),
            escape_xml(voice),
            escape_xml(&req.text)
        );

        let resp = self
            .http
            .post(self.endpoint())
            .header("Ocp-Apim-Subscription-Key", &self.api_key)
            .header("Content-Type", "application/ssml+xml")
            .header("X-Microsoft-OutputFormat", req.format.azure_speech_format())
            .body(ssml)
            .send()
            .await
            .map_err(|e| ProviderError::Http(format!("azure speech tts: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "azure speech tts HTTP {status}: {}",
                truncate(&body, 300)
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Http(format!("azure speech tts body: {e}")))?
            .to_vec();
        Ok(AudioResult {
            audio_bytes: bytes,
            format: req.format,
            duration_ms: 0,
            artifact_ref: None,
            model_used: voice.to_owned(),
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn transcribe(
        &self,
        _req: &SpeechTranscriptionRequest,
    ) -> Result<TranscriptResult, ProviderError> {
        Err(ProviderError::UnsupportedModel(
            "azure-speech:stt".to_owned(),
        ))
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "azure-neural-tts".to_owned(),
            provider: self.provider_name().to_owned(),
            modality: "tts".to_owned(),
            streaming: false,
        }]
    }

    fn list_voices(&self) -> Vec<VoiceInfo> {
        azure_voices()
    }
}

fn speech_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn nonempty_or(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

fn normalize_openai_speech_base(value: &str) -> String {
    let base = if value.trim().is_empty() {
        DEFAULT_OPENAI_SPEECH_BASE
    } else {
        value.trim().trim_end_matches('/')
    };
    base.strip_suffix("/v1").unwrap_or(base).to_owned()
}

fn audio_part(audio: Vec<u8>, format: &str) -> Result<multipart::Part, ProviderError> {
    let (filename, mime) = match format.to_ascii_lowercase().as_str() {
        "wav" => ("audio.wav", "audio/wav"),
        "ogg" | "opus" => ("audio.ogg", "audio/ogg"),
        "m4a" => ("audio.m4a", "audio/m4a"),
        "webm" => ("audio.webm", "audio/webm"),
        _ => ("audio.mp3", "audio/mpeg"),
    };
    multipart::Part::bytes(audio)
        .file_name(filename)
        .mime_str(mime)
        .map_err(|e| ProviderError::InvalidResponse(format!("mime: {e}")))
}

fn language_from_voice(voice: &str) -> String {
    let mut parts = voice.split('-').take(2);
    match (parts.next(), parts.next()) {
        (Some(language), Some(region)) => format!("{language}-{region}"),
        _ => "en-US".to_owned(),
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn openai_voices(provider: &str) -> Vec<VoiceInfo> {
    [
        ("alloy", "Alloy", "*", "Neutral"),
        ("ash", "Ash", "*", "Neutral"),
        ("ballad", "Ballad", "*", "Neutral"),
        ("coral", "Coral", "*", "Neutral"),
        ("echo", "Echo", "*", "Male"),
        ("fable", "Fable", "*", "Neutral"),
        ("nova", "Nova", "*", "Female"),
        ("onyx", "Onyx", "*", "Male"),
        ("sage", "Sage", "*", "Neutral"),
        ("shimmer", "Shimmer", "*", "Female"),
    ]
    .into_iter()
    .map(|(id, name, language, gender)| VoiceInfo {
        id: id.to_owned(),
        name: name.to_owned(),
        language: language.to_owned(),
        gender: gender.to_owned(),
        provider: provider.to_owned(),
    })
    .collect()
}

fn azure_voices() -> Vec<VoiceInfo> {
    [
        ("en-US-JennyNeural", "Jenny", "en-US", "Female"),
        ("en-US-GuyNeural", "Guy", "en-US", "Male"),
        ("en-GB-SoniaNeural", "Sonia", "en-GB", "Female"),
        ("en-AU-NatashaNeural", "Natasha", "en-AU", "Female"),
        ("nb-NO-PernilleNeural", "Pernille", "nb-NO", "Female"),
        ("nb-NO-FinnNeural", "Finn", "nb-NO", "Male"),
        ("de-DE-KatjaNeural", "Katja", "de-DE", "Female"),
        ("fr-FR-DeniseNeural", "Denise", "fr-FR", "Female"),
        ("es-ES-ElviraNeural", "Elvira", "es-ES", "Female"),
    ]
    .into_iter()
    .map(|(id, name, language, gender)| VoiceInfo {
        id: id.to_owned(),
        name: name.to_owned(),
        language: language.to_owned(),
        gender: gender.to_owned(),
        provider: "azure-speech".to_owned(),
    })
    .collect()
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_format_wire_roundtrip() {
        for s in ["mp3", "wav", "ogg", "opus", "pcm", ""] {
            let f = AudioFormat::from_wire(s);
            assert!(!f.as_wire().is_empty());
        }
    }

    #[test]
    fn audio_format_unknown_defaults_to_mp3() {
        assert_eq!(AudioFormat::from_wire("flac"), AudioFormat::Mp3);
    }

    #[test]
    fn openai_base_normalizes_v1_suffix() {
        assert_eq!(
            normalize_openai_speech_base("https://api.openai.com/v1"),
            "https://api.openai.com"
        );
    }

    #[test]
    fn speech_chain_filters_voices_by_language() {
        let chain = SpeechChain::new_with_providers(vec![(
            "azure-speech".to_owned(),
            Arc::new(AzureSpeechProvider {
                endpoint: "https://example.test".to_owned(),
                api_key: "key".to_owned(),
                default_voice: DEFAULT_AZURE_SPEECH_VOICE.to_owned(),
                http: speech_http_client(),
            }),
        )]);
        let voices = chain.list_voices("azure", "nb");
        assert!(voices
            .iter()
            .any(|voice| voice.id == "nb-NO-PernilleNeural"));
        assert!(voices.iter().all(|voice| voice.language.starts_with("nb")));
    }

    #[test]
    fn from_env_returns_none_when_openai_key_unset() {
        let prev = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        assert!(OpenAiSpeechProvider::from_openai_env().is_none());
        if let Some(v) = prev {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}
