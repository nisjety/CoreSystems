//! quarry-browser — browser lease runtime.
//!
//! Lease model (CONTRACTS §8): session affinity, sticky proxy, durable profile handle.
//! Actual CDP client: later phase (chromiumoxide or remote Browserless).
//!
//! Donor concepts: `internal/browser/`, `internal/driver/rod.go`, session/pool logic.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFrameFormat {
    Jpeg,
    Png,
}

impl LiveFrameFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveFrameOptions {
    pub format: LiveFrameFormat,
    pub quality: u8,
    pub max_width: u32,
    pub max_height: u32,
    pub every_nth_frame: u32,
    pub timeout_ms: u64,
}

impl Default for LiveFrameOptions {
    fn default() -> Self {
        Self {
            format: LiveFrameFormat::Jpeg,
            quality: 65,
            max_width: 1280,
            max_height: 800,
            every_nth_frame: 1,
            timeout_ms: 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFrame {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTab {
    pub tab_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDevtoolsEvent {
    pub sequence: u64,
    pub tab_id: Option<String>,
    pub category: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub payload: serde_json::Value,
}

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
    async fn live_frame(
        &self,
        _session: &BrowserSession,
        _options: LiveFrameOptions,
    ) -> QuarryResult<LiveFrame> {
        Err(QuarryError::unsupported_action("live_frame"))
    }
    async fn pdf(&self, session: &BrowserSession) -> QuarryResult<bytes::Bytes>;

    /// List live tabs bound to this browser session. Default: Unsupported.
    async fn list_tabs(&self, _session: &BrowserSession) -> QuarryResult<Vec<BrowserTab>> {
        Err(QuarryError::unsupported_action("list_tabs"))
    }

    /// Open a new tab, optionally navigating it, and make it active. Default: Unsupported.
    async fn new_tab(
        &self,
        _session: &BrowserSession,
        _url: Option<&str>,
    ) -> QuarryResult<BrowserTab> {
        Err(QuarryError::unsupported_action("new_tab"))
    }

    /// Select an existing tab as the active page for actions/frames. Default: Unsupported.
    async fn select_tab(&self, _session: &BrowserSession, _tab_id: &str) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("select_tab"))
    }

    /// Close an existing tab. Default: Unsupported.
    async fn close_tab(&self, _session: &BrowserSession, _tab_id: &str) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("close_tab"))
    }

    /// Return transient DevTools events newer than `after_sequence`. Default: Unsupported.
    async fn devtools_events(
        &self,
        _session: &BrowserSession,
        _after_sequence: u64,
        _limit: usize,
    ) -> QuarryResult<Vec<BrowserDevtoolsEvent>> {
        Err(QuarryError::unsupported_action("devtools_events"))
    }

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

    /// Click at viewport coordinates in CSS pixels. Default: Unsupported.
    async fn click_point(&self, _session: &BrowserSession, _x: f64, _y: f64) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("click_point"))
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

    /// Dispatch a wheel event at viewport coordinates in CSS pixels. Default: Unsupported.
    async fn mouse_wheel(
        &self,
        _session: &BrowserSession,
        _x: f64,
        _y: f64,
        _delta_x: f64,
        _delta_y: f64,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("mouse_wheel"))
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

#[derive(Debug, Clone)]
pub struct BrowserSession {
    pub lease: BrowserLease,
    pub inner: Arc<Mutex<SessionInner>>,
}

#[derive(Debug, Default)]
pub struct SessionInner {
    pub connected: bool,
    pub pages_served: u64,
}
