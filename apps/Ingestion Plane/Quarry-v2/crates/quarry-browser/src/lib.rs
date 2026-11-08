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

pub use quarry_security::preflight::{
    HostReputation, HostStanding, ReputationMode, REPUTATION_MODE_ENV,
};

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
    /// `Allowlist` (the established grant-scoped mode) or `OpenWeb` (a
    /// human-browse grant: any public http(s) host not on `denied_domains`).
    mode: EgressMode,
    /// Only consulted in `OpenWeb` mode. Canonical, lower-case, dot-suffix
    /// matched; a host equal to or under an entry is refused.
    denied_domains: Vec<String>,
    public_assets: bool,
    only_origin: Option<url::Origin>,
    /// The URL reputation hook (E3.8.7, D2-100). `None` (the default, and
    /// always when the mode is `Off`) never consults a list.
    reputation: Option<ReputationHook>,
}

/// A reputation source and the mode the run's org applies it in (E3.8.7). Two
/// hooks are equal when they share one source and one mode.
#[derive(Clone)]
pub struct ReputationHook {
    source: Arc<dyn HostReputation>,
    mode: ReputationMode,
}

impl ReputationHook {
    /// The mode this hook applies, never `Off`.
    pub fn mode(&self) -> ReputationMode {
        self.mode
    }
}

impl std::fmt::Debug for ReputationHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReputationHook")
            .field("source", &self.source.source())
            .field("mode", &self.mode)
            .finish()
    }
}

impl PartialEq for ReputationHook {
    fn eq(&self, other: &Self) -> bool {
        // The data pointers only: two vtables for one type can differ across codegen units.
        std::ptr::eq(
            Arc::as_ptr(&self.source) as *const (),
            Arc::as_ptr(&other.source) as *const (),
        ) && self.mode == other.mode
    }
}

impl Eq for ReputationHook {}

/// What the reputation hook decided for one request (E3.8.7). `source` is the
/// hook's stable name; it never carries a host or URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReputationOutcome {
    /// Listed, and the org runs in `warn`: the request goes on and the receipt says so.
    Warn { source: &'static str },
    /// Listed, and the org runs in `block`: the request is refused.
    Block { source: &'static str },
}

impl ReputationOutcome {
    pub fn is_block(&self) -> bool {
        matches!(self, Self::Block { .. })
    }

    pub fn source(&self) -> &'static str {
        match self {
            Self::Warn { source } | Self::Block { source } => source,
        }
    }

    /// The stricter of two outcomes (a block beats a warning beats nothing),
    /// for a request two policies govern (a session and its tab).
    pub fn strictest(first: Option<Self>, second: Option<Self>) -> Option<Self> {
        match (first, second) {
            (Some(outcome), _) if outcome.is_block() => Some(outcome),
            (_, Some(outcome)) if outcome.is_block() => Some(outcome),
            (Some(outcome), _) | (None, Some(outcome)) => Some(outcome),
            (None, None) => None,
        }
    }
}

/// How a browser egress policy admits hosts. Default is `Allowlist`, so every
/// existing constructor keeps its meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EgressMode {
    #[default]
    Allowlist,
    /// Any public http(s) host that is not denied. Private, loopback, link-local,
    /// single-label and cloud-metadata hosts are refused regardless of the
    /// denied list; see `open_web_host_is_local`.
    OpenWeb,
}

/// Upper bound on a broker grant's denied list (plan WS8.B, R-16 family).
pub const MAX_DENIED_DOMAINS: usize = 64;

/// The navigation authority a broker validation grants a run. Built only from
/// the broker's own answer, never from the caller's requested domains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerNavPolicy {
    /// Exact canonical host list (`policy_mode` empty or `allowlist`).
    Allowlist { domains: Vec<String> },
    /// Open web minus `denied` (`policy_mode` `open_web`).
    OpenWeb { denied: Vec<String> },
}

impl BrokerNavPolicy {
    /// Fail closed: an unknown mode, an allow-list carrying a deny list, or an
    /// open-web grant naming allowed hosts is an error, never a fallback.
    pub fn from_validation(
        policy_mode: &str,
        allowed_domains: &[String],
        denied_domains: &[String],
    ) -> Result<Self, String> {
        match policy_mode {
            "" | "allowlist" => {
                if !denied_domains.is_empty() {
                    return Err("allow-list browser grant carries a denied list".to_owned());
                }
                Ok(Self::Allowlist {
                    domains: BrowserEgressPolicy::canonical_broker_domains(allowed_domains)?,
                })
            }
            "open_web" => {
                if !allowed_domains.is_empty() {
                    return Err("open-web browser grant must not name allowed domains".to_owned());
                }
                Ok(Self::OpenWeb {
                    denied: BrowserEgressPolicy::canonical_denied_domains(denied_domains)?,
                })
            }
            _ => Err("unknown browser grant policy mode".to_owned()),
        }
    }

    /// The egress policy the browser boundary enforces for this grant.
    pub fn egress_policy(&self) -> BrowserEgressPolicy {
        match self {
            Self::Allowlist { domains } => BrowserEgressPolicy::from_allowed_domains(domains),
            Self::OpenWeb { denied } => BrowserEgressPolicy::open_web(denied),
        }
    }

