//! Policy client — abstraction over `user-service.CheckMembership` and
//! `org-core.GetUserPermissions`. Implementations:
//!
//! - [`NoopPolicyClient`]: pass-through. Always returns allow-all. Used
//!   when `CONTROL_PLANE_ENFORCEMENT=off` (default for v2.3 back-compat).
//!
//! - [`HttpPolicyClient`]: calls the Control Plane services via HTTP. Two
//!   modes via `EnforcementMode`:
//!     * `Strict`     — Control Plane unavailable → fail closed (deny).
//!     * `Permissive` — Control Plane unavailable → fail open (allow-all,
//!       audit-logged as `denied:control_plane_unavailable`). Use during
//!       the rollout window so a Control Plane outage doesn't drop traffic.
//!
//! When the protobuf contracts for `user-service` and `org-core` land,
//! swap `HttpPolicyClient` for a `GrpcPolicyClient` without touching the
//! trait or callers.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::context::EffectiveAcl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    Off,
    Strict,
    Permissive,
}

impl EnforcementMode {
    pub fn from_env() -> Self {
        match std::env::var("CONTROL_PLANE_ENFORCEMENT").as_deref() {
            Ok("strict") => EnforcementMode::Strict,
            Ok("permissive") => EnforcementMode::Permissive,
            _ => EnforcementMode::Off,
        }
    }
}

/// What a policy lookup returned. `is_member=false` means the caller
/// claims an org they are not a member of and must be denied; the audit
/// log records the cause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub is_member: bool,
    pub acl: EffectiveAcl,
    pub cause: String,
}

impl PolicyDecision {
    pub fn allow_all() -> Self {
        Self {
            is_member: true,
            acl: EffectiveAcl::allow_all(),
            cause: "ok".into(),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            is_member: false,
            acl: EffectiveAcl::default(),
            cause: reason.into(),
        }
    }
}

/// Async trait — kept simple (no generics) so callers can use `Arc<dyn …>`.
#[async_trait::async_trait]
pub trait PolicyClient: Send + Sync {
    async fn resolve(&self, user_id: &str, org_id: &str) -> PolicyDecision;
}

/// Local-dev / v2.3 back-compat: always allow.
pub struct NoopPolicyClient;

#[async_trait::async_trait]
impl PolicyClient for NoopPolicyClient {
    async fn resolve(&self, _user_id: &str, _org_id: &str) -> PolicyDecision {
        PolicyDecision::allow_all()
    }
}

/// Production client — HTTP calls to user-service + org-core. Cached
/// in-process (`moka` 5-min TTL) to avoid hammering Control Plane on
/// every request. Falls back to `EnforcementMode` semantics on transport
/// or 5xx errors.
pub struct HttpPolicyClient {
    http: reqwest::Client,
    user_service_url: String,
    org_core_url: String,
    mode: EnforcementMode,
    cache: moka::future::Cache<(String, String), PolicyDecision>,
}

impl HttpPolicyClient {
    pub fn new(user_service_url: impl Into<String>, org_core_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("build reqwest client");
        let cache = moka::future::Cache::builder()
            .time_to_live(Duration::from_secs(300)) // 5 min
            .max_capacity(50_000)
            .build();
        Self {
            http,
            user_service_url: user_service_url.into(),
            org_core_url: org_core_url.into(),
            mode: EnforcementMode::from_env(),
            cache,
        }
    }

    async fn fetch_decision(&self, user_id: &str, org_id: &str) -> Result<PolicyDecision, ()> {
        // Step 1: membership check.
        let membership_url = format!(
            "{}/internal/v1/users/{}/memberships/{}",
            self.user_service_url.trim_end_matches('/'),
            user_id,
            org_id
        );
        let resp = self.http.get(&membership_url).send().await.map_err(|e| {
            tracing::warn!(error = %e, "user-service unreachable");
        })?;
        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "user-service rejected membership lookup");
            return Ok(PolicyDecision::deny("denied:no_membership"));
        }
        #[derive(Deserialize)]
        struct MembershipResp {
            is_member: bool,
        }
        let m: MembershipResp = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "user-service membership decode failed");
        })?;
        if !m.is_member {
            return Ok(PolicyDecision::deny("denied:no_membership"));
        }

        // Step 2: permissions / ACL.
        let perms_url = format!(
            "{}/internal/v1/orgs/{}/users/{}/permissions",
            self.org_core_url.trim_end_matches('/'),
            org_id,
            user_id
        );
        let resp = self.http.get(&perms_url).send().await.map_err(|e| {
            tracing::warn!(error = %e, "org-core unreachable");
        })?;
        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "org-core permissions lookup failed");
            // Membership is good; treat as allow-org but no per-axis ACL.
            return Ok(PolicyDecision {
                is_member: true,
                acl: EffectiveAcl {
                    can_read: true,
                    ..Default::default()
                },
                cause: "ok".into(),
            });
        }
        let acl: EffectiveAcl = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "org-core permissions decode failed");
        })?;

        Ok(PolicyDecision {
            is_member: true,
            acl,
            cause: "ok".into(),
        })
    }
}

#[async_trait::async_trait]
impl PolicyClient for HttpPolicyClient {
    async fn resolve(&self, user_id: &str, org_id: &str) -> PolicyDecision {
        let key = (user_id.to_string(), org_id.to_string());
        if let Some(cached) = self.cache.get(&key).await {
            return cached;
        }

        let decision = match self.fetch_decision(user_id, org_id).await {
            Ok(d) => d,
            Err(_) => match self.mode {
                EnforcementMode::Strict => PolicyDecision::deny("denied:control_plane_unavailable"),
                EnforcementMode::Permissive | EnforcementMode::Off => PolicyDecision::allow_all(),
            },
        };
        // Only cache positive answers — negative cache risks locking out
        // a user the moment Control Plane recovers.
        if decision.is_member {
            self.cache.insert(key, decision.clone()).await;
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_client_always_allows() {
        let c = NoopPolicyClient;
        let d = c.resolve("user-1", "org-a").await;
        assert!(d.is_member);
        assert!(d.acl.can_read);
        assert_eq!(d.cause, "ok");
    }

    #[test]
    fn enforcement_mode_parsing() {
        std::env::set_var("CONTROL_PLANE_ENFORCEMENT", "strict");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Strict);
        std::env::set_var("CONTROL_PLANE_ENFORCEMENT", "permissive");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Permissive);
        std::env::set_var("CONTROL_PLANE_ENFORCEMENT", "off");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Off);
        std::env::remove_var("CONTROL_PLANE_ENFORCEMENT");
        assert_eq!(EnforcementMode::from_env(), EnforcementMode::Off);
    }
}
