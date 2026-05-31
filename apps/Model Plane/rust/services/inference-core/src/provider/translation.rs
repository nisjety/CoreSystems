//! Translation provider routing for text translation and language detection.

#![allow(dead_code)] // response DTO fields (e.g. request_id) mirror the wire contract; read incrementally

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::{
    openai::OpenAiProvider, ChatMessage, InferRequest, ModelInfo, ProviderError, ProviderRouter,
};

const DEFAULT_AZURE_TRANSLATOR_ENDPOINT: &str = "https://api.cognitive.microsofttranslator.com";
const AZURE_TRANSLATOR_API_VERSION: &str = "3.0";
const DEFAULT_OPENAI_TRANSLATION_MODEL: &str = "gpt-4o-mini";
const DEFAULT_AZURE_OPENAI_API_VERSION: &str = "2025-01-01-preview";

/// A single translation input item.
#[derive(Debug, Clone)]
pub struct TranslationInputItem {
    pub id: String,
    pub text: String,
}

/// Translation request.
#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub text: String,
    pub source_language: String,
    pub target_language: String,
    pub model: String,
}

/// Batch translation request.
#[derive(Debug, Clone)]
pub struct BatchTranslationRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub items: Vec<TranslationInputItem>,
    pub source_language: String,
    pub target_language: String,
    pub model: String,
}

/// Language detection request.
#[derive(Debug, Clone)]
pub struct LanguageDetectionRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub text: String,
    pub model: String,
}

/// Translation response.
#[derive(Debug, Clone)]
pub struct TranslationResponse {
    pub request_id: String,
    pub translated_text: String,
    pub detected_language: String,
    pub confidence: f32,
    pub model_used: String,
    pub provider_used: String,
}

/// One translated batch item.
#[derive(Debug, Clone)]
pub struct TranslationItemResult {
    pub id: String,
    pub original_text: String,
    pub translated_text: String,
    pub detected_language: String,
    pub confidence: f32,
}

/// Batch translation response.
#[derive(Debug, Clone)]
pub struct BatchTranslationResponse {
    pub request_id: String,
    pub translations: Vec<TranslationItemResult>,
    pub model_used: String,
    pub provider_used: String,
}

/// Language detection candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageDetection {
    pub language: String,
    pub confidence: f32,
    pub is_translation_supported: bool,
}

/// Language detection response.
#[derive(Debug, Clone)]
pub struct LanguageDetectionResponse {
    pub request_id: String,
    pub detections: Vec<LanguageDetection>,
    pub model_used: String,
    pub provider_used: String,
}

/// Supported translation language metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationLanguageInfo {
    pub code: String,
    pub name: String,
    pub native_name: String,
    pub direction: String,
}

/// Trait for translation providers.
#[async_trait::async_trait]
pub trait TranslationProvider: Send + Sync {
    /// Stable provider identifier.
    fn provider_name(&self) -> &'static str;

    /// Translate one text item.
    async fn translate(
        &self,
        req: &TranslationRequest,
    ) -> Result<TranslationResponse, ProviderError>;

    /// Translate multiple text items.
    async fn batch_translate(
        &self,
        req: &BatchTranslationRequest,
    ) -> Result<BatchTranslationResponse, ProviderError> {
        let mut translations = Vec::with_capacity(req.items.len());
        let mut model_used = String::new();
        let mut provider_used = String::new();

        for item in &req.items {
            let result = self
                .translate(&TranslationRequest {
                    request_id: req.request_id.clone(),
                    provider_hint: req.provider_hint.clone(),
                    text: item.text.clone(),
                    source_language: req.source_language.clone(),
                    target_language: req.target_language.clone(),
                    model: req.model.clone(),
                })
                .await?;
            model_used.clone_from(&result.model_used);
            provider_used.clone_from(&result.provider_used);
            translations.push(TranslationItemResult {
                id: item.id.clone(),
                original_text: item.text.clone(),
                translated_text: result.translated_text,
                detected_language: result.detected_language,
                confidence: result.confidence,
            });
        }

        Ok(BatchTranslationResponse {
            request_id: req.request_id.clone(),
            translations,
            model_used,
            provider_used,
        })
    }

