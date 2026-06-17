//! Language analytics provider routing for sentiment, NER, key phrases, PII, detection, and summary.

use std::{sync::Arc, time::Duration};

use serde_json::json;
use tokio::time::sleep;
use tracing::{info, warn};

use super::{
    narrow_f64, openai::OpenAiProvider, ChatMessage, InferRequest, ModelInfo, ProviderError,
    ProviderRouter,
};

const DEFAULT_AZURE_API_VERSION: &str = "2024-11-15-preview";
const DEFAULT_OPENAI_LANGUAGE_MODEL: &str = "gpt-4o-mini";
const DEFAULT_AZURE_OPENAI_API_VERSION: &str = "2025-01-01-preview";
const SUMMARY_POLL_INTERVAL_MS: u64 = 5_000;
const SUMMARY_MAX_POLL_ATTEMPTS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageOperation {
    Sentiment,
    Entities,
    KeyPhrases,
    Pii,
    Detect,
    Summary,
    /// Toxicity / content-safety classification (hate, harassment, violence,
    /// self-harm, sexual). The genuine moderation classifier — the policy
    /// thresholds/enforcement live in capability-core `safety_policies`
    /// (kind=content_safety); this produces the per-category scores.
    ContentSafety,
}

impl LanguageOperation {
    /// Parse a [`LanguageOperation`] from its wire string.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidResponse`] if `value` is not a recognized operation.
    pub fn from_wire(value: &str) -> Result<Self, ProviderError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sentiment" => Ok(Self::Sentiment),
            "entities" | "entity_recognition" | "ner" => Ok(Self::Entities),
            "key_phrases" | "key-phrases" | "keyphrases" => Ok(Self::KeyPhrases),
            "pii" | "redact" | "redaction" => Ok(Self::Pii),
            "detect" | "language_detection" | "detect_language" => Ok(Self::Detect),
            "summary" | "summarize" | "summarise" | "summary_text" => Ok(Self::Summary),
            "content_safety" | "toxicity" | "moderate" | "moderation" => Ok(Self::ContentSafety),
            other => Err(ProviderError::InvalidResponse(format!(
                "unsupported language operation: {other}"
            ))),
        }
    }

    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Sentiment => "sentiment",
            Self::Entities => "entities",
            Self::KeyPhrases => "key_phrases",
            Self::Pii => "pii",
            Self::Detect => "detect",
            Self::Summary => "summary",
            Self::ContentSafety => "content_safety",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LanguageAnalyticsRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub operation: LanguageOperation,
    pub texts: Vec<String>,
    pub language: String,
    pub model: String,
    pub sentence_count: u32,
    pub summary_kind: String,
}

#[derive(Debug, Clone, Default)]
pub struct LanguageAnalysisItem {
    pub id: String,
    pub sentiment: String,
    pub confidence_scores_json: String,
    pub sentences_json: String,
    pub entities_json: String,
    pub key_phrases: Vec<String>,
    pub redacted_text: String,
    pub detected_language_name: String,
    pub detected_language_code: String,
    pub confidence: f32,
    pub summary: String,
    /// Content-safety verdict JSON: `{flagged, categories:{hate,harassment,
    /// violence,self_harm,sexual}}` (per-category 0..1). Empty unless the
    /// operation is `ContentSafety`.
    pub content_safety_json: String,
    pub raw_json: String,
}

#[derive(Debug, Clone)]
pub struct LanguageAnalyticsResponse {
    pub operation: LanguageOperation,
    pub results: Vec<LanguageAnalysisItem>,
    pub model_used: String,
    pub provider_used: String,
}

#[async_trait::async_trait]
pub trait LanguageAnalyticsProvider: Send + Sync {
    async fn analyze(
        &self,
        req: &LanguageAnalyticsRequest,
    ) -> Result<LanguageAnalyticsResponse, ProviderError>;

    fn list_models(&self) -> Vec<ModelInfo>;
}

type BoxedLanguageProvider = Arc<dyn LanguageAnalyticsProvider>;

#[derive(Clone)]
pub struct LanguageAnalyticsChain {
    providers: Vec<(String, BoxedLanguageProvider)>,
}

