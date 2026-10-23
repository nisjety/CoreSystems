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
                served_by: DriverKind::Static,
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
    // Register the stub under its own `kind` so `build_driver(plan)`
    // resolves to the stub instead of returning an empty registry.
    let mut drivers = DriverRegistry::new(static_driver.kind());
    drivers.register(static_driver.clone());
    AppState {
        readiness: crate::state::ReadinessState {
            durable: true,
            reason: None,
        },
        receipts: Arc::new(quarry_runtime::InMemoryStepReceiptStore::new()),
        grant_validator: Arc::new(quarry_runtime::NoopGrantValidator),
        require_browser_grants: false,
        zdr_refuse_durable_resources: false,
        driver: static_driver,
        drivers,
        http3: None,
        security: Arc::new(quarry_security::preflight::DefaultEngine::new()),
        artifacts: Arc::new(InMemoryStore::new()),
        // W1: no operator pool in tests; the per-run resolver falls
        // back to first-party egress when empty.
        proxy_pool_name: String::new(),
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
        page_title_enricher: None,
        service_token_provider: None,
        answer_pipeline: None,
        local_index: None,
        usage: Arc::new(NoopUsageMeter),
        policy: RunPolicy::default(),
        scheduler: None,
        internal_signer: None,
        tombstones: None,
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
        #[cfg(feature = "browser-agent")]
        fleets: crate::fleet_routes::new_fleet_state(),
    }
}

/// [`test_state`] with a grant validator and the production posture switch:
/// `require` is the edge's `require_browser_grants` (true under
/// `ENVIRONMENT=production`), so a test states the one thing it varies instead
/// of overwriting two fields after the fact.
pub fn test_state_with_grants(
    static_driver: Arc<dyn Driver>,
    grant_validator: Arc<dyn quarry_runtime::GrantValidator>,
    require: bool,
) -> AppState {
    AppState {
        grant_validator,
        require_browser_grants: require,
        ..test_state(static_driver)
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
        zdr: None,
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

/// Insert a live agent run owned by `claims` into the registry and return its
/// handle, so a handler-level test never builds a `RunEntry` literal of its own
/// (every field a later slice adds is defaulted here, in one place).
///
/// The run is an agent-controlled, ephemeral, ungranted run with a five-step
/// bound and no domain allow-list; a test that needs more sets fields on the
/// entry through `handle.entry.lock().await`.
#[cfg(feature = "browser-agent")]
pub fn insert_live_run(state: &AppState, claims: &Claims) -> crate::agent_routes::RunHandle {
    use quarry_browser::{BrowserSession, SessionInner};
    use quarry_core::contracts::AgentConstraints;
    use quarry_core::ids::Id;
    use quarry_core::lease::BrowserLease;
    use quarry_core::zdr::ZdrMode;
    use quarry_runtime::observation::ObservationContext;
    use quarry_runtime::step_receipts::{BrowserControlMode, BrowserProfileScope};

    use crate::agent_routes::{BrowserExecutionTier, RunEntry, RunHandle, RunLive};

    let lease = BrowserLease {
        lease_id: Id::new(),
        profile_id: Id::new(),
        session_affinity_key: "test".into(),
        proxy_affinity: Default::default(),
        ttl_s: 60,
        capabilities: vec![],
        artifact_bucket: String::new(),
        persist_profile: false,
        viewport: None,
        org_id: claims.org_id.clone(),
    };
    let entry = RunEntry {
        run_id: Id::new(),
        org_id: claims.org_id.clone(),
        actor_id: claims.actor_id().to_string(),
        session: BrowserSession {
            lease: lease.clone(),
            tab_pin: None,
            inner: Arc::new(tokio::sync::Mutex::new(SessionInner::default())),
        },
        ctx: ObservationContext::default(),
        lease,
        constraints: AgentConstraints {
            max_steps: 5,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        zdr: ZdrMode::Off,
        grant_id: None,
        control_mode: BrowserControlMode::AgentControl,
        control_epoch: 0,
        profile_scope: BrowserProfileScope::Ephemeral,
        execution_tier: BrowserExecutionTier::Chromium,
        last_observation: None,
        used_sensitive_approval_grants: Default::default(),
        budget: None,
        closed: false,
        last_rejected_epoch: None,
        tab_origins: Default::default(),
    };
    let run_id = entry.run_id.to_string();
    let live = RunLive::new(
        entry.org_id.clone(),
        entry.actor_id.clone(),
        entry.control_mode,
        entry.control_epoch,
    );
    let handle = RunHandle::new(entry, live);
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .insert(run_id, handle.clone());
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_keys_are_sorted() {
        let v = serde_json::json!({ "zeta": 1, "alpha": 2 });
        assert_eq!(json_keys(&v), vec!["alpha", "zeta"]);
    }

    #[tokio::test]
    async fn test_state_with_grants_carries_the_validator_and_the_posture() {
        struct Denying;
        #[async_trait]
        impl quarry_runtime::GrantValidator for Denying {
            async fn validate(
                &self,
                _grant_id: &str,
            ) -> QuarryResult<quarry_runtime::GrantValidation> {
                Err(QuarryError::new(ErrorCode::Forbidden, "denied by the test validator"))
            }
        }
        let state = test_state_with_grants(StubDriver::ok(), Arc::new(Denying), true);
        assert!(state.require_browser_grants);
        // The org-aware call reaches the injected validator (its default body).
        let refused = state
            .grant_validator
            .validate_for_org("org-1", "grant-1")
            .await
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden);
        assert!(!test_state(StubDriver::ok()).require_browser_grants);
    }
}