    /// Detect a text language.
    async fn detect_language(
        &self,
        req: &LanguageDetectionRequest,
    ) -> Result<LanguageDetectionResponse, ProviderError>;

    /// List supported translation languages.
    async fn list_languages(&self) -> Result<Vec<TranslationLanguageInfo>, ProviderError> {
        Ok(default_languages())
    }

    /// Models/deployments known at startup.
    fn list_models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }
}

/// Sequential translation fallback chain with provider-hint routing.
#[derive(Clone, Default)]
pub struct TranslationChain {
    providers: Vec<(String, Arc<dyn TranslationProvider>)>,
}

impl TranslationChain {
    /// Build from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, Arc<dyn TranslationProvider>)> = Vec::new();

        if let Some(provider) = AzureTranslatorProvider::from_env() {
            providers.push(("azure-translator".to_owned(), Arc::new(provider)));
            info!(
                provider = "azure-translator",
                "translation provider registered"
            );
        }

        if let Some(provider) = LlmTranslationProvider::from_azure_env() {
            providers.push(("azure-openai".to_owned(), Arc::new(provider)));
            info!(provider = "azure-openai", "translation provider registered");
        }

        if let Some(provider) = LlmTranslationProvider::from_openai_env() {
            providers.push(("openai".to_owned(), Arc::new(provider)));
            info!(provider = "openai", "translation provider registered");
        }

        Self { providers }
    }

    /// Create a chain for tests.
    #[allow(dead_code)]
    #[must_use]
    pub fn new_with_providers(providers: Vec<(String, Arc<dyn TranslationProvider>)>) -> Self {
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
            || (hint == "azure" && (name == "azure-translator" || name == "azure-openai"))
            || (hint == "translator" && name == "azure-translator")
            || (hint == "azure-cognitive" && name == "azure-translator")
            || (hint == "openai" && name == "azure-openai")
    }

    /// Translate text with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every matching provider fails.
    pub async fn translate(
        &self,
        req: &TranslationRequest,
    ) -> Result<TranslationResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.translate(req).await {
                Ok(result) => {
                    info!(provider = %name, request_id = %req.request_id, "translation succeeded");
                    return Ok(result);
                }
                Err(err) => {
                    warn!(provider = %name, error = %err, "translation provider failed");
                }
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Batch translate text with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every matching provider fails.
    pub async fn batch_translate(
        &self,
        req: &BatchTranslationRequest,
    ) -> Result<BatchTranslationResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.batch_translate(req).await {
                Ok(result) => {
                    info!(provider = %name, request_id = %req.request_id, items = result.translations.len(), "batch translation succeeded");
                    return Ok(result);
                }
                Err(err) => {
                    warn!(provider = %name, error = %err, "batch translation provider failed");
                }
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Detect language with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every matching provider fails.
    pub async fn detect_language(
        &self,
        req: &LanguageDetectionRequest,
    ) -> Result<LanguageDetectionResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.detect_language(req).await {
                Ok(result) => {
                    info!(provider = %name, request_id = %req.request_id, "language detection succeeded");
                    return Ok(result);
                }
                Err(err) => {
                    warn!(provider = %name, error = %err, "language detection provider failed");
                }
            }
        }
        Err(ProviderError::AllExhausted { attempts })
    }

    /// Return translation models from matching providers.
    #[must_use]
    pub fn list_models(&self, modality: &str, provider: &str) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .filter(|(name, _)| Self::provider_matches(name, provider))
            .flat_map(|(_, provider)| provider.list_models())
            .filter(|model| modality.is_empty() || model.modality == modality)
            .collect()
    }

    /// Return supported languages from the first matching provider, or static fallback.
    pub async fn list_languages(&self, provider: &str) -> Vec<TranslationLanguageInfo> {
        for (name, provider_impl) in &self.providers {
            if !Self::provider_matches(name, provider) {
                continue;
            }
            match provider_impl.list_languages().await {
                Ok(languages) if !languages.is_empty() => return languages,
                Ok(_) => {}
                Err(err) => {
                    warn!(provider = %name, error = %err, "list languages provider failed");
                }
            }
        }
        default_languages()
    }
}

