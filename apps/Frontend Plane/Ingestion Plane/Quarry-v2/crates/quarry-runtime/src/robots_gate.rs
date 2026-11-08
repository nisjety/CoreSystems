//! The robots.txt gate (WS14.S4).
//!
//! A crawl or batch page is checked against its origin's robots.txt at fetch time,
//! inside `PageRunner::run_inner` after the security preflight and before DNS
//! resolution and the host scheduler slot. A denial therefore never reaches
//! `HostScheduler::record_bad`, which would read it as a server error and halve
//! the host's concurrency.
//!
//! The robots.txt itself is fetched through the guarded [`Driver`] chain with
//! `Driver::fetch_conditional`, so it gets the same SSRF guard, DNS pinning and
//! redirect-hop checks as a page. There is no second HTTP client here. The fetch
//! carries the caller's org (egress identity), denies third-party processing, and
//! reads at most [`ROBOTS_BODY_CAP`] bytes.
//!
//! Reading the answer (RFC 9309 2.3.1):
//! - 2xx: the body is the rules;
//! - 4xx other than 429 (401, 403, 404, 410 ...): the file does not exist, so
//!   everything is allowed;
//! - 429, 5xx, a redirect that did not resolve, a timeout, a refused target or a
//!   transport error: the file is unreachable. With the default policy
//!   (`QUARRY_EDGE__ROBOTS_UNREACHABLE=disallow`) the origin is treated as fully
//!   disallowed; `allow` treats it as no robots.txt. Two attempts are made inside
//!   the fetch timeout before a 5xx or a transport error is called unreachable.
//!
//! A decision is made over the token set [`ROBOTS_PRODUCT_TOKENS`]: the most
//! restrictive answer across the tokens wins. The rule is matched against the
//! path and the query. A request under ZDR never reads or fills the cache.
//!
//! Modes (`QUARRY_EDGE__CRAWL_ROBOTS`): `off` does not evaluate; `observe`
//! (default) evaluates and counts, and never blocks or slows a fetch; `enforce`
//! denies and applies `Crawl-delay`. Enforcing needs product sign-off (X-10).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use url::Url;

use quarry_core::crawl_denial::CrawlDenialReason;
use quarry_core::error::ErrorCode;
use quarry_core::privacy::PrivacyPolicy;
use quarry_transform::robots::path_and_query;

use crate::crawl_filter::RobotsPeek;
use crate::driver::{Driver, FetchHints};
use crate::robots_cache::{origin_key, RobotsCache, RobotsFetch, RobotsFetcher, RobotsState};

/// The user-agent tokens the gate answers for. A file may name either; the most
/// restrictive answer wins. One constant, so the product-token decision
/// (F-t2-067) changes one line.
pub const ROBOTS_PRODUCT_TOKENS: [&str; 2] = ["quarrybot", "quarry"];

/// The most of a robots.txt that is read (RFC 9309 asks for at least 500 KiB).
pub const ROBOTS_BODY_CAP: usize = 512 * 1024;

/// Fetch attempts inside the fetch timeout.
const FETCH_ATTEMPTS: usize = 2;

pub const ENV_MODE: &str = "QUARRY_EDGE__CRAWL_ROBOTS";
pub const ENV_UNREACHABLE: &str = "QUARRY_EDGE__ROBOTS_UNREACHABLE";
pub const ENV_CRAWL_DELAY_MAX_S: &str = "QUARRY_CRAWL_DELAY_MAX_S";
pub const ENV_CACHE_MAX_ENTRIES: &str = "QUARRY_ROBOTS_CACHE_MAX_ENTRIES";
pub const ENV_FETCH_TIMEOUT_S: &str = "QUARRY_ROBOTS_FETCH_TIMEOUT_S";

/// Default and ceiling of the `Crawl-delay` the gate honours, in seconds.
pub const DEFAULT_CRAWL_DELAY_MAX_S: u64 = 10;
pub const CRAWL_DELAY_CEILING_S: u64 = 60;
pub const DEFAULT_FETCH_TIMEOUT_S: u64 = 8;
const FETCH_TIMEOUT_CEILING_S: u64 = 60;

/// Whether the gate evaluates, and whether it acts on the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RobotsEnforceMode {
    /// No evaluation, no robots.txt fetch.
    Off,
    /// Evaluate and count; a denial is only counted (`would_deny`).
    #[default]
    Observe,
    /// Deny what robots.txt disallows and honour `Crawl-delay`.
    Enforce,
}

