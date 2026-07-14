//! gRPC client for session-core's `RoutingPolicy` service.
//!
//! session-core owns the durable store for the Velion intent layer's runtime
//! policy (a JSONB singleton). This client reads it ([`PolicyClient::fetch`])
//! and writes it ([`PolicyClient::set`]). The policy travels as an opaque JSON
//! string in `config_json`; the canonical schema is [`RoutingPolicy`].
//!
//! Reads are **fail-soft**, exactly like [`super::intent::BudgetClient`]: any
//! transport/parse failure (or an empty/`{}` store) yields `None` so the caller
//! keeps its in-memory default. Only the explicit write path surfaces errors.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;
use tonic::transport::{Channel, Endpoint};

use mp_contracts::model_plane::v1::routing_policy_client::RoutingPolicyClient;
use mp_contracts::model_plane::v1::{GetRoutingPolicyRequest, SetRoutingPolicyRequest};

use super::routing_policy::RoutingPolicy;

/// Service identity inference-core presents to auth-core to mint a session-core
/// token for the background routing-policy read. Must match the principal key
/// registered in `PLANE_SERVICE_PRINCIPALS_JSON`.
const ROUTING_SERVICE_ID: &str = "inference-core";
/// Sentinel tenant for the platform-scoped routing-policy read. session-core's
/// identity check requires a non-empty `org_id` even on a service token; the
/// routing policy is global (the RPC carries no org), so any stable non-empty
/// value satisfies the check without scoping the query.
const ROUTING_ORG_SENTINEL: &str = "platform";
/// Read-only scope requested for the routing-policy token. Must be a subset of
/// the `inference-core` principal's scopes in `PLANE_SERVICE_PRINCIPALS_JSON`.
const ROUTING_SCOPE: &str = "routing:read";

/// Mints and caches a short-lived `aud=session-core` service token from
/// auth-core so the background policy read can authenticate. Configured from
/// the environment; absent config → `None` (the read stays unauthenticated and
/// fail-soft, exactly as before).
struct PolicyAuth {
    http: reqwest::Client,
    token_url: String,
    service_key: String,
    cache: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
struct InternalTokenResponse {
    token: String,
    #[serde(default, rename = "expiresInSeconds")]
    expires_in_seconds: Option<u64>,
}

impl PolicyAuth {
    fn from_env() -> Option<Arc<Self>> {
        let auth_core_url = std::env::var("AUTH_CORE_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())?;
        let service_key = std::env::var("INFERENCE_ROUTING_SERVICE_API_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Arc::new(Self {
            http,
            token_url: format!("{auth_core_url}/api/session-core/internal-token"),
            service_key,
            cache: Mutex::new(None),
        }))
    }

    /// Cached-or-minted `aud=session-core` service bearer. Fail-soft: any mint
    /// failure yields `None`, and the caller proceeds unauthenticated.
    async fn bearer(&self) -> Option<String> {
        if let Some(entry) = self.cache.lock().await.as_ref() {
            if entry.expires_at > Instant::now() {
                return Some(entry.token.clone());
            }
        }
        let resp = self
            .http
            .post(&self.token_url)
            .header("x-service-id", ROUTING_SERVICE_ID)
            .header("x-service-api-key", &self.service_key)
            .json(&serde_json::json!({
                "orgId": ROUTING_ORG_SENTINEL,
                // auth-core requires a non-empty requested scope that is a subset
                // of the principal's; session-core's get_policy enforces none, so
                // this stays least-privilege (read-only, no write scope).
                "scopes": [ROUTING_SCOPE],
                "reason": "routing-policy read",
            }))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "routing-policy: session-core service token mint failed");
            return None;
        }
        let body: InternalTokenResponse = resp.json().await.ok()?;
        let ttl = Duration::from_secs(
            body.expires_in_seconds
                .unwrap_or(300)
                .saturating_sub(60)
                .max(30),
        );
        let mut cache = self.cache.lock().await;
        *cache = Some(CachedToken {
            token: body.token.clone(),
            expires_at: Instant::now() + ttl,
        });
        Some(body.token)
    }
}

/// Lazily-connected client for session-core's `RoutingPolicy` gRPC service.
#[derive(Clone)]
pub struct PolicyClient {
    channel: Channel,
    auth: Option<Arc<PolicyAuth>>,
}