/// Azure Cognitive Services Translator provider.
#[derive(Clone)]
pub struct AzureTranslatorProvider {
    endpoint: String,
    api_key: String,
    region: String,
    http: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct AzureTextInput {
    #[serde(rename = "Text")]
    text: String,
}

#[derive(Debug, Deserialize, Default)]
struct AzureTranslateResponse {
    #[serde(default, rename = "detectedLanguage")]
    detected_language: Option<AzureDetectedLanguage>,
    #[serde(default)]
    translations: Vec<AzureTranslation>,
}

#[derive(Debug, Deserialize, Default)]
struct AzureDetectedLanguage {
    #[serde(default)]
    language: String,
    #[serde(default)]
    score: f32,
}

#[derive(Debug, Deserialize, Default)]
struct AzureTranslation {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize, Default)]
struct AzureDetectionResponse {
    #[serde(default)]
    language: String,
    #[serde(default)]
    score: f32,
    #[serde(default, rename = "isTranslationSupported")]
    is_translation_supported: bool,
}

#[derive(Debug, Deserialize, Default)]
struct AzureLanguagesResponse {
    #[serde(default)]
    translation: BTreeMap<String, AzureLanguageData>,
}

#[derive(Debug, Deserialize, Default)]
struct AzureLanguageData {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "nativeName")]
    native_name: String,
    #[serde(default)]
    dir: String,
}

