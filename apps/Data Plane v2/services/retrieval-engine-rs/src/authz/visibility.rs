//! Per-user visibility client — resolves the EXPLICIT document grants a viewer
//! holds, from user-core's `resource_grants` via its internal authz facade
//! (`GET /api/v1/internal/authz/visible`). This is the per-USER ownership axis,
//! DECOUPLED from `CONTROL_PLANE_ENFORCEMENT` (the coarse org axis handled by
//! `PolicyClient`): ownership is enforced whenever a `user_id` is present,
//! regardless of enforcement mode, so a default-off deploy cannot silently
//! disable per-user privacy.
//!
//! Fail-open at the call site: a facade error yields an empty grant set, so the
//! viewer still sees owned + org/shared docs (never a leak — at worst a doc
//! shared specifically to them is briefly hidden until the facade recovers).
//! Positive-only cache (mirrors `HttpPolicyClient`) so a revoke isn't masked by
//! a stale negative; PR-4's NATS subscriber calls `invalidate` on revoke.

use std::time::Duration;

use serde::Deserialize;

/// Resolves a viewer's explicit document grants. `Arc<dyn …>` so local dev can
/// use the noop while prod uses the HTTP client.
#[async_trait::async_trait]
pub trait VisibilityClient: Send + Sync {
    /// Explicit-grant document ids the subject can see in the org. Returns an
    /// empty vec on any error (fail-open — owner + org/shared visibility, which
    /// the post-filter SQL evaluates locally, still apply).
    async fn visible_documents(&self, org_id: &str, user_id: &str) -> Vec<String>;

    /// Evict a cached entry on revoke (called by the grant invalidator below).
    /// Default no-op.
    async fn invalidate(&self, _org_id: &str, _user_id: &str) {}
}

/// Subject published by user-core when a grant is created/revoked.
const SUBJECT_RESOURCE_GRANTS_CHANGED: &str = "aqencia.controlplane.acl.resource_grants.changed";

#[derive(Deserialize)]
struct GrantChangedEvent {
    org_id: String,
    subject_type: String,
    subject_id: String,
}

/// Spawn a background task that evicts the visibility cache for the affected
/// `(subject_id, org_id)` whenever a grant changes, so a revoke is effective
/// within one query (the 5-minute TTL is the backstop). Only `user`-subject
/// grants affect the per-user cache. The post-filter still resolves against
/// canonical Postgres on a miss, so a missed event degrades only to TTL latency,
/// never to a wrong answer.
pub fn spawn_grant_invalidator(
    nats: async_nats::Client,
    visibility: std::sync::Arc<dyn VisibilityClient>,
) {
    use futures::StreamExt;
    tokio::spawn(async move {
        let mut sub = match nats.subscribe(SUBJECT_RESOURCE_GRANTS_CHANGED).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    ?e,
                    "grant invalidator subscribe failed; revoke falls back to TTL"
                );
                return;
            }
        };
        tracing::info!(
            subject = SUBJECT_RESOURCE_GRANTS_CHANGED,
            "visibility grant invalidator subscribed"
        );
        while let Some(msg) = sub.next().await {
            let evt: GrantChangedEvent = match serde_json::from_slice(&msg.payload) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(?e, "grant invalidator decode failed");
                    continue;
                }
            };
            if evt.subject_type == "user" {
                visibility.invalidate(&evt.org_id, &evt.subject_id).await;
            }
        }
    });
}

/// Local-dev / no-facade client: never resolves explicit grants. Owner and
/// org/shared visibility still apply (resolved in SQL), so this is safe.
#[allow(dead_code)] // selected for local/dev + used in tests; prod uses Http
pub struct NoopVisibilityClient;

#[async_trait::async_trait]
impl VisibilityClient for NoopVisibilityClient {
    async fn visible_documents(&self, _org_id: &str, _user_id: &str) -> Vec<String> {
        vec![]
    }
}

/// Production client — HTTP calls to user-core's authz facade, cached in-process
/// (`moka`, 5-min TTL, positive-only).
pub struct HttpVisibilityClient {
    http: reqwest::Client,
    user_core_url: String,
    internal_api_key: Option<String>,
    cache: moka::future::Cache<(String, String), Vec<String>>,
}

impl HttpVisibilityClient {
    pub fn new(user_core_url: impl Into<String>, internal_api_key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("build reqwest client");
        let cache = moka::future::Cache::builder()
            .time_to_live(Duration::from_secs(300))
            .max_capacity(50_000)
            .build();
        Self {
            http,
            user_core_url: user_core_url.into(),
            internal_api_key,
            cache,
        }
    }

    async fn fetch(&self, org_id: &str, user_id: &str) -> Result<Vec<String>, ()> {
        let url = format!(
            "{}/api/v1/internal/authz/visible",
            self.user_core_url.trim_end_matches('/')
        );
        let mut rb = self.http.get(&url).query(&[
            ("org_id", org_id),
            ("subject_id", user_id),
            ("resource_type", "document"),
        ]);
        if let Some(key) = &self.internal_api_key {
            rb = rb.header("X-Internal-Api-Key", key);
        }
        let resp = rb.send().await.map_err(|e| {
            tracing::warn!(error = %e, "user-core authz facade unreachable");
        })?;
        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "user-core authz facade returned non-success");
            return Err(());
        }
        #[derive(Deserialize)]
        struct Resp {
            ids: Vec<String>,
        }
        let r: Resp = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "user-core authz facade decode failed");
        })?;
        Ok(r.ids)
    }
}

#[async_trait::async_trait]
impl VisibilityClient for HttpVisibilityClient {
    async fn visible_documents(&self, org_id: &str, user_id: &str) -> Vec<String> {
        let key = (org_id.to_string(), user_id.to_string());
        if let Some(cached) = self.cache.get(&key).await {
            return cached;
        }
        match self.fetch(org_id, user_id).await {
            Ok(ids) => {
                self.cache.insert(key, ids.clone()).await;
                ids
            }
            // Fail-open: owner + org/shared visibility (evaluated in SQL) still
            // apply; we just can't add explicitly-shared private docs right now.
            Err(_) => vec![],
        }
    }

    async fn invalidate(&self, org_id: &str, user_id: &str) {
        self.cache
            .invalidate(&(org_id.to_string(), user_id.to_string()))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_returns_no_grants() {
        let c = NoopVisibilityClient;
        assert!(c.visible_documents("org-a", "user-1").await.is_empty());
    }
}
