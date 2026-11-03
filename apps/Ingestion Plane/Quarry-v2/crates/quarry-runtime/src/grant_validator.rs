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
    /// "session", "run" or "sensitive_approval"; empty from a broker that
    /// predates grant kinds.
    #[serde(default)]
    pub kind: String,
    /// The user the grant was issued to; empty from an older broker.
    #[serde(default)]
    pub owner_id: String,
    /// A Zero Data Retention grant. It may only back a ZDR run.
    #[serde(default)]
    pub zdr: bool,
    /// The grant's budget claim (WS6.E): an upper bound a run under it may
    /// spend. `None` from a broker that predates it, or for a grant minted
    /// without one. Carried verbatim; whether it is applied is the edge's
    /// `QUARRY_EDGE__GRANT_BUDGET_CLAIM`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<GrantBudget>,
    /// The navigation mode the broker granted (WS8.B): "" or "allowlist" (the
    /// exact host list in `allowed_domains`) or "open_web" (any public host not in
    /// `denied_domains`). Empty from a broker that predates it, which reads as
    /// "allowlist". Never derived from the caller.
    #[serde(default)]
    pub policy_mode: String,
    /// Denied hosts of an open-web grant. Empty for an allow-list grant.
    #[serde(default)]
    pub denied_domains: Vec<String>,
    /// The grant's absolute cap (WS8.B): a human-browse grant renews until this
    /// instant and no further. `None` on every other grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_expires_at: Option<DateTime<Utc>>,
    /// The resource a derived attach grant is bound to (WS8.C/WS8.F). Empty otherwise.
    #[serde(default)]
    pub resource_id: String,
    /// INNH-10: the id of the API key a key grant was issued for (never the key itself).
    /// Empty on every grant a person or a service holds, and from a broker that predates it.
    /// Quarry admits an `api_key` run only under a grant whose `key_id` equals the token's, so
    /// an empty value refuses every key run (fail closed). The vendored gRPC proto does not
    /// carry the field yet, so the gRPC validator always yields an empty one (see
    /// `docs/RUNBOOKS.md`, "API-key agent runs").
    #[serde(default)]
    pub key_id: String,
}

/// A grant's budget claim, as the broker stored it. A bound of 0 means that
/// dimension is not bounded. The claim can only tighten what a run would
/// otherwise get; Quarry takes the smaller of it and its own value.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantBudget {
    /// Estimated spend, in millionths of a dollar.
    #[serde(default)]
    pub micro_usd: u64,
    #[serde(default)]
    pub max_steps: u32,
    /// Active run time, in seconds.
    #[serde(default)]
    pub max_runtime_s: u32,
    /// `request`, `org_default` or `entitlement`: who set the numbers.
    #[serde(default)]
    pub source: String,
    /// When the broker stored the claim (unix seconds); 0 when unknown.
    #[serde(default)]
    pub minted_at_unix: i64,
}

impl GrantBudget {
    /// Whether any dimension is bounded.
    pub fn is_set(&self) -> bool {
        self.micro_usd > 0 || self.max_steps > 0 || self.max_runtime_s > 0
    }

    /// Read the five wire fields of a `ValidateGrantResponse`. A negative
    /// number is not a bound (the broker never sends one), and a response
    /// that bounds nothing is no claim at all.
    pub fn from_wire(
        micro_usd: i64,
        max_steps: i32,
        max_runtime_s: i32,
        source: String,
        minted_at_unix: i64,
    ) -> Option<Self> {
        let claim = Self {
            micro_usd: u64::try_from(micro_usd).unwrap_or(0),
            max_steps: u32::try_from(max_steps).unwrap_or(0),
            max_runtime_s: u32::try_from(max_runtime_s).unwrap_or(0),
            source,
            minted_at_unix,
        };
        claim.is_set().then_some(claim)
    }
}

impl GrantValidation {
    /// The navigation authority this grant gives a run, or the reason it is
    /// refused. Fails closed: an unknown mode, or a list shape that does not match
    /// the mode, is an error, never a widening.
    pub fn nav_policy(&self) -> Result<quarry_browser::BrokerNavPolicy, String> {
        quarry_browser::BrokerNavPolicy::from_validation(
            &self.policy_mode,
            &self.allowed_domains,
            &self.denied_domains,
        )
    }

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