    /// The wire name of the mode, as stored in a run checkpoint.
    pub fn mode_name(&self) -> &'static str {
        match self {
            Self::Allowlist { .. } => "allowlist",
            Self::OpenWeb { .. } => "open_web",
        }
    }
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
            mode: EgressMode::Allowlist,
            denied_domains: Vec::new(),
            public_assets: false,
            only_origin: None,
            reputation: None,
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
            mode: EgressMode::Allowlist,
            denied_domains: Vec::new(),
            public_assets: false,
            only_origin: None,
            reputation: None,
        }
    }

    /// An open-web egress policy: any public http(s) host except `denied`
    /// (and except private or metadata hosts, which are always refused). The
    /// caller validates `denied` with `canonical_denied_domains` first; this
    /// constructor only normalises case and trims.
    pub fn open_web(denied: &[String]) -> Self {
        let denied_domains = denied
            .iter()
            .map(|domain| domain.trim().to_ascii_lowercase())
            .filter(|domain| !domain.is_empty())
            .collect();
        Self {
            allowed_domains: Vec::new(),
            restricted: true,
            mode: EgressMode::OpenWeb,
            denied_domains,
            public_assets: false,
            only_origin: None,
            reputation: None,
        }
    }

    /// Limit credentialed traffic in a borrowed tab to the origin the owner approved.
    pub fn with_only_origin(mut self, origin: url::Origin) -> Self {
        self.only_origin = Some(origin);
        self
    }

    /// Allow the request guard to render a public asset through its
    /// credentialless fetcher. This does not permit a proxy connection.
    pub fn with_public_assets(mut self) -> Self {
        // A policy narrowed to an empty allowlist grants no network authority.
        self.public_assets = !(self.mode == EgressMode::Allowlist
            && self.restricted
            && self.allowed_domains.is_empty());
        self
    }

    pub fn allows_public_asset(&self, raw: &str) -> bool {
        self.public_assets
            && raw.starts_with("https://")
            && open_web_url_allowed(raw, &self.denied_domains)
    }

    /// Add hosts the run may never reach, in any mode (WS7.G). A host equal to or under an
    /// entry is refused for every http(s) request: navigation, redirect, frame and
    /// sub-resource. Entries are trimmed and lower-cased; an empty list changes nothing, so
    /// a policy built without this call is byte-identical to one built before it existed.
    pub fn with_denied_domains(mut self, denied: &[String]) -> Self {
        let mut added = false;
        for domain in denied {
            let domain = domain
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if !domain.is_empty() && !self.denied_domains.contains(&domain) {
                self.denied_domains.push(domain);
                added = true;
            }
        }
        if added {
            self.denied_domains.sort();
        }
        self
    }

    /// Attach a URL reputation source in the org's mode (E3.8.7, D2-100). The
    /// caller resolves the mode with `ReputationMode::from_env().for_org(..)`.
    /// `Off` removes any hook, so the policy is the one built without this call.
    /// The hook is consulted by [`Self::reputation_outcome`] only, never by
    /// [`Self::allows_url`]: the grant decides first, then the reputation, then
    /// the DNS guard, and each refusal keeps its own receipt category.
    pub fn with_reputation(mut self, source: Arc<dyn HostReputation>, mode: ReputationMode) -> Self {
        self.reputation = match mode {
            ReputationMode::Off => None,
            mode => Some(ReputationHook { source, mode }),
        };
        self
    }

    /// The reputation mode this policy applies (`Off` without a hook).
    pub fn reputation_mode(&self) -> ReputationMode {
        self.reputation
            .as_ref()
            .map_or(ReputationMode::Off, |hook| hook.mode)
    }

    /// The reputation decision for one request URL, or `None` when there is no
    /// hook, the URL is not http(s) with a host, or the host is not listed. A
    /// source that cannot answer at once (`Unknown`) is taken as unlisted.
    pub fn reputation_outcome(&self, raw: &str) -> Option<ReputationOutcome> {
        let hook = self.reputation.as_ref()?;
        let url = url::Url::parse(raw).ok()?;
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let host = url
            .host_str()?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if host.is_empty() || hook.source.standing(&host) != HostStanding::Listed {
            return None;
        }
        let source = hook.source.source();
        match hook.mode {
            ReputationMode::Off => None,
            ReputationMode::Warn => Some(ReputationOutcome::Warn { source }),
            ReputationMode::Block => Some(ReputationOutcome::Block { source }),
        }
    }

    /// The hosts this policy refuses whatever else it allows.
    pub fn denied_domains(&self) -> &[String] {
        &self.denied_domains
    }

    /// True for an allow-list policy that names no host: the "unchanged" resolution of an
    /// unrestricted run. A deny list, if any, does not make it restricted to a host set.
    pub fn is_unconstrained(&self) -> bool {
        self.mode == EgressMode::Allowlist && !self.restricted && self.allowed_domains.is_empty()
    }

    /// Whether `raw` is an http(s) URL whose host is equal to or under a denied entry.
    pub fn denies_url(&self, raw: &str) -> bool {
        if self.denied_domains.is_empty() {
            return false;
        }
        let Ok(url) = url::Url::parse(raw) else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let Some(host) = url
            .host_str()
            .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
        else {
            return false;
        };
        self.denied_domains.iter().any(|entry| matches_denied_host(&host, entry))
    }

    /// Validate a broker's denied list for an open-web grant: empty is allowed,
    /// at most `MAX_DENIED_DOMAINS` entries, each a canonical DNS name or a
    /// leading-dot suffix, sorted and unique. A non-canonical list is an
    /// error, never repaired, the same rule as `canonical_broker_domains`.
    pub fn canonical_denied_domains(domains: &[String]) -> Result<Vec<String>, String> {
        if domains.len() > MAX_DENIED_DOMAINS {
            return Err("browser grant denied list exceeds its bound".to_owned());
        }
        let mut canonical = BTreeSet::new();
        for raw in domains {
            let domain = raw.trim().to_ascii_lowercase();
            let name = domain.strip_prefix('.').unwrap_or(&domain);
            if !is_canonical_dns_name(name) {
                return Err("browser grant denied list contains an invalid canonical domain".to_owned());
            }
            canonical.insert(domain);
        }
        let canonical: Vec<_> = canonical.into_iter().collect();
        if canonical.as_slice() != domains {
            return Err("browser grant denied list is not canonical".to_owned());
        }
        Ok(canonical)
    }

    pub fn allows_url(&self, raw: &str) -> bool {
        // These schemes do not initiate a new network connection. They must
        // remain available under a host grant so an allowed page can render
        // an inline image or use an object URL it created itself.
        if raw.starts_with("data:") || raw.starts_with("blob:") {
            return true;
        }
        if let Some(origin) = &self.only_origin {
            if !url::Url::parse(raw).is_ok_and(|url| url.origin() == *origin) {
                return false;
            }
        }
        if self.mode == EgressMode::OpenWeb {
            return open_web_url_allowed(raw, &self.denied_domains);
        }
        // WS7.G: the policy's deny list wins over an allow-list entry and over an
        // unrestricted run. Empty by default, so this line costs nothing for a run without one.
        if self.denies_url(raw) {
            return false;
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

/// Whether an open-web policy may reach `raw`: http(s) only, a host that is
/// not local (see `open_web_host_is_local`) and not equal to or under a denied
/// entry.
fn open_web_url_allowed(raw: &str, denied: &[String]) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str().map(|host| host.trim_end_matches('.').to_ascii_lowercase()) else {
        return false;
    };
    if open_web_host_is_local(&host) {
        return false;
    }
    !denied.iter().any(|entry| matches_denied_host(&host, entry))
}

fn matches_denied_host(host: &str, entry: &str) -> bool {
    if entry.starts_with('.') {
        // A leading dot denotes a suffix below the named host.
        host.ends_with(entry)
    } else {
        host == entry || host.strip_suffix(entry).is_some_and(|prefix| prefix.ends_with('.'))
    }
}

/// Hosts an open-web policy never reaches, whatever the broker's deny list says:
/// loopback, private, link-local, unique-local, CGNAT, unspecified, multicast
/// addresses; single-label names; local-only suffixes; and the cloud metadata
/// names. A check on the name only: the DNS guard still resolves and checks.
fn open_web_host_is_local(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return ip_is_local(ip);
    }
    if !bare.contains('.') {
        return true;
    }
    const LOCAL_SUFFIXES: [&str; 6] = [
        ".internal",
        ".local",
        ".localhost",
        ".localdomain",
        ".lan",
        ".home.arpa",
    ];
    bare == "metadata.goog" || LOCAL_SUFFIXES.iter().any(|suffix| bare.ends_with(suffix))
}

