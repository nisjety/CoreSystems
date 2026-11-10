//! Crawl bounds applied at the edge.
//!
//! A crawl with no page limit ran until the 30-minute workflow timeout, and a
//! request of `max_pages: 0` meant "unlimited". Every crawl handed to the
//! orchestrator now carries a finite page and depth bound: the request's value
//! when it is in range, the default when it is absent or zero, the hard ceiling
//! when it is above it. The edge returns what it resolved so a caller can see
//! when it was clamped, never silently.
//!
//! The same environment names and defaults are read by the orchestrator
//! (`services/quarry-orchestrator/internal/jobs/limits.go`), which re-resolves
//! the job parameters so a job row written elsewhere is bounded too.
//!
//! A `0` request is lenient here: it takes the default rather than being
//! refused. A strict `400 invalid_option` for it belongs to the request
//! validation work and is opt-in.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const ENV_DEFAULT_PAGES: &str = "QUARRY_CRAWL_DEFAULT_MAX_PAGES";
const ENV_HARD_PAGES: &str = "QUARRY_CRAWL_HARD_MAX_PAGES";
const ENV_DEFAULT_DEPTH: &str = "QUARRY_CRAWL_DEFAULT_MAX_DEPTH";
const ENV_HARD_DEPTH: &str = "QUARRY_CRAWL_HARD_MAX_DEPTH";

pub const DEFAULT_MAX_PAGES: u32 = 1000;
pub const HARD_MAX_PAGES: u32 = 5000;
pub const DEFAULT_MAX_DEPTH: u32 = 5;
pub const HARD_MAX_DEPTH: u32 = 10;

/// Operator-set bounds. The hard ceilings are never disabled; a zero default
/// means "no default", so an absent request takes the hard ceiling instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrawlLimits {
    pub default_pages: u32,
    pub hard_pages: u32,
    pub default_depth: u32,
    pub hard_depth: u32,
}

impl Default for CrawlLimits {
    fn default() -> Self {
        Self {
            default_pages: DEFAULT_MAX_PAGES,
            hard_pages: HARD_MAX_PAGES,
            default_depth: DEFAULT_MAX_DEPTH,
            hard_depth: HARD_MAX_DEPTH,
        }
    }
}

/// Why a resolved value is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClampedBy {
    /// The request was absent or zero and took the default.
    Default,
    /// The request was above the hard ceiling and was clamped to it.
    Hard,
    /// The organization's remaining allowance was smaller than the bound
    /// (WS6.D-s; only a configured allowance can do this).
    Entitlement,
}

/// One bound as the caller sees it: what was asked, what will run, and why they
/// differ. `clamped_by` is empty when the request was used unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bound {
    pub requested: Option<u32>,
    pub effective: u32,
    /// What the organization has left of its allowance for this bound, when one
    /// is configured (WS6.D-s). Absent otherwise: unconfigured is not zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<u64>,
    pub clamped_by: Vec<ClampedBy>,
}

/// The resolved bounds echoed in the handoff response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlLimitsAck {
    pub max_pages: Bound,
    pub max_depth: Bound,
}

impl CrawlLimits {
    /// Read the bounds from the environment. An unset variable keeps the
    /// built-in value; an unparsable value, a zero hard ceiling or a default
    /// above its ceiling is an error naming the variable.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut limits = Self::default();
        for (name, slot) in [
            (ENV_DEFAULT_PAGES, &mut limits.default_pages),
            (ENV_HARD_PAGES, &mut limits.hard_pages),
            (ENV_DEFAULT_DEPTH, &mut limits.default_depth),
            (ENV_HARD_DEPTH, &mut limits.hard_depth),
        ] {
            let Some(raw) = lookup(name) else { continue };
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            *slot = raw
                .parse::<u32>()
                .map_err(|e| format!("{name}={raw:?} is not a non-negative integer: {e}"))?;
        }
        if limits.hard_pages == 0 {
            return Err(format!(
                "{ENV_HARD_PAGES} must be greater than zero: the hard page ceiling cannot be disabled"
            ));
        }
        if limits.hard_depth == 0 {
            return Err(format!(
                "{ENV_HARD_DEPTH} must be greater than zero: the hard depth ceiling cannot be disabled"
            ));
        }
        if limits.default_pages > limits.hard_pages {
            return Err(format!(
                "{ENV_DEFAULT_PAGES} ({}) exceeds {ENV_HARD_PAGES} ({})",
                limits.default_pages, limits.hard_pages
            ));
        }
        if limits.default_depth > limits.hard_depth {
            return Err(format!(
                "{ENV_DEFAULT_DEPTH} ({}) exceeds {ENV_HARD_DEPTH} ({})",
                limits.default_depth, limits.hard_depth
            ));
        }
        Ok(limits)
    }

    /// Apply the bounds to one request. A request of zero counts as absent and
    /// takes the default (or the hard ceiling when the default is disabled); a
    /// request above the ceiling is clamped to it; anything else is used as
    /// asked. The result is never zero, so a crawl is never unbounded.
    pub fn resolve(&self, req_pages: Option<u32>, req_depth: Option<u32>) -> CrawlLimitsAck {
        CrawlLimitsAck {
            max_pages: resolve_bound(req_pages, self.default_pages, self.hard_pages),
            max_depth: resolve_bound(req_depth, self.default_depth, self.hard_depth),
        }
    }
}

fn resolve_bound(requested: Option<u32>, default: u32, hard: u32) -> Bound {
    let (effective, clamped_by) = match requested {
        None | Some(0) => (
            if default == 0 { hard } else { default },
            vec![ClampedBy::Default],
        ),
        Some(value) if value > hard => (hard, vec![ClampedBy::Hard]),
        Some(value) => (value, Vec::new()),
    };
    Bound {
        requested,
        effective,
        remaining: None,
        clamped_by,
    }
}

/// The process-wide bounds, read from the environment once. Boot validates the
/// environment first ([`CrawlLimits::from_env`] in `main`), so the fallback to
/// the built-in defaults only ever covers a value that was valid at start-up.
pub fn crawl_limits() -> &'static CrawlLimits {
    static LIMITS: OnceLock<CrawlLimits> = OnceLock::new();
    LIMITS.get_or_init(|| CrawlLimits::from_env().unwrap_or_default())
}

// ---------------------------------------------------------------------------
// WS6.D-s: the organization limits resolver (static source)
// ---------------------------------------------------------------------------
//
// `CrawlLimits` above is the process-wide floor and ceiling. This half answers a
// second question: what has the organization still got left, and how much
// capacity does its plan allow? The answer is a [`LimitsResolver`] held on
// `AppState` and read by the crawl and batch handoffs and by `start_run`.
//
// The `static` source (the default) knows nothing about an organization, so
// every allowance is "unconfigured" and every capacity cap is "use the floor":
// the edge behaves exactly as it did before this module existed. The
// `billing_core` source (W2-CP-L2: a per-org read of billing-core's Quarry
// limits snapshot with a cache and a stale window, [`BillingCoreBackend`]) is a
// [`LimitsBackend`] behind the same [`FailMode`] wrapper. It is chosen only by
// `QUARRY_EDGE__LIMITS_SOURCE=billing_core` and then needs
// `QUARRY_EDGE__BILLING_CORE_URL` and `QUARRY_EDGE__BILLING_CORE_TOKEN`.
//
// Three rules hold for every source:
//
// - **Unconfigured is not zero.** An organization with no allowance policy for a
//   metric is [`Allowance::Unconfigured`] and keeps the floor. Only a configured
//   allowance that has run out is `Remaining(0)` and refuses.
// - **An entitlement never raises a budget cap.** It may only lower one (the
//   remaining agent browser seconds become a runtime ceiling). It may raise a
//   capacity cap (concurrent runs, render permits, job slots) from the floor up
//   to the process-wide cap, never past it.
// - **A resolver that cannot answer degrades by the fail mode**, `floor` by
//   default: serve the floor and say so. `open` drops the entitlement ceiling;
//   `closed` refuses the request.

use std::sync::Arc;

use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::usage_metrics;

pub const ENV_LIMITS_SOURCE: &str = "QUARRY_EDGE__LIMITS_SOURCE";
pub const ENV_LIMITS_FAIL_MODE: &str = "QUARRY_EDGE__LIMITS_FAIL_MODE";

/// The `details.dimension` of a refusal made because the organization's
/// configured allowance is spent (a typed `BUDGET_EXHAUSTED`, 402).
pub const DIMENSION_ORG_ALLOWANCE: &str = "org_allowance";

/// Where the organization's limits come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsSource {
    /// The floor only. The default; nothing is read from another plane.
    Static,
    /// billing-core's Quarry limits snapshot (W2-CP-L2), cached, behind the
    /// fail mode. Naming it without its URL and token stops boot.
    BillingCore,
}

impl LimitsSource {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "static" => Ok(Self::Static),
            "billing_core" => Ok(Self::BillingCore),
            other => Err(format!(
                "{ENV_LIMITS_SOURCE}={other:?} is not one of static, billing_core"
            )),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::BillingCore => "billing_core",
        }
    }
}

/// What the resolver does when its source cannot answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailMode {
    /// Serve the floor (the default, pinned by production overlays).
    Floor,
    /// No entitlement ceiling: capacity may use the process-wide cap.
    Open,
    /// Refuse the request (`RUNTIME_NOT_READY`, 503).
    Closed,
}

impl FailMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "floor" => Ok(Self::Floor),
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            other => Err(format!(
                "{ENV_LIMITS_FAIL_MODE}={other:?} is not one of floor, open, closed"
            )),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// The resolver settings read at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitsConfig {
    pub source: LimitsSource,
    pub fail_mode: FailMode,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            source: LimitsSource::Static,
            fail_mode: FailMode::Floor,
        }
    }
}

