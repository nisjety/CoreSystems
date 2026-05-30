//! AI output formats: `summary`, `json`, `query`.
//!
//! Quarry doesn't synthesize content — it owns the source artifact. These
//! transforms call Model Plane (via the structured-extract path or `/v1/invoke`)
//! and return validated artifacts. Each transform:
//!
//! 1. Loads the markdown artifact (or accepts inline markdown)
//! 2. Calls Model Plane with a deterministic prompt template
//! 3. Validates the output (length cap, JSON schema)
//! 4. Returns a typed result + cost telemetry
//!
//! ZDR semantics: when ZDR=on, the source markdown is sent to Model Plane
//! ephemerally only — Model Plane is contracted to not persist it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::json_schema;
use quarry_core::zdr::ZdrMode;

use crate::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};

const DEFAULT_MAX_SUMMARY_CHARS: usize = 4_000;

#[derive(Clone)]
pub struct AiFormatRunner {
    client: Arc<ModelPlaneClient>,
    model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResult {
    pub summary: String,
    pub model: String,
    pub source_chars: usize,
    pub summary_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonResult {
    pub data: serde_json::Value,
    pub schema_valid: bool,
    /// Per-issue list when validation failed; empty when `schema_valid=true`.
    /// Surfaces specific paths ("$.user.age expected integer, got string")
    /// so callers can decide whether to retry, repair, or fail loudly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_issues: Vec<String>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub answer: String,
    pub model: String,
    pub query: String,
}

impl AiFormatRunner {
    pub fn new(client: Arc<ModelPlaneClient>) -> Self {
        Self {
            client,
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Generate a concise summary of the markdown.
    /// `max_chars` caps the output length (defaults to 4 000 chars).
    pub async fn summary(
        &self,
        markdown: &str,
        max_chars: Option<usize>,
        zdr: ZdrMode,
    ) -> QuarryResult<SummaryResult> {
        validate_input(markdown, zdr)?;
        let cap = max_chars.unwrap_or(DEFAULT_MAX_SUMMARY_CHARS);

        let prompt = format!(
            "Summarize the following markdown in <= {cap} characters. \
             Keep facts and proper nouns; remove fluff. Output ONLY the summary, no preamble.\n\n\
             ===\n{markdown}\n==="
        );

        let resp = self.invoke(&prompt).await?;
        let summary = resp.content.trim().to_string();
        let mut summary = summary;
        if summary.chars().count() > cap {
            summary = summary.chars().take(cap).collect();
        }

        Ok(SummaryResult {
            source_chars: markdown.chars().count(),
            summary_chars: summary.chars().count(),
            summary,
            model: resp.model_used,
        })
    }

    /// Extract structured JSON from the markdown using `schema` as a JSON
    /// Schema. The schema is sent to Model Plane via the invoke prompt.
    /// Stronger schema enforcement happens at [`crate::structured_extract::StructuredExtractClient`].
    pub async fn json(
        &self,
        markdown: &str,
        schema: serde_json::Value,
        zdr: ZdrMode,
    ) -> QuarryResult<JsonResult> {
        validate_input(markdown, zdr)?;
        let schema_str = serde_json::to_string_pretty(&schema).map_err(|e| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("schema must be valid JSON: {e}"),
            )
        })?;

        let prompt = format!(
            "Extract structured data from the following markdown that conforms to this JSON Schema:\n\
             ```json\n{schema_str}\n```\n\n\
             Markdown:\n===\n{markdown}\n===\n\n\
             Return ONLY a valid JSON document matching the schema. No prose."
        );

        let resp = self.invoke(&prompt).await?;
        let body = strip_code_fences(&resp.content);
        let data: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model returned non-JSON: {e}"),
            )
        })?;
        let issues = json_schema::validate(&data, &schema);
        let schema_valid = issues.is_empty();
        let schema_issues = issues.iter().map(|i| i.human()).collect();
        Ok(JsonResult {
            data,
            schema_valid,
            schema_issues,
            model: resp.model_used,
        })
    }

    /// Answer a question grounded in the markdown.
    pub async fn query(
        &self,
        markdown: &str,
        question: &str,
        zdr: ZdrMode,
    ) -> QuarryResult<QueryResult> {
        validate_input(markdown, zdr)?;
        if question.trim().is_empty() {
            return Err(QuarryError::new(ErrorCode::BadRequest, "question is empty"));
        }

        let prompt = format!(
            "Answer the question using ONLY the markdown below. If the markdown does not \
             contain the answer, say \"Not found in source\". Output ONLY the answer.\n\n\
             Question: {question}\n\nMarkdown:\n===\n{markdown}\n==="
        );

        let resp = self.invoke(&prompt).await?;
        Ok(QueryResult {
            answer: resp.content.trim().to_string(),
            model: resp.model_used,
            query: question.to_string(),
        })
    }

    async fn invoke(
        &self,
        content: &str,
    ) -> QuarryResult<crate::mp_client::ModelPlaneInvokeResponse> {
        let req = ModelPlaneInvokeRequest {
            content: content.to_string(),
            model: self.model.clone(),
            session_key: None,
            thread_id: None,
        };
        self.client.invoke(&req).await
    }
}