impl RobotsEnforceMode {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "observe" => Ok(Self::Observe),
            "enforce" => Ok(Self::Enforce),
            _ => Err(format!("{ENV_MODE} must be off, observe or enforce")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }
}

/// What an unreachable robots.txt means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnreachablePolicy {
    /// RFC 9309: the origin is treated as fully disallowed.
    #[default]
    Disallow,
    /// The origin is treated as having no robots.txt.
    Allow,
}

impl UnreachablePolicy {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "disallow" => Ok(Self::Disallow),
            "allow" => Ok(Self::Allow),
            _ => Err(format!("{ENV_UNREACHABLE} must be disallow or allow")),
        }
    }
}

/// The gate's settings. A value that does not parse is an error at boot, never a
/// silent default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsGateConfig {
    pub mode: RobotsEnforceMode,
    pub unreachable: UnreachablePolicy,
    /// The longest `Crawl-delay` honoured; a longer one is clamped to it.
    pub crawl_delay_max: Duration,
    pub cache_max_entries: usize,
    pub fetch_timeout: Duration,
}

impl Default for RobotsGateConfig {
    fn default() -> Self {
        Self {
            mode: RobotsEnforceMode::default(),
            unreachable: UnreachablePolicy::default(),
            crawl_delay_max: Duration::from_secs(DEFAULT_CRAWL_DELAY_MAX_S),
            cache_max_entries: crate::robots_cache::DEFAULT_MAX_ENTRIES,
            fetch_timeout: Duration::from_secs(DEFAULT_FETCH_TIMEOUT_S),
        }
    }
}

impl RobotsGateConfig {
    /// Read the five settings from the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Read the five settings through `get`. An unset or empty variable takes its
    /// default; a set one that does not parse is an error naming the variable.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let get = |name: &str| get(name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let mut config = Self::default();
        if let Some(raw) = get(ENV_MODE) {
            config.mode = RobotsEnforceMode::parse(&raw)?;
        }
        if let Some(raw) = get(ENV_UNREACHABLE) {
            config.unreachable = UnreachablePolicy::parse(&raw)?;
        }
        if let Some(raw) = get(ENV_CRAWL_DELAY_MAX_S) {
            let secs: u64 = raw
                .parse()
                .map_err(|_| format!("{ENV_CRAWL_DELAY_MAX_S} must be a whole number of seconds"))?;
            if secs > CRAWL_DELAY_CEILING_S {
                return Err(format!(
                    "{ENV_CRAWL_DELAY_MAX_S} must be at most {CRAWL_DELAY_CEILING_S}"
                ));
            }
            config.crawl_delay_max = Duration::from_secs(secs);
        }
        if let Some(raw) = get(ENV_CACHE_MAX_ENTRIES) {
            let n: usize = raw
                .parse()
                .map_err(|_| format!("{ENV_CACHE_MAX_ENTRIES} must be a whole number"))?;
            if n == 0 {
                return Err(format!("{ENV_CACHE_MAX_ENTRIES} must be at least 1"));
            }
            config.cache_max_entries = n;
        }
        if let Some(raw) = get(ENV_FETCH_TIMEOUT_S) {
            let secs: u64 = raw
                .parse()
                .map_err(|_| format!("{ENV_FETCH_TIMEOUT_S} must be a whole number of seconds"))?;
            if secs == 0 || secs > FETCH_TIMEOUT_CEILING_S {
                return Err(format!(
                    "{ENV_FETCH_TIMEOUT_S} must be between 1 and {FETCH_TIMEOUT_CEILING_S}"
                ));
            }
            config.fetch_timeout = Duration::from_secs(secs);
        }
        Ok(config)
    }
}

/// What the gate says about one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RobotsDecision {
    /// Fetch it. `crawl_delay` is the host's clamped `Crawl-delay`, when it set one.
    Allowed { crawl_delay: Option<Duration> },
    /// robots.txt disallows the path for `token`. `path` is the path only (no query).
    Disallowed { token: String, path: String },
    /// robots.txt could not be fetched and the policy is `disallow`.
    Unreachable { host: String },
    /// Not an http(s) URL with a host: robots.txt has nothing to say.
    NotApplicable,
}

impl RobotsDecision {
    /// Whether the decision refuses the fetch.
    pub fn is_denial(&self) -> bool {
        matches!(self, Self::Disallowed { .. } | Self::Unreachable { .. })
    }

