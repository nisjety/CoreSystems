//! Vision provider — image generation, analysis, and OCR.

#![allow(dead_code)] // request DTO fields + test-only ctor wired up incrementally

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use tracing::{info, warn};

use super::{ModelInfo, ProviderError};

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_IMAGE_MODEL: &str = "gpt-image-1";
const DEFAULT_VISION_MODEL: &str = "gpt-4o";
const OCR_PROMPT: &str =
    "Extract all text from this image verbatim. Return only the text, no commentary.";

#[derive(Debug, Clone)]
pub struct GenerateImageRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub prompt: String,
    pub model: String,
    pub size: String,
    pub quality: String,
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImage {
    pub url: String,
    pub b64_json: String,
    pub revised_prompt: String,
}

#[derive(Debug, Clone)]
pub struct GenerateImageResponse {
    pub images: Vec<GeneratedImage>,
    pub model_used: String,
    pub provider_used: String,
}

#[derive(Debug, Clone)]
pub struct AnalyzeImageRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub image_url: String,
    pub image_data: Vec<u8>,
    pub mime_type: String,
    pub prompt: String,
    pub model: String,
    pub max_tokens: i32,
}

#[derive(Debug, Clone)]
pub struct AnalyzeImageResponse {
    pub description: String,
    pub model_used: String,
    pub provider_used: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
}

#[derive(Debug, Clone)]
pub struct ExtractImageTextResponse {
    pub text: String,
    pub page_count: u32,
    pub model_used: String,
    pub provider_used: String,
}

#[async_trait::async_trait]
pub trait VisionProvider: Send + Sync {
    async fn generate_image(
        &self,
        req: &GenerateImageRequest,
    ) -> Result<GenerateImageResponse, ProviderError>;

    async fn analyze_image(
        &self,
        req: &AnalyzeImageRequest,
    ) -> Result<AnalyzeImageResponse, ProviderError>;

    async fn extract_text(
        &self,
        req: &AnalyzeImageRequest,
    ) -> Result<ExtractImageTextResponse, ProviderError> {
        let mut ocr_req = req.clone();
        OCR_PROMPT.clone_into(&mut ocr_req.prompt);
        let analysis = self.analyze_image(&ocr_req).await?;
        Ok(ExtractImageTextResponse {
            text: analysis.description,
            page_count: 0,
            model_used: analysis.model_used,
            provider_used: analysis.provider_used,
        })
    }

    fn list_models(&self) -> Vec<ModelInfo>;
}

type BoxedVisionProvider = Arc<dyn VisionProvider>;

#[derive(Clone)]
pub struct VisionChain {
    providers: Vec<(String, BoxedVisionProvider)>,
}

impl VisionChain {
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, BoxedVisionProvider)> = Vec::new();

        if let Some(provider) = OpenAiVisionProvider::from_azure_env() {
            providers.push(("azure-openai".to_owned(), Arc::new(provider)));
        }
        if let Some(provider) = OpenAiVisionProvider::from_openai_env() {
            providers.push(("openai".to_owned(), Arc::new(provider)));
        }

        Self { providers }
    }

    #[cfg(test)]
    #[must_use]
    pub fn new_with_providers(providers: Vec<(String, BoxedVisionProvider)>) -> Self {
        Self { providers }
    }

    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Generate an image using the first matching provider in the chain.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn generate_image(
        &self,
        req: &GenerateImageRequest,
    ) -> Result<GenerateImageResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.generate_image(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "image generation provider failed"),
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Analyze an image using the first matching provider in the chain.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn analyze_image(
        &self,
        req: &AnalyzeImageRequest,
    ) -> Result<AnalyzeImageResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.analyze_image(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "vision provider failed"),
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Extract text (OCR) from an image using the first matching provider.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn extract_text(
        &self,
        req: &AnalyzeImageRequest,
    ) -> Result<ExtractImageTextResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.extract_text(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "OCR provider failed"),
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
enum OpenAiVisionFlavor {
    OpenAi {
        api_base: String,
    },
    Azure {
        endpoint: String,
        api_version: String,
        image_api_version: String,
    },
}

#[derive(Clone)]
pub struct OpenAiVisionProvider {
    client: reqwest::Client,
    api_key: String,
    flavor: OpenAiVisionFlavor,
    image_models: Vec<String>,
    vision_models: Vec<String>,
}