impl LimitsConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let source = match lookup(ENV_LIMITS_SOURCE) {
            Some(raw) => LimitsSource::parse(&raw)?,
            None => LimitsSource::Static,
        };
        let fail_mode = match lookup(ENV_LIMITS_FAIL_MODE) {
            Some(raw) => FailMode::parse(&raw)?,
            None => FailMode::Floor,
        };
        Ok(Self { source, fail_mode })
    }

    /// The resolver this configuration names, reading the billing-core settings
    /// from the environment when the source is `billing_core`.
    pub fn build(&self) -> Result<Arc<dyn LimitsResolver>, String> {
        self.build_with(|name| std::env::var(name).ok())
    }

    /// [`Self::build`] with the settings read through `lookup`. `billing_core`
    /// without a URL or a token is an error at boot, rather than the floor served
    /// under a name that promises entitlements. The static source reads nothing.
    pub fn build_with(
        &self,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Arc<dyn LimitsResolver>, String> {
        match self.source {
            LimitsSource::Static => Ok(Arc::new(StaticFloor)),
            LimitsSource::BillingCore => {
                let settings = BillingCoreSettings::from_lookup(lookup)?;
                let backend = BillingCoreBackend::from_settings(&settings)?;
                Ok(Arc::new(WithFailMode::new(backend, self.fail_mode)))
            }
        }
    }
}

/// What an organization has left of one metered allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allowance {
    /// No allowance is configured for the metric: the floor applies. Never zero.
    Unconfigured,
    /// A configured allowance with this much left. `Remaining(0)` is spent.
    Remaining(u64),
}

impl Allowance {
    pub fn remaining(self) -> Option<u64> {
        match self {
            Self::Unconfigured => None,
            Self::Remaining(left) => Some(left),
        }
    }

    pub fn is_exhausted(self) -> bool {
        self == Self::Remaining(0)
    }
}

/// Where an [`OrgLimits`] answer came from, as a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsOrigin {
    /// The floor: nothing about the organization is known.
    Floor,
    /// The source failed under fail mode `open`: no entitlement ceiling.
    Open,
    /// Read from the organization's entitlements.
    Entitlement,
}

impl LimitsOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::Open => "open",
            Self::Entitlement => "entitlement",
        }
    }
}

/// Plan capacity values (the billing-core plan keys `quarry.cap.*`). `None` is
/// "the plan says nothing": the floor applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanCaps {
    pub agent_runs: Option<u32>,
    pub interactive_runs: Option<u32>,
    pub render_permits: Option<u32>,
    pub crawl_jobs: Option<u32>,
    pub job_backlog: Option<u32>,
    pub rate_multiplier: Option<u32>,
}

/// What the edge knows about one organization's limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgLimits {
    pub origin: LimitsOrigin,
    /// `quarry.crawl.page`.
    pub crawl_pages: Allowance,
    /// `quarry.batch.url`.
    pub batch_urls: Allowance,
    /// `quarry.browser.second.agent`.
    pub browser_seconds_agent: Allowance,
    pub caps: PlanCaps,
}

impl OrgLimits {
    /// The floor: every allowance unconfigured, every capacity cap the floor's.
    pub fn floor() -> Self {
        Self {
            origin: LimitsOrigin::Floor,
            crawl_pages: Allowance::Unconfigured,
            batch_urls: Allowance::Unconfigured,
            browser_seconds_agent: Allowance::Unconfigured,
            caps: PlanCaps::default(),
        }
    }

    /// The answer under fail mode `open`: the same unconfigured allowances, but
    /// capacity may use the process-wide cap (see [`effective_capacity`]).
    pub fn open() -> Self {
        Self {
            origin: LimitsOrigin::Open,
            ..Self::floor()
        }
    }
}

/// Plan bounds on how many runs one organization may have live, per class
/// (WS6.G): the billing-core plan keys `quarry.cap.agent_runs` and
/// `quarry.cap.interactive_runs`. `None` for a class means no plan bound: the
/// process-wide caps of `RunLimits` apply as they always did.
///
/// A class with a plan bound answers to that bound alone (it replaces the
/// static per-organization caps for that class, so a plan can raise an
/// organization above the floor), and never to more than the process-wide cap
/// (`bound_for`). A bound of `0` is an explicit "no runs of this class" and
/// refuses every start; a plan that says nothing is `None`, not `0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunCapOverride {
    pub agent_runs: Option<u32>,
    pub interactive_runs: Option<u32>,
}

impl RunCapOverride {
    /// No plan bound for either class.
    pub const NONE: Self = Self {
        agent_runs: None,
        interactive_runs: None,
    };

    /// The plan value for one class.
    pub fn for_class(self, class: crate::budget::RunClass) -> Option<u32> {
        match class {
            crate::budget::RunClass::Agent => self.agent_runs,
            crate::budget::RunClass::Interactive => self.interactive_runs,
        }
    }

    /// The bound a start of `class` answers to: the plan value, capped by the
    /// process-wide cap (`global_cap`, `0` = none). `None` when the plan sets
    /// no bound for the class.
    pub fn bound_for(self, class: crate::budget::RunClass, global_cap: u32) -> Option<u32> {
        self.for_class(class).map(|plan| {
            if global_cap == 0 {
                plan
            } else {
                plan.min(global_cap)
            }
        })
    }
}

impl OrgLimits {
    /// The run bounds this answer sets (see [`RunCapOverride`]). The floor sets
    /// none. Under fail mode `open` the organization's cap is the process-wide
    /// cap (the same rule as [`effective_capacity`]), so both classes are bounded
    /// only by the global cap. An entitlement carries the plan's two run keys.
    pub fn run_override(&self) -> RunCapOverride {
        match self.origin {
            LimitsOrigin::Floor => RunCapOverride::NONE,
            LimitsOrigin::Open => RunCapOverride {
                agent_runs: Some(u32::MAX),
                interactive_runs: Some(u32::MAX),
            },
            LimitsOrigin::Entitlement => RunCapOverride {
                agent_runs: self.caps.agent_runs,
                interactive_runs: self.caps.interactive_runs,
            },
        }
    }
}

/// A capacity value for one organization: the plan's value capped by the
/// process-wide cap; the floor when the plan says nothing; the process-wide cap
/// itself when the answer is `open`. Never above `global_cap`.
pub fn effective_capacity(
    floor: u32,
    plan: Option<u32>,
    global_cap: u32,
    origin: LimitsOrigin,
) -> u32 {
    match (origin, plan) {
        (LimitsOrigin::Open, _) => global_cap,
        (_, Some(plan)) => plan.min(global_cap),
        (_, None) => floor.min(global_cap),
    }
}

/// Lower a budget ceiling to what the organization has left. It can only lower:
/// an unconfigured allowance, or one larger than the ceiling, changes nothing.
/// The second value is true when the allowance set the result.
pub fn tighten_ceiling(ceiling: u64, remaining: Allowance) -> (u64, bool) {
    match remaining {
        Allowance::Remaining(left) if left < ceiling => (left, true),
        _ => (ceiling, false),
    }
}

/// The organization's limits, by organization id. An `Err` is a refusal the
/// caller returns as is (only fail mode `closed` produces one).
#[async_trait]
pub trait LimitsResolver: Send + Sync {
    async fn org_limits(&self, org_id: &str) -> Result<OrgLimits, QuarryError>;
}

/// The static source: no organization has an allowance, so the floor stands.
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticFloor;

#[async_trait]
impl LimitsResolver for StaticFloor {
    async fn org_limits(&self, _org_id: &str) -> Result<OrgLimits, QuarryError> {
        Ok(OrgLimits::floor())
    }
}

/// Why a [`LimitsBackend`] could not answer: a closed class, never a response
/// body or a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendError(pub &'static str);

/// A source that can fail (the billing-core read). Wrapped by [`WithFailMode`].
#[async_trait]
pub trait LimitsBackend: Send + Sync {
    async fn read(&self, org_id: &str) -> Result<OrgLimits, BackendError>;
}

/// Applies the [`FailMode`] to a fallible backend.
pub struct WithFailMode<B> {
    backend: B,
    mode: FailMode,
}

impl<B> WithFailMode<B> {
    pub fn new(backend: B, mode: FailMode) -> Self {
        Self { backend, mode }
    }
}

