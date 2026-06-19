//! Azure `OpenAI` fine-tuning HTTP client (Wave 7 slice 2a).
//!
//! Data-plane operations against the Azure `OpenAI` REST API:
//!
//!   1. `upload_training_file` — POST `/openai/files?purpose=fine-tune` (multipart).
//!   2. `create_finetune_job`  — POST `/openai/fine_tuning/jobs`.
//!   3. `get_finetune_job`     — GET  `/openai/fine_tuning/jobs/{id}`.
//!   4. `cancel_finetune_job`  — POST `/openai/fine_tuning/jobs/{id}/cancel`.
//!
//! Auth header: `api-key: {AZURE_OPENAI_API_KEY}` on every data-plane request.
//! Query string: `?api-version={AZURE_OPENAI_API_VERSION}` on every data-plane request.
//!
//! Management-plane operation:
//!
//!   5. `create_deployment` — PUT against the Azure Resource Manager
//!      `Microsoft.CognitiveServices/accounts/{account}/deployments/{name}`
//!      resource (api-version `2024-10-01`). Deployments of fine-tuned models
//!      (and the Developer/Standard SKU) are provisioned through ARM, *not*
//!      the data-plane, and authenticate with an Azure AD bearer token obtained
//!      via the client-credentials grant. See [`AzureMgmtConfig`] +
//!      [`AzureFinetuneClient::mgmt_token`].
//!
//! The struct is constructed via [`AzureFinetuneClient::from_env`] which returns
//! `None` when the required data-plane envs are missing — routes degrade
//! gracefully and persist the job row with empty `azure_*` so the operator can
//! see the row and trace the env-config error. The management-plane config is
//! *separately* optional: when its envs are unset, `create_deployment` returns
//! [`AzureError::MgmtNotConfigured`] so callers can surface a clear 503 (or, in
//! the poller, log + retry) instead of panicking.
//!
//! [`AzureFinetuneClient::with_overrides`] is exposed for tests so wiremock can
//! swap both the data-plane endpoint and (via
//! [`AzureFinetuneClient::with_mgmt_overrides`]) the management + AAD endpoints.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::multipart;
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::Mutex;

const DEFAULT_API_VERSION: &str = "2024-08-01-preview";
/// Default management-plane (ARM) api-version for `accounts/deployments`.
/// Overridable via `AZURE_MGMT_API_VERSION`.
const DEFAULT_MGMT_API_VERSION: &str = "2024-10-01";
/// Azure Resource Manager root. Tests override via [`AzureMgmtConfig`].
const MGMT_BASE_URL: &str = "https://management.azure.com";
/// Azure AD login authority root. Tests override via [`AzureMgmtConfig`].
const AAD_AUTHORITY_BASE_URL: &str = "https://login.microsoftonline.com";
/// ARM scope requested in the client-credentials grant.
const MGMT_SCOPE: &str = "https://management.azure.com/.default";
/// Refresh the cached AAD token this many seconds *before* its stated expiry,
/// so an in-flight deploy never races a mid-flight expiry.
const TOKEN_EXPIRY_SKEW_SECS: u64 = 120;
/// Floor for the cached-token lifetime when AAD omits or under-reports
/// `expires_in`, so we never cache a near-instantly-stale token.
const MIN_TOKEN_LIFETIME_SECS: u64 = 300;

/// The deployment SKU tier. Auto-deploys (poller) always use `Developer`
/// ($0/hr, auto-deletes after 24h); an explicit operator promote uses
/// `Production` (paid Standard hosting). The Azure SKU name differs from our
/// label — [`DeploymentTier::azure_sku`] maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentTier {
    /// Free, auto-expiring test tier. Azure SKU name `Developer`.
    Developer,
    /// Paid, persistent hosting tier. Azure SKU name `Standard`.
    Production,
}