fn ip_is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..=127).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ip_is_local(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

fn is_canonical_broker_domain(domain: &str) -> bool {
    domain.contains('.') && is_canonical_dns_name(domain)
}

fn is_canonical_dns_name(domain: &str) -> bool {
    if domain.is_empty()
        || domain.len() > 253
        || domain.parse::<IpAddr>().is_ok()
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

#[cfg(feature = "chromiumoxide")]
mod actionability;
pub mod actions;
pub mod browserbase;
pub mod browserless;
#[cfg(feature = "chromiumoxide")]
pub mod chromiumoxide;
pub mod capture_plan;
/// WS13.6: tile-and-stitch planning and encoders (pure; no CDP call).
pub mod stitch;
/// WS12.S5: where the sensitive fields are on screen (the read-only scan).
pub mod sensitive_rects;
/// WS12.S5: painting those rectangles over a screenshot, or withholding it.
pub mod screenshot_mask;

/// WS12.S5: a screenshot after the sensitive fields were painted over.
///
/// `bytes` is `None` when the image was withheld: nothing may be stored or sent,
/// and `note` says why in the closed `screenshot_withheld:<cause>` vocabulary.
/// `rects` are the painted rectangles in image pixels; they stay in process (the
/// stamp carries only their count).
#[derive(Debug, Clone)]
pub struct MaskedScreenshot {
    pub bytes: Option<bytes::Bytes>,
    pub state: quarry_core::redaction::ScreenshotState,
    pub rects: Vec<screenshot_mask::PixelRect>,
    pub oopif_frames: u32,
    pub mask_ms: u32,
    pub note: Option<&'static str>,
}

impl MaskedScreenshot {
    /// An image that was not scanned: a driver with no masking reports `none`.
    pub fn unmasked(bytes: bytes::Bytes) -> Self {
        Self {
            bytes: Some(bytes),
            state: quarry_core::redaction::ScreenshotState::NotApplicable,
            rects: Vec::new(),
            oopif_frames: 0,
            mask_ms: 0,
            note: None,
        }
    }

    /// An image that was withheld.
    pub fn withheld(withheld: screenshot_mask::Withheld, oopif_frames: u32, mask_ms: u32) -> Self {
        Self {
            bytes: None,
            state: quarry_core::redaction::ScreenshotState::Withheld,
            rects: Vec::new(),
            oopif_frames,
            mask_ms,
            note: Some(withheld.note()),
        }
    }

    /// The error a caller that needs the image (an explicit `screenshot` action, a
    /// scrape capture) gets when it was withheld: `SecurityBlocked`, 403.
    pub fn withheld_error(note: &'static str) -> QuarryError {
        QuarryError::new(
            ErrorCode::SecurityBlocked,
            "the screenshot was withheld because a sensitive field could not be masked",
        )
        .with_details(serde_json::json!({ "reason": note }))
    }
}

/// WS13.6: a heavy-lane permit that lets one screenshot stitch tiles. The
/// runtime takes it from the heavy semaphore only when the page needs stitching,
/// and holds it for the one capture.
pub struct StitchPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl StitchPermit {
    pub fn new(permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        Self { _permit: permit }
    }
}
mod deadlines;
#[cfg(feature = "chromiumoxide")]
pub mod live_gate;
#[cfg(feature = "kernel")]
pub mod kernel;
pub mod navigation;
pub mod persistent_session;
pub mod pool;
pub mod session;
/// WS17.C1: snapshot schema v2 types, caps, the origin journal and the capture flags (pure).
pub mod profile_snapshot;
/// WS17.C1: capture v2 (cookies of the whole context, web storage, IndexedDB) over CDP.
#[cfg(feature = "chromiumoxide")]
mod profile_capture;
/// WS17.C2: restore of a saved profile into a fresh context (cookie plan, seeding script,
/// emulation; pure) and its CDP half (feature `chromiumoxide`).
pub mod profile_restore;
pub mod slots;
pub mod tab_control;
mod field_probe;

#[cfg(feature = "kernel")]
pub use kernel::{KernelConfig, KernelDriver};
pub use persistent_session::{PersistentSession, PersistentSessionRegistry};
pub use tab_control::OpenTabOptions;

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

/// Actual CDP pointer movement captured together with a live frame. Coordinates
/// use CSS viewport pixels and identify the pinned attachment that moved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAgentCursor {
    pub x: u32,
    pub y: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub tab_id: String,
    pub attach_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFrame {
    pub mime_type: String,
    pub data_base64: String,
    pub agent_cursor: Option<LiveAgentCursor>,
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
    /// Ref-targeted click, type and select run the actionability pipeline
    /// (visible, enabled, stable, not covered) and send trusted input
    /// (`event.isTrusted` is true) instead of in-page script events. True only
    /// when the driver was started with `QUARRY_BROWSER_TRUSTED_INPUT=on`.
    /// Additive on the wire: an older peer omits it and reads as `false`.
    #[serde(default)]
    pub trusted_input: bool,
    /// WS8.E: the driver can open a tab without making it the active tab
    /// (`OpenTabOptions::activate == false`). Additive on the wire.
    #[serde(default)]
    pub background_tabs: bool,
    /// WS8.E: a handle pinned to one tab (`BrowserSession::pinned_to`) addresses
    /// that tab and never the active tab. Additive on the wire.
    #[serde(default)]
    pub tab_pinning: bool,
    /// WS8.E: an egress policy can be installed for one tab, not only for the
    /// session. Additive on the wire.
    #[serde(default)]
    pub per_tab_egress: bool,
    /// WS13.3: the driver can run scrape-time `actions[]` on one page. Additive
    /// on the wire: an older peer omits it and reads as `false`.
    #[serde(default)]
    pub scrape_actions: bool,
    /// WS13.3: the driver takes screenshots and PDFs with options (clip, JPEG,
    /// viewport, paper size), not only the legacy viewport PNG and default PDF.
    /// Additive on the wire.
    #[serde(default)]
    pub capture_options: bool,
    /// E4.4-a: the driver emulates a page's locale, timezone and geolocation when the page opens
    /// (`BrowserDriver::set_page_emulation`). Additive on the wire.
    #[serde(default)]
    pub page_emulation: bool,
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
    /// WS12.S1: what the field-facts probe and the masking did to this projection.
    /// `None` only for a projection that was not produced by the masking pass.
    pub redaction: Option<ProjectionRedaction>,
}

/// WS12.S1 stamp for one native projection: counts and rule ids only, never
/// content. The observation-level stamp (WS12.S2) is built from it.
#[derive(Debug, Clone, Default)]
pub struct ProjectionRedaction {
    pub probe: quarry_core::redaction::ProbeState,
    pub probe_ms: u32,
    pub values_masked: u32,
    pub by_rule: std::collections::BTreeMap<String, u32>,
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
    /// WS12.S1: the field kind the probe decided for an editable control; `None`
    /// for every other role. Crate-internal: never serialised into an observation.
    #[serde(skip)]
    pub field_kind: Option<quarry_core::redaction::FieldKind>,
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

    /// Fetch a single public asset hop without Chromium's cookies, request
    /// headers or body. A provider without a pinned fetch path declines.
    async fn fetch_public_asset(
        &self,
        _session_key: &str,
        _url: &url::Url,
    ) -> QuarryResult<PublicBrowserAsset> {
        Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "credentialless browser assets unavailable",
        ))
    }

    async fn release_session(&self, session_key: &str);
}

