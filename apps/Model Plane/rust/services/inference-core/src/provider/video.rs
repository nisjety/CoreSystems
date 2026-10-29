//! Video generation job providers.

use std::sync::Arc;

use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{ModelInfo, ProviderError};

const DEFAULT_AZURE_VIDEO_API_VERSION: &str = "preview";
const DEFAULT_AZURE_VIDEO_MODEL: &str = "sora";
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_DURATION_SECONDS: u32 = 5;
const MAX_VIDEO_CONTENT_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct VideoGenerationRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: u32,
    pub n_variants: u32,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct VideoGenerationJob {
    pub request_id: String,
    pub job_id: String,
    pub status: String,
    pub model_used: String,
    pub provider_used: String,
    pub raw_json: String,
}

#[derive(Debug, Clone)]
pub struct VideoJobStatusRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub job_id: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct VideoJobStatus {
    pub request_id: String,
    pub job_id: String,
    pub status: String,
    pub generation_id: String,
    pub video_url: String,
    pub error: String,
    pub model_used: String,
    pub provider_used: String,
    pub raw_json: String,
}

#[derive(Debug, Clone)]
pub struct VideoContentRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub generation_id: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct VideoContentChunk {
    pub request_id: String,
    pub generation_id: String,
    pub data: Vec<u8>,
    pub done: bool,
    pub content_type: String,
    pub content_length: u64,
    pub provider_used: String,
}

#[async_trait::async_trait]
pub trait VideoProvider: Send + Sync {
    async fn create_job(
        &self,
        req: &VideoGenerationRequest,
    ) -> Result<VideoGenerationJob, ProviderError>;

    async fn get_job_status(
        &self,
        req: &VideoJobStatusRequest,
    ) -> Result<VideoJobStatus, ProviderError>;

    async fn stream_generation_content(
        &self,
        req: &VideoContentRequest,
    ) -> Result<mpsc::Receiver<Result<VideoContentChunk, ProviderError>>, ProviderError>;

    fn list_models(&self) -> Vec<ModelInfo>;
}

type BoxedVideoProvider = Arc<dyn VideoProvider>;

#[derive(Clone, Default)]
pub struct VideoChain {
    providers: Vec<(String, BoxedVideoProvider)>,
}

impl VideoChain {
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, BoxedVideoProvider)> = Vec::new();
        if let Some(provider) = AzureOpenAiVideoProvider::from_env() {
            providers.push(("azure-openai".to_owned(), Arc::new(provider)));
            info!(provider = "azure-openai", "video provider registered");
        }
        Self { providers }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn new_with_providers(providers: Vec<(String, BoxedVideoProvider)>) -> Self {
        Self { providers }
    }

    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Create a video generation job using the first matching provider.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn create_job(
        &self,
        req: &VideoGenerationRequest,
    ) -> Result<VideoGenerationJob, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.create_job(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "video provider failed"),
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Fetch the status of a video generation job via the first matching provider.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn get_job_status(
        &self,
        req: &VideoJobStatusRequest,
    ) -> Result<VideoJobStatus, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.get_job_status(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "video provider failed"),
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Stream generated video content via the first matching provider.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn stream_generation_content(
        &self,
        req: &VideoContentRequest,
    ) -> Result<mpsc::Receiver<Result<VideoContentChunk, ProviderError>>, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.stream_generation_content(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "video content provider failed"),
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
pub struct AzureOpenAiVideoProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    api_version: String,
    default_model: String,
    model_catalog: Vec<String>,
}

impl AzureOpenAiVideoProvider {
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_OPENAI_VIDEO_ENDPOINT")
            .or_else(|| env_nonempty("AZURE_OPENAI_ENDPOINT"))?;
        let api_key = env_nonempty("AZURE_OPENAI_VIDEO_API_KEY")
            .or_else(|| env_nonempty("AZURE_OPENAI_API_KEY"))?;
        let api_version = env_nonempty("AZURE_OPENAI_VIDEO_API_VERSION")
            .unwrap_or_else(|| DEFAULT_AZURE_VIDEO_API_VERSION.to_owned());
        let default_model = env_nonempty("AZURE_OPENAI_VIDEO_MODEL")
            .or_else(|| env_nonempty("AZURE_OPENAI_VIDEO_DEPLOYMENT"))
            .unwrap_or_else(|| DEFAULT_AZURE_VIDEO_MODEL.to_owned());
        let mut model_catalog = split_csv_env("AZURE_OPENAI_VIDEO_MODELS");
        push_unique(&mut model_catalog, default_model.clone());
        Some(Self {
            client: crate::provider::provider_http_client(),
            endpoint,
            api_key,
            api_version,
            default_model,
            model_catalog,
        })
    }

    fn jobs_url(&self) -> String {
        format!(
            "{}/openai/v1/video/generations/jobs?api-version={}",
            self.endpoint.trim_end_matches('/'),
            self.api_version
        )
    }

    fn job_status_url(&self, job_id: &str) -> String {
        format!(
            "{}/openai/v1/video/generations/jobs/{}?api-version={}",
            self.endpoint.trim_end_matches('/'),
            job_id,
            self.api_version
        )
    }

    fn video_content_url(&self, generation_id: &str) -> String {
        format!(
            "{}/openai/v1/video/generations/{}/content/video?api-version={}",
            self.endpoint.trim_end_matches('/'),
            generation_id,
            self.api_version
        )
    }

    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.header("api-key", &self.api_key)
    }
}