impl AzureTranslatorProvider {
    /// Build from Azure Translator environment variables.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let api_key = env_nonempty("AZURE_TRANSLATOR_API_KEY")
            .or_else(|| env_nonempty("AZURE_TRANSLATOR_KEY"))
            .or_else(|| env_nonempty("AZURE_TEXT_TRANSLATION_KEY"))?;
        let endpoint = env_nonempty("AZURE_TRANSLATOR_ENDPOINT")
            .unwrap_or_else(|| DEFAULT_AZURE_TRANSLATOR_ENDPOINT.to_owned())
            .trim_end_matches('/')
            .to_owned();
        Some(Self {
            endpoint,
            api_key,
            region: env_nonempty("AZURE_TRANSLATOR_REGION").unwrap_or_default(),
            http: reqwest::Client::new(),
        })
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let req = req.header("Ocp-Apim-Subscription-Key", &self.api_key);
        if self.region.is_empty() {
            req
        } else {
            req.header("Ocp-Apim-Subscription-Region", &self.region)
        }
    }

    async fn translate_items(
        &self,
        request_id: &str,
        items: &[TranslationInputItem],
        source_language: &str,
        target_language: &str,
    ) -> Result<Vec<TranslationItemResult>, ProviderError> {
        let mut query = vec![
            ("api-version", AZURE_TRANSLATOR_API_VERSION.to_owned()),
            ("to", target_language.to_owned()),
        ];
        if !source_language.trim().is_empty() {
            query.push(("from", source_language.to_owned()));
        }
        let body: Vec<AzureTextInput> = items
            .iter()
            .map(|item| AzureTextInput {
                text: item.text.clone(),
            })
            .collect();

        let resp = self
            .apply_auth(
                self.http
                    .post(format!("{}/translate", self.endpoint))
                    .query(&query)
                    .json(&body),
            )
            .send()
            .await
            .map_err(|e| ProviderError::Http(format!("azure translator translate: {e}")))?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after_ms(&resp),
            });
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "azure translator translate HTTP {status}: {}",
                truncate(&body, 300)
            )));
        }

        let parsed: Vec<AzureTranslateResponse> = resp
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(format!("azure translate json: {e}")))?;

        if parsed.len() != items.len() {
            return Err(ProviderError::InvalidResponse(format!(
                "azure translate returned {} results for {} items in {request_id}",
                parsed.len(),
                items.len()
            )));
        }

        Ok(items
            .iter()
            .zip(parsed)
            .map(|(item, result)| {
                let detected = result.detected_language.unwrap_or_default();
                TranslationItemResult {
                    id: item.id.clone(),
                    original_text: item.text.clone(),
                    translated_text: result
                        .translations
                        .first()
                        .map_or_else(|| item.text.clone(), |translation| translation.text.clone()),
                    detected_language: if detected.language.is_empty() {
                        source_language.to_owned()
                    } else {
                        detected.language
                    },
                    confidence: detected.score,
                }
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl TranslationProvider for AzureTranslatorProvider {
    fn provider_name(&self) -> &'static str {
        "azure-translator"
    }

    async fn translate(
        &self,
        req: &TranslationRequest,
    ) -> Result<TranslationResponse, ProviderError> {
        let items = vec![TranslationInputItem {
            id: "0".to_owned(),
            text: req.text.clone(),
        }];
        let mut translations = self
            .translate_items(
                &req.request_id,
                &items,
                &req.source_language,
                &req.target_language,
            )
            .await?;
        let item = translations.pop().ok_or_else(|| {
            ProviderError::InvalidResponse("azure translate returned no result".to_owned())
        })?;
        Ok(TranslationResponse {
            request_id: req.request_id.clone(),
            translated_text: item.translated_text,
            detected_language: item.detected_language,
            confidence: item.confidence,
            model_used: "azure-translator".to_owned(),
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn batch_translate(
        &self,
        req: &BatchTranslationRequest,
    ) -> Result<BatchTranslationResponse, ProviderError> {
        let translations = self
            .translate_items(
                &req.request_id,
                &req.items,
                &req.source_language,
                &req.target_language,
            )
            .await?;
        Ok(BatchTranslationResponse {
            request_id: req.request_id.clone(),
            translations,
            model_used: "azure-translator".to_owned(),
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn detect_language(
        &self,
        req: &LanguageDetectionRequest,
    ) -> Result<LanguageDetectionResponse, ProviderError> {
        let body = vec![AzureTextInput {
            text: req.text.clone(),
        }];
        let resp = self
            .apply_auth(
                self.http
                    .post(format!("{}/detect", self.endpoint))
                    .query(&[("api-version", AZURE_TRANSLATOR_API_VERSION)])
                    .json(&body),
            )
            .send()
            .await
            .map_err(|e| ProviderError::Http(format!("azure translator detect: {e}")))?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after_ms(&resp),
            });
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "azure translator detect HTTP {status}: {}",
                truncate(&body, 300)
            )));
        }

        let parsed: Vec<AzureDetectionResponse> = resp
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(format!("azure detect json: {e}")))?;
        let detections = parsed
            .into_iter()
            .map(|detected| LanguageDetection {
                language: detected.language,
                confidence: detected.score,
                is_translation_supported: detected.is_translation_supported,
            })
            .collect();

        Ok(LanguageDetectionResponse {
            request_id: req.request_id.clone(),
            detections,
            model_used: "azure-translator".to_owned(),
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn list_languages(&self) -> Result<Vec<TranslationLanguageInfo>, ProviderError> {
        let resp = self
            .http
            .get(format!("{}/languages", self.endpoint))
            .query(&[
                ("api-version", AZURE_TRANSLATOR_API_VERSION),
                ("scope", "translation"),
            ])
            .send()
            .await
            .map_err(|e| ProviderError::Http(format!("azure translator languages: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "azure translator languages HTTP {status}: {}",
                truncate(&body, 300)
            )));
        }

        let parsed: AzureLanguagesResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(format!("azure languages json: {e}")))?;
        Ok(parsed
            .translation
            .into_iter()
            .map(|(code, lang)| TranslationLanguageInfo {
                code,
                name: lang.name,
                native_name: lang.native_name,
                direction: if lang.dir.is_empty() {
                    "ltr".to_owned()
                } else {
                    lang.dir
                },
            })
            .collect())
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "azure-translator-v3".to_owned(),
            provider: self.provider_name().to_owned(),
            modality: "translation".to_owned(),
            streaming: false,
        }]
    }
}

/// LLM-backed translation fallback using the existing OpenAI-compatible provider.
#[derive(Clone)]
pub struct LlmTranslationProvider {
    provider: OpenAiProvider,
    provider_name: &'static str,
    default_model: String,
}