impl OpenAiVisionProvider {
    #[must_use]
    pub fn from_openai_env() -> Option<Self> {
        let api_key = env_nonempty("OPENAI_API_KEY")?;
        let api_base =
            env_nonempty("OPENAI_API_BASE").unwrap_or_else(|| DEFAULT_OPENAI_BASE.to_owned());
        let image_models = csv_env("OPENAI_IMAGE_MODELS", &[DEFAULT_IMAGE_MODEL]);
        let vision_models = csv_env("OPENAI_VISION_MODELS", &[DEFAULT_VISION_MODEL]);
        Some(Self {
            client: reqwest::Client::new(),
            api_key,
            flavor: OpenAiVisionFlavor::OpenAi { api_base },
            image_models,
            vision_models,
        })
    }

    #[must_use]
    pub fn from_azure_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_OPENAI_ENDPOINT")?;
        let api_key = env_nonempty("AZURE_OPENAI_API_KEY")?;
        let api_version = env_nonempty("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|| "2025-01-01-preview".to_owned());
        let image_api_version =
            env_nonempty("AZURE_OPENAI_IMAGE_API_VERSION").unwrap_or_else(|| "preview".to_owned());
        let image_models = csv_env_with_legacy(
            "AZURE_OPENAI_IMAGE_DEPLOYMENTS",
            "AZURE_OPENAI_IMAGE_DEPLOYMENT",
            &[DEFAULT_IMAGE_MODEL],
        );
        let vision_models = csv_env_with_legacy(
            "AZURE_OPENAI_VISION_DEPLOYMENTS",
            "AZURE_OPENAI_DEPLOYMENT",
            &[DEFAULT_VISION_MODEL],
        );

        Some(Self {
            client: reqwest::Client::new(),
            api_key,
            flavor: OpenAiVisionFlavor::Azure {
                endpoint,
                api_version,
                image_api_version,
            },
            image_models,
            vision_models,
        })
    }

    fn provider_name(&self) -> &'static str {
        match &self.flavor {
            OpenAiVisionFlavor::OpenAi { .. } => "openai",
            OpenAiVisionFlavor::Azure { .. } => "azure-openai",
        }
    }

    fn images_url(&self, model: &str) -> String {
        match &self.flavor {
            OpenAiVisionFlavor::OpenAi { api_base } => {
                format!("{}/images/generations", api_base.trim_end_matches('/'))
            }
            OpenAiVisionFlavor::Azure {
                endpoint,
                api_version,
                image_api_version,
            } => {
                if is_gpt_image_model(model) {
                    format!(
                        "{}/openai/v1/images/generations?api-version={}",
                        endpoint.trim_end_matches('/'),
                        image_api_version
                    )
                } else {
                    format!(
                        "{}/openai/deployments/{}/images/generations?api-version={}",
                        endpoint.trim_end_matches('/'),
                        model,
                        api_version
                    )
                }
            }
        }
    }

    fn chat_url(&self, model: &str) -> String {
        match &self.flavor {
            OpenAiVisionFlavor::OpenAi { api_base } => {
                format!("{}/chat/completions", api_base.trim_end_matches('/'))
            }
            OpenAiVisionFlavor::Azure {
                endpoint,
                api_version,
                ..
            } => format!(
                "{}/openai/deployments/{}/chat/completions?api-version={}",
                endpoint.trim_end_matches('/'),
                model,
                api_version
            ),
        }
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.flavor {
            OpenAiVisionFlavor::OpenAi { .. } => request.bearer_auth(&self.api_key),
            OpenAiVisionFlavor::Azure { .. } => request.header("api-key", &self.api_key),
        }
    }

    fn image_model<'a>(&'a self, requested: &'a str) -> &'a str {
        if requested.trim().is_empty() {
            self.image_models
                .first()
                .map_or(DEFAULT_IMAGE_MODEL, String::as_str)
        } else {
            requested
        }
    }

    fn vision_model<'a>(&'a self, requested: &'a str) -> &'a str {
        if requested.trim().is_empty() {
            self.vision_models
                .first()
                .map_or(DEFAULT_VISION_MODEL, String::as_str)
        } else {
            requested
        }
    }
}

fn is_gpt_image_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gpt-image")
}

fn image_quality(model: &str, requested: &str) -> String {
    let requested = requested.trim();
    if is_gpt_image_model(model) && (requested.is_empty() || requested == "standard") {
        "medium".to_owned()
    } else if requested.is_empty() {
        "standard".to_owned()
    } else {
        requested.to_owned()
    }
}

