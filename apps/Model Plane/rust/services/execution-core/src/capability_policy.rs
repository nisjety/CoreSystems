//! Authoritative capability policy enforcement for execution dispatch.
//!
//! Tool names are mapped to capability identifiers by trusted server code;
//! callers and model output can never pair an allowed capability with a
//! different executor. Unknown tools fail closed before dispatch.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::{
    capability_core_client::CapabilityCoreClient, EvaluatePolicyRequest,
};
use tonic::transport::Channel;
use tonic::{Request, Status};

const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDecision {
    Allow,
    Deny,
    Ask,
}

#[tonic::async_trait]
pub trait CapabilityPolicy: Send + Sync {
    async fn evaluate(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityDecision, Status>;
}

#[derive(Clone)]
pub struct GrpcCapabilityPolicy {
    channel: Channel,
    tokens: TokenSource,
}

impl std::fmt::Debug for GrpcCapabilityPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GrpcCapabilityPolicy")
            .field("bearer", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl GrpcCapabilityPolicy {
    /// Build the execution-owned capability policy client. The service
    /// credential is exchanged for an exact tenant/audience/scope token and is
    /// never sent to capability-core itself.
    ///
    /// # Errors
    ///
    /// Returns an error when the required deployment-owned service-principal
    /// configuration is missing or invalid.
    pub fn from_env(channel: Channel) -> anyhow::Result<Self> {
        Ok(Self {
            channel,
            tokens: TokenSource::Service(Arc::new(ServiceTokenProvider::from_env()?)),
        })
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_static_token(channel: Channel, bearer: &str) -> Self {
        Self {
            channel,
            tokens: TokenSource::Static(Arc::from(bearer)),
        }
    }
}

#[tonic::async_trait]
impl CapabilityPolicy for GrpcCapabilityPolicy {
    async fn evaluate(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityDecision, Status> {
        if run_id.trim().is_empty() || org_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "run and tenant are required for capability policy",
            ));
        }
        let capability_id = trusted_capability_id(tool_name)
            .ok_or_else(|| Status::permission_denied("tool has no governed capability binding"))?;
        let bearer = self.tokens.token(org_id).await.map_err(|error| {
            tracing::warn!(%error, "capability service credential unavailable");
            Status::unavailable("capability policy credential unavailable")
        })?;
        let mut request = Request::new(EvaluatePolicyRequest {
            capability_id,
            run_id: run_id.to_owned(),
            agent_id: "execution-core".to_owned(),
            org_id: org_id.to_owned(),
            scope: "global".to_owned(),
        });
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}").parse().map_err(|_| {
                Status::internal("verified capability credential is not forwardable")
            })?,
        );
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            CapabilityCoreClient::new(self.channel.clone()).evaluate_policy(request),
        )
        .await
        .map_err(|_| Status::unavailable("capability policy timed out"))?
        .map_err(|error| {
            tracing::warn!(code = ?error.code(), "capability policy unavailable");
            Status::unavailable("capability policy unavailable")
        })?
        .into_inner();
        match response.decision.as_str() {
            "allow" => Ok(CapabilityDecision::Allow),
            "ask" => Ok(CapabilityDecision::Ask),
            "deny" | "fallback" => Ok(CapabilityDecision::Deny),
            _ => Err(Status::permission_denied(
                "capability policy returned an invalid decision",
            )),
        }
    }
}

#[derive(Clone)]
enum TokenSource {
    #[cfg(test)]
    Static(Arc<str>),
    Service(Arc<ServiceTokenProvider>),
}

impl TokenSource {
    async fn token(&self, org_id: &str) -> Result<String, anyhow::Error> {
        match self {
            #[cfg(test)]
            Self::Static(token) => Ok(token.to_string()),
            Self::Service(provider) => provider.token(org_id).await,
        }
    }
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

struct ServiceTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: tokio::sync::Mutex<HashMap<String, CachedToken>>,
}

impl std::fmt::Debug for ServiceTokenProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceTokenProvider")
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in_seconds: u64,
    audience: String,
}