impl LlmTranslationProvider {
    /// Build Azure `OpenAI` translation fallback from env.
    #[must_use]
    pub fn from_azure_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_OPENAI_ENDPOINT")?;
        let key = env_nonempty("AZURE_OPENAI_API_KEY")?;
        let api_version = env_nonempty("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|| DEFAULT_AZURE_OPENAI_API_VERSION.to_owned());
        let default_model = env_nonempty("AZURE_OPENAI_TRANSLATION_DEPLOYMENT")
            .or_else(|| csv_env_first("AZURE_OPENAI_CHAT_DEPLOYMENTS"))
            .or_else(|| env_nonempty("AZURE_OPENAI_DEPLOYMENT"))
            .unwrap_or_else(|| DEFAULT_OPENAI_TRANSLATION_MODEL.to_owned());
        let provider = OpenAiProvider::new_azure(key, endpoint, api_version).ok()?;
        Some(Self {
            provider,
            provider_name: "azure-openai",
            default_model,
        })
    }

    /// Build `OpenAI` translation fallback from env.
    #[must_use]
    pub fn from_openai_env() -> Option<Self> {
        let key = env_nonempty("OPENAI_API_KEY")?;
        let default_model = env_nonempty("OPENAI_TRANSLATION_MODEL")
            .or_else(|| csv_env_first("OPENAI_CHAT_MODELS"))
            .unwrap_or_else(|| DEFAULT_OPENAI_TRANSLATION_MODEL.to_owned());
        let provider = OpenAiProvider::new(key, env_nonempty("OPENAI_API_BASE")).ok()?;
        Some(Self {
            provider,
            provider_name: "openai",
            default_model,
        })
    }

    fn model_for(&self, model: &str) -> String {
        if model.trim().is_empty() {
            self.default_model.clone()
        } else {
            model.to_owned()
        }
    }

    async fn run_prompt(
        &self,
        request_id: &str,
        model: &str,
        prompt: String,
    ) -> Result<String, ProviderError> {
        let result = self
            .provider
            .infer(&InferRequest {
                request_id: request_id.to_owned(),
                provider_hint: self.provider_name.to_owned(),
                model: model.to_owned(),
                messages: vec![ChatMessage {
                    role: "user".to_owned(),
                    content: prompt,
                    name: String::new(),
                }],
                temperature: 0.0,
                max_tokens: 2048,
                structured_output_schema: None,
                zdr: true,
            })
            .await?;
        Ok(result.content.trim().to_owned())
    }
}

#[async_trait::async_trait]
impl TranslationProvider for LlmTranslationProvider {
    fn provider_name(&self) -> &'static str {
        self.provider_name
    }

    async fn translate(
        &self,
        req: &TranslationRequest,
    ) -> Result<TranslationResponse, ProviderError> {
        let model = self.model_for(&req.model);
        let source = if req.source_language.trim().is_empty() {
            "auto-detected source language".to_owned()
        } else {
            req.source_language.clone()
        };
        let prompt = format!(
            "Translate the following text from {source} to {}. Output only the translation, with no explanation:\n\n{}",
            req.target_language, req.text
        );
        let translated = self.run_prompt(&req.request_id, &model, prompt).await?;
        Ok(TranslationResponse {
            request_id: req.request_id.clone(),
            translated_text: translated,
            detected_language: if req.source_language.is_empty() {
                "unknown".to_owned()
            } else {
                req.source_language.clone()
            },
            confidence: if req.source_language.is_empty() {
                0.0
            } else {
                1.0
            },
            model_used: model,
            provider_used: self.provider_name().to_owned(),
        })
    }

    async fn detect_language(
        &self,
        req: &LanguageDetectionRequest,
    ) -> Result<LanguageDetectionResponse, ProviderError> {
        let model = self.model_for(&req.model);
        let prompt = format!(
            "Detect the language of this text. Return only one ISO 639 language code, no punctuation:\n\n{}",
            req.text
        );
        let detected =
            sanitize_language_code(&self.run_prompt(&req.request_id, &model, prompt).await?);
        Ok(LanguageDetectionResponse {
            request_id: req.request_id.clone(),
            detections: vec![LanguageDetection {
                language: detected,
                confidence: 0.5,
                is_translation_supported: true,
            }],
            model_used: model,
            provider_used: self.provider_name().to_owned(),
        })
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: self.default_model.clone(),
            provider: self.provider_name().to_owned(),
            modality: "translation".to_owned(),
            streaming: false,
        }]
    }
}

