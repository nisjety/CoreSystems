//! Realtime session broker providers.

use std::sync::Arc;

use serde_json::{json, Value};
use tracing::{info, warn};

use super::{ModelInfo, ProviderError};

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_REALTIME_MODEL: &str = "gpt-realtime";
const DEFAULT_REALTIME_VOICE: &str = "alloy";
const DEFAULT_AUDIO_FORMAT: &str = "pcm16";
const DEFAULT_TURN_DETECTION: &str = "server_vad";

#[derive(Debug, Clone)]
pub struct RealtimeSessionRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub model: String,
    pub voice: String,
    pub instructions: String,
    pub input_audio_format: String,
    pub output_audio_format: String,
    pub turn_detection_type: String,
}

#[derive(Debug, Clone)]
pub struct RealtimeSessionResponse {
    pub request_id: String,
    pub session_id: String,
    pub client_secret: String,
    pub websocket_url: String,
    pub expires_at: i64,
    pub model_used: String,
    pub provider_used: String,
    pub voice: String,
}

#[async_trait::async_trait]
pub trait RealtimeProvider: Send + Sync {
    async fn create_session(
        &self,
        req: &RealtimeSessionRequest,
    ) -> Result<RealtimeSessionResponse, ProviderError>;

    fn list_models(&self) -> Vec<ModelInfo>;
}

type BoxedRealtimeProvider = Arc<dyn RealtimeProvider>;

#[derive(Clone, Default)]
pub struct RealtimeChain {
    providers: Vec<(String, BoxedRealtimeProvider)>,
}

impl RealtimeChain {
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, BoxedRealtimeProvider)> = Vec::new();
        if let Some(provider) = OpenAiRealtimeProvider::from_env() {
            providers.push(("openai".to_owned(), Arc::new(provider)));
            info!(provider = "openai", "realtime provider registered");
        }
        Self { providers }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn new_with_providers(providers: Vec<(String, BoxedRealtimeProvider)>) -> Self {
        Self { providers }
    }

    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    pub async fn create_session(
        &self,
        req: &RealtimeSessionRequest,
    ) -> Result<RealtimeSessionResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.create_session(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "realtime provider failed"),
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    #[must_use]
    pub fn list_models(&self, modality: &str, provider_filter: &str) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .filter(|(name, _)| provider_matches(name, provider_filter))
            .flat_map(|(_, provider)| provider.list_models())
            .filter(|model| modality.trim().is_empty() || model.modality == modality)
            .collect()
    }
}

#[derive(Clone)]
pub struct OpenAiRealtimeProvider {
    client: reqwest::Client,
    api_key: String,
    api_base: String,
    websocket_base: String,
    default_model: String,
    model_catalog: Vec<String>,
}

impl OpenAiRealtimeProvider {
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let api_key = env_nonempty("OPENAI_API_KEY")?;
        let api_base = env_nonempty("OPENAI_REALTIME_API_BASE")
            .or_else(|| env_nonempty("OPENAI_API_BASE"))
            .unwrap_or_else(|| DEFAULT_OPENAI_BASE.to_owned());
        let default_model = env_nonempty("OPENAI_REALTIME_MODEL")
            .unwrap_or_else(|| DEFAULT_OPENAI_REALTIME_MODEL.to_owned());
        let websocket_base = env_nonempty("OPENAI_REALTIME_WEBSOCKET_URL")
            .unwrap_or_else(|| websocket_base_from_api_base(&api_base));
        let mut model_catalog = split_csv_env("OPENAI_REALTIME_MODELS");
        push_unique(&mut model_catalog, default_model.clone());
        push_unique(&mut model_catalog, DEFAULT_OPENAI_REALTIME_MODEL.to_owned());
        push_unique(&mut model_catalog, "gpt-4o-realtime-preview".to_owned());
        push_unique(
            &mut model_catalog,
            "gpt-4o-mini-realtime-preview".to_owned(),
        );
        Some(Self {
            client: reqwest::Client::new(),
            api_key,
            api_base,
            websocket_base,
            default_model,
            model_catalog,
        })
    }

    fn client_secret_url(&self) -> String {
        format!(
            "{}/realtime/client_secrets",
            self.api_base.trim_end_matches('/')
        )
    }

    fn websocket_url(&self, model: &str) -> String {
        format!(
            "{}?model={}",
            self.websocket_base.trim_end_matches('?'),
            model
        )
    }
}

#[async_trait::async_trait]
impl RealtimeProvider for OpenAiRealtimeProvider {
    async fn create_session(
        &self,
        req: &RealtimeSessionRequest,
    ) -> Result<RealtimeSessionResponse, ProviderError> {
        let model = defaulted(&req.model, &self.default_model);
        let voice = defaulted(&req.voice, DEFAULT_REALTIME_VOICE);
        let body = openai_client_secret_body(req, &model, &voice);

        let response = self
            .client
            .post(self.client_secret_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1);
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after * 1000,
            });
        }

        if !response.status().is_success() {
            return Err(ProviderError::Http(format!(
                "openai realtime returned {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let json = response
            .json::<Value>()
            .await
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;
        parse_openai_client_secret_response(
            &req.request_id,
            &json,
            &model,
            &voice,
            &self.websocket_url(&model),
        )
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        self.model_catalog
            .iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                provider: "openai".to_owned(),
                modality: "realtime".to_owned(),
                streaming: true,
            })
            .collect()
    }
}