#[async_trait]
impl<B: LimitsBackend> LimitsResolver for WithFailMode<B> {
    async fn org_limits(&self, org_id: &str) -> Result<OrgLimits, QuarryError> {
        match self.backend.read(org_id).await {
            Ok(limits) => Ok(limits),
            Err(BackendError(class)) => match self.mode {
                FailMode::Floor => {
                    tracing::warn!(class, "limits: source unreachable; serving floor");
                    Ok(OrgLimits::floor())
                }
                FailMode::Open => {
                    tracing::warn!(class, "limits: source unreachable; serving no entitlement ceiling");
                    Ok(OrgLimits::open())
                }
                FailMode::Closed => Err(QuarryError::new(
                    ErrorCode::RuntimeNotReady,
                    "organization limits are unreadable",
                )
                .with_details(serde_json::json!({
                    "reason": "limits_unreadable",
                    "retry_after_s": 5,
                }))),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// W2-CP-L2: the billing_core source (U43, Rust half)
// ---------------------------------------------------------------------------
//
// One read per organization of billing-core's Quarry limits snapshot
// (`GET {QUARRY_EDGE__BILLING_CORE_URL}/api/v1/billing/orgs/{org}/quarry/limits`,
// the W2-CP-L1 contract), held in an in-process cache:
//
// - **Fresh** (younger than `QUARRY_EDGE__LIMITS_CACHE_TTL_S`, 30 s): served
//   without a read.
// - **Stale window** (`QUARRY_EDGE__LIMITS_STALE_WINDOW_S`, 300 s past the TTL):
//   a read is tried; when billing-core cannot answer, the cached answer is
//   served and the clamps it carries still apply.
// - **Past the window, or never read**: the read error goes to the
//   [`FailMode`] wrapper ([`WithFailMode`]), `floor` by default.
//
// The edge states the tenant twice (`X-Quarry-Org-Id` and the path) and refuses
// an answer whose `org_id` is not the one it asked for. Response bodies, URLs
// and the token are never logged; an error is a closed class
// ([`BackendError`]).

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const ENV_BILLING_CORE_URL: &str = "QUARRY_EDGE__BILLING_CORE_URL";
pub const ENV_BILLING_CORE_TOKEN: &str = "QUARRY_EDGE__BILLING_CORE_TOKEN";
pub const ENV_LIMITS_CACHE_TTL_S: &str = "QUARRY_EDGE__LIMITS_CACHE_TTL_S";
pub const ENV_LIMITS_STALE_WINDOW_S: &str = "QUARRY_EDGE__LIMITS_STALE_WINDOW_S";
pub const ENV_LIMITS_TIMEOUT_MS: &str = "QUARRY_EDGE__LIMITS_TIMEOUT_MS";

/// The principal billing-core pins its Quarry limits route to.
pub const BILLING_CORE_SERVICE_ID: &str = "quarry-edge";
/// The org-binding header billing-core checks against the path (W2-CP-L1).
pub const HEADER_QUARRY_ORG_ID: &str = "x-quarry-org-id";

pub const DEFAULT_LIMITS_CACHE_TTL_S: u64 = 30;
pub const DEFAULT_LIMITS_STALE_WINDOW_S: u64 = 300;
pub const DEFAULT_LIMITS_TIMEOUT_MS: u64 = 2000;

/// The largest limits answer read; anything larger is refused unread.
const MAX_LIMITS_BODY_BYTES: usize = 64 * 1024;
/// Past this many cached organizations, entries older than the stale window
/// are dropped on the next write.
const LIMITS_CACHE_SOFT_LIMIT: usize = 4096;

/// How the billing_core source reaches billing-core. Read only when
/// `QUARRY_EDGE__LIMITS_SOURCE=billing_core`.
#[derive(Clone, PartialEq, Eq)]
pub struct BillingCoreSettings {
    pub base_url: String,
    pub token: String,
    pub cache_ttl: Duration,
    pub stale_window: Duration,
    pub timeout: Duration,
}

impl fmt::Debug for BillingCoreSettings {
    /// The token is never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BillingCoreSettings")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .field("cache_ttl", &self.cache_ttl)
            .field("stale_window", &self.stale_window)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl BillingCoreSettings {
    /// Read the settings through `lookup`. The URL and the token are required;
    /// an error names the variable, never its value.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let required = |name: &str| -> Result<String, String> {
            match lookup(name).map(|value| value.trim().to_owned()) {
                Some(value) if !value.is_empty() => Ok(value),
                _ => Err(format!("{ENV_LIMITS_SOURCE}=billing_core needs {name}")),
            }
        };
        let number = |name: &str, default: u64, min: u64| -> Result<u64, String> {
            match lookup(name).map(|value| value.trim().to_owned()).as_deref() {
                None | Some("") => Ok(default),
                Some(raw) => {
                    let value: u64 = raw
                        .parse()
                        .map_err(|_| format!("{name} must be a whole number, got {raw:?}"))?;
                    if value < min {
                        return Err(format!("{name} must be at least {min}, got {value}"));
                    }
                    Ok(value)
                }
            }
        };
        let base_url = required(ENV_BILLING_CORE_URL)?;
        match reqwest::Url::parse(&base_url) {
            Ok(url) if matches!(url.scheme(), "http" | "https") && !url.cannot_be_a_base() => {}
            _ => return Err(format!("{ENV_BILLING_CORE_URL} must be an http or https base URL")),
        }
        let token = required(ENV_BILLING_CORE_TOKEN)?;
        Ok(Self {
            base_url,
            token,
            cache_ttl: Duration::from_secs(number(
                ENV_LIMITS_CACHE_TTL_S,
                DEFAULT_LIMITS_CACHE_TTL_S,
                1,
            )?),
            stale_window: Duration::from_secs(number(
                ENV_LIMITS_STALE_WINDOW_S,
                DEFAULT_LIMITS_STALE_WINDOW_S,
                0,
            )?),
            timeout: Duration::from_millis(number(
                ENV_LIMITS_TIMEOUT_MS,
                DEFAULT_LIMITS_TIMEOUT_MS,
                100,
            )?),
        })
    }
}

/// One metric of billing-core's answer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AllowanceDoc {
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub unlimited: bool,
    #[serde(default)]
    pub remaining: Option<i64>,
}

/// The plan capacity values of billing-core's answer; `null` is "the plan says
/// nothing". Read as signed so a negative value is `None`, not an error.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CapsDoc {
    #[serde(default)]
    pub agent_runs: Option<i64>,
    #[serde(default)]
    pub interactive_runs: Option<i64>,
    #[serde(default)]
    pub render_permits: Option<i64>,
    #[serde(default)]
    pub crawl_jobs: Option<i64>,
    #[serde(default)]
    pub job_backlog: Option<i64>,
    #[serde(default)]
    pub rate_multiplier: Option<i64>,
}

/// billing-core's Quarry limits snapshot (the `data` of the W2-CP-L1 answer).
/// Fields the edge does not use (`plan`, `account`, `as_of`, ...) are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsDoc {
    pub org_id: String,
    #[serde(default)]
    pub allowances: HashMap<String, AllowanceDoc>,
    #[serde(default)]
    pub caps: CapsDoc,
}

#[derive(Deserialize)]
struct LimitsEnvelope {
    data: LimitsDoc,
}

fn cap_value(value: Option<i64>) -> Option<u32> {
    value
        .filter(|v| *v >= 0)
        .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
}

impl LimitsDoc {
    fn allowance(&self, metric: &str) -> Result<Allowance, BackendError> {
        let Some(doc) = self.allowances.get(metric) else {
            return Ok(Allowance::Unconfigured);
        };
        if !doc.configured || doc.unlimited {
            return Ok(Allowance::Unconfigured);
        }
        match doc.remaining {
            Some(left) => Ok(Allowance::Remaining(u64::try_from(left).unwrap_or(0))),
            // Configured and limited but no remainder: malformed, not zero.
            None => Err(BackendError("malformed")),
        }
    }

    /// The edge's view of the answer for `expected_org`. An answer about any
    /// other organization is never served.
    pub fn into_org_limits(self, expected_org: &str) -> Result<OrgLimits, BackendError> {
        if self.org_id != expected_org {
            return Err(BackendError("org_mismatch"));
        }
        Ok(OrgLimits {
            origin: LimitsOrigin::Entitlement,
            crawl_pages: self.allowance(usage_metrics::CRAWL_PAGE)?,
            batch_urls: self.allowance(usage_metrics::BATCH_URL)?,
            browser_seconds_agent: self.allowance(usage_metrics::BROWSER_SECOND_AGENT)?,
            caps: PlanCaps {
                agent_runs: cap_value(self.caps.agent_runs),
                interactive_runs: cap_value(self.caps.interactive_runs),
                render_permits: cap_value(self.caps.render_permits),
                crawl_jobs: cap_value(self.caps.crawl_jobs),
                job_backlog: cap_value(self.caps.job_backlog),
                rate_multiplier: cap_value(self.caps.rate_multiplier),
            },
        })
    }
}