impl DeploymentTier {
    /// The Azure SKU `name` value for this tier.
    #[must_use]
    pub fn azure_sku(self) -> &'static str {
        match self {
            DeploymentTier::Developer => "Developer",
            DeploymentTier::Production => "Standard",
        }
    }

    /// The lowercase wire label persisted on the job row + surfaced to the UI.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DeploymentTier::Developer => "developer",
            DeploymentTier::Production => "production",
        }
    }

    /// Parse a wire label back into a tier. Unknown values default to
    /// `Developer` (the safe, free tier) so a bad client string can never
    /// silently provision the paid SKU.
    #[must_use]
    pub fn from_str_or_developer(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "production" | "standard" => DeploymentTier::Production,
            _ => DeploymentTier::Developer,
        }
    }
}

#[derive(Debug, Error)]
pub enum AzureError {
    #[error("azure http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("azure returned {status}: {body}")]
    Api { status: u16, body: String },

    #[error("invalid azure response: {0}")]
    Decode(String),

    #[error("azure management plane not configured (set AZURE_SUBSCRIPTION_ID, AZURE_RESOURCE_GROUP, AZURE_OPENAI_ACCOUNT_NAME, AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET)")]
    MgmtNotConfigured,
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
/// Azure falls back to its defaults. We mirror the `OpenAI` surface (`n_epochs` /
/// `batch_size` / `learning_rate_multiplier`) rather than inventing our own.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AzureHyperparameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_epochs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning_rate_multiplier: Option<f64>,
}

/// Management-plane (ARM) + Azure AD client-credentials configuration. All
/// fields are required to provision a deployment; the whole struct is `Option`
/// on the client so the data-plane surface works even when ARM is unconfigured.
///
/// `mgmt_base_url` / `aad_base_url` default to the public Azure clouds but are
/// overridable so wiremock can stand in for both in tests.
#[derive(Debug, Clone)]
pub struct AzureMgmtConfig {
    pub subscription_id: String,
    pub resource_group: String,
    pub account_name: String,
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub api_version: String,
    /// ARM root, e.g. `https://management.azure.com`.
    pub mgmt_base_url: String,
    /// AAD login authority root, e.g. `https://login.microsoftonline.com`.
    pub aad_base_url: String,
}

impl AzureMgmtConfig {
    /// Read all six required ARM/AAD envs. Returns `None` if *any* is unset or
    /// empty — partial config can't provision a deployment, so we fail closed
    /// to "not configured" rather than half-build a broken request.
    ///
    /// Optional `AZURE_MGMT_API_VERSION` defaults to [`DEFAULT_MGMT_API_VERSION`].
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let subscription_id = non_empty_env("AZURE_SUBSCRIPTION_ID")?;
        let resource_group = non_empty_env("AZURE_RESOURCE_GROUP")?;
        let account_name = non_empty_env("AZURE_OPENAI_ACCOUNT_NAME")?;
        let tenant_id = non_empty_env("AZURE_TENANT_ID")?;
        let client_id = non_empty_env("AZURE_CLIENT_ID")?;
        let client_secret = non_empty_env("AZURE_CLIENT_SECRET")?;
        let api_version = std::env::var("AZURE_MGMT_API_VERSION")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MGMT_API_VERSION.to_owned());
        Some(Self {
            subscription_id,
            resource_group,
            account_name,
            tenant_id,
            client_id,
            client_secret,
            api_version,
            mgmt_base_url: MGMT_BASE_URL.to_owned(),
            aad_base_url: AAD_AUTHORITY_BASE_URL.to_owned(),
        })
    }
}

/// Fetch an env var, returning `None` if unset or empty/whitespace.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// A bearer token plus the [`Instant`] after which it must be refreshed
/// (already adjusted for [`TOKEN_EXPIRY_SKEW_SECS`]).
#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    refresh_after: Instant,
}

