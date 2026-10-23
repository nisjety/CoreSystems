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
// Only the `static` source is built. It knows nothing about an organization, so
// every allowance is "unconfigured" and every capacity cap is "use the floor":
// the edge behaves exactly as it did before this module existed. The
// `billing_core` source (a per-org read of the quota status with a cache and a
// stale window) is a second [`LimitsBackend`] behind the same [`FailMode`]
// wrapper; it waits on the billing-core credential and is not built here.
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
    /// The billing-core quota status. Not built in this edge (it waits on the
    /// billing-core credential): naming it stops boot.
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

    /// The resolver this configuration names. `billing_core` is refused here, at
    /// boot, rather than silently serving the floor under a name that promises
    /// entitlements.
    pub fn build(&self) -> Result<Arc<dyn LimitsResolver>, String> {
        match self.source {
            LimitsSource::Static => Ok(Arc::new(StaticFloor)),
            LimitsSource::BillingCore => Err(format!(
                "{ENV_LIMITS_SOURCE}=billing_core is not built in this edge (the billing-core \
                 credential is not provisioned); unset it or use static"
            )),
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

/// The `limits` block of a handoff acknowledgement. A crawl carries
/// `max_pages` and `max_depth`; a batch carries `urls`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<Bound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<Bound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urls: Option<UrlsBound>,
}

impl From<CrawlLimitsAck> for HandoffLimits {
    fn from(ack: CrawlLimitsAck) -> Self {
        Self {
            max_pages: Some(ack.max_pages),
            max_depth: Some(ack.max_depth),
            urls: None,
        }
    }
}

impl From<UrlsBound> for HandoffLimits {
    fn from(urls: UrlsBound) -> Self {
        Self {
            max_pages: None,
            max_depth: None,
            urls: Some(urls),
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
    fn billing_core_source_is_refused_at_build_not_served_as_the_floor() {
        let config =
            LimitsConfig::from_lookup(lookup(&[(ENV_LIMITS_SOURCE, "billing_core")])).unwrap();
        assert_eq!(config.source, LimitsSource::BillingCore);
        let error = config.build().err().expect("billing_core is not built");
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