#[async_trait::async_trait]
impl VisionProvider for OpenAiVisionProvider {
    async fn generate_image(
        &self,
        req: &GenerateImageRequest,
    ) -> Result<GenerateImageResponse, ProviderError> {
        let model = self.image_model(&req.model);
        let mut body = json!({
            "prompt": req.prompt,
            "n": req.n.max(1),
            "size": if req.size.trim().is_empty() { "1024x1024" } else { &req.size },
            "quality": image_quality(model, &req.quality),
        });
        if matches!(&self.flavor, OpenAiVisionFlavor::OpenAi { .. }) || is_gpt_image_model(model) {
            body["model"] = json!(model);
        }
        if !is_gpt_image_model(model) {
            body["response_format"] = json!("b64_json");
        }

        let response = self
            .apply_auth(self.client.post(self.images_url(model)))
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;

        let json = parse_response(response).await?;
        let data = json["data"]
            .as_array()
            .ok_or_else(|| ProviderError::InvalidResponse("missing image data array".to_owned()))?;
        let images = data
            .iter()
            .map(|item| GeneratedImage {
                url: item["url"].as_str().unwrap_or("").to_owned(),
                b64_json: item["b64_json"].as_str().unwrap_or("").to_owned(),
                revised_prompt: item["revised_prompt"]
                    .as_str()
                    .unwrap_or(&req.prompt)
                    .to_owned(),
            })
            .collect();
        let model_used = json["model"].as_str().unwrap_or(model).to_owned();

        info!(
            model = model_used,
            provider = self.provider_name(),
            "image generation completed"
        );

        Ok(GenerateImageResponse {
            images,
            model_used,
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn analyze_image(
        &self,
        req: &AnalyzeImageRequest,
    ) -> Result<AnalyzeImageResponse, ProviderError> {
        let model = self.vision_model(&req.model);
        let image_url = image_url_or_data_url(req)?;
        let body = json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": req.prompt},
                    {"type": "image_url", "image_url": {"url": image_url}},
                ],
            }],
            "max_tokens": if req.max_tokens <= 0 { 1024 } else { req.max_tokens },
        });

        let response = self
            .apply_auth(self.client.post(self.chat_url(model)))
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;

        let json = parse_response(response).await?;
        let description = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        if description.trim().is_empty() {
            return Err(ProviderError::InvalidResponse(
                "vision response content is empty".to_owned(),
            ));
        }
        let model_used = json["model"].as_str().unwrap_or(model).to_owned();
        let input_tokens = to_i32_or_max(json["usage"]["prompt_tokens"].as_i64().unwrap_or(0));
        let output_tokens = to_i32_or_max(json["usage"]["completion_tokens"].as_i64().unwrap_or(0));

        info!(
            model = model_used,
            provider = self.provider_name(),
            "image analysis completed"
        );

        Ok(AnalyzeImageResponse {
            description,
            model_used,
            provider_used: self.provider_name().to_owned(),
            input_tokens,
            output_tokens,
        })
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let provider = self.provider_name().to_owned();
        self.image_models
            .iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                provider: provider.clone(),
                modality: "image".to_owned(),
                streaming: false,
                ..Default::default()
            })
            .chain(self.vision_models.iter().flat_map(|id| {
                [
                    ModelInfo {
                        id: id.clone(),
                        provider: provider.clone(),
                        modality: "vision".to_owned(),
                        streaming: false,
                        ..Default::default()
                    },
                    ModelInfo {
                        id: id.clone(),
                        provider: provider.clone(),
                        modality: "ocr".to_owned(),
                        streaming: false,
                        ..Default::default()
                    },
                ]
            }))
            .collect()
    }
}

async fn parse_response(response: reqwest::Response) -> Result<serde_json::Value, ProviderError> {
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_ms = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1)
            * 1_000;
        return Err(ProviderError::RateLimited { retry_after_ms });
    }
    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "no body".to_owned());
        return Err(ProviderError::Http(format!("{status}: {text}")));
    }
    response
        .json()
        .await
        .map_err(|error| ProviderError::InvalidResponse(error.to_string()))
}

fn image_url_or_data_url(req: &AnalyzeImageRequest) -> Result<String, ProviderError> {
    if !req.image_url.trim().is_empty() {
        return Ok(req.image_url.clone());
    }
    if req.image_data.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "image_url or image_data is required".to_owned(),
        ));
    }
    let mime_type = if req.mime_type.trim().is_empty() {
        "image/png"
    } else {
        req.mime_type.trim()
    };
    Ok(format!(
        "data:{};base64,{}",
        mime_type,
        STANDARD.encode(&req.image_data)
    ))
}