impl PolicyClient {
    /// Build a client for an explicit session-core URL. Returns `None` when the
    /// URL is empty or not a valid endpoint. The service-token minter is wired
    /// from the environment (best-effort) so the background read can present a
    /// verified `aud=session-core` credential.
    #[must_use]
    pub fn from_url(url: &str) -> Option<Self> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let channel = Endpoint::from_shared(url.to_owned()).ok()?.connect_lazy();
        Some(Self {
            channel,
            auth: PolicyAuth::from_env(),
        })
    }

    /// Wrap `message` in a request carrying the minted session-core bearer as
    /// `authorization` metadata (session-core's interceptor requires it). When
    /// no minter is configured or a mint fails, the request goes out bare.
    async fn authed_request<T>(&self, message: T) -> tonic::Request<T> {
        let mut request = tonic::Request::new(message);
        if let Some(auth) = &self.auth {
            if let Some(token) = auth.bearer().await {
                if let Ok(value) = format!("Bearer {token}").parse() {
                    request.metadata_mut().insert("authorization", value);
                }
            }
        }
        request
    }

    /// Fetch the live policy from session-core. Fail-soft: returns `None` on any
    /// transport failure, and also when the stored `config_json` is empty or
    /// `{}` (so the caller keeps its in-memory default). A partial/invalid JSON
    /// payload likewise yields `None` rather than a degraded policy.
    pub async fn fetch(&self) -> Option<RoutingPolicy> {
        let mut client = RoutingPolicyClient::new(self.channel.clone());
        let request = self.authed_request(GetRoutingPolicyRequest {}).await;
        let resp = match client.get_policy(request).await {
            Ok(r) => r.into_inner(),
            Err(error) => {
                tracing::warn!(%error, "routing-policy fetch failed; keeping default");
                return None;
            }
        };
        parse_config_json(&resp.config_json)
    }

    /// Persist `policy` to session-core, returning the stored policy as
    /// session-core echoes it back. Errors are surfaced as `Err(String)` for
    /// the explicit write path (the HTTP PUT handler).
    ///
    /// # Errors
    ///
    /// Returns `Err` when serialization fails or session-core rejects/cannot
    /// store the policy.
    pub async fn set(
        &self,
        policy: &RoutingPolicy,
        updated_by: &str,
    ) -> Result<RoutingPolicy, String> {
        let config_json =
            serde_json::to_string(policy).map_err(|e| format!("serialize policy: {e}"))?;
        let mut client = RoutingPolicyClient::new(self.channel.clone());
        let request = self
            .authed_request(SetRoutingPolicyRequest {
                config_json,
                updated_by: updated_by.to_owned(),
            })
            .await;
        let resp = client
            .set_policy(request)
            .await
            .map_err(|status| format!("session-core set_policy: {status}"))?
            .into_inner();
        serde_json::from_str(&resp.config_json).map_err(|e| format!("parse stored policy: {e}"))
    }
}

/// Parse a stored `config_json` into a [`RoutingPolicy`]. Empty / `{}` (the
/// session-core "never set" sentinel) → `None` so the caller keeps its default;
/// malformed JSON → `None` (logged) for the same reason.
fn parse_config_json(raw: &str) -> Option<RoutingPolicy> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return None;
    }
    match serde_json::from_str::<RoutingPolicy>(trimmed) {
        Ok(policy) => Some(policy),
        Err(error) => {
            tracing::warn!(%error, "routing-policy config_json parse failed; keeping default");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn from_url_rejects_empty() {
        // `connect_lazy()` needs a Tokio reactor in scope, hence `tokio::test`.
        assert!(PolicyClient::from_url("   ").is_none());
        assert!(PolicyClient::from_url("http://session-core:9091").is_some());
    }

    #[test]
    fn empty_or_object_config_keeps_default() {
        // Empty string and the empty-object sentinel both mean "never set" →
        // None → caller keeps RoutingPolicy::default().
        assert!(parse_config_json("").is_none());
        assert!(parse_config_json("   ").is_none());
        assert!(parse_config_json("{}").is_none());
        assert!(parse_config_json(" {} ").is_none());
    }

    #[test]
    fn malformed_config_keeps_default() {
        assert!(parse_config_json("not json").is_none());
        // A JSON object missing required fields is not a valid RoutingPolicy.
        assert!(parse_config_json(r#"{"enabled":true}"#).is_none());
    }

    #[test]
    fn round_trips_a_serialized_default_policy() {
        let json = serde_json::to_string(&RoutingPolicy::default()).unwrap();
        let parsed = parse_config_json(&json).expect("valid policy parses");
        assert_eq!(parsed, RoutingPolicy::default());
    }
}
