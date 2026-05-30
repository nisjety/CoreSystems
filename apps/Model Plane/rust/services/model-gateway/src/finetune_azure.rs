//! Azure OpenAI fine-tuning HTTP client (Wave 7 slice 2a).
//!
//! Five operations against the Azure OpenAI data-plane REST API:
//!
//!   1. `upload_training_file` — POST `/openai/files?purpose=fine-tune` (multipart).
//!   2. `create_finetune_job`  — POST `/openai/fine_tuning/jobs`.
//!   3. `get_finetune_job`     — GET  `/openai/fine_tuning/jobs/{id}`.
//!   4. `cancel_finetune_job`  — POST `/openai/fine_tuning/jobs/{id}/cancel`.
//!   5. `create_deployment`    — PUT  `/openai/deployments/{name}` (data-plane).
//!
//! Auth header: `api-key: {AZURE_OPENAI_API_KEY}` on every request.
//! Query string: `?api-version={AZURE_OPENAI_API_VERSION}` on every request.
//!
//! The struct is constructed via [`AzureFinetuneClient::from_env`] which returns
//! `None` when the required envs are missing — routes degrade gracefully and
//! persist the job row with empty `azure_*` so the operator can see the row
//! and trace the env-config error. [`AzureFinetuneClient::with_overrides`] is
//! exposed for tests so wiremock can swap the endpoint.

use reqwest::multipart;
use serde::Deserialize;
use thiserror::Error;

const DEFAULT_API_VERSION: &str = "2024-08-01-preview";

#[derive(Debug, Error)]
pub enum AzureError {
    #[error("azure http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("azure returned {status}: {body}")]
    Api { status: u16, body: String },

    #[error("invalid azure response: {0}")]
    Decode(String),
}

/// Subset of Azure FT job fields we care about. Other fields exist (cost
/// estimates, hyperparameters echo, training/validation file ids, etc.) but
/// we only persist what the gateway/poller need.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AzureJobStatus {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub fine_tuned_model: String,
    #[serde(default)]
    pub error: Option<AzureJobError>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AzureJobError {
    pub code: String,
    pub message: String,
}

/// Hyperparameter shape Azure accepts. Every field is optional — when omitted
/// Azure falls back to its defaults. We mirror the OpenAI surface (n_epochs /
/// batch_size / learning_rate_multiplier) rather than inventing our own.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AzureHyperparameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_epochs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning_rate_multiplier: Option<f64>,
}

/// Configurable client. Clone is cheap — `reqwest::Client` is `Arc<Inner>`.
#[derive(Debug, Clone)]
pub struct AzureFinetuneClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    api_version: String,
}