/// An organization id the edge will put in a billing-core path: 1 to 128
/// characters of `[A-Za-z0-9_-]`. Anything else is refused before any request.
fn valid_org_id(org_id: &str) -> bool {
    !org_id.is_empty()
        && org_id.len() <= 128
        && org_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One read of one organization's limits snapshot.
#[async_trait]
pub trait LimitsFetch: Send + Sync {
    async fn fetch(&self, org_id: &str) -> Result<LimitsDoc, BackendError>;
}

/// The HTTP read of billing-core's Quarry limits route.
pub struct HttpLimitsFetch {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl HttpLimitsFetch {
    pub fn new(settings: &BillingCoreSettings) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(settings.timeout)
            .connect_timeout(settings.timeout)
            .build()
            .map_err(|e| format!("limits: http client: {}", e.without_url()))?;
        Ok(Self {
            client,
            base_url: settings.base_url.clone(),
            token: settings.token.clone(),
        })
    }

    fn url_for(&self, org_id: &str) -> Result<reqwest::Url, BackendError> {
        let mut url = reqwest::Url::parse(&self.base_url).map_err(|_| BackendError("config"))?;
        url.path_segments_mut()
            .map_err(|_| BackendError("config"))?
            .pop_if_empty()
            .extend(["api", "v1", "billing", "orgs", org_id, "quarry", "limits"]);
        Ok(url)
    }
}

#[async_trait]
impl LimitsFetch for HttpLimitsFetch {
    async fn fetch(&self, org_id: &str) -> Result<LimitsDoc, BackendError> {
        if !valid_org_id(org_id) {
            return Err(BackendError("invalid_org"));
        }
        let url = self.url_for(org_id)?;
        let mut response = self
            .client
            .get(url)
            .header("x-service-id", BILLING_CORE_SERVICE_ID)
            .header("x-service-token", &self.token)
            .header(HEADER_QUARRY_ORG_ID, org_id)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| BackendError("unreachable"))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(BackendError(match status {
                401 => "unauthorized",
                // Includes 403 organization_inactive: the fail mode decides
                // (the floor by default), the body is not read.
                403 => "forbidden",
                404 => "not_found",
                500..=599 => "unavailable",
                _ => "rejected",
            }));
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_LIMITS_BODY_BYTES as u64)
        {
            return Err(BackendError("oversize"));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| BackendError("unreachable"))?
        {
            if body.len() + chunk.len() > MAX_LIMITS_BODY_BYTES {
                return Err(BackendError("oversize"));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<LimitsEnvelope>(&body)
            .map(|envelope| envelope.data)
            .map_err(|_| BackendError("malformed"))
    }
}

/// The time source of the cache (a manual clock in tests).
pub type LimitsClock = Arc<dyn Fn() -> Instant + Send + Sync>;

#[derive(Clone)]
struct CachedLimits {
    limits: OrgLimits,
    fetched_at: Instant,
}

/// The billing_core [`LimitsBackend`]: a [`LimitsFetch`] behind the fresh
/// cache and the stale window. Wrap it in [`WithFailMode`].
pub struct BillingCoreBackend<F = HttpLimitsFetch> {
    fetch: F,
    cache_ttl: Duration,
    stale_window: Duration,
    cache: Mutex<HashMap<String, CachedLimits>>,
    clock: LimitsClock,
}

impl BillingCoreBackend<HttpLimitsFetch> {
    /// The HTTP backend the settings name.
    pub fn from_settings(settings: &BillingCoreSettings) -> Result<Self, String> {
        Ok(Self::with_fetch(
            HttpLimitsFetch::new(settings)?,
            settings.cache_ttl,
            settings.stale_window,
        ))
    }
}

impl<F: LimitsFetch> BillingCoreBackend<F> {
    pub fn with_fetch(fetch: F, cache_ttl: Duration, stale_window: Duration) -> Self {
        Self {
            fetch,
            cache_ttl,
            stale_window,
            cache: Mutex::new(HashMap::new()),
            clock: Arc::new(Instant::now),
        }
    }

    pub fn with_clock(mut self, clock: LimitsClock) -> Self {
        self.clock = clock;
        self
    }

    fn cached(&self, org_id: &str) -> Option<CachedLimits> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(org_id)
            .cloned()
    }

    fn remember(&self, org_id: &str, limits: OrgLimits, now: Instant) {
        let keep = self.cache_ttl + self.stale_window;
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cache.len() >= LIMITS_CACHE_SOFT_LIMIT {
            cache.retain(|_, entry| now.saturating_duration_since(entry.fetched_at) < keep);
        }
        cache.insert(
            org_id.to_owned(),
            CachedLimits {
                limits,
                fetched_at: now,
            },
        );
    }
}

#[async_trait]
impl<F: LimitsFetch> LimitsBackend for BillingCoreBackend<F> {
    async fn read(&self, org_id: &str) -> Result<OrgLimits, BackendError> {
        let now = (self.clock)();
        let cached = self.cached(org_id);
        if let Some(entry) = &cached {
            if now.saturating_duration_since(entry.fetched_at) < self.cache_ttl {
                return Ok(entry.limits.clone());
            }
        }
        let answer = match self.fetch.fetch(org_id).await {
            Ok(doc) => doc.into_org_limits(org_id),
            Err(error) => Err(error),
        };
        match answer {
            Ok(limits) => {
                self.remember(org_id, limits.clone(), now);
                Ok(limits)
            }
            Err(BackendError(class)) => {
                if let Some(entry) = cached {
                    let age = now.saturating_duration_since(entry.fetched_at);
                    if age < self.cache_ttl + self.stale_window {
                        tracing::warn!(
                            class,
                            age_s = age.as_secs(),
                            "limits: billing-core unreadable; serving the cached answer inside the stale window"
                        );
                        return Ok(entry.limits);
                    }
                }
                Err(BackendError(class))
            }
        }
    }
}

/// W2-CP-L2 (U59): one organization's share of a browser pool.
///
/// `pool_org_max` is `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` and stays the switch:
/// `0` (the default) is "no per-organization cap" and the plan is not read, so
/// nothing changes. With a cap set, it is the floor; the plan's
/// `render_permits` replaces it (raising it up to the pool size, never past),
/// fail-open uses the pool size. A plan value of `0` is read as `1`: the pool
/// reads `0` as "no cap", so a stamped zero would mean the opposite of what it
/// says. The result is at least 1.
pub fn org_render_permits(pool_org_max: usize, pool_size: usize, limits: &OrgLimits) -> usize {
    if pool_org_max == 0 {
        return 0;
    }
    let floor = u32::try_from(pool_org_max).unwrap_or(u32::MAX);
    let global = u32::try_from(pool_size).unwrap_or(u32::MAX);
    let plan = limits.caps.render_permits.map(|value| value.max(1));
    effective_capacity(floor, plan, global, limits.origin).max(1) as usize
}

/// The organization's allowance for one metric is spent. Becomes a 402
/// `BUDGET_EXHAUSTED` with `details.dimension = org_allowance`. It carries the
/// metric name only: no counts, no organization id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowanceExhausted {
    pub metric: &'static str,
}

impl AllowanceExhausted {
    pub fn into_error(self) -> QuarryError {
        QuarryError::new(
            ErrorCode::BudgetExhausted,
            "the organization's allowance for this operation is used up",
        )
        .with_details(serde_json::json!({
            "dimension": DIMENSION_ORG_ALLOWANCE,
            "metric": self.metric,
            "limit_source": "entitlement",
            "resumable": false,
        }))
    }
}

impl CrawlLimitsAck {
    /// Clamp the page bound to the organization's remaining `quarry.crawl.page`.
    /// `Unconfigured` leaves it as it is. A spent allowance is an error, so no
    /// job is created. A smaller remainder lowers `effective` and adds
    /// `entitlement` to `clamped_by`; the remainder is reported either way.
    pub fn clamp_pages_to(mut self, remaining: Allowance) -> Result<Self, AllowanceExhausted> {
        let Some(left) = remaining.remaining() else {
            return Ok(self);
        };
        if left == 0 {
            return Err(AllowanceExhausted {
                metric: usage_metrics::CRAWL_PAGE,
            });
        }
        self.max_pages.remaining = Some(left);
        if u64::from(self.max_pages.effective) > left {
            // `left` is below a u32 here.
            self.max_pages.effective = left as u32;
            self.max_pages.clamped_by.push(ClampedBy::Entitlement);
        }
        Ok(self)
    }
}

/// The batch bound as the caller sees it: how many URLs were asked for, how
/// many fit, and how many were dropped. Counts only, never a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlsBound {
    pub requested: usize,
    pub accepted: usize,
    pub dropped: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<u64>,
    pub clamped_by: Vec<ClampedBy>,
}

/// How many of `requested` URLs fit the organization's remaining
/// `quarry.batch.url`. The caller keeps the first `accepted` URLs. A spent
/// allowance is an error before any job exists.
pub fn clamp_batch_urls(
    requested: usize,
    remaining: Allowance,
) -> Result<UrlsBound, AllowanceExhausted> {
    let Some(left) = remaining.remaining() else {
        return Ok(UrlsBound {
            requested,
            accepted: requested,
            dropped: 0,
            remaining: None,
            clamped_by: Vec::new(),
        });
    };
    if left == 0 {
        return Err(AllowanceExhausted {
            metric: usage_metrics::BATCH_URL,
        });
    }
    let accepted = if (requested as u64) > left {
        left as usize
    } else {
        requested
    };
    Ok(UrlsBound {
        requested,
        accepted,
        dropped: requested - accepted,
        remaining: Some(left),
        clamped_by: if accepted < requested {
            vec![ClampedBy::Entitlement]
        } else {
            Vec::new()
        },
    })
}

// ---------------------------------------------------------------------------
// W3-E3.7.2w: the parallel pages of a batch
// ---------------------------------------------------------------------------
//
// A batch runs its URLs one page at a time unless the orchestrator is told how many to run at once.
// With `QUARRY_EDGE__BATCH_PARALLEL` on, the edge states that number on the job as
// `params.max_parallel` (`routes::batch_handoff`): what the request asked for, clamped to the edge
// ceiling below, and reported back as `limits.max_parallel` so a clamp is never silent. The
// orchestrator caps it again by its own flag (`QUARRY_BATCH_PARALLEL`, hard maximum 16).

/// The environment name of the edge's ceiling on a batch's parallel pages.
pub const ENV_BATCH_MAX_PARALLEL: &str = "QUARRY_EDGE__BATCH_MAX_PARALLEL";
/// The ceiling when the variable is absent or blank.
pub const DEFAULT_BATCH_MAX_PARALLEL: u32 = 4;
/// The largest value the ceiling may take.
pub const HARD_BATCH_MAX_PARALLEL: u32 = 16;
/// The parallel pages a batch asks for when its request does not say, before the ceiling.
pub const DEFAULT_BATCH_PARALLEL: u32 = 4;

/// Parse the ceiling. Absent or blank is the default; anything that is not a whole number from 1 to
/// [`HARD_BATCH_MAX_PARALLEL`] is an error that names the variable and never echoes the value.
pub fn parse_batch_max_parallel(raw: Option<&str>) -> Result<u32, String> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(DEFAULT_BATCH_MAX_PARALLEL);
    };
    match value.parse::<u32>() {
        Ok(n) if (1..=HARD_BATCH_MAX_PARALLEL).contains(&n) => Ok(n),
        _ => Err(format!(
            "{ENV_BATCH_MAX_PARALLEL} must be a whole number from 1 to {HARD_BATCH_MAX_PARALLEL}"
        )),
    }
}

