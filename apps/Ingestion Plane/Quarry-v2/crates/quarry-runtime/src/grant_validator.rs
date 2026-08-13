//! BrowserBroker grant validation.
//!
//! Quarry brokers browser leases internally, but Model Plane has its own
//! `BrowserBroker` (proto §browser.proto) that issues *grants* —
//! signed, expiring tokens that authorize a specific Model Plane session
//! to use Quarry's browser. Before a lease handoff, Quarry MUST validate
//! the grant via `ValidateGrant(grant_id) → {active, expires_at}`.
//!
//! The trait below defines the validation surface so Quarry stays decoupled
//! from gRPC tooling. A real `GrpcGrantValidator` (against
//! `BrowserBroker` over tonic) is the natural follow-up; for now we
//! ship `HttpGrantValidator` (against an HTTP shim Model Plane can expose) and
//! `NoopGrantValidator` (dev/test).

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantValidation {
    pub grant_id: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Broker-owned navigation policy. Quarry must never derive agent browser
    /// authority from the caller's requested domains when a broker grant is
    /// present.
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    /// Empty for a run grant. Sensitive grants must explicitly name every
    /// action they authorize; Quarry never treats domain scope as approval.
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    #[serde(default)]
    pub allowed_frame_ids: Vec<String>,
    #[serde(default)]
    pub allowed_dialog_ids: Vec<String>,
    #[serde(default)]
    pub allowed_artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_grant_id: Option<String>,
}

impl GrantValidation {
    pub fn is_usable(&self) -> bool {
        if !self.active {
            return false;
        }
        match self.expires_at {
            Some(exp) => exp > Utc::now(),
            None => true,
        }
    }

    pub fn authorizes_sensitive_action(
        &self,
        action: &str,
        frame_id: Option<&str>,
        dialog_id: Option<&str>,
        artifact_id: Option<&str>,
    ) -> bool {
        if !self.allowed_actions.iter().any(|allowed| allowed == action) {
            return false;
        }
        if let Some(frame_id) = frame_id {
            if self.allowed_frame_ids.len() != 1 || self.allowed_frame_ids[0] != frame_id {
                return false;
            }
        } else if !self.allowed_frame_ids.is_empty() {
            return false;
        }
        if let Some(dialog_id) = dialog_id {
            if self.allowed_dialog_ids.len() != 1 || self.allowed_dialog_ids[0] != dialog_id {
                return false;
            }
        } else if !self.allowed_dialog_ids.is_empty() {
            return false;
        }
        if let Some(artifact_id) = artifact_id {
            if self.allowed_artifact_ids.len() != 1 || self.allowed_artifact_ids[0] != artifact_id {
                return false;
            }
        } else if !self.allowed_artifact_ids.is_empty() {
            return false;
        }
        true
    }

    /// A sensitive approval must be strict single-action authority. This
    /// prevents a valid grant that happens to include an allowed action from
    /// also smuggling unrelated irreversible permissions into one replayable
    /// token.
    pub fn authorizes_exact_sensitive_action(
        &self,
        action: &str,
        parent_grant_id: &str,
        frame_id: Option<&str>,
        dialog_id: Option<&str>,
        artifact_id: Option<&str>,
    ) -> bool {
        self.allowed_actions.len() == 1
            && self.allowed_actions[0] == action
            && self.parent_grant_id.as_deref() == Some(parent_grant_id)
            && self.authorizes_sensitive_action(action, frame_id, dialog_id, artifact_id)
    }
}

#[async_trait]
pub trait GrantValidator: Send + Sync {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation>;
}

/// Always-allow validator. Useful for dev, internal-only edges, and tests.
/// Production deployments behind Model Plane MUST use a real validator.
#[derive(Default, Clone)]
pub struct NoopGrantValidator;

#[async_trait]
impl GrantValidator for NoopGrantValidator {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        Ok(GrantValidation {
            grant_id: grant_id.to_string(),
            active: true,
            expires_at: None,
            allowed_domains: Vec::new(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            parent_grant_id: None,
        })
    }
}

