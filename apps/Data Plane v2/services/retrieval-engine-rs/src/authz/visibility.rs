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
//! The secure MVP deliberately performs no grant caching: a revoke is effective
//! on the next request without depending on cross-plane event-bus topology.

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Resolves a viewer's explicit document grants. `Arc<dyn …>` so local dev can
/// use the noop while prod uses the HTTP client.
#[async_trait::async_trait]
pub trait VisibilityClient: Send + Sync {
    /// Explicit-grant document ids the subject can see in the org. Returns an
    /// empty vec on any error (fail-open — owner + org/shared visibility, which
    /// the post-filter SQL evaluates locally, still apply).
    async fn visible_documents(
        &self,
        org_id: &str,
        user_id: &str,
        verified_bearer: Option<&str>,
    ) -> Vec<String>;

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
#[allow(dead_code)]
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
    async fn visible_documents(
        &self,
        _org_id: &str,
        _user_id: &str,
        _verified_bearer: Option<&str>,
    ) -> Vec<String> {
        vec![]
    }
}

/// Production client — uncached HTTP calls to user-core's authz facade.
pub struct HttpVisibilityClient {
    http: reqwest::Client,
    user_core_url: String,
    service_token: Option<String>,
}

struct DelegationHeaders {
    timestamp: String,
    nonce: String,
    body_sha256: String,
    signature: String,
}

fn delegation_headers(
    token: &str,
    method: &str,
    uri: &str,
    org_id: &str,
    user_id: &str,
    timestamp: &str,
    nonce: &str,
) -> DelegationHeaders {
    let body_sha256 = URL_SAFE_NO_PAD.encode(Sha256::digest([]));
    let canonical = [
        "v2",
        "retrieval-engine",
        "user-core",
        timestamp,
        method,
        uri,
        user_id,
        org_id,
        "authz:visible",
        "document",
        "",
        "resolve explicit document grants",
        "true",
        nonce,
        &body_sha256,
    ]
    .join("\n");
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
        .expect("HMAC accepts service tokens of any length");
    mac.update(canonical.as_bytes());
    DelegationHeaders {
        timestamp: timestamp.to_string(),
        nonce: nonce.to_string(),
        body_sha256,
        signature: URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()),
    }
}

impl HttpVisibilityClient {
    pub fn new(user_core_url: impl Into<String>, service_token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("build reqwest client");
        Self {
            http,
            user_core_url: user_core_url.into(),
            service_token,
        }
    }

    async fn fetch(
        &self,
        org_id: &str,
        user_id: &str,
        verified_bearer: Option<&str>,
    ) -> Result<Vec<String>, ()> {
        let verified_bearer = verified_bearer
            .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
            .ok_or_else(|| {
                tracing::warn!("verified user proof unavailable; explicit grants fail closed");
            })?;
        let url = format!(
            "{}/api/v1/internal/authz/visible",
            self.user_core_url.trim_end_matches('/')
        );
        let rb = self.http.get(&url).query(&[
            ("org_id", org_id),
            ("subject_id", user_id),
            ("resource_type", "document"),
        ]);
        let mut request = rb.build().map_err(|e| {
            tracing::warn!(error = %e, "user-core authz facade request build failed");
        })?;
        if let Some(token) = &self.service_token {
            let request_uri = match request.url().query() {
                Some(query) => format!("{}?{}", request.url().path(), query),
                None => request.url().path().to_string(),
            };
            let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            let nonce = uuid::Uuid::new_v4().to_string();
            let delegation = delegation_headers(
                token,
                request.method().as_str(),
                &request_uri,
                org_id,
                user_id,
                &timestamp,
                &nonce,
            );
            let authorization = format!("Bearer {verified_bearer}");
            for (name, value) in [
                ("Authorization", authorization.as_str()),
                ("X-Service-Token", token.as_str()),
                ("X-Service-Id", "retrieval-engine"),
                ("X-User-Id", user_id),
                ("X-Org-Id", org_id),
                ("X-Delegation-Version", "v2"),
                ("X-Delegation-Nonce", &delegation.nonce),
                ("X-Delegation-Timestamp", &delegation.timestamp),
                ("X-Delegation-Operation", "authz:visible"),
                ("X-Delegation-Resource-Type", "document"),
                ("X-Delegation-Resource-Id", ""),
                ("X-Delegation-Reason", "resolve explicit document grants"),
                ("X-Delegation-ZDR", "true"),
                ("X-Delegation-Body-SHA256", &delegation.body_sha256),
                ("X-Delegation-Signature", &delegation.signature),
            ] {
                request.headers_mut().insert(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| ())?,
                    reqwest::header::HeaderValue::from_str(value).map_err(|_| ())?,
                );
            }
        }
        let resp = self.http.execute(request).await.map_err(|e| {
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
    async fn visible_documents(
        &self,
        org_id: &str,
        user_id: &str,
        verified_bearer: Option<&str>,
    ) -> Vec<String> {
        // Fail closed for grant-only documents. Owner and org-visible access are
        // independently evaluated by the canonical SQL visibility predicate.
        self.fetch(org_id, user_id, verified_bearer)
            .await
            .unwrap_or_default()
    }

    async fn invalidate(&self, org_id: &str, user_id: &str) {
        let _ = (org_id, user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::get, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn noop_returns_no_grants() {
        let c = NoopVisibilityClient;
        assert!(c
            .visible_documents("org-a", "user-1", None)
            .await
            .is_empty());
    }

    #[test]
    fn delegation_signature_matches_control_cross_language_vector() {
        let headers = delegation_headers(
            "0123456789abcdef0123456789abcdef",
            "GET",
            "/api/v1/internal/authz/visible?org_id=org-1&subject_id=user-1&resource_type=document",
            "org-1",
            "user-1",
            "2026-07-11T02:00:00Z",
            "test-nonce-1234567890",
        );
        assert_eq!(
            headers.body_sha256,
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"
        );
        assert_eq!(
            headers.signature,
            "ryYDD6IcyZ8bHaQcAuFrzXHj9cjbQ9O3qFA2Yni0cFM"
        );
    }

    #[tokio::test]
    async fn grant_lookup_forwards_verified_user_proof_and_fails_closed_without_it() {
        async fn visible(
            State(calls): State<std::sync::Arc<AtomicUsize>>,
            headers: HeaderMap,
        ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
            calls.fetch_add(1, Ordering::SeqCst);
            if headers.get("authorization").and_then(|v| v.to_str().ok())
                != Some("Bearer verified.jwt.token")
            {
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error":"missing proof"})),
                );
            }
            (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({"ids":["document-1"],"all_org":false})),
            )
        }

        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/api/v1/internal/authz/visible", get(visible))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve test") });

        let client = HttpVisibilityClient::new(
            format!("http://{address}"),
            Some("0123456789abcdef0123456789abcdef".into()),
        );
        assert!(client
            .visible_documents("org-1", "user-1", None)
            .await
            .is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            client
                .visible_documents("org-1", "user-1", Some("verified.jwt.token"))
                .await,
            vec!["document-1"]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