pub struct PublicBrowserAsset {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
}

/// How a browser driver's own processes are doing, for `/ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHealthState {
    /// The driver runs no local browser (a remote provider, a test double).
    NotApplicable,
    /// No browser has been launched yet. Ready: the first run launches one.
    Idle,
    Healthy,
    /// Some launched browsers are unhealthy or dead, not all.
    Degraded,
    /// Every launched browser is unhealthy or dead.
    Unhealthy,
}

/// One browser process of a local driver. Content-free: counts and states only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotHealth {
    pub slot: usize,
    pub status: String,
    pub generation: u64,
    pub sessions: usize,
    pub pages: usize,
    /// `None` until the first probe answered.
    pub healthy: Option<bool>,
    pub probe_failures: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHealth {
    pub state: BrowserHealthState,
    /// 0 = no cap.
    pub max_sessions: usize,
    pub sessions_open: usize,
    pub slots: Vec<SlotHealth>,
}

impl BrowserHealth {
    pub fn not_applicable() -> Self {
        Self {
            state: BrowserHealthState::NotApplicable,
            max_sessions: 0,
            sessions_open: 0,
            slots: Vec::new(),
        }
    }

    /// The overall state from the slots: never launched is `Idle`, every
    /// launched slot dead or unhealthy is `Unhealthy`, some is `Degraded`.
    pub fn from_slots(max_sessions: usize, sessions_open: usize, slots: Vec<SlotHealth>) -> Self {
        let launched: Vec<&SlotHealth> = slots.iter().filter(|slot| slot.generation > 0).collect();
        let bad = launched
            .iter()
            .filter(|slot| slot.status == "dead" || slot.healthy == Some(false))
            .count();
        let state = if launched.is_empty() {
            BrowserHealthState::Idle
        } else if bad == launched.len() {
            BrowserHealthState::Unhealthy
        } else if bad > 0 {
            BrowserHealthState::Degraded
        } else {
            BrowserHealthState::Healthy
        };
        Self {
            state,
            max_sessions,
            sessions_open,
            slots,
        }
    }
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
    /// Must be idempotent: releasing a session twice (or a clone of a released
    /// one) is not an error and changes nothing the second time.
    async fn release(&self, session: BrowserSession) -> QuarryResult<()>;