fn validate_input(markdown: &str, zdr: ZdrMode) -> QuarryResult<()> {
    if markdown.trim().is_empty() {
        return Err(QuarryError::new(ErrorCode::BadRequest, "markdown is empty"));
    }
    // ZDR=on is fine to send to Model Plane (Model Plane is contracted to not persist),
    // but the caller is responsible for ensuring no durable artifact is created on return.
    let _ = zdr;
    Ok(())
}

fn strip_code_fences(body: &str) -> String {
    let trimmed = body.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mp_response(content: &str) -> serde_json::Value {
        json!({
            "request_id": "req_1",
            "content": content,
            "model_used": "claude-sonnet-4-6",
        })
    }

    async fn make_runner(server_uri: &str) -> AiFormatRunner {
        let client = Arc::new(ModelPlaneClient::new(server_uri).unwrap());
        AiFormatRunner::new(client)
    }

    #[tokio::test]
    async fn summary_returns_expected_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response("Tldr: bla")))
            .mount(&server)
            .await;

        let runner = make_runner(&server.uri()).await;
        let r = runner
            .summary("# Hello\n\nQuarry rules.", Some(50), ZdrMode::Off)
            .await
            .unwrap();
        assert!(r.summary.contains("Tldr"));
        assert!(r.summary_chars <= 50);
        assert_eq!(r.model, "claude-sonnet-4-6");
    }

    #[tokio::test]
    async fn summary_caps_output_length() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response(&"x".repeat(500))))
            .mount(&server)
            .await;

        let runner = make_runner(&server.uri()).await;
        let r = runner
            .summary("body", Some(100), ZdrMode::Off)
            .await
            .unwrap();
        assert_eq!(r.summary_chars, 100);
    }

    #[tokio::test]
    async fn summary_rejects_empty_markdown() {
        let runner = make_runner("http://localhost:1").await;
        let err = runner.summary("  ", None, ZdrMode::Off).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn json_extracts_and_validates() {
        let server = MockServer::start().await;
        let body = "```json\n{\"name\": \"Alice\", \"age\": 30}\n```";
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response(body)))
            .mount(&server)
            .await;

        let runner = make_runner(&server.uri()).await;
        let schema = json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        let r = runner
            .json("Alice is 30", schema, ZdrMode::Off)
            .await
            .unwrap();
        assert_eq!(r.data["name"], "Alice");
        assert_eq!(r.data["age"], 30);
        assert!(r.schema_valid);
    }

    #[tokio::test]
    async fn json_invalid_when_required_missing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mp_response("{\"name\": \"A\"}")),
            )
            .mount(&server)
            .await;

        let runner = make_runner(&server.uri()).await;
        let schema = json!({"type": "object", "required": ["name", "age"]});
        let r = runner.json("A", schema, ZdrMode::Off).await.unwrap();
        assert!(!r.schema_valid);
    }

    #[tokio::test]
    async fn json_returns_error_when_model_returns_non_json() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response("not json")))
            .mount(&server)
            .await;

        let runner = make_runner(&server.uri()).await;
        let err = runner
            .json("md", json!({}), ZdrMode::Off)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn query_returns_grounded_answer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response("42")))
            .mount(&server)
            .await;

        let runner = make_runner(&server.uri()).await;
        let r = runner
            .query("The answer is 42", "What is the answer?", ZdrMode::Off)
            .await
            .unwrap();
        assert_eq!(r.answer, "42");
        assert_eq!(r.query, "What is the answer?");
    }

    #[tokio::test]
    async fn query_rejects_empty_question() {
        let runner = make_runner("http://localhost:1").await;
        let err = runner.query("md", "  ", ZdrMode::Off).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