impl LanguageAnalyticsChain {
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, BoxedLanguageProvider)> = Vec::new();
        if let Some(provider) = AzureLanguageProvider::from_env() {
            providers.push(("azure-language".to_owned(), Arc::new(provider)));
        }
        if let Some(provider) = LlmLanguageProvider::from_azure_env() {
            providers.push(("azure-openai".to_owned(), Arc::new(provider)));
        }
        if let Some(provider) = LlmLanguageProvider::from_openai_env() {
            providers.push(("openai".to_owned(), Arc::new(provider)));
        }
        Self { providers }
    }

    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Run language analytics using the first matching provider in the chain.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn analyze(
        &self,
        req: &LanguageAnalyticsRequest,
    ) -> Result<LanguageAnalyticsResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.analyze(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "language analytics provider failed"),
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
pub struct AzureLanguageProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    api_version: String,
}

impl AzureLanguageProvider {
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_AI_LANGUAGE_ENDPOINT")
            .or_else(|| env_nonempty("AZURE_LANGUAGE_ENDPOINT"))?;
        let api_key =
            env_nonempty("AZURE_AI_LANGUAGE_KEY").or_else(|| env_nonempty("AZURE_LANGUAGE_KEY"))?;
        let api_version = env_nonempty("AZURE_AI_LANGUAGE_API_VERSION")
            .or_else(|| env_nonempty("AZURE_LANGUAGE_API_VERSION"))
            .unwrap_or_else(|| DEFAULT_AZURE_API_VERSION.to_owned());
        Some(Self {
            client: reqwest::Client::new(),
            endpoint,
            api_key,
            api_version,
        })
    }

    fn analyze_url(&self) -> String {
        format!(
            "{}/language/:analyze-text?api-version={}",
            self.endpoint.trim_end_matches('/'),
            self.api_version
        )
    }

    fn jobs_url(&self) -> String {
        format!(
            "{}/language/analyze-text/jobs?api-version={}",
            self.endpoint.trim_end_matches('/'),
            self.api_version
        )
    }

    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.header("Ocp-Apim-Subscription-Key", &self.api_key)
    }

    async fn analyze_sync(
        &self,
        req: &LanguageAnalyticsRequest,
    ) -> Result<LanguageAnalyticsResponse, ProviderError> {
        let body = json!({
            "kind": azure_kind(req.operation),
            "analysisInput": { "documents": documents(&req.texts, &req.language, req.operation != LanguageOperation::Detect) },
            "parameters": azure_parameters(req.operation),
        });
        let response = self
            .auth(self.client.post(self.analyze_url()))
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        let json = parse_response(response).await?;
        let docs = json["results"]["documents"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let results = docs
            .iter()
            .map(|doc| azure_doc_to_item(req.operation, doc))
            .collect();
        Ok(LanguageAnalyticsResponse {
            operation: req.operation,
            results,
            model_used: azure_kind(req.operation).to_owned(),
            provider_used: "azure-language".to_owned(),
        })
    }

    async fn summarize(
        &self,
        req: &LanguageAnalyticsRequest,
    ) -> Result<LanguageAnalyticsResponse, ProviderError> {
        let body = json!({
            "displayName": format!("summary-{}", req.request_id),
            "analysisInput": { "documents": documents(&req.texts, &req.language, true) },
            "tasks": [{
                "kind": summary_kind(&req.summary_kind),
                "parameters": {
                    "summaryCount": req.sentence_count.max(1),
                    "modelVersion": "latest",
                },
            }],
        });
        let response = self
            .auth(self.client.post(self.jobs_url()))
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after_ms(&response),
            });
        }
        if !response.status().is_success() && response.status() != reqwest::StatusCode::ACCEPTED {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            return Err(ProviderError::Http(format!("{status}: {text}")));
        }
        let operation_url = response
            .headers()
            .get("operation-location")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let json = if let Some(operation_url) = operation_url {
            self.poll_summary(&operation_url).await?
        } else {
            response
                .json()
                .await
                .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?
        };
        Ok(LanguageAnalyticsResponse {
            operation: req.operation,
            results: extract_summary_items(&json),
            model_used: summary_kind(&req.summary_kind).to_owned(),
            provider_used: "azure-language".to_owned(),
        })
    }

    async fn poll_summary(&self, operation_url: &str) -> Result<serde_json::Value, ProviderError> {
        for _ in 0..SUMMARY_MAX_POLL_ATTEMPTS {
            let response = self
                .auth(self.client.get(operation_url))
                .send()
                .await
                .map_err(|error| ProviderError::Http(error.to_string()))?;
            let json = parse_response(response).await?;
            match json["status"].as_str().unwrap_or("") {
                "succeeded" => return Ok(json),
                "failed" => {
                    return Err(ProviderError::InvalidResponse(format!(
                        "language summary failed: {}",
                        json["error"]
                    )));
                }
                _ => sleep(Duration::from_millis(SUMMARY_POLL_INTERVAL_MS)).await,
            }
        }
        Err(ProviderError::Unavailable(
            "language summary polling timed out".to_owned(),
        ))
    }
}

