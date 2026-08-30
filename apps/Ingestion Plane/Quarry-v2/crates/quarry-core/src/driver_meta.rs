//! W2 — Browser driver metadata types shared between the runtime and
//! the browser layer. Lives in `quarry-core` (no I/O, no deps) so
//! the browser drivers can name `LiveViewRef` without depending on
//! the runtime crate (which would create a circular crate graph).

use serde::{Deserialize, Serialize};

/// What kind of live-view URL `BrowserMeta::live_view` points at.
///
/// The App Shell renders each kind differently: `Hls` becomes a
/// `<video>` element, `Iframe` becomes a sandboxed `<iframe>`,
/// `Provider` is a deep-link to the vendor's own player page (e.g.
/// `app.browserbase.com/sessions/...`), `Frames` is the local
/// Chromiumoxide fallback (screenshot stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveViewKind {
    /// HTTP Live Streaming URL (`<video>`).
    Hls,
    /// Direct iframe URL.
    Iframe,
    /// Provider-hosted player; a normal `<a target="_blank">` link
    /// from the App Shell.
    Provider,
    /// Local screenshot frames are the only live view
    /// (Chromiumoxide fallback).
    Frames,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiveViewRef {
    /// The URL the App Shell renders.
    pub url: String,
    /// How the App Shell should render the URL. `None` means
    /// "unknown — infer from the URL scheme" (preserves the
    /// pre-W2 behaviour).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<LiveViewKind>,
    /// When the URL expires and the App Shell must ask the driver
    /// to refresh. `None` means "never expires" (e.g. HLS playback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserMeta {
    pub session_id: Option<String>,
    /// Provider-specific live view. W2 — `BrowserSession` exposes
    /// this through `BrowserDriver::live_view_ref` so the App Shell
    /// can render the agent's cloud browser without polling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_view: Option<LiveViewRef>,
    /// Legacy alias kept for back-compat with pipelines that
    /// already serialize `live_view_url`. New code reads
    /// `BrowserMeta::live_view` and falls back to this for
    /// pre-W2 consumers.
    pub live_view_url: Option<String>,
    pub recording_id: Option<String>,
}