    /// Stop whatever browser process this driver owns and wait for it, for the process's
    /// graceful exit (after the sessions have been released). Idempotent. Default: nothing to
    /// stop (remote providers, test doubles).
    async fn shutdown(&self) {}

    /// How this driver's own browser processes are doing, for `/ready`.
    /// Default: not applicable (remote providers, test doubles).
    async fn health(&self) -> BrowserHealth {
        BrowserHealth::not_applicable()
    }

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

    /// E4.4-a: the locale, timezone and geolocation the next page this session opens emulates.
    /// It is set after `acquire` and before the session's first navigation; a session whose page
    /// is already open refuses it. A driver that cannot emulate refuses here too, so a page never
    /// quietly has the host's locale when the caller asked for another. Default: refused.
    async fn set_page_emulation(
        &self,
        _session: &BrowserSession,
        _emulation: &quarry_core::page_emulation::PageEmulation,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("page_emulation"))
    }
    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()>;

    /// Give a session that has no page yet one to observe (SC4 D1): a tab on
    /// `about:blank`, navigated nowhere, which the session's next `goto` uses. A session
    /// that has a page keeps it; a lost session and a pinned handle whose tab is gone keep
    /// their errors. Observation paths call it before reading a page they did not
    /// navigate to. Default: nothing (a driver whose session always has a page).
    async fn ensure_page(&self, _session: &BrowserSession) -> QuarryResult<()> {
        Ok(())
    }
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

    /// Best-effort viewport bounds for an observed native target. These bounds
    /// are presentation only and must never authorize or select an action.
    /// Drivers without a trustworthy geometry source omit them.
    async fn native_target_rect(
        &self,
        _session: &BrowserSession,
        _target: &BrowserNativeTarget,
    ) -> Option<quarry_core::contracts::ElementRect> {
        None
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
    /// WS12.S5: a screenshot with the sensitive fields painted over. A driver that
    /// cannot scan (the default) reports `none`: the image is as `screenshot` made it.
    /// A driver that can scan overrides this and makes `screenshot` return the
    /// masked bytes, so no caller can reach the raw image.
    async fn screenshot_masked(
        &self,
        session: &BrowserSession,
        full_page: bool,
    ) -> QuarryResult<MaskedScreenshot> {
        let bytes = self.screenshot(session, full_page).await?;
        Ok(MaskedScreenshot::unmasked(bytes))
    }
    /// Move the real pointer for a governed attached action. Unsupported drivers
    /// omit the cursor; the browser action itself remains separately governed.
    async fn move_agent_pointer(
        &self,
        _session: &BrowserSession,
        _attach_id: &str,
        _x: u32,
        _y: u32,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("move_agent_pointer"))
    }

    /// Hide the pointer telemetry before another operation can move the mouse.
    async fn clear_agent_pointer(&self, _session: &BrowserSession, _attach_id: &str) {}

    async fn live_frame(
        &self,
        _session: &BrowserSession,
        _options: LiveFrameOptions,
    ) -> QuarryResult<LiveFrame> {
        Err(QuarryError::unsupported_action("live_frame"))
    }
    async fn pdf(&self, session: &BrowserSession) -> QuarryResult<bytes::Bytes>;

    /// WS13.3: a screenshot with options. Legacy options (every option at its
    /// default: the viewport, PNG) delegate to `screenshot`; any other option
    /// needs a driver that overrides this method, and is refused here rather than
    /// silently dropped.
    async fn capture_screenshot(
        &self,
        session: &BrowserSession,
        options: &quarry_core::capture::ScreenshotOptions,
        limits: &quarry_core::capture::CaptureLimits,
    ) -> QuarryResult<quarry_core::capture::CapturedImage> {
        if !options.is_legacy() {
            return Err(QuarryError::unsupported_action("capture_screenshot_options"));
        }
        let bytes = self.screenshot(session, false).await?;
        crate::capture_plan::check_capture_size(bytes.len(), limits)?;
        let Some((width_px, height_px)) = crate::capture_plan::image_dimensions(&bytes) else {
            return Err(
                QuarryError::new(ErrorCode::DriverFailed, "screenshot is not a PNG or JPEG")
                    .with_details(serde_json::json!({ "reason": "screenshot_unreadable" })),
            );
        };
        Ok(quarry_core::capture::CapturedImage {
            bytes: bytes.to_vec(),
            format: quarry_core::capture::ImageFormat::Png,
            width_px,
            height_px,
            tiles: 1,
            stitched: false,
            truncated: false,
        })
    }

    /// WS13.3: a PDF with options. The default options delegate to `pdf`; any
    /// other option needs a driver that overrides this method.
    async fn capture_pdf(
        &self,
        session: &BrowserSession,
        options: &quarry_core::capture::PdfOptions,
        limits: &quarry_core::capture::CaptureLimits,
    ) -> QuarryResult<quarry_core::capture::CapturedPdf> {
        if !options.is_default() {
            return Err(QuarryError::unsupported_action("capture_pdf_options"));
        }
        let bytes = self.pdf(session).await?;
        crate::capture_plan::check_capture_size(bytes.len(), limits)?;
        Ok(quarry_core::capture::CapturedPdf {
            bytes: bytes.to_vec(),
        })
    }

    /// WS13.3: the URL the page is on now, or `None` when the driver cannot say.
    async fn current_url(&self, _session: &BrowserSession) -> QuarryResult<Option<String>> {
        Ok(None)
    }

    /// WS13.6: whether a full-page screenshot of this page is taller than one
    /// capture holds at its device scale, so it would need tile-and-stitch. Default:
    /// no. The runtime asks this before it takes a heavy-lane permit.
    async fn screenshot_needs_stitch(
        &self,
        _session: &BrowserSession,
        _options: &quarry_core::capture::ScreenshotOptions,
    ) -> QuarryResult<bool> {
        Ok(false)
    }

    /// WS13.6: a screenshot that may stitch tiles when `stitch` holds a heavy-lane
    /// permit. Without a permit, and on a driver that does not stitch, this is
    /// `capture_screenshot`: the image stops at the single-capture limit.
    async fn capture_screenshot_stitched(
        &self,
        session: &BrowserSession,
        options: &quarry_core::capture::ScreenshotOptions,
        limits: &quarry_core::capture::CaptureLimits,
        stitch: Option<&StitchPermit>,
    ) -> QuarryResult<quarry_core::capture::CapturedImage> {
        let _ = stitch;
        self.capture_screenshot(session, options, limits).await
    }

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

    /// Open a tab with its own options (WS8.E). `activate: false` leaves the
    /// session's active tab where it is; `egress_policy` governs this tab only.
    /// Default: Unsupported.
    async fn open_tab(
        &self,
        _session: &BrowserSession,
        _options: OpenTabOptions,
    ) -> QuarryResult<BrowserTab> {
        Err(QuarryError::unsupported_action("open_tab"))
    }

    /// Install the egress policy of one tab (WS8.E). A tab's policy takes
    /// precedence over the session policy for requests made from that tab.
    /// Default: Unsupported.
    async fn configure_tab_egress_policy(
        &self,
        _session: &BrowserSession,
        _tab_id: &str,
        _policy: BrowserEgressPolicy,
    ) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("configure_tab_egress_policy"))
    }

    /// Atomically replace a tab's override and return its previous value. Passing
    /// `None` removes the override and leaves the session boundary in effect.
    async fn swap_tab_egress_policy(
        &self,
        _session: &BrowserSession,
        _tab_id: &str,
        _policy: Option<BrowserEgressPolicy>,
    ) -> QuarryResult<Option<BrowserEgressPolicy>> {
        Err(QuarryError::unsupported_action("swap_tab_egress_policy"))
    }

    /// The session's active tab id, the tab an unpinned handle acts on
    /// (WS8.E). A pinned handle may name a different tab. Default: Unsupported.
    async fn active_tab_id(&self, _session: &BrowserSession) -> QuarryResult<Option<String>> {
        Err(QuarryError::unsupported_action("active_tab_id"))
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

    /// Reload the current page (WS9.B). `ignore_cache` bypasses the HTTP cache.
    /// Default: Unsupported.
    async fn reload(&self, _session: &BrowserSession, _ignore_cache: bool) -> QuarryResult<()> {
        Err(QuarryError::unsupported_action("reload"))
    }
}

