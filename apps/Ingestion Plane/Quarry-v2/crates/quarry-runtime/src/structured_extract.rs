//! StructuredExtractClient — Quarry-side forwarder for structured extraction.
//!
//! When a scrape needs a JSON-shaped extraction over a captured artifact,
//! Quarry's job is to validate the artifact reference and ZDR mode, then
//! forward the request to Model Plane (which owns inference). Quarry does NOT
//! call providers directly — it brokers the request through the gateway,
//! enforcing cost/token limits returned in the response.
//!
//! Wire shape mirrors `quarry_core::contracts::StructuredExtractRequest/Response`.

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;

use quarry_core::contracts::{StructuredExtractRequest, StructuredExtractResponse};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::json_schema;
use quarry_core::zdr::ZdrMode;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_MAX_COST_USD: f64 = 1.00;

#[derive(Clone)]
pub struct StructuredExtractClient {
    http: Arc<Client>,
    base_url: String,
    bearer_token: Option<String>,
    default_max_cost_usd: f64,
}

impl StructuredExtractClient {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("http client build failed: {e}"),
                )
            })?;
        Ok(Self {
            http: Arc::new(http),
            base_url: base_url.into(),
            bearer_token: None,
            default_max_cost_usd: DEFAULT_MAX_COST_USD,
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    pub fn with_default_max_cost_usd(mut self, usd: f64) -> Self {
        self.default_max_cost_usd = usd;
        self
    }

    /// Forward a structured-extract request to Model Plane.
    /// Performs Quarry-side validation: ZDR consistency, cost ceiling defaults,
    /// non-empty artifact ref. Cost ceiling is enforced *after* the call:
    /// if usage exceeds `max_cost_usd`, the response is rejected with
    /// `ErrorCode::Forbidden` so callers don't accidentally bill over-budget.
    pub async fn extract(
        &self,
        req: &StructuredExtractRequest,
    ) -> QuarryResult<StructuredExtractResponse> {
        // Quarry-side validation
        if req.source_artifact_ref.to_string().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "source_artifact_ref must not be empty",
            ));
        }
        if matches!(req.zdr, ZdrMode::On) && req.markdown.is_none() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "ZDR=on requires inline markdown (no artifact lookup permitted)",
            ));
        }

        let url = format!(
            "{}/v1/structured/extract",
            self.base_url.trim_end_matches('/')
        );
        let mut builder = self.http.post(&url).json(req);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }
        let resp = builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("structured extract transport failure: {e}"),
            )
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("structured extract returned {status}: {body}"),
            ));
        }

        let mut response: StructuredExtractResponse = resp.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("structured extract decode failure: {e}"),
            )
        })?;

        // Cost ceiling enforcement (post-hoc — Model Plane is authoritative on usage)
        let ceiling = req.max_cost_usd.unwrap_or(self.default_max_cost_usd);
        if response.usage.cost_usd > ceiling {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                format!(
                    "extraction cost ${:.4} exceeded ceiling ${:.4}",
                    response.usage.cost_usd, ceiling
                ),
            ));
        }

        // Real schema validation — Model Plane reports `schema_valid` based
        // on its provider's structured-output mode, but providers regularly
        // lie (especially when JSON-mode falls back to free-text). We
        // validate Quarry-side against the schema the caller actually sent.
        if let Some(schema) = &req.structured_output_schema {
            let issues = json_schema::validate(&response.data, schema);
            if !issues.is_empty() {
                response.schema_valid = false;
                tracing::warn!(
                    issue_count = issues.len(),
                    first_issue = %issues[0].human(),
                    "structured extract returned data that failed schema validation"
                );
            } else {
                response.schema_valid = true;
            }
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::contracts::{ExtractionUsage, SourceTrace};
    use quarry_core::ids::kinds::ArtifactKind;
    use quarry_core::ids::Id;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req() -> StructuredExtractRequest {
        let artifact_id: ArtifactKind = Id::new();
        StructuredExtractRequest {
            source_artifact_ref: artifact_id,
            markdown: Some("# Hello".into()),
            structured_output_schema: Some(json!({"type": "object"})),
            source_trace_required: false,
            max_cost_usd: Some(0.50),
            max_tokens: Some(1000),
            zdr: ZdrMode::Off,
        }
    }

    fn resp_with_cost(cost_usd: f64) -> serde_json::Value {
        let artifact_id: ArtifactKind = Id::new();
        json!({
            "artifact_id": artifact_id.to_string(),
            "data": {"hello": "world"},
            "schema_valid": true,
            "usage": {"input_tokens": 100, "output_tokens": 50, "cost_usd": cost_usd},
            "model": "claude-sonnet-4-6",
            "provider": "anthropic"
        })
    }

    #[tokio::test]
    async fn extract_success_under_budget() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/structured/extract"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp_with_cost(0.10)))
            .mount(&server)
            .await;

        let client = StructuredExtractClient::new(server.uri()).unwrap();
        let result = client.extract(&req()).await.unwrap();
        assert!(result.schema_valid);
        assert_eq!(result.provider, "anthropic");
        assert!(result.usage.cost_usd < 0.50);
    }

    #[tokio::test]
    async fn extract_rejects_when_cost_exceeds_ceiling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/structured/extract"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp_with_cost(5.00)))
            .mount(&server)
            .await;

        let client = StructuredExtractClient::new(server.uri()).unwrap();
        let err = client.extract(&req()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.to_string().contains("exceeded ceiling"));
    }

    #[tokio::test]
    async fn extract_rejects_zdr_without_inline_markdown() {
        let mut r = req();
        r.zdr = ZdrMode::On;
        r.markdown = None;
        let client = StructuredExtractClient::new("http://localhost:1").unwrap();
        let err = client.extract(&r).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert!(err.to_string().contains("ZDR"));
    }

    #[tokio::test]
    async fn extract_propagates_5xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/structured/extract"))
            .respond_with(ResponseTemplate::new(500).set_body_string("upstream"))
            .mount(&server)
            .await;

        let client = StructuredExtractClient::new(server.uri()).unwrap();
        let err = client.extract(&req()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn extract_uses_default_ceiling_when_request_omits_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/structured/extract"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp_with_cost(2.50)))
            .mount(&server)
            .await;

        let mut r = req();
        r.max_cost_usd = None;

        let client = StructuredExtractClient::new(server.uri())
            .unwrap()
            .with_default_max_cost_usd(0.10);
        let err = client.extract(&r).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[test]
    fn ensure_response_has_usage() {
        // Compile-time check: ensure StructuredExtractResponse exposes the expected fields.
        let r = StructuredExtractResponse {
            artifact_id: Id::new(),
            data: json!({}),
            schema_valid: true,
            usage: ExtractionUsage {
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
            },
            source_trace: None::<SourceTrace>,
            model: "m".into(),
            provider: "p".into(),
        };
        assert_eq!(r.usage.input_tokens, 0);
    }
}
