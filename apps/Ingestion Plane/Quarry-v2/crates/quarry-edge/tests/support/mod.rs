//! Shared `AppState` fixture for the edge integration tests.
//!
//! `AppState` has ~35 fields and is not `Default` (several fields are trait
//! objects with no sensible default), so every integration test used to carry
//! its own exhaustive struct literal. Adding one field to `AppState` therefore
//! broke four test binaries at once. The fixture below is the single place a
//! new field has to be defaulted; tests keep their differences visible by
//! overriding only what they exercise:
//!
//! ```ignore
//! let state = AppState {
//!     redis: Some(conn.clone()),
//!     cache: Some(PageCache::new(conn.clone(), ttl)),
//!     ..support::base_state(driver, event_sink)
//! };
//! ```
//!
//! Every dependency is an in-memory double and `allow_private_hosts` is on so
//! tests can point at `127.0.0.1` wiremock servers. `readiness.durable` is
//! `true` because these tests exercise admitted-traffic behaviour.

// Not every test binary overrides the same fields, and each `tests/*.rs`
// compiles this module separately.
#![allow(dead_code)]

use std::sync::Arc;

use quarry_core::output::DriverKind;
use quarry_edge::state::{AppState, ReadinessState};
use quarry_runtime::driver::Driver;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::{artifact_store::InMemoryStore, EventSink};
use quarry_security::preflight::DefaultEngine;

/// Baseline edge state: `driver` is registered as the sole (and default)
/// driver in the registry, so `build_driver` resolves to it for every plan.
pub fn base_state(driver: Arc<dyn Driver>, event_sink: EventSink) -> AppState {
    let mut drivers = DriverRegistry::new(DriverKind::Static);
    drivers.register(driver.clone());

    AppState {
        readiness: ReadinessState {
            durable: true,
            reason: None,
        },
        receipts: Arc::new(quarry_runtime::InMemoryStepReceiptStore::new()),
        grant_validator: Arc::new(quarry_runtime::NoopGrantValidator),
        require_browser_grants: false,
        driver,
        drivers,
        http3: None,
        security: Arc::new(DefaultEngine::new().with_allow_private_hosts(true)),
        artifacts: Arc::new(InMemoryStore::new()),
        control_base_url: String::new(),
        redis: None,
        cache: None,
        event_sink,
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
        usage: Arc::new(quarry_runtime::NoopUsageMeter),
        policy: quarry_runtime::RunPolicy::default(),
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
        agent_runs: quarry_edge::agent_routes::new_runs(),
    }
}

/// Agent-driver double for tests that exercise `POST /v1/agent/runs`
/// *request validation*.
///
/// `start_run` refuses agent execution outright unless the driver proves
/// isolated egress and current security evidence
/// (`agent_routes::require_isolated_agent_egress`), and the real
/// `ChromiumoxideDriver` claims those two only when it was constructed with a
/// pinned egress proxy. A bare `ChromiumoxideDriver::new()` therefore answers
/// every agent request with `501 Unsupported` before any body-level guard
/// runs, which hides the guards these tests are about.
///
/// This double affirms exactly the capabilities needed to reach request
/// validation and fails closed on every actual browser operation — no
/// Chromium binary and no real session are involved, which is what the
/// guard-only tests need.
pub fn governed_agent_driver() -> Arc<dyn quarry_browser::BrowserDriver> {
    Arc::new(GuardOnlyAgentDriver)
}

struct GuardOnlyAgentDriver;

impl GuardOnlyAgentDriver {
    fn no_browser() -> quarry_core::error::QuarryError {
        quarry_core::error::QuarryError::new(
            quarry_core::error::ErrorCode::DriverFailed,
            "test agent driver never launches a browser",
        )
    }
}

#[async_trait::async_trait]
impl quarry_browser::BrowserDriver for GuardOnlyAgentDriver {
    fn capabilities(&self) -> quarry_browser::BrowserDriverCapabilities {
        quarry_browser::BrowserDriverCapabilities {
            // Only what `start_run`'s pre-body admission gate demands. Every
            // other capability stays at the deny-by-default value so a test
            // that starts depending on one has to say so.
            isolated_egress: true,
            security_evidence: true,
            persistent_profile: true,
            ..quarry_browser::BrowserDriverCapabilities::default()
        }
    }

    async fn acquire(
        &self,
        _lease: &quarry_core::lease::BrowserLease,
    ) -> quarry_core::error::QuarryResult<quarry_browser::BrowserSession> {
        Err(Self::no_browser())
    }

    async fn release(
        &self,
        _session: quarry_browser::BrowserSession,
    ) -> quarry_core::error::QuarryResult<()> {
        Err(Self::no_browser())
    }

    async fn goto(
        &self,
        _session: &quarry_browser::BrowserSession,
        _url: &str,
    ) -> quarry_core::error::QuarryResult<()> {
        Err(Self::no_browser())
    }

    async fn content(
        &self,
        _session: &quarry_browser::BrowserSession,
    ) -> quarry_core::error::QuarryResult<bytes::Bytes> {
        Err(Self::no_browser())
    }

    async fn screenshot(
        &self,
        _session: &quarry_browser::BrowserSession,
        _full_page: bool,
    ) -> quarry_core::error::QuarryResult<bytes::Bytes> {
        Err(Self::no_browser())
    }

    async fn pdf(
        &self,
        _session: &quarry_browser::BrowserSession,
    ) -> quarry_core::error::QuarryResult<bytes::Bytes> {
        Err(Self::no_browser())
    }
}