/// Configurable client. Clone is cheap — `reqwest::Client` is `Arc<Inner>`,
/// `AzureMgmtConfig` is small, and the token cache is an `Arc<Mutex<_>>`.
#[derive(Debug, Clone)]
pub struct AzureFinetuneClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    api_version: String,
    /// `None` when the management-plane envs are unset; `create_deployment`
    /// then returns [`AzureError::MgmtNotConfigured`].
    mgmt: Option<AzureMgmtConfig>,
    /// Cached AAD client-credentials token, refreshed lazily on expiry.
    token_cache: Arc<Mutex<Option<CachedToken>>>,
}

impl AzureFinetuneClient {
    /// Build from environment. Returns `None` if `AZURE_OPENAI_ENDPOINT` or
    /// `AZURE_OPENAI_API_KEY` is unset/empty. Callers should treat None as
    /// "Azure not configured; persist row with empty azure_* and let the
    /// operator fix the env".
    ///
    /// The management-plane config ([`AzureMgmtConfig`]) is read *separately*
    /// from [`AzureMgmtConfig::from_env`]; it may be `None` even when the
    /// data-plane is configured (deployments simply can't be provisioned until
    /// the operator sets the ARM/AAD envs).
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
            mgmt: AzureMgmtConfig::from_env(),
            token_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Test/dev constructor that lets wiremock provide the data-plane endpoint.
    /// Management-plane config is left unset — use
    /// [`AzureFinetuneClient::with_mgmt_overrides`] to attach one.
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
            mgmt: None,
            token_cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Test/dev constructor that attaches a management-plane config (with
    /// wiremock-supplied ARM + AAD base URLs) to a data-plane client.
    #[must_use]
    pub fn with_mgmt_overrides(mut self, mgmt: AzureMgmtConfig) -> Self {
        self.mgmt = Some(mgmt);
        self.token_cache = Arc::new(Mutex::new(None));
        self
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{endpoint}{path}?api-version={ver}",
            endpoint = self.endpoint,
            path = path,
            ver = self.api_version,
        )
    }

    /// `true` when the management-plane config is present, i.e.
    /// `create_deployment` can actually provision a deployment rather than
    /// returning [`AzureError::MgmtNotConfigured`].
    #[must_use]
    pub fn mgmt_configured(&self) -> bool {
        self.mgmt.is_some()
    }

    /// Return a valid AAD bearer token for the ARM scope, using the cache when
    /// still fresh and otherwise requesting a new one via the client-credentials
    /// grant. Serialized through the cache mutex so concurrent deploys share a
    /// single token request.
    ///
    /// # Errors
    ///
    /// Returns [`AzureError::MgmtNotConfigured`] when no management config is
    /// set, [`AzureError::Http`] on transport failure, [`AzureError::Api`] when
    /// AAD returns a non-success status, or [`AzureError::Decode`] when the token
    /// response cannot be parsed.
    async fn mgmt_token(&self, mgmt: &AzureMgmtConfig) -> Result<String, AzureError> {
        // AAD client-credentials token response. `expires_in` is seconds.
        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
            #[serde(default)]
            expires_in: u64,
        }

        let mut guard = self.token_cache.lock().await;
        if let Some(cached) = guard.as_ref() {
            if Instant::now() < cached.refresh_after {
                return Ok(cached.token.clone());
            }
        }