impl ServiceTokenProvider {
    fn from_env() -> anyhow::Result<Self> {
        let required = |name: &'static str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("{name} is required"))
        };
        Ok(Self {
            auth_core_url: required("AUTH_CORE_URL")?.trim_end_matches('/').to_owned(),
            service_id: std::env::var("EXECUTION_CORE_SERVICE_ID")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "execution-core".to_owned()),
            credential: required("EXECUTION_CORE_SERVICE_API_KEY")?,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    #[cfg(test)]
    fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test HTTP client"),
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    async fn token(&self, org_id: &str) -> Result<String, anyhow::Error> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            anyhow::bail!("capability token tenant is required");
        }
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, token| token.expires_at > now + TOKEN_REFRESH_SKEW);
            if let Some(token) = cache.get(org_id) {
                return Ok(token.value.clone());
            }
        }

        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| anyhow::anyhow!("execution service credential is malformed"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/capability-core/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": ["capability:read"],
                "reason": "execution dispatch policy",
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Auth Core refused capability policy credential");
        }
        let bundle = response.json::<TokenResponse>().await?;
        if bundle.token.trim().is_empty()
            || bundle.audience != "capability-core"
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            anyhow::bail!("Auth Core returned an invalid capability policy credential");
        }
        self.cache.lock().await.insert(
            org_id.to_owned(),
            CachedToken {
                value: bundle.token.clone(),
                expires_at: Instant::now() + Duration::from_secs(bundle.expires_in_seconds),
            },
        );
        Ok(bundle.token)
    }
}

/// Map an executor name to its immutable policy capability. The mapping is
/// server-owned, so caller/model input can select a tool but cannot forge the
/// capability evaluated for that tool.
#[must_use]
pub fn trusted_capability_id(tool_name: &str) -> Option<String> {
    if tool_name.starts_with("subagent.") && tool_name.len() > "subagent.".len() {
        return Some("cap.agent.spawn".to_owned());
    }
    let capability = match tool_name {
        "shell" => "cap.command.shell",
        "browser_agent" => "cap.browser.open",
        "web_search" | "web_fetch" => "cap.tool.http",
        "knowledge_search" => "cap.retrieval.query",
        "yr_weather" | "traffic" | "news" | "company_lookup" => "cap.tool.information.read",
        "track_shipment" => "cap.tool.shipping.track",
        "get_shipping_quotes" | "shipping_carriers" => "cap.tool.shipping.read",
        "book_shipment" => "cap.tool.shipping.book",
        "list_social_accounts" => "cap.tool.social.read",
        "publish_social_post" => "cap.tool.social.publish",
        "list_provider_actions" => "cap.tool.provider.read",
        "execute_provider_action" => "cap.tool.provider.execute",
        // Dynamic MCP tools require a durable registry-owned binding. Do not
        // synthesize one from an untrusted model-facing name.
        _ => return None,
    };
    Some(capability.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn bindings_are_exact_and_unknown_or_dynamic_tools_fail_closed() {
        assert_eq!(
            trusted_capability_id("knowledge_search").as_deref(),
            Some("cap.retrieval.query")
        );
        assert_eq!(
            trusted_capability_id("book_shipment").as_deref(),
            Some("cap.tool.shipping.book")
        );
        assert!(trusted_capability_id("knowledge_search ").is_none());
        assert!(trusted_capability_id("echo").is_none());
        assert!(trusted_capability_id("mcp__server__tool").is_none());
        assert_eq!(
            trusted_capability_id("subagent.research").as_deref(),
            Some("cap.agent.spawn")
        );
    }

    #[tokio::test]
    async fn service_token_is_exactly_scoped_and_cached_without_disclosing_credential() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/capability-core/internal-token"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-api-key", "service-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["capability:read"],
                "reason": "execution dispatch policy"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "capability-token",
                "expiresInSeconds": 300,
                "audience": "capability-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        let provider =
            ServiceTokenProvider::new_for_test(&auth.uri(), "execution-core", "service-secret");

        assert_eq!(provider.token("org-a").await.unwrap(), "capability-token");
        assert_eq!(provider.token("org-a").await.unwrap(), "capability-token");
        assert!(!format!("{provider:?}").contains("service-secret"));
    }

    #[tokio::test]
    async fn service_token_rejects_wrong_audience_and_redirects() {
        let target = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong-audience-token",
                "expiresInSeconds": 300,
                "audience": "model-gateway"
            })))
            .mount(&target)
            .await;
        let redirect = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/stolen", target.uri())),
            )
            .mount(&redirect)
            .await;
        let provider =
            ServiceTokenProvider::new_for_test(&redirect.uri(), "execution-core", "secret");

        let error = provider
            .token("org-a")
            .await
            .expect_err("redirected service credentials must fail closed");
        assert!(error.to_string().contains("refused"));

        let direct = ServiceTokenProvider::new_for_test(&target.uri(), "execution-core", "secret");
        let error = direct
            .token("org-a")
            .await
            .expect_err("wrong token audience must fail closed");
        assert!(error
            .to_string()
            .contains("invalid capability policy credential"));
    }
}
