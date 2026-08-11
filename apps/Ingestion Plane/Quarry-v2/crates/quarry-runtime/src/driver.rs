//! Driver selection. Static vs browser vs TLS-profile fetch.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_core::output::DriverKind;
use quarry_core::privacy::PrivacyPolicy;
use quarry_core::QuarryResult;
use quarry_tls::TlsProfile;

use crate::fetch::FetchResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverSelection {
    pub requested: Option<DriverKind>,
    pub chosen: DriverKind,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BrowserMeta {
    pub session_id: Option<String>,
    pub live_view_url: Option<String>,
    pub recording_id: Option<String>,
}

/// Cache validators to send with a fetch so the upstream can return
/// `304 Not Modified` instead of re-shipping unchanged content.
/// Populated by `PageRunner` from the previous artifact's stored
/// `etag` / `last_modified` (see `NormalizedOutput`).
///
/// Empty by default — drivers that don't implement conditional GET
/// just ignore the hints.
#[derive(Debug, Clone, Default)]
pub struct FetchHints {
    /// Previous response's `ETag`. Sent as `If-None-Match`.
    pub if_none_match: Option<String>,
    /// Previous response's `Last-Modified`. Sent as
    /// `If-Modified-Since` when `If-None-Match` is absent.
    pub if_modified_since: Option<String>,
    /// Render hints. Only browser drivers honour these; static and
    /// TLS-profile drivers ignore them. Plumbed from the public
    /// `ScrapeRequest.render` field on the edge.
    pub render: RenderHints,
    /// Tenant identifier used by the proxy pool to pick a sticky
    /// egress IP for the (org, host) tuple. Empty string is treated
    /// as anonymous and maps deterministically onto slot 0 of the
    /// pool. Ignored when no proxy pool is wired.
    pub org_id: String,
    /// Privacy policy for this fetch. Drivers consult this before using any
    /// third-party proxy, browser, unblocker, or managed provider.
    pub privacy: PrivacyPolicy,
    /// Pre-resolved, validated address set for the request's target host
    /// (see `crate::dns_guard::resolve_public_url`). When present, a driver
    /// that supports it should pin its connection to these addresses rather
    /// than re-resolving DNS, closing the gap between the SSRF guard's check
    /// and the actual connection. `None` when the caller didn't preflight
    /// (or `allow_private_hosts()` made preflighting a no-op) — drivers must
    /// treat that the same as before this field existed.
    pub resolved_target: Option<crate::dns_guard::ResolvedTarget>,
}

/// Browser-only render hints. Static fetch drivers ignore these.
///
/// Today only `wait_for` is plumbed; future work may add `wait_until`
/// (idle/load), `script_to_inject`, viewport, etc.
#[derive(Debug, Clone, Default)]
pub struct RenderHints {
    /// CSS selector to block on after `goto`. Browser drivers call
    /// `wait_for(selector, timeout_ms)` between navigation and content
    /// extraction so SPA-style pages have a chance to render before we
    /// snapshot the DOM. `None` = no wait (default).
    pub wait_for_selector: Option<String>,
    /// Timeout for `wait_for_selector` in milliseconds. Falls back to a
    /// driver-defined default (typically 5000 ms) when unset.
    pub wait_for_timeout_ms: Option<u32>,
}

#[async_trait]
pub trait Driver: Send + Sync {
    fn kind(&self) -> DriverKind;
    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse>;
    /// Conditional GET: send `If-None-Match` / `If-Modified-Since`
    /// when validators are provided. Default impl ignores the hints
    /// and delegates to `fetch` — backward-compatible for drivers
    /// (browser, kernel, browserbase) that don't support conditional
    /// semantics. Only `StaticDriver` overrides this today.
    async fn fetch_conditional(
        &self,
        url: &Url,
        _hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.fetch(url).await
    }
    fn tls_profile(&self) -> Option<TlsProfile> {
        None
    }
    fn browser_meta(&self) -> Option<BrowserMeta> {
        None
    }
}

/// Select driver by url + hints. Simple rule for scaffold:
/// - explicit hint wins
/// - otherwise `static`
/// - upgrade to `browser` if hostname is on JS-required list (populated later)
pub fn select(url: &Url, hint: Option<DriverKind>, js_required_hosts: &[&str]) -> DriverSelection {
    if let Some(k) = hint {
        return DriverSelection {
            requested: Some(k),
            chosen: k,
            reasons: vec!["explicit hint".into()],
        };
    }
    let host = url.host_str().unwrap_or("");
    if js_required_hosts.iter().any(|h| host.ends_with(h)) {
        return DriverSelection {
            requested: None,
            chosen: DriverKind::Browser,
            reasons: vec![format!("host requires js: {host}")],
        };
    }
    DriverSelection {
        requested: None,
        chosen: DriverKind::Static,
        reasons: vec!["default static fetch".into()],
    }
}