/// HTTP-shim validator. Calls `POST {base_url}/v1/broker/grants/{id}/validate`
/// with optional bearer auth, expecting a JSON body shaped like `GrantValidation`.
///
/// Wire this against a Model Plane HTTP shim that proxies
/// `BrowserBroker.ValidateGrant` until a tonic gRPC client lands.
#[derive(Clone)]
pub struct HttpGrantValidator {
    http: Client,
    base_url: String,
    bearer_token: Option<String>,
}

impl HttpGrantValidator {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("http client build failed: {e}"),
                )
            })?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            bearer_token: None,
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }
}

#[async_trait]
impl GrantValidator for HttpGrantValidator {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        if grant_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "grant_id must not be empty",
            ));
        }

        let url = format!(
            "{}/v1/broker/grants/{}/validate",
            self.base_url.trim_end_matches('/'),
            grant_id
        );
        let mut builder = self.http.post(&url);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }

        let resp = builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("grant validate transport failure: {e}"),
            )
        })?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(GrantValidation {
                grant_id: grant_id.to_string(),
                active: false,
                expires_at: None,
                allowed_domains: Vec::new(),
                allowed_actions: Vec::new(),
                allowed_frame_ids: Vec::new(),
                allowed_dialog_ids: Vec::new(),
                allowed_artifact_ids: Vec::new(),
                parent_grant_id: None,
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("grant validate returned {status}: {body}"),
            ));
        }

        resp.json::<GrantValidation>().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("grant validate decode failure: {e}"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn is_usable_returns_false_when_inactive() {
        let g = GrantValidation {
            grant_id: "x".into(),
            active: false,
            expires_at: None,
            allowed_domains: Vec::new(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            parent_grant_id: None,
        };
        assert!(!g.is_usable());
    }

    #[test]
    fn is_usable_returns_false_when_expired() {
        let g = GrantValidation {
            grant_id: "x".into(),
            active: true,
            expires_at: Some(Utc::now() - ChronoDuration::seconds(1)),
            allowed_domains: Vec::new(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            parent_grant_id: None,
        };
        assert!(!g.is_usable());
    }

    #[test]
    fn is_usable_returns_true_when_active_and_unexpired() {
        let g = GrantValidation {
            grant_id: "x".into(),
            active: true,
            expires_at: Some(Utc::now() + ChronoDuration::minutes(5)),
            allowed_domains: Vec::new(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            parent_grant_id: None,
        };
        assert!(g.is_usable());
    }

    #[test]
    fn is_usable_returns_true_when_no_expiry() {
        let g = GrantValidation {
            grant_id: "x".into(),
            active: true,
            expires_at: None,
            allowed_domains: Vec::new(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            parent_grant_id: None,
        };
        assert!(g.is_usable());
    }

    #[tokio::test]
    async fn noop_validator_always_allows() {
        let v = NoopGrantValidator;
        let result = v.validate("any").await.unwrap();
        assert!(result.is_usable());
    }

    #[tokio::test]
    async fn http_validator_calls_endpoint_and_parses_response() {
        let server = MockServer::start().await;
        let exp = Utc::now() + ChronoDuration::minutes(10);
        Mock::given(method("POST"))
            .and(path("/v1/broker/grants/grant_abc/validate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "grant_id": "grant_abc",
                "active": true,
                "expires_at": exp.to_rfc3339(),
            })))
            .mount(&server)
            .await;

        let v = HttpGrantValidator::new(server.uri()).unwrap();
        let result = v.validate("grant_abc").await.unwrap();
        assert_eq!(result.grant_id, "grant_abc");
        assert!(result.active);
        assert!(result.is_usable());
    }

    #[tokio::test]
    async fn http_validator_404_returns_inactive_grant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/broker/grants/missing/validate"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let v = HttpGrantValidator::new(server.uri()).unwrap();
        let result = v.validate("missing").await.unwrap();
        assert!(!result.active);
        assert!(!result.is_usable());
    }

    #[tokio::test]
    async fn http_validator_5xx_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/broker/grants/g/validate"))
            .respond_with(ResponseTemplate::new(500).set_body_string("upstream down"))
            .mount(&server)
            .await;

        let v = HttpGrantValidator::new(server.uri()).unwrap();
        let result = v.validate("g").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn http_validator_rejects_empty_grant_id() {
        let v = HttpGrantValidator::new("http://localhost:1").unwrap();
        let result = v.validate("").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("grant_id must not be empty"));
    }
}