fn openai_client_secret_body(req: &RealtimeSessionRequest, model: &str, voice: &str) -> Value {
    let input_audio_format = defaulted(&req.input_audio_format, DEFAULT_AUDIO_FORMAT);
    let output_audio_format = defaulted(&req.output_audio_format, DEFAULT_AUDIO_FORMAT);
    let turn_detection_type = defaulted(&req.turn_detection_type, DEFAULT_TURN_DETECTION);
    let mut session = json!({
        "type": "realtime",
        "model": model,
        "audio": {
            "input": {
                "format": { "type": input_audio_format },
                "turn_detection": { "type": turn_detection_type },
            },
            "output": {
                "format": { "type": output_audio_format },
                "voice": voice,
            },
        },
    });
    if !req.instructions.trim().is_empty() {
        session["instructions"] = Value::String(req.instructions.clone());
    }
    json!({ "session": session })
}

fn parse_openai_client_secret_response(
    request_id: &str,
    value: &Value,
    fallback_model: &str,
    fallback_voice: &str,
    websocket_url: &str,
) -> Result<RealtimeSessionResponse, ProviderError> {
    let client_secret = value
        .pointer("/value")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/client_secret/value")
                .and_then(Value::as_str)
        })
        .or_else(|| value.pointer("/client_secret").and_then(Value::as_str))
        .unwrap_or_default();
    if client_secret.trim().is_empty() {
        return Err(ProviderError::InvalidResponse(
            "openai realtime response missing client secret".to_owned(),
        ));
    }

    let expires_at = value
        .pointer("/expires_at")
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .pointer("/client_secret/expires_at")
                .and_then(Value::as_i64)
        })
        .unwrap_or_default();
    let session_id = value
        .pointer("/session/id")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    let model_used = value
        .pointer("/session/model")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/model").and_then(Value::as_str))
        .unwrap_or(fallback_model)
        .to_owned();
    let voice = value
        .pointer("/session/audio/output/voice")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/voice").and_then(Value::as_str))
        .unwrap_or(fallback_voice)
        .to_owned();

    Ok(RealtimeSessionResponse {
        request_id: request_id.to_owned(),
        session_id,
        client_secret: client_secret.to_owned(),
        websocket_url: websocket_url.to_owned(),
        expires_at,
        model_used,
        provider_used: "openai".to_owned(),
        voice,
    })
}

fn provider_matches(name: &str, hint: &str) -> bool {
    let hint = hint.trim().to_ascii_lowercase();
    hint.is_empty()
        || hint == name
        || (hint == "realtime" && name == "openai")
        || (hint == "openai-realtime" && name == "openai")
}

fn defaulted(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn websocket_base_from_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim_end_matches('/');
    let without_v1 = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    let ws_scheme = without_v1
        .strip_prefix("https://")
        .map(|rest| format!("wss://{rest}"))
        .or_else(|| {
            without_v1
                .strip_prefix("http://")
                .map(|rest| format!("ws://{rest}"))
        })
        .unwrap_or_else(|| without_v1.to_owned());
    format!("{}/v1/realtime", ws_scheme.trim_end_matches('/'))
}

fn split_csv_env(name: &str) -> Vec<String> {
    env_nonempty(name)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RealtimeSessionRequest {
        RealtimeSessionRequest {
            request_id: "req-1".to_owned(),
            provider_hint: String::new(),
            model: "gpt-realtime".to_owned(),
            voice: "alloy".to_owned(),
            instructions: "Be concise.".to_owned(),
            input_audio_format: "pcm16".to_owned(),
            output_audio_format: "pcm16".to_owned(),
            turn_detection_type: "server_vad".to_owned(),
        }
    }

    #[test]
    fn websocket_base_derives_from_openai_api_base() {
        assert_eq!(
            websocket_base_from_api_base("https://api.openai.com/v1"),
            "wss://api.openai.com/v1/realtime"
        );
    }

    #[test]
    fn openai_body_uses_current_client_secret_shape() {
        let body = openai_client_secret_body(&request(), "gpt-realtime", "alloy");

        assert_eq!(body["session"]["type"], "realtime");
        assert_eq!(body["session"]["model"], "gpt-realtime");
        assert_eq!(body["session"]["audio"]["output"]["voice"], "alloy");
        assert_eq!(
            body["session"]["audio"]["input"]["turn_detection"]["type"],
            "server_vad"
        );
    }

    #[test]
    fn parses_current_and_legacy_client_secret_shapes() {
        let current = json!({
            "value": "ek_current",
            "expires_at": 1_234,
            "session": {
                "id": "sess_1",
                "model": "gpt-realtime",
                "audio": { "output": { "voice": "alloy" } }
            }
        });
        let parsed = parse_openai_client_secret_response(
            "req-1",
            &current,
            "fallback",
            "fallback_voice",
            "wss://example.test/v1/realtime?model=gpt-realtime",
        )
        .expect("current shape should parse");
        assert_eq!(parsed.client_secret, "ek_current");
        assert_eq!(parsed.session_id, "sess_1");
        assert_eq!(parsed.expires_at, 1_234);

        let legacy = json!({
            "id": "sess_legacy",
            "model": "gpt-4o-realtime-preview",
            "client_secret": { "value": "ek_legacy", "expires_at": 5_678 },
        });
        let parsed = parse_openai_client_secret_response(
            "req-2",
            &legacy,
            "fallback",
            "alloy",
            "wss://example.test/v1/realtime?model=gpt-4o-realtime-preview",
        )
        .expect("legacy shape should parse");
        assert_eq!(parsed.client_secret, "ek_legacy");
        assert_eq!(parsed.session_id, "sess_legacy");
        assert_eq!(parsed.expires_at, 5_678);
    }
}