#[async_trait::async_trait]
impl LanguageAnalyticsProvider for AzureLanguageProvider {
    async fn analyze(
        &self,
        req: &LanguageAnalyticsRequest,
    ) -> Result<LanguageAnalyticsResponse, ProviderError> {
        // Azure AI Language has no toxicity task (that is Azure AI Content
        // Safety, a separate resource). Decline so the chain falls through to
        // the LLM classifier (LlmLanguageProvider).
        if req.operation == LanguageOperation::ContentSafety {
            return Err(ProviderError::UnsupportedModel(
                "azure-language does not support content_safety".to_owned(),
            ));
        }
        let response = if req.operation == LanguageOperation::Summary {
            self.summarize(req).await?
        } else {
            self.analyze_sync(req).await?
        };
        info!(
            operation = req.operation.as_wire(),
            provider = "azure-language",
            "language analytics completed"
        );
        Ok(response)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        [
            "SentimentAnalysis",
            "EntityRecognition",
            "KeyPhraseExtraction",
            "PiiEntityRecognition",
            "LanguageDetection",
            "AbstractiveSummarization",
            "ExtractiveSummarization",
        ]
        .into_iter()
        .map(|id| ModelInfo {
            id: id.to_owned(),
            provider: "azure-language".to_owned(),
            modality: "language_analytics".to_owned(),
            streaming: false,
            ..Default::default()
        })
        .collect()
    }
}

#[derive(Clone)]
pub struct LlmLanguageProvider {
    provider: OpenAiProvider,
    model: String,
    provider_used: String,
}

impl LlmLanguageProvider {
    #[must_use]
    pub fn from_azure_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_OPENAI_ENDPOINT")?;
        let api_key = env_nonempty("AZURE_OPENAI_API_KEY")?;
        let api_version = env_nonempty("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|| DEFAULT_AZURE_OPENAI_API_VERSION.to_owned());
        let model = env_nonempty("AZURE_OPENAI_LANGUAGE_DEPLOYMENT")
            .or_else(|| csv_env_first("AZURE_OPENAI_CHAT_DEPLOYMENTS"))
            .or_else(|| env_nonempty("AZURE_OPENAI_DEPLOYMENT"))
            .unwrap_or_else(|| DEFAULT_OPENAI_LANGUAGE_MODEL.to_owned());
        let provider = OpenAiProvider::new_azure(api_key, endpoint, api_version).ok()?;
        Some(Self {
            provider,
            model,
            provider_used: "azure-openai".to_owned(),
        })
    }

    #[must_use]
    pub fn from_openai_env() -> Option<Self> {
        let api_key = env_nonempty("OPENAI_API_KEY")?;
        let model = env_nonempty("OPENAI_LANGUAGE_MODEL")
            .or_else(|| csv_env_first("OPENAI_CHAT_MODELS"))
            .unwrap_or_else(|| DEFAULT_OPENAI_LANGUAGE_MODEL.to_owned());
        let provider = OpenAiProvider::new(api_key, env_nonempty("OPENAI_API_BASE")).ok()?;
        Some(Self {
            provider,
            model,
            provider_used: "openai".to_owned(),
        })
    }
}

#[async_trait::async_trait]
impl LanguageAnalyticsProvider for LlmLanguageProvider {
    async fn analyze(
        &self,
        req: &LanguageAnalyticsRequest,
    ) -> Result<LanguageAnalyticsResponse, ProviderError> {
        let prompt = language_prompt(req);
        let response = self
            .provider
            .infer(&InferRequest {
                request_id: req.request_id.clone(),
                provider_hint: String::new(),
                model: if req.model.trim().is_empty() {
                    self.model.clone()
                } else {
                    req.model.clone()
                },
                messages: vec![
                    ChatMessage {
                        role: "system".to_owned(),
                        content: "Return strict JSON only. Do not include markdown.".to_owned(),
                        name: String::new(),
                    },
                    ChatMessage {
                        role: "user".to_owned(),
                        content: prompt,
                        name: String::new(),
                    },
                ],
                temperature: 0.0,
                max_tokens: 2_048,
                structured_output_schema: None,
                zdr: true,
                ..Default::default()
            })
            .await?;
        let results = parse_llm_results(req.operation, &response.content, &req.texts);
        Ok(LanguageAnalyticsResponse {
            operation: req.operation,
            results,
            model_used: response.model_used,
            provider_used: self.provider_used.clone(),
        })
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: self.model.clone(),
            provider: self.provider_used.clone(),
            modality: "language_analytics".to_owned(),
            streaming: false,
            ..Default::default()
        }]
    }
}

