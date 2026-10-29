//! Document intelligence provider — structured extraction from documents.

#![allow(dead_code)] // request DTO fields mirror the wire contract; read incrementally

use std::{sync::Arc, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use tokio::time::sleep;
use tracing::{info, warn};

use super::{narrow_f64, ModelInfo, ProviderError};

const DEFAULT_AZURE_API_VERSION: &str = "2024-11-30";
// `prebuilt-document` was removed in the v4 GA API (2024-11-30) → HTTP 404
// ModelNotFound. `prebuilt-layout` is its general-purpose replacement
// (text + tables + structure) and is available under this api-version.
const DEFAULT_MODEL: &str = "prebuilt-layout";
const POLL_INTERVAL_MS: u64 = 500;
const MAX_POLL_ATTEMPTS: usize = 60;

#[derive(Debug, Clone)]
pub struct AnalyzeDocumentRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub document_url: String,
    pub document_data: Vec<u8>,
    pub model: String,
    pub pages: String,
    pub locale: String,
}

#[derive(Debug, Clone)]
pub struct AnalyzeDocumentResponse {
    pub status: String,
    pub content: String,
    pub fields_json: String,
    pub tables_json: String,
    pub paragraphs: Vec<String>,
    pub raw_json: String,
    pub pages_processed: u32,
    pub confidence: f32,
    pub model_used: String,
    pub provider_used: String,
}

#[async_trait::async_trait]
pub trait DocIntelProvider: Send + Sync {
    async fn analyze_document(
        &self,
        req: &AnalyzeDocumentRequest,
    ) -> Result<AnalyzeDocumentResponse, ProviderError>;

    fn list_models(&self) -> Vec<ModelInfo>;
}

type BoxedDocIntelProvider = Arc<dyn DocIntelProvider>;

#[derive(Clone)]
pub struct DocIntelChain {
    providers: Vec<(String, BoxedDocIntelProvider)>,
}