#[derive(Debug, Clone)]
pub struct BrowserSession {
    pub lease: BrowserLease,
    pub inner: Arc<Mutex<SessionInner>>,
    /// The tab this handle addresses when it is pinned (WS8.E). `None` means
    /// the session's active tab.
    pub tab_pin: Option<Arc<str>>,
}

impl BrowserSession {
    /// A handle on the same session whose actions address `tab_id` and never
    /// another tab, even when `tab_id` is closed (no fallback to the active tab).
    pub fn pinned_to(&self, tab_id: &str) -> Self {
        Self {
            lease: self.lease.clone(),
            inner: self.inner.clone(),
            tab_pin: Some(Arc::from(tab_id)),
        }
    }

    /// The pinned tab id, if this handle is pinned.
    pub fn tab_pin(&self) -> Option<&str> {
        self.tab_pin.as_deref()
    }
}

#[derive(Debug, Default)]
pub struct SessionInner {
    pub connected: bool,
    pub pages_served: u64,
}

#[cfg(test)]
mod reputation_hook_tests {
    use super::*;

    struct Listed(&'static [&'static str]);

    impl HostReputation for Listed {
        fn standing(&self, host: &str) -> HostStanding {
            if self.0.contains(&host) {
                HostStanding::Listed
            } else {
                HostStanding::Unlisted
            }
        }

        fn source(&self) -> &'static str {
            "test_list"
        }
    }

    struct Busy;

    impl HostReputation for Busy {
        fn standing(&self, _host: &str) -> HostStanding {
            HostStanding::Unknown
        }
    }

    fn listed() -> Arc<dyn HostReputation> {
        Arc::new(Listed(&["evil.example"]))
    }

    fn open() -> BrowserEgressPolicy {
        BrowserEgressPolicy::from_allowed_domains(&[])
    }

    #[test]
    fn borrowed_tab_egress_limits_credentialed_requests_to_the_approved_origin() {
        let origin = url::Url::parse("https://example.com").unwrap().origin();
        let policy = BrowserEgressPolicy::from_allowed_domains(&["example.com".to_owned()])
            .with_public_assets()
            .with_only_origin(origin);

        assert!(policy.allows_url("https://example.com/page"));
        assert!(!policy.allows_url("http://example.com/page"));
        assert!(!policy.allows_url("https://example.com:8443/page"));
        assert!(!policy.allows_url("https://account.example.com/page"));
        assert!(policy.allows_public_asset("https://cdn.example.com/style.css"));
    }

    #[test]
    fn off_attaches_nothing_and_leaves_the_policy_unchanged() {
        let policy = open().with_reputation(listed(), ReputationMode::Off);
        assert_eq!(policy, open());
        assert_eq!(policy.reputation_mode(), ReputationMode::Off);
        assert_eq!(policy.reputation_outcome("https://evil.example/"), None);
        // An earlier hook is removed by an Off call.
        let cleared = open()
            .with_reputation(listed(), ReputationMode::Block)
            .with_reputation(listed(), ReputationMode::Off);
        assert_eq!(cleared, open());
    }

    #[test]
    fn warn_and_block_name_the_listed_host_with_the_source() {
        let warn = open().with_reputation(listed(), ReputationMode::Warn);
        assert_eq!(
            warn.reputation_outcome("https://evil.example/path?q=1"),
            Some(ReputationOutcome::Warn { source: "test_list" })
        );
        let block = open().with_reputation(listed(), ReputationMode::Block);
        assert_eq!(
            block.reputation_outcome("http://EVIL.example./"),
            Some(ReputationOutcome::Block { source: "test_list" })
        );
        assert_eq!(block.reputation_outcome("https://good.example/"), None);
        assert_eq!(block.reputation_mode(), ReputationMode::Block);
    }

    #[test]
    fn non_web_urls_and_unknown_standing_are_not_listed() {
        let block = open().with_reputation(listed(), ReputationMode::Block);
        for raw in [
            "data:text/plain,evil.example",
            "blob:https://evil.example/uuid",
            "ftp://evil.example/",
            "not a url",
        ] {
            assert_eq!(block.reputation_outcome(raw), None, "{raw}");
        }
        let busy = open().with_reputation(Arc::new(Busy), ReputationMode::Block);
        assert_eq!(busy.reputation_outcome("https://evil.example/"), None);
    }

    #[test]
    fn the_hook_never_changes_what_the_grant_allows() {
        let grant = BrowserEgressPolicy::from_allowed_domains(&["evil.example".to_owned()]);
        let hooked = grant.clone().with_reputation(listed(), ReputationMode::Block);
        for raw in ["https://evil.example/", "https://other.example/"] {
            assert_eq!(hooked.allows_url(raw), grant.allows_url(raw), "{raw}");
        }
    }

    #[test]
    fn hooks_are_equal_only_for_one_source_in_one_mode() {
        let source = listed();
        let a = open().with_reputation(source.clone(), ReputationMode::Warn);
        let b = open().with_reputation(source.clone(), ReputationMode::Warn);
        assert_eq!(a, b);
        assert_ne!(a, open().with_reputation(source, ReputationMode::Block));
        assert_ne!(a, open().with_reputation(listed(), ReputationMode::Warn));
        assert!(format!("{a:?}").contains("test_list"));
    }

    #[test]
    fn the_strictest_outcome_wins() {
        let warn = Some(ReputationOutcome::Warn { source: "a" });
        let block = Some(ReputationOutcome::Block { source: "b" });
        assert_eq!(ReputationOutcome::strictest(None, None), None);
        assert_eq!(ReputationOutcome::strictest(warn, None), warn);
        assert_eq!(ReputationOutcome::strictest(None, warn), warn);
        assert_eq!(ReputationOutcome::strictest(warn, block), block);
        assert_eq!(ReputationOutcome::strictest(block, warn), block);
    }
}