/// Validate the ceiling from the process environment (used by boot).
pub fn check_batch_max_parallel_boot() -> Result<u32, String> {
    parse_batch_max_parallel(std::env::var(ENV_BATCH_MAX_PARALLEL).ok().as_deref())
}

/// The ceiling the process runs with, read once. Boot validates the environment first, so a value
/// that reaches here was valid at start-up.
pub fn batch_max_parallel() -> u32 {
    static CEILING: OnceLock<u32> = OnceLock::new();
    *CEILING.get_or_init(|| check_batch_max_parallel_boot().unwrap_or(DEFAULT_BATCH_MAX_PARALLEL))
}

/// The parallel pages one batch runs with. `requested` is what the request sent, `None` when it
/// said nothing. Silent takes [`DEFAULT_BATCH_PARALLEL`] clamped to the `ceiling` (`default`); a
/// value above the ceiling is clamped to it (`hard`); anything else is used as asked. The result is
/// at least 1, because a stamped `0` would read as "no organization cap" to the orchestrator.
/// `remaining` is never set: no allowance counts pages in flight.
///
/// There is no organization term. [`OrgLimits`] has no figure that bounds one batch's fan-out:
/// `caps.crawl_jobs` bounds the organization's jobs in flight, `caps.job_backlog` its waiting jobs,
/// and `caps.render_permits` its browser permits. A plan key for batch pages would add that term.
pub fn resolve_batch_parallel(requested: Option<u32>, ceiling: u32) -> Bound {
    let ceiling = ceiling.max(1);
    let (wanted, mut clamped_by) = match requested {
        None => (DEFAULT_BATCH_PARALLEL, vec![ClampedBy::Default]),
        Some(value) => (value.max(1), Vec::new()),
    };
    let effective = if wanted > ceiling {
        clamped_by.push(ClampedBy::Hard);
        ceiling
    } else {
        wanted
    };
    Bound {
        requested,
        effective,
        remaining: None,
        clamped_by,
    }
}

/// The `limits` block of a handoff acknowledgement. A crawl carries `max_pages` and `max_depth`; a
/// batch carries `urls`, and with `QUARRY_EDGE__BATCH_PARALLEL` on also `max_parallel`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<Bound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<Bound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urls: Option<UrlsBound>,
    /// W3-E3.7.2w: the pages of the batch that run at a time. Absent unless the switch is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<Bound>,
}

impl From<CrawlLimitsAck> for HandoffLimits {
    fn from(ack: CrawlLimitsAck) -> Self {
        Self {
            max_pages: Some(ack.max_pages),
            max_depth: Some(ack.max_depth),
            urls: None,
            max_parallel: None,
        }
    }
}