async fn parse_response(response: reqwest::Response) -> Result<serde_json::Value, ProviderError> {
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::RateLimited {
            retry_after_ms: retry_after_ms(&response),
        });
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

fn azure_kind(operation: LanguageOperation) -> &'static str {
    match operation {
        LanguageOperation::Sentiment => "SentimentAnalysis",
        LanguageOperation::Entities => "EntityRecognition",
        LanguageOperation::KeyPhrases => "KeyPhraseExtraction",
        LanguageOperation::Pii => "PiiEntityRecognition",
        LanguageOperation::Detect => "LanguageDetection",
        LanguageOperation::Summary => "AbstractiveSummarization",
        // Unreachable: AzureLanguageProvider::analyze declines ContentSafety
        // before reaching azure_kind (handled by the LLM provider instead).
        LanguageOperation::ContentSafety => "ContentSafety",
    }
}

fn azure_parameters(operation: LanguageOperation) -> serde_json::Value {
    match operation {
        LanguageOperation::Sentiment => json!({ "opinionMining": true }),
        LanguageOperation::Pii => json!({ "piiCategories": ["All"], "redactionCharacter": "*" }),
        _ => json!({}),
    }
}

fn documents(texts: &[String], language: &str, include_language: bool) -> Vec<serde_json::Value> {
    texts
        .iter()
        .enumerate()
        .map(|(idx, text)| {
            let mut doc = json!({ "id": (idx + 1).to_string(), "text": text });
            if include_language && !language.trim().is_empty() {
                doc["language"] = json!(language.trim());
            }
            doc
        })
        .collect()
}

