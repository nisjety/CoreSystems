//! quarry-browser — browser lease runtime.
//!
//! Lease model (CONTRACTS §8): session affinity, sticky proxy, durable profile handle.
//! Actual CDP client: later phase (chromiumoxide or remote Browserless).
//!
//! Donor concepts: `internal/browser/`, `internal/driver/rod.go`, session/pool logic.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use quarry_core::contracts::{
    AccessibilityProjection, BrowserDialog, BrowserEgressReceipt, BrowserFrame, BrowserTelemetry,
};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::lease::BrowserLease;
use quarry_core::QuarryResult;

use crate::actions::ScrollTarget;

/// The run-scoped authority enforced at the browser request boundary.
///
/// This deliberately lives with the browser abstraction rather than the HTTP
/// edge: a redirect, frame, service-worker fetch, or image load never returns
/// to the edge for another authorization check. An empty allow-list keeps the
/// established unconstrained-run semantics; a non-empty list admits an exact
/// host or one of its subdomains only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserEgressPolicy {
    allowed_domains: Vec<String>,
    /// Distinguish an intentionally unconstrained run from a malformed
    /// non-empty grant list. Normalizing `[" "]` to `[]` must never turn a
    /// deny-all grant into allow-all authority.
    restricted: bool,
}

impl BrowserEgressPolicy {
    /// A fail-closed policy for a transport boundary that has not yet received
    /// the run's explicit authority. This is intentionally distinct from
    /// `Default`: legacy/read-only callers may still configure an explicitly
    /// unconstrained policy with an empty allowed-domain list, but a freshly
    /// bound browser proxy must never treat missing configuration as that
    /// authority.
    pub fn deny_all() -> Self {
        Self {
            allowed_domains: Vec::new(),
            restricted: true,
        }
    }

    /// Validate the canonical, broker-owned host policy used for governed
    /// agent runs. It intentionally rejects wildcards, IP literals, empty
    /// values, and non-canonical ordering: a validator response must be an
    /// authority, never a hint Quarry quietly repairs.
    pub fn canonical_broker_domains(domains: &[String]) -> Result<Vec<String>, String> {
        const MAX_ALLOWED_DOMAINS: usize = 32;
        if domains.is_empty() || domains.len() > MAX_ALLOWED_DOMAINS {
            return Err("browser grant requires a non-empty bounded domain policy".to_owned());
        }
        let mut canonical = BTreeSet::new();
        for raw in domains {
            let domain = raw.trim().to_ascii_lowercase();
            if !is_canonical_broker_domain(&domain) {
                return Err("browser grant contains an invalid canonical domain".to_owned());
            }
            canonical.insert(domain);
        }
        let canonical: Vec<_> = canonical.into_iter().collect();
        if canonical.as_slice() != domains {
            return Err("browser grant domain policy is not canonical".to_owned());
        }
        Ok(canonical)
    }

    pub fn from_allowed_domains(allowed_domains: &[String]) -> Self {
        let restricted = !allowed_domains.is_empty();
        let allowed_domains = allowed_domains
            .iter()
            .map(|domain| {
                domain
                    .trim()
                    .to_ascii_lowercase()
                    .trim_start_matches("*.")
                    .trim_start_matches('.')
                    .to_owned()
            })
            .filter(|domain| !domain.is_empty())
            .collect();
        Self {
            allowed_domains,
            restricted,
        }
    }