        let token_url = format!(
            "{base}/{tenant}/oauth2/v2.0/token",
            base = mgmt.aad_base_url.trim_end_matches('/'),
            tenant = mgmt.tenant_id,
        );
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", mgmt.client_id.as_str()),
            ("client_secret", mgmt.client_secret.as_str()),
            ("scope", MGMT_SCOPE),
        ];
        let resp = self.http.post(token_url).form(&form).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(AzureError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let parsed: TokenResp = serde_json::from_str(&body)
            .map_err(|e| AzureError::Decode(format!("aad token response: {e}")))?;

        // Refresh slightly early. If `expires_in` is missing/tiny, fall back to
        // a conservative 5-minute lifetime so we never cache a stale token.
        let lifetime = parsed
            .expires_in
            .max(MIN_TOKEN_LIFETIME_SECS)
            .saturating_sub(TOKEN_EXPIRY_SKEW_SECS)
            .max(1);
        *guard = Some(CachedToken {
            token: parsed.access_token.clone(),
            refresh_after: Instant::now() + Duration::from_secs(lifetime),
        });
        Ok(parsed.access_token)
    }

    /// Upload a JSONL training file. Returns the Azure `file_id` (e.g. `file-abc123`).
    ///
    /// # Errors
    ///
    /// Returns an [`AzureError`] if the multipart form cannot be built, the request
    /// fails, Azure returns a non-success status, or the response cannot be decoded.
    pub async fn upload_training_file(
        &self,
        bytes: Vec<u8>,
        filename: &str,
    ) -> Result<String, AzureError> {
        // Response shape: { "id": "file-abc", ... }
        #[derive(Deserialize)]
        struct FileResp {
            id: String,
        }

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
        let parsed: FileResp = serde_json::from_str(&body)
            .map_err(|e| AzureError::Decode(format!("upload response: {e}")))?;
        Ok(parsed.id)
    }

    /// Kick off a fine-tuning job against a previously-uploaded training file.
    /// Returns the Azure job id (e.g. `ftjob-xyz`).
    ///
    /// # Errors
    ///
    /// Returns an [`AzureError`] if the hyperparameters cannot be encoded, the request
    /// fails, Azure returns a non-success status, or the response cannot be decoded.
    pub async fn create_finetune_job(
        &self,
        base_model: &str,
        training_file_id: &str,
        hyperparameters: Option<AzureHyperparameters>,
        suffix: Option<&str>,
    ) -> Result<String, AzureError> {
        #[derive(Deserialize)]
        struct JobResp {
            id: String,
        }

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
        let parsed: JobResp = serde_json::from_str(&body_text)
            .map_err(|e| AzureError::Decode(format!("create job response: {e}")))?;
        Ok(parsed.id)
    }

    /// Refresh a job's current state from Azure. Used by the polling worker
    /// (slice 2b) and by `GET /v1/finetune/jobs/:id` when `status='running'`.
    ///
    /// # Errors
    ///
    /// Returns an [`AzureError`] if the request fails, Azure returns a non-success
    /// status, or the response cannot be decoded.
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
    ///
    /// # Errors
    ///
    /// Returns an [`AzureError`] if the request fails or Azure returns a non-success status.
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

    /// Provision (create-or-update) an Azure deployment for a fine-tuned model
    /// so the gateway can route traffic to it, via the Azure Resource Manager
    /// **management plane** (not the data plane). Idempotent — ARM returns 200
    /// for an existing deployment, 201 on first create, or 202 when the
    /// create-or-update is accepted asynchronously.
    ///
    /// `tier` selects the SKU: [`DeploymentTier::Developer`] ($0/hr, auto-expires
    /// in 24h) for auto-deploys, [`DeploymentTier::Production`] (paid Standard
    /// hosting) for an explicit operator promote.
    ///
    /// The request authenticates with an Azure AD bearer token from the
    /// client-credentials grant (see [`AzureFinetuneClient::mgmt_token`]).
    ///
    /// # Errors
    ///
    /// Returns [`AzureError::MgmtNotConfigured`] when the ARM/AAD envs are unset,
    /// [`AzureError::Http`] on transport failure, [`AzureError::Api`] when AAD or
    /// ARM returns a non-success status, or [`AzureError::Decode`] on a malformed
    /// token response.
    pub async fn create_deployment(
        &self,
        deployment_name: &str,
        model_name: &str,
        tier: DeploymentTier,
    ) -> Result<(), AzureError> {
        let Some(mgmt) = self.mgmt.as_ref() else {
            return Err(AzureError::MgmtNotConfigured);
        };

        let token = self.mgmt_token(mgmt).await?;

        // ARM resource URL:
        //   {mgmt}/subscriptions/{sub}/resourceGroups/{rg}/providers/
        //   Microsoft.CognitiveServices/accounts/{account}/deployments/{name}
        //   ?api-version={ver}
        let url = format!(
            "{base}/subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{account}/deployments/{name}?api-version={ver}",
            base = mgmt.mgmt_base_url.trim_end_matches('/'),
            sub = mgmt.subscription_id,
            rg = mgmt.resource_group,
            account = mgmt.account_name,
            name = deployment_name,
            ver = mgmt.api_version,
        );

        // Body per Microsoft.CognitiveServices/accounts/deployments 2024-10-01:
        //   sku.name = "Developer" | "Standard", sku.capacity = 1
        //   properties.model = { format: "OpenAI", name: <fine_tuned_model>, version: "1" }
        let body = serde_json::json!({
            "sku": { "name": tier.azure_sku(), "capacity": 1 },
            "properties": {
                "model": { "format": "OpenAI", "name": model_name, "version": "1" }
            }
        });

        let resp = self
            .http
            .put(url)
            .bearer_auth(token)
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
    use wiremock::matchers::{body_json, body_string_contains, header, method, path, query_param};
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

    /// Build a management config whose ARM + AAD base URLs both point at the
    /// same wiremock server. The token endpoint and the deployment PUT live on
    /// distinct paths, so one server can stand in for both planes.
    fn mgmt_for(server: &MockServer) -> AzureMgmtConfig {
        AzureMgmtConfig {
            subscription_id: "sub-123".into(),
            resource_group: "rg-ai".into(),
            account_name: "core-ai-acct".into(),
            tenant_id: "tenant-abc".into(),
            client_id: "client-1".into(),
            client_secret: "secret-shh".into(),
            api_version: "2024-10-01".into(),
            mgmt_base_url: server.uri(),
            aad_base_url: server.uri(),
        }
    }

    /// Mount a standard AAD client-credentials token endpoint returning a
    /// bearer token with a 1h lifetime.
    async fn mount_token(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/tenant-abc/oauth2/v2.0/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "aad-token-xyz",
                "token_type": "Bearer",
                "expires_in": 3600,
            })))
            .mount(server)
            .await;
    }

    #[test]
    fn deployment_tier_maps_to_azure_sku() {
        assert_eq!(DeploymentTier::Developer.azure_sku(), "Developer");
        assert_eq!(DeploymentTier::Production.azure_sku(), "Standard");
    }

    #[test]
    fn deployment_tier_wire_label_round_trips() {
        assert_eq!(DeploymentTier::Developer.as_str(), "developer");
        assert_eq!(DeploymentTier::Production.as_str(), "production");
        assert_eq!(
            DeploymentTier::from_str_or_developer("production"),
            DeploymentTier::Production
        );
        assert_eq!(
            DeploymentTier::from_str_or_developer("Standard"),
            DeploymentTier::Production
        );
        assert_eq!(
            DeploymentTier::from_str_or_developer("developer"),
            DeploymentTier::Developer
        );
        // Unknown / garbage defaults to the free tier — never silently paid.
        assert_eq!(
            DeploymentTier::from_str_or_developer("garbage"),
            DeploymentTier::Developer
        );
        assert_eq!(
            DeploymentTier::from_str_or_developer(""),
            DeploymentTier::Developer
        );
    }

    #[tokio::test]
    async fn create_deployment_without_mgmt_config_returns_not_configured() {
        let server = MockServer::start().await;
        let c = client_for(&server); // no mgmt config attached
        let err = c
            .create_deployment(
                "acme-support-v1",
                "gpt-4o-mini.ft-xyz",
                DeploymentTier::Developer,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AzureError::MgmtNotConfigured));
    }

    #[tokio::test]
    async fn create_deployment_uses_management_url_and_developer_sku() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        // Assert the exact ARM resource path + api-version, and that the body
        // carries the Developer SKU + OpenAI model block.
        Mock::given(method("PUT"))
            .and(path(
                "/subscriptions/sub-123/resourceGroups/rg-ai/providers/Microsoft.CognitiveServices/accounts/core-ai-acct/deployments/ft-abc",
            ))
            .and(query_param("api-version", "2024-10-01"))
            .and(header("authorization", "Bearer aad-token-xyz"))
            .and(body_json(serde_json::json!({
                "sku": { "name": "Developer", "capacity": 1 },
                "properties": {
                    "model": { "format": "OpenAI", "name": "gpt-4o-mini.ft-xyz", "version": "1" }
                }
            })))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let c = client_for(&server).with_mgmt_overrides(mgmt_for(&server));
        c.create_deployment("ft-abc", "gpt-4o-mini.ft-xyz", DeploymentTier::Developer)
            .await
            .expect("deploy developer");
    }

    #[tokio::test]
    async fn create_deployment_production_sends_standard_sku() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        Mock::given(method("PUT"))
            .and(path(
                "/subscriptions/sub-123/resourceGroups/rg-ai/providers/Microsoft.CognitiveServices/accounts/core-ai-acct/deployments/ft-prod",
            ))
            .and(body_json(serde_json::json!({
                "sku": { "name": "Standard", "capacity": 1 },
                "properties": {
                    "model": { "format": "OpenAI", "name": "gpt-4o-mini.ft-prod", "version": "1" }
                }
            })))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let c = client_for(&server).with_mgmt_overrides(mgmt_for(&server));
        c.create_deployment("ft-prod", "gpt-4o-mini.ft-prod", DeploymentTier::Production)
            .await
            .expect("deploy production");
    }

    #[tokio::test]
    async fn create_deployment_requests_aad_token_with_client_credentials_shape() {
        let server = MockServer::start().await;
        // Assert the token request body shape: client-credentials grant with
        // the ARM .default scope and the configured client_id/secret.
        Mock::given(method("POST"))
            .and(path("/tenant-abc/oauth2/v2.0/token"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=client-1"))
            .and(body_string_contains("client_secret=secret-shh"))
            .and(body_string_contains(
                "scope=https%3A%2F%2Fmanagement.azure.com%2F.default",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "aad-token-xyz",
                "expires_in": 3600,
            })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(
                "/subscriptions/sub-123/resourceGroups/rg-ai/providers/Microsoft.CognitiveServices/accounts/core-ai-acct/deployments/ft-tok",
            ))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let c = client_for(&server).with_mgmt_overrides(mgmt_for(&server));
        c.create_deployment("ft-tok", "gpt-4o-mini.ft-xyz", DeploymentTier::Developer)
            .await
            .expect("deploy with token");
    }

    #[tokio::test]
    async fn create_deployment_surfaces_arm_error() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        Mock::given(method("PUT"))
            .and(path(
                "/subscriptions/sub-123/resourceGroups/rg-ai/providers/Microsoft.CognitiveServices/accounts/core-ai-acct/deployments/ft-bad",
            ))
            .respond_with(ResponseTemplate::new(409).set_body_string("conflict"))
            .mount(&server)
            .await;

        let c = client_for(&server).with_mgmt_overrides(mgmt_for(&server));
        let err = c
            .create_deployment("ft-bad", "gpt-4o-mini.ft-xyz", DeploymentTier::Developer)
            .await
            .unwrap_err();
        match err {
            AzureError::Api { status, body } => {
                assert_eq!(status, 409);
                assert_eq!(body, "conflict");
            }
            other => panic!("expected Api 409, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_deployment_surfaces_aad_auth_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/tenant-abc/oauth2/v2.0/token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid_client"))
            .mount(&server)
            .await;

        let c = client_for(&server).with_mgmt_overrides(mgmt_for(&server));
        let err = c
            .create_deployment("ft-x", "gpt-4o-mini.ft-xyz", DeploymentTier::Developer)
            .await
            .unwrap_err();
        match err {
            AzureError::Api { status, .. } => assert_eq!(status, 401),
            other => panic!("expected Api 401 from token endpoint, got {other:?}"),
        }
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