    /// The typed crawl denial for a refusal, the same vocabulary the discovery
    /// filter uses.
    pub fn reason(&self) -> Option<CrawlDenialReason> {
        match self {
            Self::Disallowed { token, path } => Some(CrawlDenialReason::RobotsDisallowed {
                user_agent: token.clone(),
                path: path.clone(),
            }),
            Self::Unreachable { host } => {
                Some(CrawlDenialReason::RobotsUnreachable { host: host.clone() })
            }
            _ => None,
        }
    }

    /// `details.denial` of the 403 a refusal becomes: `{code, message, remedy,
    /// user_agent?, path?, host?}`.
    pub fn denial_json(&self) -> Option<serde_json::Value> {
        let reason = self.reason()?;
        let mut denial = serde_json::json!({
            "code": reason.code(),
            "message": reason.message(),
            "remedy": reason.remedy(),
        });
        match &reason {
            CrawlDenialReason::RobotsDisallowed { user_agent, path } => {
                denial["user_agent"] = serde_json::json!(user_agent);
                denial["path"] = serde_json::json!(path);
            }
            CrawlDenialReason::RobotsUnreachable { host } => {
                denial["host"] = serde_json::json!(host);
            }
            _ => {}
        }
        Some(denial)
    }
}

/// Counts of what the gate decided. Content-free: no URL, host or path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RobotsStats {
    pub allowed: u64,
    /// Refused in `enforce` mode: disallowed.
    pub denied: u64,
    /// Refused in `enforce` mode: unreachable.
    pub denied_unreachable: u64,
    /// Would have been refused in `observe` mode: disallowed.
    pub would_deny: u64,
    /// Would have been refused in `observe` mode: unreachable.
    pub would_deny_unreachable: u64,
}

#[derive(Default)]
struct Counters {
    allowed: AtomicU64,
    denied: AtomicU64,
    denied_unreachable: AtomicU64,
    would_deny: AtomicU64,
    would_deny_unreachable: AtomicU64,
}

/// The robots.txt fetcher that goes through the guarded driver chain.
pub struct DriverRobotsFetcher {
    driver: Arc<dyn Driver>,
    timeout: Duration,
}

impl DriverRobotsFetcher {
    pub fn new(driver: Arc<dyn Driver>, timeout: Duration) -> Self {
        Self { driver, timeout }
    }

    /// The hints a robots.txt fetch carries: the org (egress identity), no third
    /// party, at most 512 KiB read, a longer body cut instead of refused.
    fn hints(org_id: &str) -> FetchHints {
        FetchHints {
            org_id: org_id.to_string(),
            privacy: PrivacyPolicy::default(),
            max_body_bytes: Some(ROBOTS_BODY_CAP),
            truncate_at_cap: true,
            ..FetchHints::default()
        }
    }

    async fn attempts(&self, robots_url: &Url, org_id: &str) -> RobotsFetch {
        let hints = Self::hints(org_id);
        let mut last = "error".to_string();
        for _ in 0..FETCH_ATTEMPTS {
            match self.driver.fetch_conditional(robots_url, &hints).await {
                Ok(resp) => match classify_status(resp.status) {
                    StatusClass::Found => {
                        return RobotsFetch::Found(String::from_utf8_lossy(&resp.body).into_owned())
                    }
                    StatusClass::Absent => return RobotsFetch::Absent,
                    StatusClass::Unreachable(label) => last = label,
                },
                Err(e) => {
                    last = error_class(e.code).to_string();
                    // A refused target does not get better with a second try.
                    if e.code == ErrorCode::SecurityBlocked {
                        return RobotsFetch::Unreachable(last);
                    }
                }
            }
        }
        RobotsFetch::Unreachable(last)
    }
}

#[async_trait]
impl RobotsFetcher for DriverRobotsFetcher {
    async fn fetch(&self, origin: &str, org_id: &str) -> RobotsFetch {
        let robots_url = match Url::parse(origin).and_then(|base| base.join("/robots.txt")) {
            Ok(url) => url,
            Err(_) => return RobotsFetch::Unreachable("bad_origin".to_string()),
        };
        match tokio::time::timeout(self.timeout, self.attempts(&robots_url, org_id)).await {
            Ok(fetch) => fetch,
            Err(_) => RobotsFetch::Unreachable("timeout".to_string()),
        }
    }
}