#[cfg(test)]
mod capture_default_tests {
    use crate::{BrowserDriver, BrowserSession, SessionInner};
    use async_trait::async_trait;
    use quarry_core::capture::{CaptureLimits, ImageFormat, PdfOptions, ScreenshotOptions};
    use quarry_core::error::{ErrorCode, QuarryResult};
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserLease, ProxyAffinity};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// A PNG header for a 3 by 2 picture. The planning code reads only the header.
    fn png_3x2() -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&3u32.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    /// Implements only the methods a driver must have. The capture methods are
    /// the trait defaults under test.
    struct DefaultsOnly;

    #[async_trait]
    impl BrowserDriver for DefaultsOnly {
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(Mutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::from(png_3x2()))
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::from_static(b"%PDF-1.7"))
        }
    }

    fn lease() -> BrowserLease {
        BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "capture-defaults".to_owned(),
            proxy_affinity: ProxyAffinity {
                pool: "default".into(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            persist_profile: false,
            viewport: None,
            org_id: "test_org".into(),
        }
    }

    /// WS13.3: a driver that does not override the capture methods serves the
    /// legacy options through `screenshot` and `pdf`, and refuses every other
    /// option with `unsupported`. Nothing is dropped silently.
    #[tokio::test]
    async fn default_capture_methods_delegate_for_legacy_options_and_refuse_others() {
        let driver = DefaultsOnly;
        let session = driver.acquire(&lease()).await.expect("acquire");
        let limits = CaptureLimits::default();

        let image = driver
            .capture_screenshot(&session, &ScreenshotOptions::default(), &limits)
            .await
            .expect("legacy screenshot");
        assert_eq!((image.width_px, image.height_px), (3, 2));
        assert_eq!(image.format, ImageFormat::Png);
        assert_eq!((image.tiles, image.stitched, image.truncated), (1, false, false));

        let jpeg = ScreenshotOptions {
            quality: Some(80),
            ..ScreenshotOptions::default()
        };
        let refused = driver
            .capture_screenshot(&session, &jpeg, &limits)
            .await
            .expect_err("jpeg is refused by the default method");
        assert!(matches!(refused.code, ErrorCode::Unsupported));

        let pdf = driver
            .capture_pdf(&session, &PdfOptions::default(), &limits)
            .await
            .expect("default pdf");
        assert!(pdf.bytes.starts_with(b"%PDF-"));
        let landscape = PdfOptions {
            landscape: true,
            ..PdfOptions::default()
        };
        let refused = driver
            .capture_pdf(&session, &landscape, &limits)
            .await
            .expect_err("landscape is refused by the default method");
        assert!(matches!(refused.code, ErrorCode::Unsupported));

        assert_eq!(driver.current_url(&session).await.expect("url"), None);
        driver.release(session).await.expect("release");
    }
}

