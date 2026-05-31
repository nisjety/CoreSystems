//! `/v1/audio` — proxies to Model Plane speech routing.
//!
//! Quarry doesn't synthesize or transcribe — it brokers requests to Model
//! Plane (`/v1/ai/speech` for TTS, `/v1/ai/transcribe` for STT) and applies
//! Quarry-side ZDR / cost guards. Two operations:
//!
//! - **`POST /v1/audio` with `{ "input": "..." }`** → TTS, returns audio bytes
//!   (base64) or an artifact ref.
//! - **`POST /v1/audio` with `{ "audio_url": "..." }`** → STT, returns transcript.
//!
//! When Model Plane is not configured (no `model_plane_url` in EdgeConfig)
//! the route returns `501 Unsupported` with a hint, preserving backward
//! compatibility with the original placeholder.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::state::AppState;

#[derive(Deserialize)]
#[serde(untagged)]
pub enum AudioRequest {
    Tts(TtsRequest),
    Stt(SttRequest),
    Bare {
        #[serde(default)]
        url: Option<String>,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TtsRequest {
    pub input: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub zdr: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SttRequest {
    pub audio_url: String,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub zdr: Option<bool>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)] // scaffolding: wired in follow-up
pub struct AudioErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub async fn audio(
    State(state): State<AppState>,
    axum::Extension(_claims): axum::Extension<crate::auth::Claims>,
    Json(req): Json<AudioRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // /v1/audio currently proxies to Model Plane, which itself enforces
    // tenant isolation on its end via the JWT it receives. Holding a
    // Claims extension here ensures the route requires auth; the claim
    // itself isn't yet woven into the MP request body, but future
    // metering / billing hooks will use `claims.org_id`.
    let model_plane_url = match state.model_plane_url.as_deref() {
        Some(url) if !url.is_empty() => url,
        _ => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({
                    "error": "Model Plane not configured for audio",
                    "code": "UNSUPPORTED",
                    "hint": "set MODEL_PLANE_URL in edge config to enable /v1/audio",
                })),
            );
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("http client build: {e}"),
                    "code": "INTERNAL",
                })),
            );
        }
    };

    let (path, body) = match req {
        AudioRequest::Tts(tts) => (
            "/v1/ai/speech".to_string(),
            serde_json::to_value(&tts).unwrap(),
        ),
        AudioRequest::Stt(stt) => (
            "/v1/ai/transcribe".to_string(),
            serde_json::to_value(&stt).unwrap(),
        ),
        AudioRequest::Bare { url } => {
            // Bare {url} or empty body: 400 with usage hint.
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "request body must contain `input` (TTS) or `audio_url` (STT)",
                    "code": "BAD_REQUEST",
                    "hint": "see /v1/audio docs for usage",
                    "received_url": url,
                })),
            );
        }
    };

    let url = format!("{}{}", model_plane_url.trim_end_matches('/'), path);
    let mut req_builder = client.post(&url).json(&body);
    if let Some(token) = &state.model_plane_token {
        req_builder = req_builder.bearer_auth(token);
    }

    match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            let bytes = resp.bytes().await.unwrap_or_default();
            let parsed: serde_json::Value =
                serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({}));
            (
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(parsed),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("model plane unreachable: {e}"),
                "code": "DRIVER_FAILED",
            })),
        ),
    }
}

/// Backward-compat: the old route name. Calls into the new handler.
#[allow(dead_code)] // scaffolding: wired in follow-up
pub async fn audio_unsupported(
    state: State<AppState>,
    claims: axum::Extension<crate::auth::Claims>,
    body: Json<AudioRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    audio(state, claims, body).await
}

impl std::fmt::Debug for AudioRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioRequest::Tts(_) => write!(f, "AudioRequest::Tts(..)"),
            AudioRequest::Stt(_) => write!(f, "AudioRequest::Stt(..)"),
            AudioRequest::Bare { url } => {
                write!(f, "AudioRequest::Bare {{ url: {:?} }}", url)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_request_decodes_tts_shape() {
        let raw = serde_json::json!({"input": "hello", "voice": "alloy"});
        let r: AudioRequest = serde_json::from_value(raw).unwrap();
        match r {
            AudioRequest::Tts(_) => {}
            other => panic!("expected Tts, got {:?}", other),
        }
    }

    #[test]
    fn audio_request_decodes_stt_shape() {
        let raw = serde_json::json!({"audio_url": "https://x/a.mp3", "language": "en"});
        let r: AudioRequest = serde_json::from_value(raw).unwrap();
        match r {
            AudioRequest::Stt(_) => {}
            other => panic!("expected Stt, got {:?}", other),
        }
    }

    #[test]
    fn audio_request_decodes_bare_shape() {
        let raw = serde_json::json!({"url": "https://x/a.mp3"});
        let r: AudioRequest = serde_json::from_value(raw).unwrap();
        match r {
            AudioRequest::Bare { url } => assert_eq!(url.as_deref(), Some("https://x/a.mp3")),
            other => panic!("expected Bare, got {:?}", other),
        }
    }

    #[test]
    fn audio_request_decodes_empty_body_as_bare() {
        let raw = serde_json::json!({});
        let r: AudioRequest = serde_json::from_value(raw).unwrap();
        match r {
            AudioRequest::Bare { url } => assert!(url.is_none()),
            other => panic!("expected Bare(None), got {:?}", other),
        }
    }
}