#[async_trait::async_trait]
impl VideoProvider for AzureOpenAiVideoProvider {
    async fn create_job(
        &self,
        req: &VideoGenerationRequest,
    ) -> Result<VideoGenerationJob, ProviderError> {
        let model = defaulted(&req.model, &self.default_model);
        let body = azure_video_job_body(req, &model);
        let response = self
            .auth(self.client.post(self.jobs_url()))
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        let value = parse_response(response).await?;
        parse_job_response(&req.request_id, &value, &model)
    }

    async fn get_job_status(
        &self,
        req: &VideoJobStatusRequest,
    ) -> Result<VideoJobStatus, ProviderError> {
        let model = defaulted(&req.model, &self.default_model);
        let response = self
            .auth(self.client.get(self.job_status_url(&req.job_id)))
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        let value = parse_response(response).await?;
        Ok(parse_status_response(
            &req.request_id,
            &req.job_id,
            &value,
            &model,
            |generation_id| self.video_content_url(generation_id),
        ))
    }

    async fn stream_generation_content(
        &self,
        req: &VideoContentRequest,
    ) -> Result<mpsc::Receiver<Result<VideoContentChunk, ProviderError>>, ProviderError> {
        let _model = defaulted(&req.model, &self.default_model);
        let response = self
            .auth(self.client.get(self.video_content_url(&req.generation_id)))
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        stream_video_response(&req.request_id, &req.generation_id, response).await
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        self.model_catalog
            .iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                provider: "azure-openai".to_owned(),
                modality: "video".to_owned(),
                streaming: false,
                ..Default::default()
            })
            .collect()
    }
}

fn azure_video_job_body(req: &VideoGenerationRequest, model: &str) -> Value {
    let mut body = json!({
        "model": model,
        "prompt": req.prompt,
        "width": if req.width == 0 { DEFAULT_WIDTH } else { req.width },
        "height": if req.height == 0 { DEFAULT_HEIGHT } else { req.height },
        "n_seconds": if req.duration_seconds == 0 {
            DEFAULT_DURATION_SECONDS
        } else {
            req.duration_seconds
        },
    });
    if req.n_variants > 0 {
        body["n_variants"] = json!(req.n_variants);
    }
    body
}

fn parse_job_response(
    request_id: &str,
    value: &Value,
    fallback_model: &str,
) -> Result<VideoGenerationJob, ProviderError> {
    let job_id = value
        .pointer("/id")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/job_id").and_then(Value::as_str))
        .unwrap_or_default();
    if job_id.trim().is_empty() {
        return Err(ProviderError::InvalidResponse(
            "azure video response missing job id".to_owned(),
        ));
    }
    Ok(VideoGenerationJob {
        request_id: request_id.to_owned(),
        job_id: job_id.to_owned(),
        status: provider_status(value),
        model_used: value
            .pointer("/model")
            .and_then(Value::as_str)
            .unwrap_or(fallback_model)
            .to_owned(),
        provider_used: "azure-openai".to_owned(),
        raw_json: value.to_string(),
    })
}

fn parse_status_response(
    request_id: &str,
    requested_job_id: &str,
    value: &Value,
    fallback_model: &str,
    content_url: impl FnOnce(&str) -> String,
) -> VideoJobStatus {
    let job_id = value
        .pointer("/id")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/job_id").and_then(Value::as_str))
        .unwrap_or(requested_job_id)
        .to_owned();
    let generation_id = first_generation_id(value);
    let video_url = if generation_id.is_empty() {
        String::new()
    } else {
        content_url(&generation_id)
    };
    VideoJobStatus {
        request_id: request_id.to_owned(),
        job_id,
        status: provider_status(value),
        generation_id,
        video_url,
        error: provider_error(value),
        model_used: value
            .pointer("/model")
            .and_then(Value::as_str)
            .unwrap_or(fallback_model)
            .to_owned(),
        provider_used: "azure-openai".to_owned(),
        raw_json: value.to_string(),
    }
}