    pub fn allows_url(&self, raw: &str) -> bool {
        // These schemes do not initiate a new network connection. They must
        // remain available under a host grant so an allowed page can render
        // an inline image or use an object URL it created itself.
        if raw.starts_with("data:") || raw.starts_with("blob:") {
            return true;
        }
        if !self.restricted {
            return true;
        }
        let Ok(url) = url::Url::parse(raw) else {
            return false;
        };
        // The browser egress authority is explicitly an HTTP(S) policy. Do
        // not let a hostname-bearing non-web scheme (or `file:`, `about:`,
        // `javascript:`, etc.) inherit domain-grant authority merely because
        // it parses as a URL. `data:` and `blob:` were handled above because
        // they do not establish a network connection.
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let Some(host) = url.host_str().map(|host| host.to_ascii_lowercase()) else {
            return false;
        };
        self.allowed_domains.iter().any(|domain| {
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    }
}

fn is_canonical_broker_domain(domain: &str) -> bool {
    if domain.is_empty()
        || domain.len() > 253
        || domain.parse::<IpAddr>().is_ok()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

pub mod actions;
pub mod browserbase;
pub mod browserless;
#[cfg(feature = "chromiumoxide")]
pub mod chromiumoxide;
#[cfg(feature = "kernel")]
pub mod kernel;
pub mod navigation;
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

/// A browser-driver request that binds target verification and an effectful
/// operation into one driver operation. Runtime must never degrade this to a
/// later `click(selector)`: between those calls a page could replace a target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedTargetAction {
    /// Browser-native binding captured from the accessibility tree. When
    /// present, drivers must resolve this exact backend DOM node and must not
    /// fall back to the selector fields below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_target: Option<BrowserNativeTarget>,
    /// Legacy compatibility binding for drivers that do not yet expose a
    /// native accessibility projection. It is never authoritative when
    /// `native_target` is present.
    pub selector: String,
    pub tag: String,
    pub attributes: Vec<(String, String)>,
    pub normalized_text: String,
    pub operation: VerifiedTargetOperation,
}

/// Provider-neutral browser capabilities. A `true` value is an affirmative
/// implementation claim; omitted/default values are intentionally deny-by-
/// default for routing and agent action policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDriverCapabilities {
    pub persistent_profile: bool,
    pub devtools_trace: bool,
    pub downloads_to_artifacts: bool,
    /// The driver can attach a tenant-approved Quarry artifact to a native
    /// file input without accepting a caller-controlled host path.
    pub uploads_from_artifacts: bool,
    /// The driver can render the full Chromium-compatible visual surface
    /// required by interactive flows (rather than a lightweight evidence
    /// renderer with explicit fallback requirements).
    pub full_visual_fidelity: bool,
    pub isolated_egress: bool,
    /// There is current, independently verifiable evidence that the driver's
    /// network enforcement covers its advertised request surface. A driver
    /// must not infer this merely from a top-level URL guard.
    pub security_evidence: bool,
    pub atomic_target_actions: bool,
}

/// Browser-authored page state for an agent observation. Drivers return only
/// redacted, bounded data; execution remains separately governed by Quarry.
#[derive(Debug, Clone, Default)]
pub struct BrowserNativeProjection {
    pub accessibility: Option<AccessibilityProjection>,
    pub frames: Vec<BrowserFrame>,
    /// Opaque execution bindings corresponding to the public accessibility
    /// projection. This is deliberately not serialized into an observation:
    /// Model Plane receives only `@e…` refs, never CDP node handles.
    pub targets: Vec<BrowserNativeTarget>,
}

/// A short-lived renderer binding for one accessible DOM node. The binding is
/// valid only for the snapshot that produced it. A child-frame binding may be
/// executed only through Quarry's separate, explicit frame-ref contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserNativeTarget {
    pub ax_node_id: String,
    pub backend_node_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// A browser download that has completed inside Quarry's private per-session
/// directory. It deliberately carries bytes and bounded metadata only: host
/// filesystem paths never cross the browser-driver boundary.
#[derive(Debug, Clone)]
pub struct BrowserDownloadedFile {
    pub bytes: bytes::Bytes,
    pub suggested_filename: String,
    pub source_url: String,
}

/// Loopback endpoint of Quarry's DNS-pinning browser egress authority. It is
/// intentionally a distinct type so a driver cannot accidentally advertise a
/// generic third-party proxy as isolated egress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedEgressProxyEndpoint(String);

impl PinnedEgressProxyEndpoint {
    pub fn parse(raw: impl Into<String>) -> QuarryResult<Self> {
        let raw = raw.into();
        let url = url::Url::parse(&raw).map_err(|_| {
            QuarryError::new(
                ErrorCode::BadRequest,
                "invalid pinned egress proxy endpoint",
            )
        })?;
        let is_loopback = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(_)) | None => false,
        };
        if url.scheme() != "http"
            || !is_loopback
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "pinned egress proxy must be a credential-free loopback HTTP endpoint",
            ));
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Supplies a private, DNS-pinning proxy endpoint for one browser lease.
/// Implementations must return a distinct endpoint per session so transport
/// receipts can be attributed without putting an internal run id on the wire.
#[async_trait]
pub trait BrowserEgressProxyProvider: Send + Sync {
    async fn endpoint_for_session(
        &self,
        session_key: &str,
    ) -> QuarryResult<PinnedEgressProxyEndpoint>;

    async fn receipts_after(
        &self,
        session_key: &str,
        after_sequence: u64,
        limit: usize,
    ) -> QuarryResult<Vec<BrowserEgressReceipt>>;

    /// Install the same grant-derived policy at the transport boundary before
    /// a browser context can initiate a connection. Fetch interception remains
    /// defense in depth; it is not the proxy's sole authorization mechanism.
    async fn configure_policy(
        &self,
        session_key: &str,
        policy: BrowserEgressPolicy,
    ) -> QuarryResult<()>;