impl From<UrlsBound> for HandoffLimits {
    fn from(urls: UrlsBound) -> Self {
        Self {
            max_pages: None,
            max_depth: None,
            urls: Some(urls),
            max_parallel: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn zero_is_absent_and_takes_default() {
        let ack = CrawlLimits::default().resolve(Some(0), None);
        assert_eq!(ack.max_pages.effective, 1000);
        assert_eq!(ack.max_pages.clamped_by, vec![ClampedBy::Default]);
        assert_eq!(ack.max_pages.requested, Some(0));
        let ack = CrawlLimits::default().resolve(None, None);
        assert_eq!(ack.max_pages.effective, 1000);
        assert_eq!(ack.max_pages.requested, None);
    }

    #[test]
    fn in_range_request_is_used_as_asked() {
        let ack = CrawlLimits::default().resolve(Some(42), Some(3));
        assert_eq!(ack.max_pages.effective, 42);
        assert!(ack.max_pages.clamped_by.is_empty());
        assert_eq!(ack.max_depth.effective, 3);
        // The ceiling itself is in range.
        let ack = CrawlLimits::default().resolve(Some(5000), Some(10));
        assert_eq!(ack.max_pages.effective, 5000);
        assert!(ack.max_pages.clamped_by.is_empty());
    }

    #[test]
    fn above_hard_is_clamped_and_reported() {
        let ack = CrawlLimits::default().resolve(Some(1_000_000), Some(99));
        assert_eq!(ack.max_pages.effective, 5000);
        assert_eq!(ack.max_pages.clamped_by, vec![ClampedBy::Hard]);
        assert_eq!(ack.max_pages.requested, Some(1_000_000));
        assert_eq!(ack.max_depth.effective, 10);
        assert_eq!(ack.max_depth.clamped_by, vec![ClampedBy::Hard]);
    }

    #[test]
    fn depth_resolves_like_pages() {
        let ack = CrawlLimits::default().resolve(None, Some(0));
        assert_eq!(ack.max_depth.effective, 5);
        assert_eq!(ack.max_depth.clamped_by, vec![ClampedBy::Default]);
    }

    #[test]
    fn a_disabled_default_takes_the_hard_ceiling_never_unbounded() {
        let limits = CrawlLimits {
            default_pages: 0,
            ..CrawlLimits::default()
        };
        assert_eq!(limits.resolve(None, None).max_pages.effective, 5000);
        assert_eq!(limits.resolve(Some(9999), None).max_pages.effective, 5000);
    }

    #[test]
    fn from_env_overrides_and_keeps_defaults_for_unset() {
        let limits =
            CrawlLimits::from_lookup(env(&[(ENV_DEFAULT_PAGES, "7"), (ENV_HARD_PAGES, " 20 ")]))
                .unwrap();
        assert_eq!(limits.default_pages, 7);
        assert_eq!(limits.hard_pages, 20);
        assert_eq!(limits.default_depth, DEFAULT_MAX_DEPTH);
        assert_eq!(
            CrawlLimits::from_lookup(env(&[])).unwrap(),
            CrawlLimits::default()
        );
    }

    #[test]
    fn from_env_rejects_unparsable_naming_the_variable() {
        for (pairs, name) in [
            (&[(ENV_HARD_PAGES, "lots")][..], ENV_HARD_PAGES),
            (&[(ENV_DEFAULT_DEPTH, "-1")][..], ENV_DEFAULT_DEPTH),
            (&[(ENV_HARD_PAGES, "0")][..], ENV_HARD_PAGES),
            (&[(ENV_HARD_DEPTH, "0")][..], ENV_HARD_DEPTH),
            (&[(ENV_DEFAULT_PAGES, "9000")][..], ENV_DEFAULT_PAGES),
            (&[(ENV_DEFAULT_DEPTH, "11")][..], ENV_DEFAULT_DEPTH),
        ] {
            let pairs: &'static [(&str, &str)] = Box::leak(pairs.to_vec().into_boxed_slice());
            let error = CrawlLimits::from_lookup(env(pairs)).unwrap_err();
            assert!(error.contains(name), "{error} must name {name}");
        }
    }
}

#[cfg(test)]
mod resolver_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    /// A backend that answers with a fixed result and counts its calls.
    struct Scripted {
        answer: Result<OrgLimits, BackendError>,
        calls: AtomicUsize,
    }

    impl Scripted {
        fn down() -> Self {
            Self {
                answer: Err(BackendError("unreachable")),
                calls: AtomicUsize::new(0),
            }
        }

        fn up(limits: OrgLimits) -> Self {
            Self {
                answer: Ok(limits),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl LimitsBackend for Scripted {
        async fn read(&self, _org_id: &str) -> Result<OrgLimits, BackendError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answer.clone()
        }
    }

    fn entitled(pages: u64, urls: u64, seconds: u64) -> OrgLimits {
        OrgLimits {
            origin: LimitsOrigin::Entitlement,
            crawl_pages: Allowance::Remaining(pages),
            batch_urls: Allowance::Remaining(urls),
            browser_seconds_agent: Allowance::Remaining(seconds),
            caps: PlanCaps::default(),
        }
    }

    #[test]
    fn run_override_is_empty_for_the_floor_and_carries_the_plan_for_an_entitlement() {
        use crate::budget::RunClass::{Agent, Interactive};
        assert_eq!(OrgLimits::floor().run_override(), RunCapOverride::NONE);

        let mut limits = entitled(1, 1, 1);
        assert_eq!(limits.run_override(), RunCapOverride::NONE, "a plan that says nothing is no bound");
        limits.caps.agent_runs = Some(3);
        let over = limits.run_override();
        assert_eq!(over.for_class(Agent), Some(3));
        assert_eq!(over.for_class(Interactive), None);
        limits.caps.interactive_runs = Some(0);
        assert_eq!(limits.run_override().for_class(Interactive), Some(0), "an explicit zero is kept");
    }

    #[test]
    fn a_run_bound_never_exceeds_the_process_wide_cap() {
        use crate::budget::RunClass::{Agent, Interactive};
        let over = RunCapOverride {
            agent_runs: Some(50),
            interactive_runs: Some(2),
        };
        assert_eq!(over.bound_for(Agent, 16), Some(16));
        assert_eq!(over.bound_for(Interactive, 16), Some(2));
        assert_eq!(over.bound_for(Agent, 0), Some(50), "no global cap, the plan value stands");
        assert_eq!(RunCapOverride::NONE.bound_for(Agent, 16), None);
    }

    #[test]
    fn fail_open_bounds_runs_only_by_the_global_cap() {
        use crate::budget::RunClass::Agent;
        let over = OrgLimits::open().run_override();
        assert_eq!(over.bound_for(Agent, 16), Some(16));
    }

    #[test]
    fn defaults_are_static_and_floor() {
        let config = LimitsConfig::from_lookup(lookup(&[])).unwrap();
        assert_eq!(config, LimitsConfig::default());
        assert_eq!(config.source, LimitsSource::Static);
        assert_eq!(config.fail_mode, FailMode::Floor);
        // Empty values are the defaults too.
        let config = LimitsConfig::from_lookup(lookup(&[
            (ENV_LIMITS_SOURCE, " "),
            (ENV_LIMITS_FAIL_MODE, ""),
        ]))
        .unwrap();
        assert_eq!(config, LimitsConfig::default());
    }

    #[test]
    fn a_misspelt_source_or_fail_mode_names_its_variable() {
        let error =
            LimitsConfig::from_lookup(lookup(&[(ENV_LIMITS_SOURCE, "billing")])).unwrap_err();
        assert!(error.contains(ENV_LIMITS_SOURCE), "{error}");
        let error =
            LimitsConfig::from_lookup(lookup(&[(ENV_LIMITS_FAIL_MODE, "shut")])).unwrap_err();
        assert!(error.contains(ENV_LIMITS_FAIL_MODE), "{error}");
        let config = LimitsConfig::from_lookup(lookup(&[(ENV_LIMITS_FAIL_MODE, " Closed ")])).unwrap();
        assert_eq!(config.fail_mode, FailMode::Closed);
    }

    #[test]
    fn billing_core_source_without_settings_is_refused_at_build_not_served_as_the_floor() {
        let config =
            LimitsConfig::from_lookup(lookup(&[(ENV_LIMITS_SOURCE, "billing_core")])).unwrap();
        assert_eq!(config.source, LimitsSource::BillingCore);
        let error = config.build_with(lookup(&[])).err().expect("billing_core needs its settings");
        assert!(error.contains("billing_core"), "{error}");
        assert!(LimitsConfig::default().build().is_ok());
    }

    #[tokio::test]
    async fn the_static_floor_has_no_allowance_for_any_organization() {
        let resolver = LimitsConfig::default().build().unwrap();
        for org in ["org_a", "org_b", ""] {
            let limits = resolver.org_limits(org).await.unwrap();
            assert_eq!(limits, OrgLimits::floor());
            assert_eq!(limits.origin, LimitsOrigin::Floor);
        }
    }

    #[test]
    fn unconfigured_quota_means_floor_not_zero() {
        // An unconfigured allowance never refuses and never lowers a bound.
        assert!(!Allowance::Unconfigured.is_exhausted());
        assert_eq!(Allowance::Unconfigured.remaining(), None);
        assert!(Allowance::Remaining(0).is_exhausted());
        let ack = CrawlLimits::default()
            .resolve(None, None)
            .clamp_pages_to(Allowance::Unconfigured)
            .unwrap();
        assert_eq!(ack.max_pages.effective, DEFAULT_MAX_PAGES);
        assert_eq!(ack.max_pages.remaining, None);
        assert_eq!(ack.max_pages.clamped_by, vec![ClampedBy::Default]);
        let urls = clamp_batch_urls(40, Allowance::Unconfigured).unwrap();
        assert_eq!((urls.accepted, urls.dropped, urls.remaining), (40, 0, None));
        assert!(urls.clamped_by.is_empty());
        assert_eq!(tighten_ceiling(900, Allowance::Unconfigured), (900, false));
    }

    #[test]
    fn entitlement_never_raises_budget_caps() {
        // Remaining above the ceiling leaves the ceiling; remaining equal to it too.
        assert_eq!(tighten_ceiling(900, Allowance::Remaining(10_000)), (900, false));
        assert_eq!(tighten_ceiling(900, Allowance::Remaining(900)), (900, false));
        // Below it lowers it, and says so.
        assert_eq!(tighten_ceiling(900, Allowance::Remaining(120)), (120, true));
        // The same holds for a page bound: a request of 10 with 5000 left stays 10.
        let ack = CrawlLimits::default()
            .resolve(Some(10), None)
            .clamp_pages_to(Allowance::Remaining(5000))
            .unwrap();
        assert_eq!(ack.max_pages.effective, 10);
        assert_eq!(ack.max_pages.remaining, Some(5000));
        assert!(ack.max_pages.clamped_by.is_empty());
    }

    #[test]
    fn entitlement_may_raise_capacity_up_to_the_global_cap() {
        let floor = 3;
        // The plan raises the floor.
        assert_eq!(effective_capacity(floor, Some(8), 16, LimitsOrigin::Entitlement), 8);
        // It never passes the process-wide cap.
        assert_eq!(effective_capacity(floor, Some(50), 16, LimitsOrigin::Entitlement), 16);
        // A plan below the floor is the plan's value.
        assert_eq!(effective_capacity(floor, Some(1), 16, LimitsOrigin::Entitlement), 1);
        // The plan says nothing: the floor.
        assert_eq!(effective_capacity(floor, None, 16, LimitsOrigin::Floor), 3);
        assert_eq!(effective_capacity(floor, None, 16, LimitsOrigin::Entitlement), 3);
        // The floor itself never exceeds the cap.
        assert_eq!(effective_capacity(40, None, 16, LimitsOrigin::Floor), 16);
    }

    #[test]
    fn crawl_pages_are_clamped_to_the_remaining_allowance() {
        let ack = CrawlLimits::default()
            .resolve(None, None)
            .clamp_pages_to(Allowance::Remaining(25))
            .unwrap();
        assert_eq!(ack.max_pages.requested, None);
        assert_eq!(ack.max_pages.effective, 25);
        assert_eq!(ack.max_pages.remaining, Some(25));
        assert_eq!(
            ack.max_pages.clamped_by,
            vec![ClampedBy::Default, ClampedBy::Entitlement]
        );
        // The depth bound is untouched.
        assert_eq!(ack.max_depth.effective, DEFAULT_MAX_DEPTH);
        assert!(ack.max_depth.remaining.is_none());
    }

    #[test]
    fn a_spent_page_allowance_refuses_with_the_metric_only() {
        let refused = CrawlLimits::default()
            .resolve(Some(10), None)
            .clamp_pages_to(Allowance::Remaining(0))
            .unwrap_err();
        assert_eq!(refused.metric, "quarry.crawl.page");
        let error = refused.into_error();
        assert_eq!(error.code, ErrorCode::BudgetExhausted);
        assert_eq!(error.code.http_status(), 402);
        assert!(!error.retryable);
        let details = error.details.unwrap();
        assert_eq!(details["dimension"], "org_allowance");
        assert_eq!(details["metric"], "quarry.crawl.page");
        assert_eq!(details["limit_source"], "entitlement");
        let keys: Vec<&str> = details.as_object().unwrap().keys().map(String::as_str).collect();
        assert!(!keys.contains(&"org_id") && !keys.contains(&"url"), "{keys:?}");
    }

    #[test]
    fn a_batch_keeps_the_first_urls_that_fit() {
        let urls = clamp_batch_urls(40, Allowance::Remaining(25)).unwrap();
        assert_eq!(urls.requested, 40);
        assert_eq!(urls.accepted, 25);
        assert_eq!(urls.dropped, 15);
        assert_eq!(urls.remaining, Some(25));
        assert_eq!(urls.clamped_by, vec![ClampedBy::Entitlement]);
        // Everything fits: nothing dropped, nothing clamped, the remainder reported.
        let urls = clamp_batch_urls(10, Allowance::Remaining(25)).unwrap();
        assert_eq!((urls.accepted, urls.dropped), (10, 0));
        assert!(urls.clamped_by.is_empty());
        assert_eq!(urls.remaining, Some(25));
        // Spent: refused, naming the batch metric.
        assert_eq!(
            clamp_batch_urls(10, Allowance::Remaining(0)).unwrap_err().metric,
            "quarry.batch.url"
        );
    }

    #[test]
    fn handoff_limits_serialise_only_the_block_that_applies() {
        let crawl: HandoffLimits = CrawlLimits::default()
            .resolve(Some(7), None)
            .clamp_pages_to(Allowance::Remaining(3))
            .unwrap()
            .into();
        let json = serde_json::to_value(&crawl).unwrap();
        assert_eq!(json["max_pages"]["effective"], 3);
        assert_eq!(json["max_pages"]["remaining"], 3);
        assert_eq!(json["max_pages"]["clamped_by"], serde_json::json!(["entitlement"]));
        assert!(json.get("urls").is_none());
        let batch: HandoffLimits = clamp_batch_urls(4, Allowance::Remaining(2)).unwrap().into();
        let json = serde_json::to_value(&batch).unwrap();
        assert_eq!(json["urls"]["accepted"], 2);
        assert_eq!(json["urls"]["dropped"], 2);
        assert!(json.get("max_pages").is_none() && json.get("max_depth").is_none());
        // `remaining` is absent for an unconfigured allowance, as before this slice.
        let plain: HandoffLimits = CrawlLimits::default().resolve(None, None).into();
        let json = serde_json::to_value(&plain).unwrap();
        assert!(json["max_pages"].get("remaining").is_none());
    }

    #[tokio::test]
    async fn billing_error_never_blocks_start_run_in_floor_mode() {
        let backend = Scripted::down();
        let resolver = WithFailMode::new(backend, FailMode::Floor);
        let limits = resolver.org_limits("org_a").await.unwrap();
        assert_eq!(limits, OrgLimits::floor());
        assert_eq!(limits.browser_seconds_agent, Allowance::Unconfigured);
    }

    #[tokio::test]
    async fn closed_mode_refuses_when_unreadable() {
        let resolver = WithFailMode::new(Scripted::down(), FailMode::Closed);
        let error = resolver.org_limits("org_a").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::RuntimeNotReady);
        assert_eq!(error.details.unwrap()["reason"], "limits_unreadable");
    }

    #[tokio::test]
    async fn open_mode_uses_no_ceiling() {
        let resolver = WithFailMode::new(Scripted::down(), FailMode::Open);
        let limits = resolver.org_limits("org_a").await.unwrap();
        assert_eq!(limits.origin, LimitsOrigin::Open);
        assert_eq!(limits.crawl_pages, Allowance::Unconfigured);
        // Open capacity is the process-wide cap, not the floor.
        assert_eq!(effective_capacity(3, None, 16, limits.origin), 16);
    }

    #[tokio::test]
    async fn a_readable_backend_is_passed_through_in_every_mode() {
        for mode in [FailMode::Floor, FailMode::Open, FailMode::Closed] {
            let backend = Scripted::up(entitled(25, 25, 600));
            let resolver = WithFailMode::new(backend, mode);
            let limits = resolver.org_limits("org_a").await.unwrap();
            assert_eq!(limits.origin, LimitsOrigin::Entitlement);
            assert_eq!(limits.crawl_pages, Allowance::Remaining(25));
            assert_eq!(resolver.backend.calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[cfg(test)]
mod billing_core_tests {
    //! W2-CP-L2: the billing_core source. A stub fetch and a manual clock drive
    //! the cache, the stale window and the fail mode; one wiremock test covers
    //! the HTTP read (headers, path, status classes).
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A billing-core stand-in whose answer the test can switch, counting reads.
    struct StubBilling {
        answer: Mutex<Result<serde_json::Value, BackendError>>,
        calls: AtomicUsize,
    }

    impl StubBilling {
        fn answering(doc: serde_json::Value) -> Self {
            Self {
                answer: Mutex::new(Ok(doc)),
                calls: AtomicUsize::new(0),
            }
        }

        fn down() -> Self {
            Self {
                answer: Mutex::new(Err(BackendError("unavailable"))),
                calls: AtomicUsize::new(0),
            }
        }

        fn go_down(&self) {
            *self.answer.lock().unwrap() = Err(BackendError("unavailable"));
        }

        fn answer(&self, doc: serde_json::Value) {
            *self.answer.lock().unwrap() = Ok(doc);
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LimitsFetch for StubBilling {
        async fn fetch(&self, _org_id: &str) -> Result<LimitsDoc, BackendError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let answer = self.answer.lock().unwrap().clone();
            answer.and_then(|doc| {
                serde_json::from_value::<LimitsDoc>(doc).map_err(|_| BackendError("malformed"))
            })
        }
    }

    /// A clock the test moves by hand.
    fn manual_clock() -> (Arc<Mutex<Instant>>, LimitsClock) {
        let now = Arc::new(Mutex::new(Instant::now()));
        let read = now.clone();
        (now, Arc::new(move || *read.lock().unwrap()))
    }

    fn advance(clock: &Mutex<Instant>, secs: u64) {
        *clock.lock().unwrap() += Duration::from_secs(secs);
    }

    const TTL: Duration = Duration::from_secs(30);
    const STALE: Duration = Duration::from_secs(300);

    /// A W2-CP-L1 `data` object with a configured page allowance.
    fn snapshot(org: &str, pages_left: i64, caps: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "org_id": org,
            "account": "present",
            "plan": "standard",
            "allowances": {
                "quarry.crawl.page": {
                    "metric": "quarry.crawl.page", "configured": true, "source": "policy",
                    "active": true, "unlimited": false, "limit": 1000, "used": 1000 - pages_left,
                    "remaining": pages_left, "exhausted": pages_left == 0
                },
                "quarry.batch.url": {
                    "metric": "quarry.batch.url", "configured": false, "source": "none",
                    "active": false, "unlimited": false, "limit": null, "used": 0,
                    "remaining": null, "exhausted": false
                },
                "quarry.browser.second.agent": {
                    "metric": "quarry.browser.second.agent", "configured": true, "source": "plan",
                    "active": true, "unlimited": true, "limit": null, "used": 5,
                    "remaining": null, "exhausted": false
                }
            },
            "caps": caps,
            "plan_cap_defaults": true,
            "as_of": "2026-11-03T00:00:00Z"
        })
    }

    fn no_caps() -> serde_json::Value {
        serde_json::json!({
            "agent_runs": null, "interactive_runs": null, "render_permits": null,
            "crawl_jobs": null, "job_backlog": null, "rate_multiplier": null
        })
    }

    fn backend(stub: StubBilling, clock: LimitsClock) -> BillingCoreBackend<StubBilling> {
        BillingCoreBackend::with_fetch(stub, TTL, STALE).with_clock(clock)
    }

    #[test]
    fn the_snapshot_maps_configured_unconfigured_and_unlimited() {
        let doc: LimitsDoc = serde_json::from_value(snapshot("org_a", 25, no_caps())).unwrap();
        let limits = doc.into_org_limits("org_a").unwrap();
        assert_eq!(limits.origin, LimitsOrigin::Entitlement);
        assert_eq!(limits.crawl_pages, Allowance::Remaining(25));
        // Unconfigured is not zero, and unlimited is unconfigured for the edge.
        assert_eq!(limits.batch_urls, Allowance::Unconfigured);
        assert_eq!(limits.browser_seconds_agent, Allowance::Unconfigured);
        assert_eq!(limits.caps, PlanCaps::default());
        // A metric billing-core does not name is unconfigured too.
        let doc: LimitsDoc =
            serde_json::from_value(serde_json::json!({ "org_id": "org_a" })).unwrap();
        assert_eq!(doc.into_org_limits("org_a").unwrap().crawl_pages, Allowance::Unconfigured);
    }

    #[test]
    fn caps_map_null_to_none_keep_zero_and_drop_negatives() {
        let caps = serde_json::json!({
            "agent_runs": 4, "interactive_runs": 0, "render_permits": -1,
            "crawl_jobs": 3, "job_backlog": null, "rate_multiplier": 99_999_999_999i64
        });
        let doc: LimitsDoc = serde_json::from_value(snapshot("org_a", 1, caps)).unwrap();
        let caps = doc.into_org_limits("org_a").unwrap().caps;
        assert_eq!(caps.agent_runs, Some(4));
        assert_eq!(caps.interactive_runs, Some(0), "an explicit zero is a real cap");
        assert_eq!(caps.render_permits, None, "a negative value is no plan value");
        assert_eq!(caps.crawl_jobs, Some(3));
        assert_eq!(caps.job_backlog, None);
        assert_eq!(caps.rate_multiplier, Some(u32::MAX));
    }

    #[test]
    fn an_answer_about_another_org_or_a_malformed_allowance_is_never_served() {
        let doc: LimitsDoc = serde_json::from_value(snapshot("org_b", 25, no_caps())).unwrap();
        assert_eq!(doc.into_org_limits("org_a").unwrap_err(), BackendError("org_mismatch"));
        let doc: LimitsDoc = serde_json::from_value(serde_json::json!({
            "org_id": "org_a",
            "allowances": { "quarry.crawl.page": { "configured": true, "unlimited": false, "remaining": null } }
        }))
        .unwrap();
        assert_eq!(doc.into_org_limits("org_a").unwrap_err(), BackendError("malformed"));
    }

    #[tokio::test]
    async fn a_fresh_answer_is_cached_and_read_again_after_the_ttl() {
        let (now, clock) = manual_clock();
        let backend = backend(StubBilling::answering(snapshot("org_a", 25, no_caps())), clock);
        assert_eq!(backend.read("org_a").await.unwrap().crawl_pages, Allowance::Remaining(25));
        advance(&now, 29);
        assert_eq!(backend.read("org_a").await.unwrap().crawl_pages, Allowance::Remaining(25));
        assert_eq!(backend.fetch.calls(), 1, "a fresh entry is served without a read");
        backend.fetch.answer(snapshot("org_a", 7, no_caps()));
        advance(&now, 2);
        assert_eq!(backend.read("org_a").await.unwrap().crawl_pages, Allowance::Remaining(7));
        assert_eq!(backend.fetch.calls(), 2);
    }

    #[tokio::test]
    async fn the_cache_is_per_organization() {
        let (_now, clock) = manual_clock();
        let backend = backend(StubBilling::answering(snapshot("org_a", 25, no_caps())), clock);
        backend.read("org_a").await.unwrap();
        // The stub answers about org_a whoever asks: org_b's read is refused, not
        // served from org_a's entry.
        assert_eq!(backend.read("org_b").await.unwrap_err(), BackendError("org_mismatch"));
        assert_eq!(backend.fetch.calls(), 2);
    }

    #[tokio::test]
    async fn inside_the_stale_window_a_down_billing_core_still_clamps() {
        let (now, clock) = manual_clock();
        let backend = backend(StubBilling::answering(snapshot("org_a", 25, no_caps())), clock);
        let resolver = WithFailMode::new(backend, FailMode::Floor);
        resolver.org_limits("org_a").await.unwrap();
        resolver.backend.fetch.go_down();
        advance(&now, 30 + 299);
        let limits = resolver.org_limits("org_a").await.unwrap();
        assert_eq!(limits.origin, LimitsOrigin::Entitlement, "the cached answer, not the floor");
        assert_eq!(resolver.backend.fetch.calls(), 2, "a stale entry still tries a read");
        // The cached allowance still clamps the crawl.
        let ack = CrawlLimits::default()
            .resolve(Some(100), None)
            .clamp_pages_to(limits.crawl_pages)
            .unwrap();
        assert_eq!(ack.max_pages.effective, 25);
        assert_eq!(ack.max_pages.clamped_by, vec![ClampedBy::Entitlement]);
    }

    #[tokio::test]
    async fn past_the_stale_window_a_down_billing_core_falls_to_the_fail_mode() {
        for (mode, expected) in [
            (FailMode::Floor, Some(LimitsOrigin::Floor)),
            (FailMode::Open, Some(LimitsOrigin::Open)),
            (FailMode::Closed, None),
        ] {
            let (now, clock) = manual_clock();
            let backend = backend(StubBilling::answering(snapshot("org_a", 25, no_caps())), clock);
            let resolver = WithFailMode::new(backend, mode);
            resolver.org_limits("org_a").await.unwrap();
            resolver.backend.fetch.go_down();
            advance(&now, 30 + 300);
            match (resolver.org_limits("org_a").await, expected) {
                (Ok(limits), Some(origin)) => {
                    assert_eq!(limits.origin, origin, "{mode:?}");
                    // The floor never clamps: the stub-down edge does not refuse.
                    assert_eq!(limits.crawl_pages, Allowance::Unconfigured, "{mode:?}");
                }
                (Err(error), None) => {
                    assert_eq!(error.code, ErrorCode::RuntimeNotReady);
                    assert_eq!(error.details.unwrap()["reason"], "limits_unreadable");
                }
                (other, _) => panic!("{mode:?}: unexpected {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn down_with_nothing_cached_is_the_fail_mode_at_once() {
        let (_now, clock) = manual_clock();
        let resolver = WithFailMode::new(backend(StubBilling::down(), clock), FailMode::Floor);
        assert_eq!(resolver.org_limits("org_a").await.unwrap(), OrgLimits::floor());
    }

    #[tokio::test]
    async fn billing_core_recovering_replaces_the_stale_answer() {
        let (now, clock) = manual_clock();
        let backend = backend(StubBilling::answering(snapshot("org_a", 25, no_caps())), clock);
        backend.read("org_a").await.unwrap();
        backend.fetch.go_down();
        advance(&now, 40);
        assert_eq!(backend.read("org_a").await.unwrap().crawl_pages, Allowance::Remaining(25));
        backend.fetch.answer(snapshot("org_a", 0, no_caps()));
        advance(&now, 1);
        let limits = backend.read("org_a").await.unwrap();
        assert!(limits.crawl_pages.is_exhausted(), "the new answer is served and cached");
        assert_eq!(backend.read("org_a").await.unwrap().crawl_pages, Allowance::Remaining(0));
    }

    fn plan(agent: u32, interactive: u32, render: u32, jobs: u32) -> OrgLimits {
        let caps = serde_json::json!({
            "agent_runs": agent, "interactive_runs": interactive, "render_permits": render,
            "crawl_jobs": jobs, "job_backlog": 10 * jobs, "rate_multiplier": 1
        });
        let doc: LimitsDoc = serde_json::from_value(snapshot("org_a", 1000, caps)).unwrap();
        doc.into_org_limits("org_a").unwrap()
    }

    #[test]
    fn two_plans_give_two_capacities_never_past_the_global_caps() {
        use crate::budget::RunClass::{Agent, Interactive};
        let free = plan(1, 2, 1, 1);
        let pro = plan(8, 8, 5, 5);
        // Run caps (U59): the plan value, capped by MAX_CONCURRENT_RUNS.
        assert_eq!(free.run_override().bound_for(Agent, 32), Some(1));
        assert_eq!(pro.run_override().bound_for(Agent, 32), Some(8));
        assert_eq!(pro.run_override().bound_for(Interactive, 4), Some(4));
        // Job slots: the plan value, capped by the process-wide value.
        assert_eq!(effective_capacity(3, free.caps.crawl_jobs, 16, free.origin), 1);
        assert_eq!(effective_capacity(3, pro.caps.crawl_jobs, 16, pro.origin), 5);
        // Render permits only once the pool cap is on (U59).
        assert_eq!(org_render_permits(2, 4, &free), 1);
        assert_eq!(org_render_permits(2, 4, &pro), 4, "never past the pool size");
    }

    #[test]
    fn render_permits_stay_off_while_the_pool_cap_is_zero() {
        assert_eq!(org_render_permits(0, 4, &plan(8, 8, 3, 5)), 0);
        assert_eq!(org_render_permits(0, 4, &OrgLimits::open()), 0);
        // The floor and a plan that says nothing keep the operator's cap.
        assert_eq!(org_render_permits(2, 4, &OrgLimits::floor()), 2);
        // Fail-open uses the whole pool.
        assert_eq!(org_render_permits(2, 4, &OrgLimits::open()), 4);
        // A plan zero is one permit, never "no cap".
        assert_eq!(org_render_permits(2, 4, &plan(1, 1, 0, 1)), 1);
    }

    fn settings_env(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn settings_need_a_url_and_a_token_and_name_what_is_missing() {
        let error = BillingCoreSettings::from_lookup(settings_env(&[(ENV_BILLING_CORE_TOKEN, "t")]))
            .unwrap_err();
        assert!(error.contains(ENV_BILLING_CORE_URL), "{error}");
        let error = BillingCoreSettings::from_lookup(settings_env(&[(
            ENV_BILLING_CORE_URL,
            "http://billing-core:8080",
        )]))
        .unwrap_err();
        assert!(error.contains(ENV_BILLING_CORE_TOKEN), "{error}");
        let error = BillingCoreSettings::from_lookup(settings_env(&[
            (ENV_BILLING_CORE_URL, "billing-core"),
            (ENV_BILLING_CORE_TOKEN, "t"),
        ]))
        .unwrap_err();
        assert!(error.contains(ENV_BILLING_CORE_URL), "{error}");
        let error = BillingCoreSettings::from_lookup(settings_env(&[
            (ENV_BILLING_CORE_URL, "http://billing-core:8080"),
            (ENV_BILLING_CORE_TOKEN, "t"),
            (ENV_LIMITS_CACHE_TTL_S, "0"),
        ]))
        .unwrap_err();
        assert!(error.contains(ENV_LIMITS_CACHE_TTL_S), "{error}");
    }

    #[test]
    fn settings_defaults_and_the_token_is_never_printed() {
        let settings = BillingCoreSettings::from_lookup(settings_env(&[
            (ENV_BILLING_CORE_URL, " http://billing-core:8080 "),
            (ENV_BILLING_CORE_TOKEN, "not-a-real-token"),
        ]))
        .unwrap();
        assert_eq!(settings.base_url, "http://billing-core:8080");
        assert_eq!(settings.cache_ttl, Duration::from_secs(DEFAULT_LIMITS_CACHE_TTL_S));
        assert_eq!(settings.stale_window, Duration::from_secs(DEFAULT_LIMITS_STALE_WINDOW_S));
        assert_eq!(settings.timeout, Duration::from_millis(DEFAULT_LIMITS_TIMEOUT_MS));
        let printed = format!("{settings:?}");
        assert!(!printed.contains("not-a-real-token"), "{printed}");
    }

    #[test]
    fn billing_core_source_builds_once_configured() {
        let config = LimitsConfig::from_lookup(settings_env(&[(ENV_LIMITS_SOURCE, "billing_core")]))
            .unwrap();
        assert_eq!(config.source, LimitsSource::BillingCore);
        // Without a URL and a token the edge does not boot under a name that
        // promises entitlements.
        let error = config.build_with(settings_env(&[])).err().expect("needs settings");
        assert!(error.contains(ENV_BILLING_CORE_URL), "{error}");
        assert!(config
            .build_with(settings_env(&[
                (ENV_BILLING_CORE_URL, "http://billing-core:8080"),
                (ENV_BILLING_CORE_TOKEN, "t"),
            ]))
            .is_ok());
        // The static default never reads the billing settings.
        assert!(LimitsConfig::default().build_with(settings_env(&[])).is_ok());
    }

    #[tokio::test]
    async fn the_http_read_binds_the_org_and_maps_status_classes() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/billing/orgs/org_a/quarry/limits"))
            .and(header("x-service-id", "quarry-edge"))
            .and(header("x-service-token", "stub-token"))
            .and(header("x-quarry-org-id", "org_a"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": snapshot("org_a", 12, no_caps()) })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/billing/orgs/org_down/quarry/limits"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/billing/orgs/org_gone/quarry/limits"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": { "code": "organization_inactive", "message": "inactive" }
            })))
            .mount(&server)
            .await;

        let settings = BillingCoreSettings {
            base_url: format!("{}/", server.uri()),
            token: "stub-token".to_owned(),
            cache_ttl: TTL,
            stale_window: STALE,
            timeout: Duration::from_secs(2),
        };
        let fetch = HttpLimitsFetch::new(&settings).unwrap();
        let doc = fetch.fetch("org_a").await.unwrap();
        assert_eq!(doc.into_org_limits("org_a").unwrap().crawl_pages, Allowance::Remaining(12));
        assert_eq!(fetch.fetch("org_down").await.unwrap_err(), BackendError("unavailable"));
        assert_eq!(fetch.fetch("org_gone").await.unwrap_err(), BackendError("forbidden"));
        // An org id that is not a plain path segment is refused before any request.
        let before = server.received_requests().await.unwrap_or_default().len();
        for bad in ["", "../org_a", "org a", "org/a", "org%2Fa"] {
            assert_eq!(fetch.fetch(bad).await.unwrap_err(), BackendError("invalid_org"), "{bad:?}");
        }
        let after = server.received_requests().await.unwrap_or_default().len();
        assert_eq!(before, after);

        // Through the whole resolver: a 503 with nothing cached is the floor.
        let resolver = WithFailMode::new(BillingCoreBackend::from_settings(&settings).unwrap(), FailMode::Floor);
        assert_eq!(resolver.org_limits("org_down").await.unwrap(), OrgLimits::floor());
        assert_eq!(
            resolver.org_limits("org_a").await.unwrap().crawl_pages,
            Allowance::Remaining(12)
        );
    }
}