    /// Validate `grant_id` on behalf of `org_id`, the organization of the
    /// verified run (never one taken from a request body). A validator that
    /// can bind the call to an organization (the broker, with a service token
    /// minted for that org) overrides this; the default ignores the org and
    /// delegates to [`GrantValidator::validate`], so every existing validator
    /// keeps its behaviour. Callers on the agent routes always use this
    /// method, so the org reaches the validator on every check.
    async fn validate_for_org(
        &self,
        org_id: &str,
        grant_id: &str,
    ) -> QuarryResult<GrantValidation> {
        let _ = org_id;
        self.validate(grant_id).await
    }
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
            kind: String::new(),
            owner_id: String::new(),
            zdr: false,
            budget: None,
            policy_mode: String::new(),
            denied_domains: Vec::new(),
            absolute_expires_at: None,
            resource_id: String::new(),
            key_id: String::new(),
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
                    format!("http client build failed: {}", e.without_url()),
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
                format!("grant validate transport failure: {}", e.without_url()),
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
                kind: String::new(),
                owner_id: String::new(),
                zdr: false,
                budget: None,
                policy_mode: String::new(),
                denied_domains: Vec::new(),
                absolute_expires_at: None,
                resource_id: String::new(),
                key_id: String::new(),
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
                format!("grant validate decode failure: {}", e.without_url()),
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
            kind: String::new(),
            owner_id: String::new(),
            zdr: false,
            budget: None,
            policy_mode: String::new(),
            denied_domains: Vec::new(),
            absolute_expires_at: None,
            resource_id: String::new(),
            key_id: String::new(),
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
            kind: String::new(),
            owner_id: String::new(),
            zdr: false,
            budget: None,
            policy_mode: String::new(),
            denied_domains: Vec::new(),
            absolute_expires_at: None,
            resource_id: String::new(),
            key_id: String::new(),
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
            kind: String::new(),
            owner_id: String::new(),
            zdr: false,
            budget: None,
            policy_mode: String::new(),
            denied_domains: Vec::new(),
            absolute_expires_at: None,
            resource_id: String::new(),
            key_id: String::new(),
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
            kind: String::new(),
            owner_id: String::new(),
            zdr: false,
            budget: None,
            policy_mode: String::new(),
            denied_domains: Vec::new(),
            absolute_expires_at: None,
            resource_id: String::new(),
            key_id: String::new(),
        };
        assert!(g.is_usable());
    }

    #[tokio::test]
    async fn noop_validator_always_allows() {
        let v = NoopGrantValidator;
        let result = v.validate("any").await.unwrap();
        assert!(result.is_usable());
    }

    /// A validator that only knows `validate`: `validate_for_org` reaches it
    /// through the default body, with the grant id unchanged.
    #[tokio::test]
    async fn default_validate_for_org_delegates_to_validate() {
        struct PlainOnly(std::sync::Mutex<Vec<String>>);
        #[async_trait]
        impl GrantValidator for PlainOnly {
            async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
                self.0.lock().unwrap().push(grant_id.to_owned());
                NoopGrantValidator.validate(grant_id).await
            }
        }
        let plain = PlainOnly(std::sync::Mutex::new(Vec::new()));
        let grant = plain.validate_for_org("org-a", "grant-1").await.unwrap();
        assert_eq!(grant.grant_id, "grant-1");
        assert!(grant.is_usable());
        assert_eq!(*plain.0.lock().unwrap(), vec!["grant-1".to_owned()]);

        // An empty org is not this method's business: the default does not
        // invent a refusal, and the validator it delegates to keeps its own.
        let noop = NoopGrantValidator
            .validate_for_org("", "grant-2")
            .await
            .unwrap();
        assert_eq!(noop.grant_id, "grant-2");
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

    #[test]
    fn a_wire_claim_with_no_bound_is_no_claim_and_negative_bounds_are_dropped() {
        assert_eq!(GrantBudget::from_wire(0, 0, 0, "request".into(), 5), None);
        assert_eq!(GrantBudget::from_wire(-1, -1, -1, String::new(), 0), None);
        let claim = GrantBudget::from_wire(-3, 7, 0, "request".into(), 9).expect("one bound is a claim");
        assert_eq!((claim.micro_usd, claim.max_steps, claim.max_runtime_s), (0, 7, 0));
        assert!(claim.is_set());
    }

    #[test]
    fn the_http_shim_json_may_omit_the_claim_and_may_carry_it() {
        let old: GrantValidation =
            serde_json::from_value(json!({ "grant_id": "g", "active": true })).unwrap();
        assert_eq!(old.budget, None);
        let new: GrantValidation = serde_json::from_value(json!({
            "grant_id": "g",
            "active": true,
            "budget": { "micro_usd": 250000, "max_steps": 10, "source": "request" },
        }))
        .unwrap();
        let claim = new.budget.expect("a claim");
        assert_eq!((claim.micro_usd, claim.max_steps, claim.max_runtime_s), (250_000, 10, 0));
        // And an absent claim is not written back out.
        assert!(serde_json::to_value(&old).unwrap().get("budget").is_none());
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
