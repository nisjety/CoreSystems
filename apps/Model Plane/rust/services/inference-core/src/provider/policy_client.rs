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

use tonic::transport::{Channel, Endpoint};

use mp_contracts::model_plane::v1::routing_policy_client::RoutingPolicyClient;
use mp_contracts::model_plane::v1::{GetRoutingPolicyRequest, SetRoutingPolicyRequest};

use super::routing_policy::RoutingPolicy;

/// Lazily-connected client for session-core's `RoutingPolicy` gRPC service.
#[derive(Clone)]
pub struct PolicyClient {
    channel: Channel,
}

impl PolicyClient {
    /// Build a client for an explicit session-core URL. Returns `None` when the
    /// URL is empty or not a valid endpoint.
    #[must_use]
    pub fn from_url(url: &str) -> Option<Self> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let channel = Endpoint::from_shared(url.to_owned()).ok()?.connect_lazy();
        Some(Self { channel })
    }

    /// Fetch the live policy from session-core. Fail-soft: returns `None` on any
    /// transport failure, and also when the stored `config_json` is empty or
    /// `{}` (so the caller keeps its in-memory default). A partial/invalid JSON
    /// payload likewise yields `None` rather than a degraded policy.
    pub async fn fetch(&self) -> Option<RoutingPolicy> {
        let mut client = RoutingPolicyClient::new(self.channel.clone());
        let resp = match client.get_policy(GetRoutingPolicyRequest {}).await {
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
        let resp = client
            .set_policy(SetRoutingPolicyRequest {
                config_json,
                updated_by: updated_by.to_owned(),
            })
            .await
            .map_err(|status| format!("session-core set_policy: {status}"))?
            .into_inner();
        serde_json::from_str(&resp.config_json)
            .map_err(|e| format!("parse stored policy: {e}"))
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
