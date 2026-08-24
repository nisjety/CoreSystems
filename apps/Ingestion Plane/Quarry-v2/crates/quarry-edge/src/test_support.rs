//! Shared `#[cfg(test)]` fixtures for handler-level route tests.
//!
//! Lives entirely behind `cfg(test)` (wired from `lib.rs`) so release
//! builds and the default-feature matrix never see it.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_runtime::artifact_store::InMemoryStore;
use quarry_runtime::driver::{Driver, FetchHints};
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::{EventSink, NoopUsageMeter, RunPolicy};

use crate::auth::Claims;
use crate::state::AppState;

/// Deterministic driver stub: fixed 200 HTML body or a fixed transport
/// error code. Counts fetches so tests can prove a path never dialed out.
pub struct StubDriver {
    kind: DriverKind,
    pub calls: Arc<AtomicU32>,
    error: Option<ErrorCode>,
}

impl StubDriver {
    /// Always answers 200 with a link-free HTML body (drives the
    /// "upstream reachable but empty" paths).
    pub fn ok() -> Arc<Self> {
        Arc::new(Self {
            kind: DriverKind::Static,
            calls: Arc::new(AtomicU32::new(0)),
            error: None,
        })
    }

    /// Always fails with the given error code (e.g. `RateLimited`).
    pub fn err(code: ErrorCode) -> Arc<Self> {
        Arc::new(Self {
            kind: DriverKind::Static,
            calls: Arc::new(AtomicU32::new(0)),
            error: Some(code),
        })
    }
}

#[async_trait]
impl Driver for StubDriver {
    fn kind(&self) -> DriverKind {
        self.kind
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetch_conditional(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        _hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match self.error {
            Some(code) => Err(QuarryError::new(code, "simulated transport failure")),
            None => Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: b"<html><body>plain text, no anchors</body></html>".to_vec(),
                duration_ms: 1,
            }),
        }
    }
}

/// Minimal in-memory [`AppState`] wired to the given static driver.
/// Mirrors the fixture in `routes.rs` tests; kept separate so route
/// modules don't depend on each other's test internals.
pub fn test_state(static_driver: Arc<dyn Driver>) -> AppState {
    let (tx, mut rx) = mpsc::channel(8);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    AppState {
        readiness: crate::state::ReadinessState {
            durable: true,
            reason: None,
        },
        receipts: Arc::new(quarry_runtime::InMemoryStepReceiptStore::new()),
        grant_validator: Arc::new(quarry_runtime::NoopGrantValidator),
        require_browser_grants: false,
        driver: static_driver,
        drivers: DriverRegistry::new(DriverKind::Static),
        http3: None,
        security: Arc::new(quarry_security::preflight::DefaultEngine::new()),
        artifacts: Arc::new(InMemoryStore::new()),
        control_base_url: String::new(),
        redis: None,
        cache: None,
        event_sink: EventSink::new(tx),
        ingest: None,
        profiles: Arc::new(quarry_browser::session::InMemoryProfileStore::new()),
        search: None,
        vector_index: None,
        searxng_url: None,
        model_plane_url: None,
        model_plane_token: None,
        service_token_provider: None,
        answer_pipeline: None,
        local_index: None,
        usage: Arc::new(NoopUsageMeter),
        policy: RunPolicy::default(),
        scheduler: None,
        internal_signer: None,
        page_renderer: None,
        visual_processor: None,
        #[cfg(feature = "postgres-queue")]
        event_history: None,
        #[cfg(feature = "postgres-queue")]
        queue_pool: None,
        #[cfg(feature = "postgres-queue")]
        baseline_store: None,
        #[cfg(feature = "browser-agent")]
        agent_driver: Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
        #[cfg(feature = "browser-agent")]
        browser_egress_proxy: None,
        #[cfg(feature = "browser-agent")]
        agent_runs: crate::agent_routes::new_runs(),
    }
}

/// Verified-looking JWT claims scoped to `org_id` (dev-bypass equivalent).
pub fn claims_for_org(org_id: &str) -> Claims {
    Claims {
        sub: "u1".into(),
        iss: "auth-core".into(),
        exp: i64::MAX,
        org_id: org_id.into(),
        user_id: "u1".into(),
        principal_type: None,
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
    }
}

/// Decode a response body as JSON (asserting content type separately).
pub async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body reads");
    serde_json::from_slice(&body).expect("response body is JSON")
}

/// Sorted key set of a JSON object — pins the exact envelope shape
/// (optional fields absent vs present).
pub fn json_keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value.as_object().expect("json object").keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_keys_are_sorted() {
        let v = serde_json::json!({ "zeta": 1, "alpha": 2 });
        assert_eq!(json_keys(&v), vec!["alpha", "zeta"]);
    }
}