async fn parse_response(response: reqwest::Response) -> Result<Value, ProviderError> {
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
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| ProviderError::Http(error.to_string()))?;
    if !status.is_success() {
        return Err(ProviderError::Http(format!(
            "azure video returned {status}: {text}"
        )));
    }
    serde_json::from_str(&text).map_err(|error| ProviderError::InvalidResponse(error.to_string()))
}

async fn stream_video_response(
    request_id: &str,
    generation_id: &str,
    response: reqwest::Response,
) -> Result<mpsc::Receiver<Result<VideoContentChunk, ProviderError>>, ProviderError> {
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
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::Http(format!(
            "azure video content returned {status}: {text}"
        )));
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("video/mp4")
        .to_owned();
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    if content_length > MAX_VIDEO_CONTENT_BYTES {
        return Err(ProviderError::InvalidResponse(format!(
            "video content exceeds {}-MB cap",
            MAX_VIDEO_CONTENT_BYTES / (1024 * 1024)
        )));
    }

    let (tx, rx) = mpsc::channel(16);
    let request_id = request_id.to_owned();
    let generation_id = generation_id.to_owned();

    tokio::spawn(async move {
        let mut total: u64 = 0;
        let mut stream = response.bytes_stream();
        while let Some(next) = stream.next().await {
            match next {
                Ok(bytes) => {
                    total = total.saturating_add(bytes.len() as u64);
                    if total > MAX_VIDEO_CONTENT_BYTES {
                        let _ = tx
                            .send(Err(ProviderError::InvalidResponse(format!(
                                "video content exceeds {}-MB cap",
                                MAX_VIDEO_CONTENT_BYTES / (1024 * 1024)
                            ))))
                            .await;
                        return;
                    }
                    let chunk = VideoContentChunk {
                        request_id: request_id.clone(),
                        generation_id: generation_id.clone(),
                        data: bytes.to_vec(),
                        done: false,
                        content_type: content_type.clone(),
                        content_length,
                        provider_used: "azure-openai".to_owned(),
                    };
                    if tx.send(Ok(chunk)).await.is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(ProviderError::Http(error.to_string()))).await;
                    return;
                }
            }
        }

        let done = VideoContentChunk {
            request_id,
            generation_id,
            data: Vec::new(),
            done: true,
            content_type,
            content_length,
            provider_used: "azure-openai".to_owned(),
        };
        let _ = tx.send(Ok(done)).await;
    });

    Ok(rx)
}

fn provider_status(value: &Value) -> String {
    value
        .pointer("/status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}

fn provider_error(value: &Value) -> String {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/error").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn first_generation_id(value: &Value) -> String {
    value
        .pointer("/generations/0/id")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/result/id").and_then(Value::as_str))
        .or_else(|| value.pointer("/generation_id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn provider_matches(name: &str, hint: &str) -> bool {
    let hint = hint.trim().to_ascii_lowercase();
    hint.is_empty()
        || hint == name
        || (hint == "azure" && name == "azure-openai")
        || (hint == "sora" && name == "azure-openai")
        || (hint == "azure-video" && name == "azure-openai")
}

fn defaulted(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
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

    fn request() -> VideoGenerationRequest {
        VideoGenerationRequest {
            request_id: "req-video".to_owned(),
            provider_hint: String::new(),
            prompt: "A ship entering a Norwegian fjord".to_owned(),
            width: 1280,
            height: 720,
            duration_seconds: 5,
            n_variants: 1,
            model: "sora".to_owned(),
        }
    }

    #[test]
    fn azure_video_body_uses_current_jobs_shape() {
        let body = azure_video_job_body(&request(), "sora");

        assert_eq!(body["model"], "sora");
        assert_eq!(body["prompt"], "A ship entering a Norwegian fjord");
        assert_eq!(body["width"], 1280);
        assert_eq!(body["height"], 720);
        assert_eq!(body["n_seconds"], 5);
        assert_eq!(body["n_variants"], 1);
    }

    #[test]
    fn parses_job_submission_response() {
        let value = json!({
            "id": "job_123",
            "status": "queued",
            "model": "sora",
        });
        let parsed = parse_job_response("req-1", &value, "fallback").unwrap();
        assert_eq!(parsed.job_id, "job_123");
        assert_eq!(parsed.status, "queued");
        assert_eq!(parsed.model_used, "sora");
    }

    #[test]
    fn parses_status_response_and_constructs_content_url() {
        let value = json!({
            "id": "job_123",
            "status": "succeeded",
            "generations": [{ "id": "gen_456" }],
            "model": "sora",
        });
        let parsed = parse_status_response(
            "req-1",
            "job_123",
            &value,
            "fallback",
            |generation_id| {
                format!("https://example.test/openai/v1/video/generations/{generation_id}/content/video?api-version=preview")
            },
        );

        assert_eq!(parsed.status, "succeeded");
        assert_eq!(parsed.generation_id, "gen_456");
        assert!(parsed.video_url.contains("gen_456/content/video"));
    }
}