fn provider_matches(name: &str, hint: &str) -> bool {
    let hint = hint.trim().to_ascii_lowercase();
    hint.is_empty()
        || hint == name
        || (hint == "openai" && name == "azure-openai")
        || (hint == "azure" && name == "azure-openai")
}

fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn csv_env(name: &str, default: &[&str]) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|value| parse_csv(&value))
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| default.iter().map(|value| (*value).to_owned()).collect())
}

fn csv_env_with_legacy(name: &str, legacy: &str, default: &[&str]) -> Vec<String> {
    // A SET-BUT-EMPTY primary (e.g. `AZURE_OPENAI_VISION_DEPLOYMENTS=`) must be
    // treated as absent so the legacy var is consulted — `var().or_else()`
    // would otherwise stop at `Ok("")` and skip the legacy fallback, landing on
    // the hardcoded default (a model that may not be deployed → 404).
    let from = |key: &str| {
        std::env::var(key)
            .ok()
            .map(|value| parse_csv(&value))
            .filter(|values| !values.is_empty())
    };
    from(name)
        .or_else(|| from(legacy))
        .unwrap_or_else(|| default.iter().map(|value| (*value).to_owned()).collect())
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_defaults_to_png() {
        let req = AnalyzeImageRequest {
            request_id: "req-1".to_owned(),
            provider_hint: String::new(),
            image_url: String::new(),
            image_data: b"abc".to_vec(),
            mime_type: String::new(),
            prompt: "describe".to_owned(),
            model: String::new(),
            max_tokens: 0,
        };

        let data_url = image_url_or_data_url(&req).expect("data url");
        assert_eq!(data_url, "data:image/png;base64,YWJj");
    }

    #[test]
    fn provider_matches_azure_aliases() {
        assert!(provider_matches("azure-openai", "azure"));
        assert!(provider_matches("azure-openai", "openai"));
        assert!(provider_matches("openai", ""));
        assert!(!provider_matches("openai", "azure"));
    }

    #[test]
    fn openai_image_url_uses_images_generation_endpoint() {
        let provider = OpenAiVisionProvider {
            client: reqwest::Client::new(),
            api_key: "key".to_owned(),
            flavor: OpenAiVisionFlavor::OpenAi {
                api_base: "https://api.openai.com/v1/".to_owned(),
            },
            image_models: vec!["dall-e-3".to_owned()],
            vision_models: vec!["gpt-4o".to_owned()],
        };

        assert_eq!(
            provider.images_url("dall-e-3"),
            "https://api.openai.com/v1/images/generations"
        );
    }

    #[test]
    fn azure_image_url_uses_deployment_endpoint() {
        let provider = OpenAiVisionProvider {
            client: reqwest::Client::new(),
            api_key: "key".to_owned(),
            flavor: OpenAiVisionFlavor::Azure {
                endpoint: "https://example.openai.azure.com/".to_owned(),
                api_version: "2025-01-01-preview".to_owned(),
                image_api_version: "preview".to_owned(),
            },
            image_models: vec!["dalle".to_owned()],
            vision_models: vec!["gpt-4o".to_owned()],
        };

        assert_eq!(
            provider.images_url("dalle"),
            "https://example.openai.azure.com/openai/deployments/dalle/images/generations?api-version=2025-01-01-preview"
        );
    }

    #[test]
    fn azure_gpt_image_url_uses_current_v1_endpoint() {
        let provider = OpenAiVisionProvider {
            client: reqwest::Client::new(),
            api_key: "key".to_owned(),
            flavor: OpenAiVisionFlavor::Azure {
                endpoint: "https://example.openai.azure.com/".to_owned(),
                api_version: "2025-01-01-preview".to_owned(),
                image_api_version: "preview".to_owned(),
            },
            image_models: vec!["gpt-image-1".to_owned()],
            vision_models: vec!["gpt-4o".to_owned()],
        };

        assert_eq!(
            provider.images_url("gpt-image-1"),
            "https://example.openai.azure.com/openai/v1/images/generations?api-version=preview"
        );
        assert_eq!(image_quality("gpt-image-1", ""), "medium");
        assert_eq!(image_quality("gpt-image-1", "standard"), "medium");
        assert_eq!(image_quality("dall-e-3", ""), "standard");
    }
}
