//! quarry-browser — browser lease runtime.
//!
//! Lease model (CONTRACTS §8): session affinity, sticky proxy, durable profile handle.
//! Actual CDP client: later phase (chromiumoxide or remote Browserless).
//!
//! Donor concepts: `internal/browser/`, `internal/driver/rod.go`, session/pool logic.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use quarry_core::error::QuarryError;
use quarry_core::lease::BrowserLease;
use quarry_core::QuarryResult;

use crate::actions::ScrollTarget;

pub mod actions;
pub mod browserbase;
pub mod browserless;
#[cfg(feature = "chromiumoxide")]
pub mod chromiumoxide;
#[cfg(feature = "kernel")]
pub mod kernel;
pub mod persistent_session;
pub mod pool;
pub mod session;
pub mod stealth;

#[cfg(feature = "kernel")]
pub use kernel::{KernelConfig, KernelDriver};
pub use persistent_session::{PersistentSession, PersistentSessionRegistry};

#[async_trait]
pub trait BrowserDriver: Send + Sync {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession>;
    async fn release(&self, session: BrowserSession) -> QuarryResult<()>;
    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()>;
    async fn content(&self, session: &BrowserSession) -> QuarryResult<bytes::Bytes>;
    async fn screenshot(
        &self,
        session: &BrowserSession,
        full_page: bool,
    ) -> QuarryResult<bytes::Bytes>;
    async fn pdf(&self, session: &BrowserSession) -> QuarryResult<bytes::Bytes>;

    /// Block until `selector` matches or `timeout_ms` elapses. Default: Unsupported.
    async fn wait_for(
        &self,
        _session: &BrowserSession,
        _selector: &str,
        _timeout_ms: u32,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("wait_for"))
    }

    /// Click the first element matching `selector`. Default: Unsupported.
    async fn click(&self, _session: &BrowserSession, _selector: &str) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("click"))
    }

    /// Focus the element matching `selector` and type `text`. Default: Unsupported.
    async fn type_text(
        &self,
        _session: &BrowserSession,
        _selector: &str,
        _text: &str,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("type"))
    }

    /// Scroll the page to `target`. Default: Unsupported.
    async fn scroll(&self, _session: &BrowserSession, _target: &ScrollTarget) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("scroll"))
    }

    /// Dispatch a single keypress (e.g. "Enter", "Tab"). Default: Unsupported.
    async fn press(&self, _session: &BrowserSession, _key: &str) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("press"))
    }

    /// Evaluate `script` in the page context and return its JSON value. Default: Unsupported.
    async fn evaluate(
        &self,
        _session: &BrowserSession,
        _script: &str,
    ) -> QuarryResult<serde_json::Value> {
        Err(QuarryError::unsupported_action("evaluate"))
    }

    /// Select an `<option>` by value within a `<select>` element. Default: Unsupported.
    async fn select(
        &self,
        _session: &BrowserSession,
        _selector: &str,
        _value: &str,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("select"))
    }

    /// Navigate back in the browser history. Default: Unsupported.
    async fn back(&self, _session: &BrowserSession) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("back"))
    }

    /// Navigate forward in the browser history. Default: Unsupported.
    async fn forward(&self, _session: &BrowserSession) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("forward"))
    }
}

#[derive(Debug)]
pub struct BrowserSession {
    pub lease: BrowserLease,
    pub inner: Arc<Mutex<SessionInner>>,
}

#[derive(Debug, Default)]
pub struct SessionInner {
    pub connected: bool,
    pub pages_served: u64,
}