    async fn release_session(&self, session_key: &str);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VerifiedTargetOperation {
    Click,
    Type { text: String },
    Select { value: String },
    WaitFor { timeout_ms: u32 },
}

#[async_trait]
pub trait BrowserDriver: Send + Sync {
    fn capabilities(&self) -> BrowserDriverCapabilities {
        BrowserDriverCapabilities::default()
    }

    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession>;
    async fn release(&self, session: BrowserSession) -> QuarryResult<()>;

    /// W2 — return the live-view reference for the given session, if
    /// the driver has one. Cloud providers (Browserbase, Browserless,
    /// Kernel) populate this; the local Chromiumoxide driver
    /// returns `None` and the App Shell falls back to its own
    /// frame stream. Default: `Ok(None)`.
    async fn live_view(
        &self,
        _session: &BrowserSession,
    ) -> QuarryResult<Option<quarry_core::driver_meta::LiveViewRef>> {
        Ok(None)
    }

    /// W2 — refresh the live-view reference when the previous one
    /// has expired. Cloud providers rotate the URL with the same
    /// session id; the local driver returns `Ok(None)` and the App
    /// Shell keeps the last good URL. Default: `Ok(None)`.
    async fn refresh_live_view(
        &self,
        _session: &BrowserSession,
    ) -> QuarryResult<Option<quarry_core::driver_meta::LiveViewRef>> {
        Ok(None)
    }

    /// Set the run-scoped host grants used by the driver's network boundary.
    /// Implementations must consult this policy for every browser-originated
    /// request, including redirects, frames, XHR/fetch, and subresources.
    /// An implementation that does not own this boundary is not
    /// source-compatible for browser navigation: accepting a policy as a
    /// no-op would make a remote provider look governed when it is not.
    async fn configure_egress_policy(
        &self,
        _session: &BrowserSession,
        _policy: BrowserEgressPolicy,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("configure_egress_policy"))
    }
    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()>;
    async fn content(&self, session: &BrowserSession) -> QuarryResult<bytes::Bytes>;

    /// Verify a uniquely-selected target and perform the operation as one
    /// driver operation. Drivers without an atomic implementation must reject
    /// snapshot actions rather than silently falling back to raw CSS.
    async fn act_on_verified_target(
        &self,
        _session: &BrowserSession,
        _action: VerifiedTargetAction,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("verified_target_action"))
    }

    /// Attach a Quarry-internal staged artifact to one exact native file input.
    /// `staged_file` is deliberately not part of any public action contract:
    /// callers supply opaque artifact ids and this method is the sole point at
    /// which an implementation may see a temporary host path.
    async fn upload_staged_file_to_target(
        &self,
        _session: &BrowserSession,
        _target: BrowserNativeTarget,
        _staged_file: &Path,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("upload_ref"))
    }

    /// Click an exact native target and collect its resulting download from a
    /// driver-private quarantine directory. Implementations must reject any
    /// file outside that directory and must not expose its local path.
    async fn download_from_verified_target(
        &self,
        _session: &BrowserSession,
        _target: BrowserNativeTarget,
    ) -> QuarryResult<BrowserDownloadedFile> {
        Err(QuarryError::unsupported_action("download_ref"))
    }
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

    /// Return redacted egress-policy decisions newer than `after_sequence`.
    /// Drivers that cannot prove a central request boundary must leave this
    /// unsupported and must not claim `isolated_egress`.
    async fn egress_receipts(
        &self,
        _session: &BrowserSession,
        _after_sequence: u64,
        _limit: usize,
    ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
        Err(QuarryError::unsupported_action("egress_receipts"))
    }

    /// Return measured browser facts for the active session. The default is an
    /// explicit unknown/empty sample so a read-only driver remains usable
    /// without inventing CPU, memory, startup, or cost data.
    async fn telemetry(&self, _session: &BrowserSession) -> QuarryResult<BrowserTelemetry> {
        Ok(BrowserTelemetry::default())
    }

    /// Return the browser's computed accessibility tree and frame topology.
    /// This is read-only observation data, never a raw CDP escape hatch.
    async fn native_page_projection(
        &self,
        _session: &BrowserSession,
    ) -> QuarryResult<BrowserNativeProjection> {
        Err(QuarryError::unsupported_action("native_page_projection"))
    }

    /// Return currently open JavaScript dialogs. Drivers must never resolve a
    /// dialog automatically as a side effect of observation.
    async fn dialogs(&self, _session: &BrowserSession) -> QuarryResult<Vec<BrowserDialog>> {
        Err(QuarryError::unsupported_action("dialogs"))
    }

    /// Accept or dismiss one specific observed dialog. Callers must validate
    /// their approval/grant before entering the driver; the driver validates
    /// that the dialog id remains live in this session.
    async fn respond_dialog(
        &self,
        _session: &BrowserSession,
        _dialog_id: &str,
        _accept: bool,
        _prompt_text: Option<&str>,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("respond_dialog"))
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