impl AzureFinetuneClient {
    /// Build from environment. Returns `None` if `AZURE_OPENAI_ENDPOINT` or
    /// `AZURE_OPENAI_API_KEY` is unset/empty. Callers should treat None as
    /// "Azure not configured; persist row with empty azure_* and let the
    /// operator fix the env".
    #[must_use]
    pub fn from_env(http: reqwest::Client) -> Option<Self> {
        let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT").ok()?;
        let api_key = std::env::var("AZURE_OPENAI_API_KEY").ok()?;
        if endpoint.is_empty() || api_key.is_empty() {
            return None;
        }
        let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|_| DEFAULT_API_VERSION.to_owned());
        Some(Self {
            http,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            api_key,
            api_version,
        })
    }

    /// Test/dev constructor that lets wiremock provide the endpoint.
    #[must_use]
    pub fn with_overrides(
        http: reqwest::Client,
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        api_version: impl Into<String>,
    ) -> Self {
        Self {
            http,
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            api_version: api_version.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{endpoint}{path}?api-version={ver}",
            endpoint = self.endpoint,
            path = path,
            ver = self.api_version,
        )
    }

    /// Upload a JSONL training file. Returns the Azure file_id (e.g. `file-abc123`).
    pub async fn upload_training_file(
        &self,
        bytes: Vec<u8>,
        filename: &str,
    ) -> Result<String, AzureError> {
        let part = multipart::Part::bytes(bytes)
            .file_name(filename.to_owned())
            .mime_str("application/jsonl")
            .map_err(|e| AzureError::Decode(format!("multipart mime: {e}")))?;

        let form = multipart::Form::new()
            .part("file", part)
            .text("purpose", "fine-tune");

        let resp = self
            .http
            .post(self.url("/openai/files"))
            .header("api-key", &self.api_key)
            .multipart(form)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(AzureError::Api {
                status: status.as_u16(),
                body,
            });
        }
        // Response shape: { "id": "file-abc", ... }
        #[derive(Deserialize)]
        struct FileResp {
            id: String,
        }
        let parsed: FileResp = serde_json::from_str(&body)
            .map_err(|e| AzureError::Decode(format!("upload response: {e}")))?;
        Ok(parsed.id)
    }

    /// Kick off a fine-tuning job against a previously-uploaded training file.
    /// Returns the Azure job id (e.g. `ftjob-xyz`).
    pub async fn create_finetune_job(
        &self,
        base_model: &str,
        training_file_id: &str,
        hyperparameters: Option<AzureHyperparameters>,
        suffix: Option<&str>,
    ) -> Result<String, AzureError> {
        let mut body = serde_json::json!({
            "model": base_model,
            "training_file": training_file_id,
        });
        if let Some(hp) = hyperparameters {
            body["hyperparameters"] = serde_json::to_value(hp)
                .map_err(|e| AzureError::Decode(format!("encode hp: {e}")))?;
        }
        if let Some(s) = suffix {
            body["suffix"] = serde_json::Value::String(s.to_owned());
        }

        let resp = self
            .http
            .post(self.url("/openai/fine_tuning/jobs"))
            .header("api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let body_text = resp.text().await?;
        if !status.is_success() {
            return Err(AzureError::Api {
                status: status.as_u16(),
                body: body_text,
            });
        }
        #[derive(Deserialize)]
        struct JobResp {
            id: String,
        }
        let parsed: JobResp = serde_json::from_str(&body_text)
            .map_err(|e| AzureError::Decode(format!("create job response: {e}")))?;
        Ok(parsed.id)
    }

    /// Refresh a job's current state from Azure. Used by the polling worker
    /// (slice 2b) and by `GET /v1/finetune/jobs/:id` when `status='running'`.
    pub async fn get_finetune_job(&self, job_id: &str) -> Result<AzureJobStatus, AzureError> {
        let path = format!("/openai/fine_tuning/jobs/{job_id}");
        let resp = self
            .http
            .get(self.url(&path))
            .header("api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        let body_text = resp.text().await?;
        if !status.is_success() {
            return Err(AzureError::Api {
                status: status.as_u16(),
                body: body_text,
            });
        }
        serde_json::from_str::<AzureJobStatus>(&body_text)
            .map_err(|e| AzureError::Decode(format!("get job response: {e}")))
    }

    /// Cancel an in-flight job. Azure returns the post-cancel job document;
    /// we only care that the call succeeded.
    pub async fn cancel_finetune_job(&self, job_id: &str) -> Result<(), AzureError> {
        let path = format!("/openai/fine_tuning/jobs/{job_id}/cancel");
        let resp = self
            .http
            .post(self.url(&path))
            .header("api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AzureError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(())
    }

    /// Provision a deployment for a fine-tuned model so the gateway can route
    /// traffic to it. Idempotent — repeated calls with the same name return 200.
    pub async fn create_deployment(
        &self,
        deployment_name: &str,
        model_name: &str,
    ) -> Result<(), AzureError> {
        let path = format!("/openai/deployments/{deployment_name}");
        let body = serde_json::json!({
            "model": model_name,
            "scale_settings": { "scale_type": "standard" },
        });
        let resp = self
            .http
            .put(self.url(&path))
            .header("api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AzureError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> AzureFinetuneClient {
        AzureFinetuneClient::with_overrides(
            reqwest::Client::new(),
            server.uri(),
            "test-key",
            "2024-08-01-preview",
        )
    }

    #[tokio::test]
    async fn upload_training_file_returns_file_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/files"))
            .and(query_param("api-version", "2024-08-01-preview"))
            .and(header("api-key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "file-abc123",
                "object": "file",
                "purpose": "fine-tune",
                "status": "uploaded",
            })))
            .mount(&server)
            .await;

        let c = client_for(&server);
        let id = c
            .upload_training_file(b"{}\n".to_vec(), "train.jsonl")
            .await
            .expect("upload");
        assert_eq!(id, "file-abc123");
    }

    #[tokio::test]
    async fn upload_training_file_surfaces_api_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/files"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad jsonl"))
            .mount(&server)
            .await;

        let c = client_for(&server);
        let err = c
            .upload_training_file(b"x".to_vec(), "t.jsonl")
            .await
            .unwrap_err();
        match err {
            AzureError::Api { status, body } => {
                assert_eq!(status, 400);
                assert_eq!(body, "bad jsonl");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_finetune_job_returns_job_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/fine_tuning/jobs"))
            .and(query_param("api-version", "2024-08-01-preview"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ftjob-xyz",
                "object": "fine_tuning.job",
                "status": "running",
            })))
            .mount(&server)
            .await;

        let c = client_for(&server);
        let id = c
            .create_finetune_job(
                "gpt-4o-mini-2024-07-18",
                "file-abc123",
                Some(AzureHyperparameters {
                    n_epochs: Some(3),
                    ..Default::default()
                }),
                Some("acme-support-v1"),
            )
            .await
            .expect("create");
        assert_eq!(id, "ftjob-xyz");
    }

    #[tokio::test]
    async fn get_finetune_job_parses_succeeded_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/openai/fine_tuning/jobs/ftjob-xyz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ftjob-xyz",
                "status": "succeeded",
                "fine_tuned_model": "gpt-4o-mini-2024-07-18.ft-xyz",
            })))
            .mount(&server)
            .await;

        let c = client_for(&server);
        let job = c.get_finetune_job("ftjob-xyz").await.expect("get");
        assert_eq!(job.status, "succeeded");
        assert_eq!(job.fine_tuned_model, "gpt-4o-mini-2024-07-18.ft-xyz");
        assert!(job.error.is_none());
    }

    #[tokio::test]
    async fn get_finetune_job_parses_failed_with_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/openai/fine_tuning/jobs/ftjob-bad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ftjob-bad",
                "status": "failed",
                "error": {
                    "code": "invalid_training_file",
                    "message": "Training file must contain at least 10 examples.",
                },
            })))
            .mount(&server)
            .await;

        let c = client_for(&server);
        let job = c.get_finetune_job("ftjob-bad").await.expect("get");
        assert_eq!(job.status, "failed");
        let err = job.error.expect("error populated");
        assert_eq!(err.code, "invalid_training_file");
        assert!(err.message.contains("at least 10"));
    }

    #[tokio::test]
    async fn cancel_finetune_job_succeeds_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/fine_tuning/jobs/ftjob-xyz/cancel"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ftjob-xyz",
                "status": "cancelled",
            })))
            .mount(&server)
            .await;

        let c = client_for(&server);
        c.cancel_finetune_job("ftjob-xyz").await.expect("cancel");
    }

    #[tokio::test]
    async fn cancel_finetune_job_surfaces_404() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/fine_tuning/jobs/ftjob-missing/cancel"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let c = client_for(&server);
        let err = c.cancel_finetune_job("ftjob-missing").await.unwrap_err();
        match err {
            AzureError::Api { status, .. } => assert_eq!(status, 404),
            other => panic!("expected Api 404, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_deployment_succeeds_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/openai/deployments/acme-support-v1"))
            .and(query_param("api-version", "2024-08-01-preview"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let c = client_for(&server);
        c.create_deployment("acme-support-v1", "gpt-4o-mini-2024-07-18.ft-xyz")
            .await
            .expect("deploy");
    }

    #[test]
    fn from_env_returns_none_when_endpoint_missing() {
        // Pure logic check — we never set these envs in this test, so they
        // are either unset (None) or set externally. If externally set we
        // skip the assertion to keep the suite hermetic.
        if std::env::var("AZURE_OPENAI_ENDPOINT").is_err()
            && std::env::var("AZURE_OPENAI_API_KEY").is_err()
        {
            assert!(AzureFinetuneClient::from_env(reqwest::Client::new()).is_none());
        }
    }

    #[test]
    fn with_overrides_trims_trailing_slash_on_endpoint() {
        let c = AzureFinetuneClient::with_overrides(
            reqwest::Client::new(),
            "https://example.openai.azure.com/",
            "k",
            "v",
        );
        assert_eq!(
            c.url("/openai/files"),
            "https://example.openai.azure.com/openai/files?api-version=v"
        );
    }
}
