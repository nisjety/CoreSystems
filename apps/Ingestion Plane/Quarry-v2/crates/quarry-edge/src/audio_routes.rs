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

/// The `zdr` the Model Plane is told about this request. The body is forwarded
/// as the caller wrote it, except that the token's ZDR floor (a user of a ZDR
/// org) raises it to `true`: the caller cannot lower the org's posture. With no
/// floor the field is passed through untouched, so an ordinary request keeps
/// exactly the wire shape it had.
fn raise_to_zdr_floor(claims: &crate::auth::Claims, requested: Option<bool>) -> Option<bool> {
    if claims.zdr_floor() {
        Some(true)
    } else {
        requested
    }
}

pub async fn audio(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<crate::auth::Claims>,
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
        AudioRequest::Tts(mut tts) => {
            tts.zdr = raise_to_zdr_floor(&claims, tts.zdr);
            (
                "/v1/ai/speech".to_string(),
                serde_json::to_value(&tts).unwrap(),
            )
        }
        AudioRequest::Stt(mut stt) => {
            stt.zdr = raise_to_zdr_floor(&claims, stt.zdr);
            (
                "/v1/ai/transcribe".to_string(),
                serde_json::to_value(&stt).unwrap(),
            )
        }
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
    // The `model-audio` row of quarry-runtime's
    // contracts/cross-plane-token-requests.json: the request is written down there
    // and nowhere else.
    let token_request =
        match quarry_runtime::service_tokens::ServiceTokenRequest::from_manifest("model-audio") {
            Ok(request) => request,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "service-token request manifest unavailable",
                        "code": "INTERNAL",
                    })),
                );
            }
        };
    let bearer = if let Some(provider) = &state.service_token_provider {
        match provider
            .token_for_org(&claims.org_id, &token_request, false)
            .await
        {
            Ok(token) => Some(token.expose().to_owned()),
            Err(_) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "Model Plane authentication unavailable",
                        "code": "UPSTREAM_BLOCKED",
                    })),
                );
            }
        }
    } else {
        state.model_plane_token.clone()
    };
    let send = |bearer: Option<&str>| {
        let mut request = client.post(&url).json(&body);
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        request.send()
    };
    let mut response = send(bearer.as_deref()).await;
    if matches!(&response, Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED) {
        if let Some(provider) = &state.service_token_provider {
            response = match provider
                .token_for_org(&claims.org_id, &token_request, true)
                .await
            {
                Ok(token) => send(Some(token.expose())).await,
                Err(_) => response,
            };
        }
    }

    match response {
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

    /// The token request the handler makes is the `model-audio` row of the
    /// cross-plane manifest, as quarry-runtime's `token_request_contract` tests
    /// hold every other call path to its row. The handler, the real
    /// `AuthCoreServiceTokenProvider` and a recording Auth Core are real; the row
    /// is read as plain JSON, not through the loader under test.
    #[tokio::test]
    async fn audio_asks_auth_core_for_exactly_the_audio_row() {
        use crate::test_support::{claims_for_org, test_state, StubDriver};
        use quarry_runtime::service_tokens::{
            AuthCoreServiceTokenProvider, SharedServiceTokenProvider, TOKEN_REQUEST_MANIFEST_JSON,
        };
        use serde_json::{json, Value};
        use std::sync::Arc;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "header.payload.signature",
                "expiresInSeconds": 300
            })))
            .mount(&auth_core)
            .await;
        let model_plane = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/ai/speech"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
            .mount(&model_plane)
            .await;

        let provider: SharedServiceTokenProvider = Arc::new(
            AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough").unwrap(),
        );
        let state = AppState {
            model_plane_url: Some(model_plane.uri()),
            service_token_provider: Some(provider),
            ..test_state(StubDriver::ok())
        };
        let request: AudioRequest = serde_json::from_value(json!({ "input": "hello" })).unwrap();
        let (status, _) = audio(
            State(state),
            axum::Extension(claims_for_org("org_audio")),
            Json(request),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let manifest: Value = serde_json::from_str(TOKEN_REQUEST_MANIFEST_JSON).unwrap();
        let row = manifest["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "model-audio")
            .expect("the manifest has a model-audio row");
        let mints = auth_core.received_requests().await.unwrap();
        assert_eq!(mints.len(), 1, "one audio request mints exactly one token");
        assert_eq!(mints[0].url.path(), row["mint_path"].as_str().unwrap());
        assert_eq!(
            mints[0]
                .headers
                .get("x-service-id")
                .and_then(|value| value.to_str().ok()),
            Some("quarry-edge")
        );
        // Scopes and reason are the row's and nothing else rides along: Auth Core
        // refuses a request that names a retention posture.
        let body: Value = serde_json::from_slice(&mints[0].body).unwrap();
        assert_eq!(
            body,
            json!({
                "orgId": "org_audio",
                "scopes": row["scopes"],
                "reason": row["reason"]
            }),
            "the audio mint request differs from the manifest row"
        );
        let forwarded = model_plane.received_requests().await.unwrap();
        assert_eq!(
            forwarded[0]
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer header.payload.signature",
            "the token the handler minted is the one it sent to the Model Plane"
        );
    }
}