#[cfg(test)]
mod capability_wire_tests {
    use super::*;

    #[test]
    fn capabilities_flag_defaults_false_and_is_additive_on_the_wire() {
        // Deny by default: a driver that does not say so has no trusted input.
        assert!(!BrowserDriverCapabilities::default().trusted_input);
        // A peer that predates every additive key sends none of them and still parses: each
        // additive key reads as false.
        let oldest = serde_json::json!({
            "persistent_profile": true,
            "devtools_trace": false,
            "downloads_to_artifacts": false,
            "uploads_from_artifacts": false,
            "full_visual_fidelity": true,
            "isolated_egress": true,
            "security_evidence": true,
            "atomic_target_actions": true
        });
        let parsed_oldest: BrowserDriverCapabilities =
            serde_json::from_value(oldest).expect("oldest shape parses");
        assert!(parsed_oldest.atomic_target_actions);
        for additive in [
            parsed_oldest.trusted_input,
            parsed_oldest.background_tabs,
            parsed_oldest.tab_pinning,
            parsed_oldest.per_tab_egress,
            parsed_oldest.scrape_actions,
            parsed_oldest.capture_options,
            parsed_oldest.page_emulation,
        ] {
            assert!(!additive);
        }
        // A peer that predates trusted_input but has the later additive keys (WS8.E tab keys,
        // WS13.3 capture keys, E4.4-a page emulation) sends them as false, and the new key is the
        // only difference.
        let older = serde_json::json!({
            "persistent_profile": true,
            "devtools_trace": false,
            "downloads_to_artifacts": false,
            "uploads_from_artifacts": false,
            "full_visual_fidelity": true,
            "isolated_egress": true,
            "security_evidence": true,
            "atomic_target_actions": true,
            "background_tabs": false,
            "tab_pinning": false,
            "per_tab_egress": false,
            "scrape_actions": false,
            "capture_options": false,
            "page_emulation": false
        });
        let parsed: BrowserDriverCapabilities =
            serde_json::from_value(older.clone()).expect("older shape parses");
        assert!(!parsed.trusted_input);
        assert!(parsed.atomic_target_actions);
        // The new key is the only difference on the way out, and it round-trips.
        let mut on = parsed;
        on.trusted_input = true;
        let wire = serde_json::to_value(on).expect("serializes");
        assert_eq!(wire["trusted_input"], true);
        let mut without = wire.clone();
        without.as_object_mut().unwrap().remove("trusted_input");
        assert_eq!(without, older);
        let back: BrowserDriverCapabilities = serde_json::from_value(wire).expect("round-trips");
        assert!(back.trusted_input);
    }
}