enum StatusClass {
    Found,
    Absent,
    Unreachable(String),
}

/// RFC 9309 2.3.1: success is the rules, a client error says the file is not
/// there, everything else is unavailable. 429 is a client error that asks the
/// crawler to back off, so it is read as unavailable.
fn classify_status(status: u16) -> StatusClass {
    match status {
        200..=299 => StatusClass::Found,
        429 => StatusClass::Unreachable("status_429".to_string()),
        400..=499 => StatusClass::Absent,
        500..=599 => StatusClass::Unreachable("status_5xx".to_string()),
        _ => StatusClass::Unreachable("status_other".to_string()),
    }
}

fn error_class(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::SecurityBlocked => "blocked",
        ErrorCode::Timeout => "timeout",
        ErrorCode::RateLimited => "rate_limited",
        _ => "error",
    }
}

impl RobotsStats {
    /// Every decision counted, whatever its outcome. The soak's go criterion (X-10) is
    /// the share of this total that is `would_deny` or `would_deny_unreachable`.
    pub fn total(&self) -> u64 {
        self.allowed
            + self.denied
            + self.denied_unreachable
            + self.would_deny
            + self.would_deny_unreachable
    }
}

/// The robots.txt gate: a cache of per-origin rules and the decision over them.
#[derive(Clone)]
pub struct RobotsGate {
    cache: RobotsCache,
    config: RobotsGateConfig,
    counters: Arc<Counters>,
}

impl RobotsGate {
    /// A gate that fetches robots.txt through `driver` (the guarded default
    /// driver chain).
    pub fn new(driver: Arc<dyn Driver>, config: RobotsGateConfig) -> Self {
        let fetcher = Arc::new(DriverRobotsFetcher::new(driver, config.fetch_timeout));
        Self::with_fetcher(fetcher, config)
    }

    /// A gate over any fetcher. For tests.
    pub fn with_fetcher(fetcher: Arc<dyn RobotsFetcher>, config: RobotsGateConfig) -> Self {
        Self {
            cache: RobotsCache::new(fetcher).with_max_entries(config.cache_max_entries),
            config,
            counters: Arc::new(Counters::default()),
        }
    }

    pub fn config(&self) -> &RobotsGateConfig {
        &self.config
    }

    pub fn mode(&self) -> RobotsEnforceMode {
        self.config.mode
    }