fn azure_doc_to_item(
    operation: LanguageOperation,
    doc: &serde_json::Value,
) -> LanguageAnalysisItem {
    match operation {
        LanguageOperation::Sentiment => LanguageAnalysisItem {
            id: doc["id"].as_str().unwrap_or("").to_owned(),
            sentiment: doc["sentiment"].as_str().unwrap_or("").to_owned(),
            confidence_scores_json: to_json_string(&doc["confidenceScores"], "{}"),
            sentences_json: to_json_string(&doc["sentences"], "[]"),
            raw_json: to_json_string(doc, "{}"),
            ..LanguageAnalysisItem::default()
        },
        LanguageOperation::Entities => LanguageAnalysisItem {
            id: doc["id"].as_str().unwrap_or("").to_owned(),
            entities_json: to_json_string(&doc["entities"], "[]"),
            raw_json: to_json_string(doc, "{}"),
            ..LanguageAnalysisItem::default()
        },
        LanguageOperation::KeyPhrases => LanguageAnalysisItem {
            id: doc["id"].as_str().unwrap_or("").to_owned(),
            key_phrases: doc["keyPhrases"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect(),
            raw_json: to_json_string(doc, "{}"),
            ..LanguageAnalysisItem::default()
        },
        LanguageOperation::Pii => LanguageAnalysisItem {
            id: doc["id"].as_str().unwrap_or("").to_owned(),
            redacted_text: doc["redactedText"].as_str().unwrap_or("").to_owned(),
            entities_json: to_json_string(&doc["entities"], "[]"),
            raw_json: to_json_string(doc, "{}"),
            ..LanguageAnalysisItem::default()
        },
        LanguageOperation::Detect => {
            let detected = &doc["detectedLanguage"];
            LanguageAnalysisItem {
                id: doc["id"].as_str().unwrap_or("").to_owned(),
                detected_language_name: detected["name"].as_str().unwrap_or("").to_owned(),
                detected_language_code: detected["iso6391Name"].as_str().unwrap_or("").to_owned(),
                confidence: narrow_f64(detected["confidenceScore"].as_f64().unwrap_or(0.0)),
                raw_json: to_json_string(doc, "{}"),
                ..LanguageAnalysisItem::default()
            }
        }
        LanguageOperation::Summary => LanguageAnalysisItem::default(),
        // Unreachable: ContentSafety is served by the LLM provider, never Azure.
        LanguageOperation::ContentSafety => LanguageAnalysisItem::default(),
    }
}

fn extract_summary_items(json: &serde_json::Value) -> Vec<LanguageAnalysisItem> {
    json["tasks"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|task| {
            task["results"]["documents"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|doc| {
                    let summaries = doc["summaries"]
                        .as_array()
                        .or_else(|| doc["sentences"].as_array())
                        .cloned()
                        .unwrap_or_default();
                    let summary = summaries
                        .iter()
                        .filter_map(|item| item["text"].as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    LanguageAnalysisItem {
                        id: doc["id"].as_str().unwrap_or("").to_owned(),
                        summary,
                        raw_json: to_json_string(doc, "{}"),
                        ..LanguageAnalysisItem::default()
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn language_prompt(req: &LanguageAnalyticsRequest) -> String {
    let texts = serde_json::to_string(&req.texts).unwrap_or_else(|_| "[]".to_owned());
    if req.operation == LanguageOperation::ContentSafety {
        // Toxicity / content-safety classifier. Per-category float scores in
        // [0,1] + a `flagged` boolean (true if any category is unsafe). The
        // categories mirror the OpenAI moderation / Azure content-safety taxonomy.
        return format!(
            "You are a content-safety classifier. For EACH input text, score these categories \
             from 0.0 (safe) to 1.0 (severe): hate, harassment, violence, self_harm, sexual. \
             Set \"flagged\" true if ANY category is clearly unsafe (score >= 0.5). \
             Return ONLY a JSON array; one object per input in order, shape: \
             {{\"id\":\"1\",\"flagged\":false,\"categories\":{{\"hate\":0.0,\"harassment\":0.0,\
             \"violence\":0.0,\"self_harm\":0.0,\"sexual\":0.0}}}}. Texts: {texts}"
        );
    }
    format!(
        "Operation: {}. Language hint: {}. Return JSON array of result objects with fields matching the operation. Texts: {}",
        req.operation.as_wire(),
        if req.language.is_empty() { "auto" } else { &req.language },
        texts
    )
}

fn parse_llm_results(
    operation: LanguageOperation,
    content: &str,
    texts: &[String],
) -> Vec<LanguageAnalysisItem> {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .map_or_else(
            || {
                texts
                    .iter()
                    .enumerate()
                    .map(|(idx, text)| LanguageAnalysisItem {
                        id: (idx + 1).to_string(),
                        summary: if operation == LanguageOperation::Summary {
                            content.to_owned()
                        } else {
                            String::new()
                        },
                        raw_json: json!({ "text": text, "content": content }).to_string(),
                        ..LanguageAnalysisItem::default()
                    })
                    .collect()
            },
            |items| {
                items
                    .iter()
                    .enumerate()
                    .map(|(idx, item)| llm_item(operation, idx, item))
                    .collect()
            },
        )
}

fn llm_item(
    operation: LanguageOperation,
    idx: usize,
    item: &serde_json::Value,
) -> LanguageAnalysisItem {
    LanguageAnalysisItem {
        id: item["id"]
            .as_str()
            .map_or_else(|| (idx + 1).to_string(), ToOwned::to_owned),
        sentiment: item["sentiment"].as_str().unwrap_or("").to_owned(),
        confidence_scores_json: to_json_string(&item["confidence_scores"], "{}"),
        sentences_json: to_json_string(&item["sentences"], "[]"),
        entities_json: to_json_string(&item["entities"], "[]"),
        key_phrases: item["key_phrases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        redacted_text: item["redacted_text"].as_str().unwrap_or("").to_owned(),
        detected_language_name: item["language_name"].as_str().unwrap_or("").to_owned(),
        detected_language_code: item["iso_code"].as_str().unwrap_or("").to_owned(),
        confidence: narrow_f64(item["confidence"].as_f64().unwrap_or(0.0)),
        summary: item["summary"]
            .as_str()
            .unwrap_or(if operation == LanguageOperation::Summary {
                item.as_str().unwrap_or("")
            } else {
                ""
            })
            .to_owned(),
        content_safety_json: if operation == LanguageOperation::ContentSafety {
            json!({
                "flagged": item["flagged"].as_bool().unwrap_or(false),
                "categories": if item["categories"].is_object() {
                    item["categories"].clone()
                } else {
                    json!({})
                },
            })
            .to_string()
        } else {
            String::new()
        },
        raw_json: to_json_string(item, "{}"),
    }
}

fn summary_kind(value: &str) -> &'static str {
    match value.trim() {
        "ExtractiveSummarization" => "ExtractiveSummarization",
        _ => "AbstractiveSummarization",
    }
}

fn provider_matches(name: &str, hint: &str) -> bool {
    let hint = hint.trim().to_ascii_lowercase();
    hint.is_empty()
        || hint == name
        || (hint == "azure" && (name == "azure-language" || name == "azure-openai"))
        || (hint == "azure-ai-language" && name == "azure-language")
        || (hint == "openai" && name == "azure-openai")
}

fn retry_after_ms(response: &reqwest::Response) -> u64 {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1)
        * 1_000
}

fn to_json_string(value: &serde_json::Value, default: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| default.to_owned())
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn csv_env_first(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .and_then(|value| {
            value
                .split(',')
                .next()
                .map(str::trim)
                .map(ToOwned::to_owned)
        })
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operation_aliases() {
        assert_eq!(
            LanguageOperation::from_wire("key-phrases").expect("operation"),
            LanguageOperation::KeyPhrases
        );
        assert_eq!(
            LanguageOperation::from_wire("ner").expect("operation"),
            LanguageOperation::Entities
        );
    }

    #[test]
    fn parses_content_safety_aliases() {
        for alias in ["content_safety", "toxicity", "moderate", "moderation"] {
            assert_eq!(
                LanguageOperation::from_wire(alias).expect("operation"),
                LanguageOperation::ContentSafety,
            );
        }
    }

    #[test]
    fn content_safety_prompt_lists_categories() {
        let req = LanguageAnalyticsRequest {
            request_id: "r1".to_owned(),
            provider_hint: String::new(),
            operation: LanguageOperation::ContentSafety,
            texts: vec!["you are awful".to_owned()],
            language: String::new(),
            model: String::new(),
            sentence_count: 0,
            summary_kind: String::new(),
        };
        let p = language_prompt(&req);
        for cat in [
            "hate",
            "harassment",
            "violence",
            "self_harm",
            "sexual",
            "flagged",
        ] {
            assert!(p.contains(cat), "prompt must mention {cat}");
        }
    }

    #[test]
    fn parses_content_safety_llm_result_into_json() {
        let content = r#"[{"id":"1","flagged":true,"categories":{"hate":0.9,"harassment":0.8,"violence":0.1,"self_harm":0.0,"sexual":0.0}}]"#;
        let items = parse_llm_results(LanguageOperation::ContentSafety, content, &["x".to_owned()]);
        assert_eq!(items.len(), 1);
        let v: serde_json::Value =
            serde_json::from_str(&items[0].content_safety_json).expect("valid json");
        assert_eq!(v["flagged"], true);
        assert_eq!(v["categories"]["hate"], 0.9);
        // non-content-safety ops leave the field empty
        let s = parse_llm_results(
            LanguageOperation::Sentiment,
            "[{\"sentiment\":\"neg\"}]",
            &["x".to_owned()],
        );
        assert!(s[0].content_safety_json.is_empty());
    }

    #[test]
    fn azure_analyze_url_uses_language_endpoint() {
        let provider = AzureLanguageProvider {
            client: reqwest::Client::new(),
            endpoint: "https://example.cognitiveservices.azure.com/".to_owned(),
            api_key: "key".to_owned(),
            api_version: "2024-11-15-preview".to_owned(),
        };
        assert_eq!(
            provider.analyze_url(),
            "https://example.cognitiveservices.azure.com/language/:analyze-text?api-version=2024-11-15-preview"
        );
    }

    #[test]
    fn azure_sentiment_doc_maps_to_result_item() {
        let doc = json!({
            "id": "1",
            "sentiment": "positive",
            "confidenceScores": { "positive": 0.9 },
            "sentences": []
        });
        let item = azure_doc_to_item(LanguageOperation::Sentiment, &doc);
        assert_eq!(item.id, "1");
        assert_eq!(item.sentiment, "positive");
        assert!(item.confidence_scores_json.contains("positive"));
    }
}