fn retry_after_ms(resp: &reqwest::Response) -> u64 {
    resp.headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1)
        * 1000
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn csv_env_first(key: &str) -> Option<String> {
    env_nonempty(key).and_then(|value| {
        value
            .split(',')
            .map(str::trim)
            .find(|part| !part.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn sanitize_language_code(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '.' || ch.is_whitespace())
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
        .to_ascii_lowercase()
}

fn default_languages() -> Vec<TranslationLanguageInfo> {
    [
        ("af", "Afrikaans", "Afrikaans", "ltr"),
        ("ar", "Arabic", "Arabic", "rtl"),
        ("de", "German", "Deutsch", "ltr"),
        ("en", "English", "English", "ltr"),
        ("es", "Spanish", "Espanol", "ltr"),
        ("fr", "French", "Francais", "ltr"),
        ("it", "Italian", "Italiano", "ltr"),
        ("nb", "Norwegian Bokmal", "Norsk Bokmal", "ltr"),
        ("nl", "Dutch", "Nederlands", "ltr"),
        ("pt", "Portuguese", "Portugues", "ltr"),
        ("sv", "Swedish", "Svenska", "ltr"),
        ("zh-Hans", "Chinese Simplified", "Chinese Simplified", "ltr"),
        (
            "zh-Hant",
            "Chinese Traditional",
            "Chinese Traditional",
            "ltr",
        ),
    ]
    .into_iter()
    .map(
        |(code, name, native_name, direction)| TranslationLanguageInfo {
            code: code.to_owned(),
            name: name.to_owned(),
            native_name: native_name.to_owned(),
            direction: direction.to_owned(),
        },
    )
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
    fn provider_matches_azure_aliases() {
        assert!(TranslationChain::provider_matches(
            "azure-translator",
            "azure"
        ));
        assert!(TranslationChain::provider_matches("azure-openai", "azure"));
        assert!(TranslationChain::provider_matches(
            "azure-translator",
            "translator"
        ));
        assert!(!TranslationChain::provider_matches(
            "openai",
            "azure-translator"
        ));
    }

    #[test]
    fn sanitize_language_code_strips_extra_text() {
        assert_eq!(sanitize_language_code("\"EN\"."), "en");
        assert_eq!(sanitize_language_code("nb confidence high"), "nb");
    }

    #[test]
    fn default_languages_include_norwegian_and_english() {
        let languages = default_languages();
        assert!(languages.iter().any(|language| language.code == "en"));
        assert!(languages.iter().any(|language| language.code == "nb"));
    }

    #[test]
    fn azure_translator_from_env_handles_key_aliases() {
        let prev = std::env::var("AZURE_TRANSLATOR_API_KEY").ok();
        let prev_short = std::env::var("AZURE_TRANSLATOR_KEY").ok();
        let prev_legacy = std::env::var("AZURE_TEXT_TRANSLATION_KEY").ok();
        std::env::remove_var("AZURE_TRANSLATOR_API_KEY");
        std::env::remove_var("AZURE_TRANSLATOR_KEY");
        std::env::remove_var("AZURE_TEXT_TRANSLATION_KEY");

        assert!(AzureTranslatorProvider::from_env().is_none());
        std::env::set_var("AZURE_TRANSLATOR_KEY", "translator-key");
        assert!(AzureTranslatorProvider::from_env().is_some());

        if let Some(value) = prev {
            std::env::set_var("AZURE_TRANSLATOR_API_KEY", value);
        } else {
            std::env::remove_var("AZURE_TRANSLATOR_API_KEY");
        }
        if let Some(value) = prev_short {
            std::env::set_var("AZURE_TRANSLATOR_KEY", value);
        } else {
            std::env::remove_var("AZURE_TRANSLATOR_KEY");
        }
        if let Some(value) = prev_legacy {
            std::env::set_var("AZURE_TEXT_TRANSLATION_KEY", value);
        } else {
            std::env::remove_var("AZURE_TEXT_TRANSLATION_KEY");
        }
    }
}