    /// Origins in the cache. A request under ZDR leaves this unchanged.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn stats(&self) -> RobotsStats {
        RobotsStats {
            allowed: self.counters.allowed.load(Ordering::Relaxed),
            denied: self.counters.denied.load(Ordering::Relaxed),
            denied_unreachable: self.counters.denied_unreachable.load(Ordering::Relaxed),
            would_deny: self.counters.would_deny.load(Ordering::Relaxed),
            would_deny_unreachable: self.counters.would_deny_unreachable.load(Ordering::Relaxed),
        }
    }

    /// Count one decision under `mode`. Content-free. `Off` and `NotApplicable`
    /// count nothing.
    pub fn record(&self, mode: RobotsEnforceMode, decision: &RobotsDecision) {
        let counter = match (mode, decision) {
            (RobotsEnforceMode::Off, _) | (_, RobotsDecision::NotApplicable) => return,
            (_, RobotsDecision::Allowed { .. }) => &self.counters.allowed,
            (RobotsEnforceMode::Enforce, RobotsDecision::Disallowed { .. }) => &self.counters.denied,
            (RobotsEnforceMode::Enforce, RobotsDecision::Unreachable { .. }) => {
                &self.counters.denied_unreachable
            }
            (RobotsEnforceMode::Observe, RobotsDecision::Disallowed { .. }) => {
                &self.counters.would_deny
            }
            (RobotsEnforceMode::Observe, RobotsDecision::Unreachable { .. }) => {
                &self.counters.would_deny_unreachable
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// The rules of the origin of `url`: from the cache while fresh, otherwise
    /// fetched. Under ZDR the fetch is made and nothing is stored or read.
    /// `None` when `url` is not an http(s) URL with a host.
    pub async fn rules_for_origin(&self, url: &Url, org_id: &str, zdr: bool) -> Option<RobotsState> {
        let origin = origin_key(url)?;
        Some(if zdr {
            self.cache.get_uncached(&origin, org_id).await
        } else {
            self.cache.get(&origin, org_id).await
        })
    }

    /// Decide `url`: allowed, disallowed (with the token that disallows it),
    /// unreachable, or not applicable. Fetches robots.txt when the origin is not
    /// cached (always, under ZDR).
    pub async fn check(&self, url: &Url, org_id: &str, zdr: bool) -> RobotsDecision {
        match self.rules_for_origin(url, org_id, zdr).await {
            Some(state) => self.decide(&state, url),
            None => RobotsDecision::NotApplicable,
        }
    }

    /// What the cache already knows about `url`'s origin, as a decision. Never a
    /// network call: `None` when the origin is not cached.
    pub fn peek_decision(&self, url: &Url) -> Option<RobotsDecision> {
        let origin = origin_key(url)?;
        self.cache.peek(&origin).map(|state| self.decide(&state, url))
    }

    /// The cached answer for the discovery filter (`crawl_filter`): `Unknown`
    /// when the origin is not cached, so a link is never denied on a guess.
    pub fn peek(&self, url: &Url) -> RobotsPeek {
        match self.peek_decision(url) {
            None | Some(RobotsDecision::NotApplicable) => RobotsPeek::Unknown,
            Some(RobotsDecision::Allowed { .. }) => RobotsPeek::Allowed,
            Some(RobotsDecision::Disallowed { token, path }) => RobotsPeek::Disallowed {
                user_agent: token,
                path,
            },
            Some(RobotsDecision::Unreachable { .. }) => RobotsPeek::Unreachable,
        }
    }

    /// The `Crawl-delay` of `url`'s origin in milliseconds, from the cache only.
    /// `None` when none is cached or the origin sets none.
    pub fn crawl_delay_ms(&self, url: &Url) -> Option<u64> {
        match self.peek_decision(url)? {
            RobotsDecision::Allowed {
                crawl_delay: Some(delay),
            } => Some(delay.as_millis().min(u128::from(u64::MAX)) as u64),
            _ => None,
        }
    }

    fn decide(&self, state: &RobotsState, url: &Url) -> RobotsDecision {
        match state {
            RobotsState::Rules(rules) => {
                let target = path_and_query(url);
                match rules.denying_token(&ROBOTS_PRODUCT_TOKENS, &target) {
                    Some(token) => RobotsDecision::Disallowed {
                        token: token.to_string(),
                        path: url.path().to_string(),
                    },
                    None => RobotsDecision::Allowed {
                        crawl_delay: rules
                            .crawl_delay_any(&ROBOTS_PRODUCT_TOKENS)
                            .map(|secs| self.clamp_delay(secs)),
                    },
                }
            }
            RobotsState::Absent => RobotsDecision::Allowed { crawl_delay: None },
            RobotsState::Unreachable => match self.config.unreachable {
                UnreachablePolicy::Disallow => RobotsDecision::Unreachable {
                    host: url.host_str().unwrap_or_default().to_string(),
                },
                UnreachablePolicy::Allow => RobotsDecision::Allowed { crawl_delay: None },
            },
        }
    }

    fn clamp_delay(&self, secs: f64) -> Duration {
        Duration::try_from_secs_f64(secs)
            .unwrap_or(self.config.crawl_delay_max)
            .min(self.config.crawl_delay_max)
    }
}

/// A gate with the mode a page run applies it under. `PageRunner.robots`.
#[derive(Clone)]
pub struct RobotsEnforcement {
    pub gate: Arc<RobotsGate>,
    pub mode: RobotsEnforceMode,
}

impl RobotsEnforcement {
    pub fn new(gate: Arc<RobotsGate>, mode: RobotsEnforceMode) -> Self {
        Self { gate, mode }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::FetchResponse;
    use quarry_core::error::QuarryError;
    use quarry_core::output::DriverKind;
    use quarry_core::QuarryResult;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    /// A driver that answers from a script and records how it was called.
    struct StubDriver {
        script: Mutex<VecDeque<Result<(u16, Vec<u8>), QuarryError>>>,
        conditional_calls: Mutex<Vec<(String, FetchHints)>>,
        plain_calls: AtomicUsize,
        delay: Duration,
    }

    impl StubDriver {
        fn answering(steps: Vec<Result<(u16, Vec<u8>), QuarryError>>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(steps.into()),
                conditional_calls: Mutex::new(Vec::new()),
                plain_calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
            })
        }

        fn status(status: u16, body: &str) -> Arc<Self> {
            Self::answering(vec![Ok((status, body.as_bytes().to_vec())); 4])
        }

        fn calls(&self) -> usize {
            self.conditional_calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl Driver for StubDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            self.plain_calls.fetch_add(1, Ordering::SeqCst);
            Err(QuarryError::new(ErrorCode::Internal, "fetch must not be called"))
        }

        async fn fetch_conditional(
            &self,
            url: &Url,
            hints: &FetchHints,
        ) -> QuarryResult<FetchResponse> {
            self.conditional_calls
                .lock()
                .unwrap()
                .push((url.to_string(), hints.clone()));
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let step = self
                .script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(QuarryError::new(ErrorCode::Internal, "script exhausted")));
            step.map(|(status, body)| {
                FetchResponse::new(status, url.clone(), Vec::new(), body, 1, DriverKind::Static)
            })
        }
    }

    fn gate_over(driver: Arc<StubDriver>) -> RobotsGate {
        RobotsGate::new(driver, RobotsGateConfig::default())
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    const PRIVATE_RULES: &str = "User-agent: *\nDisallow: /private/\nCrawl-delay: 2\n";

    #[tokio::test]
    async fn the_fetch_goes_through_fetch_conditional_with_org_and_deny_third_party() {
        let driver = StubDriver::status(200, PRIVATE_RULES);
        let gate = gate_over(driver.clone());
        gate.check(&url("https://example.test/page?x=1"), "org_a", false).await;
        assert_eq!(driver.plain_calls.load(Ordering::SeqCst), 0, "never Driver::fetch");
        let calls = driver.conditional_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (fetched, hints) = &calls[0];
        assert_eq!(fetched, "https://example.test/robots.txt", "no query, no path of the page");
        assert_eq!(hints.org_id, "org_a");
        assert!(!hints.privacy.allow_third_party_processing);
        assert_eq!(hints.max_body_bytes, Some(512 * 1024));
        assert!(hints.truncate_at_cap);
    }

    #[tokio::test]
    async fn a_disallowed_path_names_the_token_and_the_path_without_the_query() {
        let gate = gate_over(StubDriver::status(200, PRIVATE_RULES));
        let decision = gate
            .check(&url("https://example.test/private/secret?token=abc"), "org", false)
            .await;
        match decision {
            RobotsDecision::Disallowed { token, path } => {
                assert_eq!(token, "quarrybot");
                assert_eq!(path, "/private/secret", "the query is not echoed");
            }
            other => panic!("expected Disallowed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_allowed_path_carries_the_clamped_crawl_delay() {
        let gate = gate_over(StubDriver::status(200, PRIVATE_RULES));
        let decision = gate.check(&url("https://example.test/docs/a"), "org", false).await;
        assert_eq!(
            decision,
            RobotsDecision::Allowed {
                crawl_delay: Some(Duration::from_secs(2))
            }
        );
        assert_eq!(gate.crawl_delay_ms(&url("https://example.test/x")), Some(2000));

        let slow = gate_over(StubDriver::status(200, "User-agent: *\nCrawl-delay: 600\n"));
        match slow.check(&url("https://example.test/a"), "org", false).await {
            RobotsDecision::Allowed { crawl_delay } => {
                assert_eq!(crawl_delay, Some(Duration::from_secs(10)), "clamped to the default max");
            }
            other => panic!("expected Allowed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_most_restrictive_token_wins() {
        let rules = "User-agent: quarry\nDisallow: /only-quarry/\n\nUser-agent: quarrybot\nDisallow:\n";
        let gate = gate_over(StubDriver::status(200, rules));
        let decision = gate.check(&url("https://example.test/only-quarry/x"), "org", false).await;
        assert!(matches!(decision, RobotsDecision::Disallowed { ref token, .. } if token == "quarry"));
    }

    #[tokio::test]
    async fn query_aware_rules_deny_the_session_url() {
        let gate = gate_over(StubDriver::status(200, "User-agent: *\nDisallow: /*?session=\n"));
        assert!(gate
            .check(&url("https://example.test/private/page?session=9"), "org", false)
            .await
            .is_denial());
        assert!(!gate
            .check(&url("https://example.test/private/page?id=9"), "org", false)
            .await
            .is_denial());
    }

    #[tokio::test]
    async fn four_xx_means_no_robots_txt_and_everything_is_allowed() {
        for status in [401u16, 403, 404, 410] {
            let gate = gate_over(StubDriver::status(status, "User-agent: *\nDisallow: /\n"));
            let decision = gate.check(&url("https://example.test/a"), "org", false).await;
            assert_eq!(decision, RobotsDecision::Allowed { crawl_delay: None }, "status {status}");
        }
    }

    #[tokio::test]
    async fn five_xx_is_unreachable_after_two_attempts_and_disallows_by_default() {
        let driver = StubDriver::status(503, "");
        let gate = gate_over(driver.clone());
        let decision = gate.check(&url("https://example.test/a"), "org", false).await;
        assert_eq!(decision, RobotsDecision::Unreachable { host: "example.test".into() });
        assert_eq!(driver.calls(), 2, "two attempts, then unreachable");
    }

    #[tokio::test]
    async fn a_second_attempt_can_succeed() {
        let driver = StubDriver::answering(vec![Ok((503, Vec::new())), Ok((200, PRIVATE_RULES.as_bytes().to_vec()))]);
        let gate = gate_over(driver.clone());
        let decision = gate.check(&url("https://example.test/private/x"), "org", false).await;
        assert!(matches!(decision, RobotsDecision::Disallowed { .. }));
        assert_eq!(driver.calls(), 2);
    }

    #[tokio::test]
    async fn unreachable_with_the_allow_policy_allows() {
        let config = RobotsGateConfig {
            unreachable: UnreachablePolicy::Allow,
            ..RobotsGateConfig::default()
        };
        let gate = RobotsGate::new(StubDriver::status(500, ""), config);
        let decision = gate.check(&url("https://example.test/a"), "org", false).await;
        assert_eq!(decision, RobotsDecision::Allowed { crawl_delay: None });
    }

    #[tokio::test]
    async fn a_refused_target_is_unreachable_never_allowed_and_is_not_retried() {
        let blocked = Err(QuarryError::new(ErrorCode::SecurityBlocked, "private target"));
        let driver = StubDriver::answering(vec![blocked]);
        let gate = gate_over(driver.clone());
        let decision = gate.check(&url("https://example.test/a"), "org", false).await;
        assert!(matches!(decision, RobotsDecision::Unreachable { .. }), "{decision:?}");
        assert_eq!(driver.calls(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_timeout_is_unreachable() {
        let driver = Arc::new(StubDriver {
            script: Mutex::new(vec![Ok((200, Vec::new())); 4].into()),
            conditional_calls: Mutex::new(Vec::new()),
            plain_calls: AtomicUsize::new(0),
            delay: Duration::from_secs(60),
        });
        let gate = gate_over(driver);
        let decision = gate.check(&url("https://example.test/a"), "org", false).await;
        assert!(matches!(decision, RobotsDecision::Unreachable { .. }), "{decision:?}");
    }

    #[tokio::test]
    async fn a_zdr_request_fills_no_cache_and_a_normal_one_does() {
        let gate = gate_over(StubDriver::status(200, PRIVATE_RULES));
        gate.check(&url("https://example.test/a"), "org", true).await;
        gate.check(&url("https://example.test/a"), "org", true).await;
        assert_eq!(gate.len(), 0);
        gate.check(&url("https://example.test/a"), "org", false).await;
        assert_eq!(gate.len(), 1);
    }

    #[tokio::test]
    async fn peek_never_fetches_and_reports_unknown_until_cached() {
        let driver = StubDriver::status(200, PRIVATE_RULES);
        let gate = gate_over(driver.clone());
        let link = url("https://example.test/private/x");
        assert_eq!(gate.peek(&link), RobotsPeek::Unknown);
        assert_eq!(driver.calls(), 0);
        gate.check(&url("https://example.test/"), "org", false).await;
        assert!(matches!(gate.peek(&link), RobotsPeek::Disallowed { .. }));
        assert_eq!(gate.peek(&url("https://example.test/docs")), RobotsPeek::Allowed);
        assert_eq!(gate.peek(&url("https://other.test/")), RobotsPeek::Unknown);
    }

    #[tokio::test]
    async fn a_non_http_url_is_not_applicable() {
        let gate = gate_over(StubDriver::status(200, PRIVATE_RULES));
        let decision = gate.check(&url("ftp://example.test/a"), "org", false).await;
        assert_eq!(decision, RobotsDecision::NotApplicable);
    }

    #[tokio::test]
    async fn counters_split_observe_and_enforce() {
        let gate = gate_over(StubDriver::status(200, PRIVATE_RULES));
        let denied = RobotsDecision::Disallowed { token: "quarry".into(), path: "/p".into() };
        let unreachable = RobotsDecision::Unreachable { host: "h".into() };
        let allowed = RobotsDecision::Allowed { crawl_delay: None };
        gate.record(RobotsEnforceMode::Observe, &denied);
        gate.record(RobotsEnforceMode::Observe, &unreachable);
        gate.record(RobotsEnforceMode::Enforce, &denied);
        gate.record(RobotsEnforceMode::Enforce, &unreachable);
        gate.record(RobotsEnforceMode::Enforce, &allowed);
        gate.record(RobotsEnforceMode::Off, &denied);
        gate.record(RobotsEnforceMode::Enforce, &RobotsDecision::NotApplicable);
        assert_eq!(
            gate.stats(),
            RobotsStats {
                allowed: 1,
                denied: 1,
                denied_unreachable: 1,
                would_deny: 1,
                would_deny_unreachable: 1,
            }
        );
    }

    #[test]
    fn total_counts_every_recorded_decision_once_and_nothing_for_off() {
        let gate = gate_over(StubDriver::status(200, PRIVATE_RULES));
        let denied = RobotsDecision::Disallowed { token: "quarry".into(), path: "/p".into() };
        gate.record(RobotsEnforceMode::Observe, &denied);
        gate.record(RobotsEnforceMode::Enforce, &denied);
        gate.record(RobotsEnforceMode::Off, &denied);
        gate.record(RobotsEnforceMode::Observe, &RobotsDecision::NotApplicable);
        assert_eq!(gate.stats().total(), 2, "off and not-applicable count nothing");
    }

    #[test]
    fn the_denial_json_has_the_documented_shape() {
        let denied = RobotsDecision::Disallowed { token: "quarrybot".into(), path: "/private/secret".into() };
        let json = denied.denial_json().unwrap();
        assert_eq!(json["code"], "robots_disallowed");
        assert_eq!(json["user_agent"], "quarrybot");
        assert_eq!(json["path"], "/private/secret");
        assert!(json["message"].as_str().unwrap().contains("/private/secret"));
        assert!(json["remedy"].is_string());
        let unreachable = RobotsDecision::Unreachable { host: "example.test".into() }.denial_json().unwrap();
        assert_eq!(unreachable["code"], "robots_unreachable");
        assert_eq!(unreachable["host"], "example.test");
        assert!(RobotsDecision::Allowed { crawl_delay: None }.denial_json().is_none());
    }

    #[test]
    fn config_defaults_and_garbage() {
        let lookup = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_string())
            }
        };
        let config = RobotsGateConfig::from_lookup(lookup(&[])).unwrap();
        assert_eq!(config, RobotsGateConfig::default());
        assert_eq!(config.mode, RobotsEnforceMode::Observe);
        assert_eq!(config.unreachable, UnreachablePolicy::Disallow);
        let set = RobotsGateConfig::from_lookup(lookup(&[
            (ENV_MODE, "Enforce"),
            (ENV_UNREACHABLE, "allow"),
            (ENV_CRAWL_DELAY_MAX_S, "30"),
            (ENV_CACHE_MAX_ENTRIES, "50"),
            (ENV_FETCH_TIMEOUT_S, "3"),
        ]))
        .unwrap();
        assert_eq!(set.mode, RobotsEnforceMode::Enforce);
        assert_eq!(set.unreachable, UnreachablePolicy::Allow);
        assert_eq!(set.crawl_delay_max, Duration::from_secs(30));
        assert_eq!(set.cache_max_entries, 50);
        assert_eq!(set.fetch_timeout, Duration::from_secs(3));
        for bad in [
            (ENV_MODE, "maybe"),
            (ENV_UNREACHABLE, "ignore"),
            (ENV_CRAWL_DELAY_MAX_S, "61"),
            (ENV_CRAWL_DELAY_MAX_S, "ten"),
            (ENV_CACHE_MAX_ENTRIES, "0"),
            (ENV_FETCH_TIMEOUT_S, "0"),
        ] {
            let err = RobotsGateConfig::from_lookup(move |name: &str| {
                (name == bad.0).then(|| bad.1.to_string())
            })
            .unwrap_err();
            assert!(err.contains(bad.0), "{err}");
        }
    }
}