impl DocIntelChain {
    #[must_use]
    pub fn from_env() -> Self {
        let mut providers: Vec<(String, BoxedDocIntelProvider)> = Vec::new();
        if let Some(provider) = AzureDocIntelProvider::from_env() {
            providers.push(("azure-document-intelligence".to_owned(), Arc::new(provider)));
        }
        Self { providers }
    }

    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Analyze a document using the first matching provider in the chain.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::RateLimited`] if a provider is rate limited, or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn analyze_document(
        &self,
        req: &AnalyzeDocumentRequest,
    ) -> Result<AnalyzeDocumentResponse, ProviderError> {
        let mut attempts = 0;
        for (name, provider) in &self.providers {
            if !provider_matches(name, &req.provider_hint) {
                continue;
            }
            attempts += 1;
            match provider.analyze_document(req).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::RateLimited { retry_after_ms }) => {
                    return Err(ProviderError::RateLimited { retry_after_ms });
                }
                Err(error) => warn!(provider = %name, %error, "doc-intel provider failed"),
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
pub struct AzureDocIntelProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    api_version: String,
}

impl AzureDocIntelProvider {
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let endpoint = env_nonempty("AZURE_DOCUMENT_INTELLIGENCE_ENDPOINT")
            .or_else(|| env_nonempty("AZURE_FORM_RECOGNIZER_ENDPOINT"))?;
        let api_key = env_nonempty("AZURE_DOCUMENT_INTELLIGENCE_KEY")
            .or_else(|| env_nonempty("AZURE_FORM_RECOGNIZER_KEY"))?;
        let api_version = env_nonempty("AZURE_DOCUMENT_INTELLIGENCE_API_VERSION")
            .unwrap_or_else(|| DEFAULT_AZURE_API_VERSION.to_owned());
        Some(Self {
            client: crate::provider::provider_http_client(),
            endpoint,
            api_key,
            api_version,
        })
    }

    fn analyze_url(&self, model: &str, pages: &str, locale: &str) -> String {
        let mut url = format!(
            "{}/documentintelligence/documentModels/{}:analyze?_overload=analyzeDocument&api-version={}",
            self.endpoint.trim_end_matches('/'),
            model,
            self.api_version
        );
        if !pages.trim().is_empty() {
            url.push_str("&pages=");
            url.push_str(pages.trim());
        }
        if !locale.trim().is_empty() {
            url.push_str("&locale=");
            url.push_str(locale.trim());
        }
        url
    }

    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.header("Ocp-Apim-Subscription-Key", &self.api_key)
    }

    async fn poll_result(&self, operation_url: &str) -> Result<serde_json::Value, ProviderError> {
        for _ in 0..MAX_POLL_ATTEMPTS {
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
                        "document analysis failed: {}",
                        json["error"]
                    )));
                }
                _ => sleep(Duration::from_millis(POLL_INTERVAL_MS)).await,
            }
        }
        Err(ProviderError::Unavailable(
            "document analysis polling timed out".to_owned(),
        ))
    }
}

#[async_trait::async_trait]
impl DocIntelProvider for AzureDocIntelProvider {
    async fn analyze_document(
        &self,
        req: &AnalyzeDocumentRequest,
    ) -> Result<AnalyzeDocumentResponse, ProviderError> {
        let model = if req.model.trim().is_empty() {
            DEFAULT_MODEL
        } else {
            req.model.trim()
        };
        let url = self.analyze_url(model, &req.pages, &req.locale);
        let request_body = if req.document_data.is_empty() {
            json!({ "urlSource": req.document_url })
        } else {
            json!({ "base64Source": STANDARD.encode(&req.document_data) })
        };
        let request = self.auth(self.client.post(url)).json(&request_body);

        let response = request
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after_ms = retry_after_ms(&response);
            return Err(ProviderError::RateLimited { retry_after_ms });
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
            .ok_or_else(|| {
                ProviderError::InvalidResponse("missing operation-location header".to_owned())
            })?
            .to_owned();

        let result = self.poll_result(&operation_url).await?;
        let analysis = result.get("analyzeResult").unwrap_or(&result);
        let response = analyze_result_to_response(analysis, model, "azure-document-intelligence");

        info!(
            model = model,
            provider = "azure-document-intelligence",
            "document analysis completed"
        );

        Ok(response)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        [
            "prebuilt-document",
            "prebuilt-layout",
            "prebuilt-read",
            "prebuilt-receipt",
            "prebuilt-invoice",
            "prebuilt-businessCard",
            "prebuilt-idDocument",
        ]
        .into_iter()
        .map(|id| ModelInfo {
            id: id.to_owned(),
            provider: "azure-document-intelligence".to_owned(),
            modality: "document_intelligence".to_owned(),
            streaming: false,
            ..Default::default()
        })
        .collect()
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

fn analyze_result_to_response(
    analysis: &serde_json::Value,
    model: &str,
    provider: &str,
) -> AnalyzeDocumentResponse {
    let fields = extract_fields(analysis);
    let paragraphs: Vec<String> = analysis["paragraphs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|paragraph| paragraph["content"].as_str().map(ToOwned::to_owned))
        .collect();
    let pages_processed = analysis["pages"]
        .as_array()
        .and_then(|pages| u32::try_from(pages.len()).ok())
        .unwrap_or(0);
    let confidence = narrow_f64(
        analysis["documents"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|doc| doc["confidence"].as_f64())
            .unwrap_or(0.0),
    );

    AnalyzeDocumentResponse {
        status: "succeeded".to_owned(),
        content: analysis["content"].as_str().unwrap_or("").to_owned(),
        fields_json: serde_json::to_string(&fields).unwrap_or_else(|_| "{}".to_owned()),
        tables_json: serde_json::to_string(&analysis["tables"]).unwrap_or_else(|_| "[]".to_owned()),
        paragraphs,
        raw_json: serde_json::to_string(analysis).unwrap_or_else(|_| "{}".to_owned()),
        pages_processed,
        confidence,
        model_used: model.to_owned(),
        provider_used: provider.to_owned(),
    }
}

fn extract_fields(analysis: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    let mut fields = serde_json::Map::new();
    for document in analysis["documents"].as_array().into_iter().flatten() {
        for (key, value) in document["fields"].as_object().into_iter().flatten() {
            let extracted = value
                .get("content")
                .cloned()
                .or_else(|| value.get("valueString").cloned())
                .or_else(|| value.get("value").cloned())
                .unwrap_or_else(|| value.clone());
            fields.insert(key.clone(), extracted);
        }
    }
    fields
}

fn provider_matches(name: &str, hint: &str) -> bool {
    let hint = hint.trim().to_ascii_lowercase();
    hint.is_empty()
        || hint == name
        || (hint == "azure" && name == "azure-document-intelligence")
        || (hint == "document-intelligence" && name == "azure-document-intelligence")
        || (hint == "azure-document-intelligence" && name == "azure-document-intelligence")
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

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_analyze_url_includes_model_and_filters() {
        let provider = AzureDocIntelProvider {
            client: crate::provider::provider_http_client(),
            endpoint: "https://example.cognitiveservices.azure.com/".to_owned(),
            api_key: "key".to_owned(),
            api_version: "2024-11-30".to_owned(),
        };

        assert_eq!(
            provider.analyze_url("prebuilt-layout", "1-2", "en-US"),
            "https://example.cognitiveservices.azure.com/documentintelligence/documentModels/prebuilt-layout:analyze?_overload=analyzeDocument&api-version=2024-11-30&pages=1-2&locale=en-US"
        );
    }

    #[test]
    fn extracts_fields_paragraphs_and_pages() {
        let analysis = json!({
            "content": "hello",
            "pages": [{ "pageNumber": 1 }],
            "paragraphs": [{ "content": "hello" }],
            "tables": [{ "rowCount": 1 }],
            "documents": [{
                "confidence": 0.9,
                "fields": {
                    "VendorName": { "content": "ACME" }
                }
            }]
        });

        let response = analyze_result_to_response(
            &analysis,
            "prebuilt-invoice",
            "azure-document-intelligence",
        );
        assert_eq!(response.content, "hello");
        assert_eq!(response.paragraphs, vec!["hello"]);
        assert_eq!(response.pages_processed, 1);
        assert!(response.fields_json.contains("VendorName"));
        assert!(response.tables_json.contains("rowCount"));
    }
}
