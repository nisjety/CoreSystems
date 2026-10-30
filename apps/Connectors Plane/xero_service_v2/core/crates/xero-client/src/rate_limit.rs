//! Per-tenant adaptive rate limiting for the Xero Accounting API.
//!
//! Xero documented limits (per tenant unless noted):
//!   - 60 calls / minute
//!   - daily allowance depends on the app tier (read from response headers)
//!   - 5 concurrent calls per tenant
//!
//! A single in-process [`TenantRateLimiter`] per tenant caps in-flight
//! concurrency, reads response headers to track remaining quota, and
//! proactively sleeps when `min_remaining` is dangerously low. Cooldowns and
//! quota reservations are process-local unless Redis coordination is enabled.
//! Redis coordination is opt-in and fail-closed when unavailable.

mod quota;

use reqwest::header::HeaderMap;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::trace;
use xero_common::{Error, Result};

const PROACTIVE_THROTTLE_THRESHOLD: u32 = 5;
const MAX_CONCURRENT_PER_TENANT: usize = 5;
const DAILY_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const DAILY_PROBE_INTERVAL: Duration = Duration::from_secs(60);
const APP_MINUTE_LIMIT: u32 = 10_000;
const MINUTE_WINDOW: Duration = Duration::from_secs(60);

// ── Local in-process limiter ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
struct RateLimitState {
    min_remaining: Option<u32>,
    day_remaining: Option<u32>,
    day_window_started_at: Option<Instant>,
    next_daily_probe_at: Option<Instant>,
    daily_probe_sent: bool,
    cooldown_until: Option<Instant>,
}

#[derive(Default)]
struct AppBudget {
    state: Mutex<AppBudgetState>,
}

#[derive(Default)]
struct AppBudgetState {
    remaining: Option<u32>,
    window_started_at: Option<Instant>,
    reservations: VecDeque<(u64, Instant)>,
    next_reservation_id: u64,
}

#[derive(Clone, Copy)]
struct AppReservation {
    id: u64,
    prior_remaining: Option<u32>,
    expected_remaining: Option<u32>,
}

#[derive(Clone, Copy)]
struct DailyReservation {
    before: RateLimitState,
    after: RateLimitState,
}

impl AppBudget {
    fn reserve_at(&self, now: Instant) -> Result<AppReservation> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::XeroApi("app quota state poisoned".into()))?;
        while state
            .reservations
            .front()
            .is_some_and(|(_, at)| now >= deadline_after(*at, MINUTE_WINDOW))
        {
            state.reservations.pop_front();
        }
        if app_window_expired(state.window_started_at, now) {
            state.remaining = None;
            state.window_started_at = Some(now);
        }
        if state.reservations.len() >= APP_MINUTE_LIMIT as usize || state.remaining == Some(0) {
            return Err(Error::XeroApi("Xero app minute allowance exhausted".into()));
        }
        let prior_remaining = state.remaining;
        if let Some(remaining) = state.remaining.as_mut() {
            *remaining -= 1;
        }
        let id = state.next_reservation_id;
        state.next_reservation_id = state
            .next_reservation_id
            .checked_add(1)
            .ok_or_else(|| Error::XeroApi("app quota reservation identifier exhausted".into()))?;
        state.reservations.push_back((id, now));
        Ok(AppReservation {
            id,
            prior_remaining,
            expected_remaining: state.remaining,
        })
    }

    fn rollback(&self, reservation: AppReservation) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let existed = state
            .reservations
            .iter()
            .position(|(id, _)| *id == reservation.id)
            .and_then(|index| state.reservations.remove(index))
            .is_some();
        if existed && state.remaining == reservation.expected_remaining {
            state.remaining = reservation.prior_remaining;
        }
    }

    fn observe_at(&self, remaining: u32, now: Instant) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::XeroApi("app quota state poisoned".into()))?;
        if remaining == 0 || app_window_expired(state.window_started_at, now) {
            state.remaining = Some(remaining);
            state.window_started_at = Some(now);
        } else {
            state.remaining = Some(state.remaining.map_or(remaining, |old| old.min(remaining)));
        }
        Ok(())
    }
}

fn same_daily_state(left: &RateLimitState, right: &RateLimitState) -> bool {
    left.day_remaining == right.day_remaining
        && left.day_window_started_at == right.day_window_started_at
        && left.next_daily_probe_at == right.next_daily_probe_at
        && left.daily_probe_sent == right.daily_probe_sent
}

fn restore_daily_state(state: &mut RateLimitState, before: RateLimitState) {
    state.day_remaining = before.day_remaining;
    state.day_window_started_at = before.day_window_started_at;
    state.next_daily_probe_at = before.next_daily_probe_at;
    state.daily_probe_sent = before.daily_probe_sent;
}

fn app_window_expired(started: Option<Instant>, now: Instant) -> bool {
    match started {
        Some(started) => now >= deadline_after(started, MINUTE_WINDOW),
        None => true,
    }
}

pub struct TenantRateLimiter {
    state: Mutex<RateLimitState>,
    in_flight: Arc<Semaphore>,
    tenant_id: String,
    app_budget: Arc<AppBudget>,
    distributed: Option<Arc<quota::RedisQuota>>,
    configuration_error: bool,
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<TenantRateLimiter>>>> = OnceLock::new();
static APP_BUDGET: OnceLock<Arc<AppBudget>> = OnceLock::new();
static DISTRIBUTED: OnceLock<std::result::Result<Option<Arc<quota::RedisQuota>>, ()>> =
    OnceLock::new();

fn configured_quota() -> &'static std::result::Result<Option<Arc<quota::RedisQuota>>, ()> {
    DISTRIBUTED.get_or_init(|| {
        let quota = quota::RedisQuota::from_env().map_err(|_| ())?;
        if let Some(redis) = &quota {
            redis.validate_startup().map_err(|_| ())?;
        }
        Ok(quota.map(Arc::new))
    })
}

/// Validate opt-in Redis configuration and credentials at HTTP-client startup.
/// A failed backend requires restart rather than local-only fallback.
pub(crate) fn validate_configuration() -> Result<()> {
    configured_quota()
        .as_ref()
        .map(|_| ())
        .map_err(|_| Error::Config("Cannot initialize distributed Xero quota coordination".into()))
}

/// Get (or lazily create) the shared limiter for a tenant.
pub fn for_tenant(tenant_id: &str) -> Arc<TenantRateLimiter> {
    let reg = REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = reg.lock().expect("rate-limit registry poisoned");
    map.entry(tenant_id.to_owned())
        .or_insert_with(|| {
            let configured = configured_quota();
            Arc::new(TenantRateLimiter {
                state: Mutex::new(RateLimitState::default()),
                in_flight: Arc::new(Semaphore::new(MAX_CONCURRENT_PER_TENANT)),
                tenant_id: tenant_id.to_owned(),
                app_budget: APP_BUDGET
                    .get_or_init(|| Arc::new(AppBudget::default()))
                    .clone(),
                distributed: configured.as_ref().ok().and_then(Clone::clone),
                configuration_error: configured.is_err(),
            })
        })
        .clone()
}

impl TenantRateLimiter {
    #[cfg(test)]
    fn new(tenant_id: String) -> Self {
        Self {
            state: Mutex::new(RateLimitState::default()),
            in_flight: Arc::new(Semaphore::new(MAX_CONCURRENT_PER_TENANT)),
            tenant_id,
            app_budget: Arc::new(AppBudget::default()),
            distributed: None,
            configuration_error: false,
        }
    }

    /// Block until safe to issue another request. Returns a RAII permit;
    /// in-flight counter is decremented on Drop.
    pub async fn acquire(self: &Arc<Self>) -> Result<AcquiredPermit> {
        if self.configuration_error {
            return Err(Error::XeroApi(
                "distributed quota configuration invalid".into(),
            ));
        }
        let permit = Arc::clone(&self.in_flight)
            .acquire_owned()
            .await
            .map_err(|_| Error::XeroApi("rate limiter closed".into()))?;

        self.wait_for_cooldown().await?;

        // Local proactive throttle from observed min_remaining.
        if let Some(wait) = self.proactive_wait() {
            trace!(tenant = %self.tenant_id, wait_ms = wait.as_millis() as u64, "proactive throttle (min_remaining low)");
            tokio::time::sleep(wait).await;
        }

        self.wait_for_cooldown().await?;

        // Reserve local budgets before the distributed call. That makes every
        // post-Redis path infallible, so a rejected request cannot leave a
        // remote minute/day reservation behind.
        let local = self.reserve_local(Instant::now())?;
        let remote_lease = match &self.distributed {
            Some(redis) => match redis.acquire(&self.tenant_id).await {
                Ok(lease) => Some(lease),
                Err(error) => {
                    self.rollback_local(local);
                    return Err(error);
                }
            },
            None => None,
        };
        Ok(AcquiredPermit {
            _permit: permit,
            _remote_lease: remote_lease,
        })
    }

    async fn wait_for_cooldown(&self) -> Result<()> {
        while let Some(wait) = self.cooldown_remaining_at(Instant::now())? {
            tokio::time::sleep(wait).await;
        }
        Ok(())
    }

    fn cooldown_remaining_at(&self, now: Instant) -> Result<Option<Duration>> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::XeroApi("rate-limit state poisoned".into()))?;
        Ok(state
            .cooldown_until
            .and_then(|deadline| deadline.checked_duration_since(now))
            .filter(|wait| !wait.is_zero()))
    }

    /// Defer every caller for this tenant after a 429, including after the
    /// final retry. Callers should drop their permit and retry through acquire.
    /// Share a 429 cooldown across all processes in the configured cohort.
    pub async fn defer_for(&self, wait: Duration) -> Result<()> {
        self.defer_for_at(wait, Instant::now())?;
        if self.configuration_error {
            return Err(Error::XeroApi(
                "distributed quota configuration invalid".into(),
            ));
        }
        if let Some(redis) = &self.distributed {
            redis.defer(&self.tenant_id, wait).await?;
        }
        Ok(())
    }

    fn defer_for_at(&self, wait: Duration, now: Instant) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::XeroApi("rate-limit state poisoned".into()))?;
        let deadline = deadline_after(now, wait);
        state.cooldown_until = Some(
            state
                .cooldown_until
                .map_or(deadline, |old| old.max(deadline)),
        );
        if state.day_remaining == Some(0) {
            // A 429's Retry-After is a stronger recovery signal than our
            // conservative 24-hour fallback: allow one probe after it expires.
            state.next_daily_probe_at = state.cooldown_until;
        }
        Ok(())
    }

    fn proactive_wait(&self) -> Option<Duration> {
        let s = self.state.lock().ok()?;
        let rem = s.min_remaining?;
        if rem == 0 {
            Some(Duration::from_secs(2))
        } else if rem <= PROACTIVE_THROTTLE_THRESHOLD {
            let factor = PROACTIVE_THROTTLE_THRESHOLD + 1 - rem;
            Some(Duration::from_millis(150u64 * factor as u64))
        } else {
            None
        }
    }

    fn reserve_local(&self, now: Instant) -> Result<(AppReservation, DailyReservation)> {
        let app = self.app_budget.reserve_at(now)?;
        match self.reserve_daily_call_at(now) {
            Ok(daily) => Ok((app, daily)),
            Err(error) => {
                self.app_budget.rollback(app);
                Err(error)
            }
        }
    }

    fn rollback_local(&self, reservation: (AppReservation, DailyReservation)) {
        self.rollback_daily(reservation.1);
        self.app_budget.rollback(reservation.0);
    }

    fn reserve_daily_call_at(&self, now: Instant) -> Result<DailyReservation> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::XeroApi("rate-limit state poisoned".into()))?;
        let before = *state;
        if let Some(remaining) = state.day_remaining {
            if remaining == 0 {
                if state.next_daily_probe_at.is_some_and(|at| now >= at) {
                    state.next_daily_probe_at = Some(deadline_after(now, DAILY_PROBE_INTERVAL));
                    state.daily_probe_sent = true;
                    return Ok(DailyReservation {
                        before,
                        after: *state,
                    });
                }
                return Err(Error::XeroApi(format!(
                    "daily Xero API allowance exhausted for tenant {}; resume in a later quota window",
                    self.tenant_id
                )));
            }
            state.day_remaining = Some(remaining - 1);
            if remaining == 1 && state.next_daily_probe_at.is_none() {
                let started = state.day_window_started_at.unwrap_or(now);
                state.next_daily_probe_at = Some(deadline_after(started, DAILY_WINDOW));
            }
        }
        Ok(DailyReservation {
            before,
            after: *state,
        })
    }

    fn rollback_daily(&self, reservation: DailyReservation) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if same_daily_state(&state, &reservation.after) {
            restore_daily_state(&mut state, reservation.before);
        }
    }

    /// Update local and distributed observed budgets after every response.
    pub async fn update_from_headers(&self, headers: &HeaderMap) -> Result<()> {
        self.update_from_headers_at(headers, Instant::now());
        if self.configuration_error {
            return Err(Error::XeroApi(
                "distributed quota configuration invalid".into(),
            ));
        }
        if let Some(redis) = &self.distributed {
            redis
                .observe(
                    &self.tenant_id,
                    parse_header_u32(headers, "x-minlimit-remaining"),
                    parse_header_u32(headers, "x-daylimit-remaining"),
                    parse_header_u32(headers, "x-appminlimit-remaining"),
                )
                .await?;
        }
        Ok(())
    }

    fn update_from_headers_at(&self, headers: &HeaderMap, now: Instant) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        if let Some(v) = parse_header_u32(headers, "x-minlimit-remaining") {
            state.min_remaining = Some(v);
        }
        if let Some(v) = parse_header_u32(headers, "x-daylimit-remaining") {
            let probe_sent = state.daily_probe_sent;
            let window_expired = state
                .day_window_started_at
                .is_some_and(|started| now >= deadline_after(started, DAILY_WINDOW));
            if state.day_window_started_at.is_none() || window_expired || state.daily_probe_sent {
                state.day_remaining = Some(v);
                state.day_window_started_at = Some(now);
            } else {
                state.day_remaining = Some(state.day_remaining.map_or(v, |current| current.min(v)));
            }
            state.daily_probe_sent = false;
            state.next_daily_probe_at = if state.day_remaining == Some(0) {
                let fallback =
                    deadline_after(state.day_window_started_at.unwrap_or(now), DAILY_WINDOW);
                if probe_sent {
                    state.next_daily_probe_at.or(Some(fallback))
                } else {
                    state
                        .cooldown_until
                        .filter(|until| *until > now)
                        .or(Some(fallback))
                }
            } else {
                None
            };
        }
        drop(state);
        if let Some(v) = parse_header_u32(headers, "x-appminlimit-remaining") {
            let _ = self.app_budget.observe_at(v, now);
        }
    }
}

fn deadline_after(now: Instant, wait: Duration) -> Instant {
    now.checked_add(wait).unwrap_or_else(|| now + DAILY_WINDOW)
}

pub struct AcquiredPermit {
    _permit: OwnedSemaphorePermit,
    _remote_lease: Option<quota::RemoteLease>,
}

fn parse_header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn configuration_child() {
        let Ok(expected) = std::env::var("XERO_QUOTA_CONFIG_CHILD") else {
            return;
        };
        let result = crate::validate_quota_configuration();
        if expected == "valid" {
            assert!(result.is_ok());
        } else {
            let error = result.expect_err("invalid configuration accepted");
            let message = error.to_string();
            assert!(message.contains("quota coordination"));
            assert!(!message.contains("secret"));
        }
    }

    fn run_configuration_child(url: Option<&str>, app_key: Option<&str>, expected: &str) {
        let executable = std::env::current_exe().unwrap();
        let mut command = std::process::Command::new(executable);
        command.args([
            "--exact",
            "rate_limit::tests::configuration_child",
            "--nocapture",
        ]);
        command.env("XERO_QUOTA_CONFIG_CHILD", expected);
        command.env_remove("XERO_REDIS_URL");
        command.env_remove("XERO_REDIS_APP_KEY");
        if let Some(url) = url {
            command.env("XERO_REDIS_URL", url);
        }
        if let Some(app_key) = app_key {
            command.env("XERO_REDIS_APP_KEY", app_key);
        }
        let output = command.output().unwrap();
        let transcript = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success(),
            "child validation failed: {transcript}"
        );
        assert!(!transcript.contains("secret"));
    }

    #[test]
    fn startup_rejects_bad_configuration_without_exposing_credentials() {
        run_configuration_child(Some("redis://:secret@127.0.0.1:1/"), None, "invalid");
        run_configuration_child(
            Some("invalid://:secret@localhost"),
            Some("cohort"),
            "invalid",
        );
        run_configuration_child(None, None, "valid");
    }

    #[test]
    fn startup_rejects_bad_redis_credentials_before_first_query() {
        let Ok(url) = std::env::var("XERO_TEST_REDIS_URL") else {
            return;
        };
        run_configuration_child(Some(&url), Some("startup-fixture"), "valid");
        let bad_url = url.replacen("redis://", "redis://:secret@", 1);
        run_configuration_child(Some(&bad_url), Some("startup-fixture"), "invalid");
    }

    #[test]
    fn app_budget_is_shared_by_tenants_and_recovers_after_minute() {
        let budget = AppBudget::default();
        let start = Instant::now();
        budget.observe_at(1, start).unwrap();
        budget.reserve_at(start).unwrap();
        assert!(budget.reserve_at(start).is_err());
        budget.reserve_at(start + Duration::from_secs(61)).unwrap();
    }

    #[test]
    fn app_budget_enforces_rolling_minute_without_headers() {
        let budget = AppBudget::default();
        let start = Instant::now();
        for _ in 0..APP_MINUTE_LIMIT {
            budget.reserve_at(start).unwrap();
        }
        assert!(budget.reserve_at(start + Duration::from_secs(59)).is_err());
        budget.reserve_at(start + Duration::from_secs(60)).unwrap();
    }

    #[test]
    fn test_only_limiters_have_isolated_app_budgets() {
        let a = TenantRateLimiter::new("fixture-a".into());
        let b = TenantRateLimiter::new("fixture-b".into());
        assert!(!Arc::ptr_eq(&a.app_budget, &b.app_budget));
        if let Some(global) = APP_BUDGET.get() {
            assert!(!Arc::ptr_eq(&a.app_budget, global));
        }
    }

    #[test]
    fn late_zero_header_keeps_app_gate_closed_for_full_minute() {
        let budget = AppBudget::default();
        let start = Instant::now();
        budget.observe_at(2, start).unwrap();
        budget
            .observe_at(0, start + Duration::from_secs(59))
            .unwrap();
        assert!(budget.reserve_at(start + Duration::from_secs(60)).is_err());
        budget.reserve_at(start + Duration::from_secs(119)).unwrap();
    }

    #[test]
    fn redis_configuration_requires_explicit_cohort_and_hides_url() {
        assert!(
            quota::RedisQuota::from_settings(Some("redis://secret@localhost/"), None)
                .unwrap_err()
                .to_string()
                .contains("cohort")
        );
        assert!(
            !quota::RedisQuota::from_settings(Some("redis://secret@localhost/"), None)
                .unwrap_err()
                .to_string()
                .contains("secret")
        );
    }

    fn headers_with_daily(remaining: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-daylimit-remaining",
            HeaderValue::from_str(remaining).unwrap(),
        );
        headers
    }

    #[test]
    fn exhausted_daily_budget_recovers_with_one_probe_after_window() {
        let limiter = TenantRateLimiter::new("daily-recovery".into());
        let start = std::time::Instant::now();
        limiter.update_from_headers_at(&headers_with_daily("0"), start);
        assert!(limiter
            .reserve_daily_call_at(start + Duration::from_secs(23 * 60 * 60))
            .is_err());
        let probe_time = start + Duration::from_secs(24 * 60 * 60);
        assert!(limiter.reserve_daily_call_at(probe_time).is_ok());
        assert!(limiter.reserve_daily_call_at(probe_time).is_err());
        limiter.update_from_headers_at(&headers_with_daily("4"), probe_time);
        assert!(limiter.reserve_daily_call_at(probe_time).is_ok());
    }

    #[test]
    fn retry_after_is_shared_and_allows_a_single_daily_probe() {
        let limiter = TenantRateLimiter::new("retry-after".into());
        let start = std::time::Instant::now();
        limiter.update_from_headers_at(&headers_with_daily("0"), start);
        limiter
            .defer_for_at(Duration::from_secs(30), start)
            .unwrap();
        assert_eq!(
            limiter
                .cooldown_remaining_at(start + Duration::from_secs(10))
                .unwrap(),
            Some(Duration::from_secs(20))
        );
        limiter.defer_for_at(Duration::from_secs(5), start).unwrap();
        assert!(limiter
            .reserve_daily_call_at(start + Duration::from_secs(29))
            .is_err());
        assert!(limiter
            .reserve_daily_call_at(start + Duration::from_secs(30))
            .is_ok());
        assert!(limiter
            .reserve_daily_call_at(start + Duration::from_secs(30))
            .is_err());
    }

    #[tokio::test]
    async fn shared_cooldown_blocks_another_caller_before_network() {
        let limiter = Arc::new(TenantRateLimiter::new("shared-cooldown".into()));
        limiter.defer_for(Duration::from_secs(5)).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), limiter.acquire())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn elapsed_cooldown_resumes_without_losing_daily_reservations() {
        let limiter = Arc::new(TenantRateLimiter::new("cooldown-resume".into()));
        limiter
            .update_from_headers(&headers_with_daily("2"))
            .await
            .unwrap();
        limiter.defer_for(Duration::from_millis(5)).await.unwrap();
        drop(
            tokio::time::timeout(Duration::from_secs(1), limiter.acquire())
                .await
                .unwrap()
                .unwrap(),
        );
        assert_eq!(limiter.state.lock().unwrap().day_remaining, Some(1));
        drop(limiter.acquire().await.unwrap());
        assert!(limiter.acquire().await.is_err());
    }

    #[test]
    fn stale_headers_cannot_refill_budget_and_failed_probes_remain_bounded() {
        let limiter = TenantRateLimiter::new("probe-recovery".into());
        let start = Instant::now();
        limiter.update_from_headers_at(&headers_with_daily("2"), start);
        limiter.reserve_daily_call_at(start).unwrap();
        limiter.update_from_headers_at(&headers_with_daily("9"), start);
        assert_eq!(limiter.state.lock().unwrap().day_remaining, Some(1));
        limiter.reserve_daily_call_at(start).unwrap();
        limiter
            .defer_for_at(Duration::from_secs(30), start)
            .unwrap();
        let first = start + Duration::from_secs(30);
        limiter.reserve_daily_call_at(first).unwrap();
        limiter.update_from_headers_at(&headers_with_daily("0"), first);
        assert!(limiter
            .reserve_daily_call_at(first + Duration::from_secs(59))
            .is_err());
        let second = first + DAILY_PROBE_INTERVAL;
        limiter.reserve_daily_call_at(second).unwrap();
        limiter.update_from_headers_at(&headers_with_daily("3"), second);
        assert_eq!(limiter.state.lock().unwrap().day_remaining, Some(3));
        limiter.reserve_daily_call_at(second).unwrap();
    }

    #[tokio::test]
    async fn closed_or_poisoned_limiter_fails_before_network() {
        let closed = Arc::new(TenantRateLimiter::new("closed".into()));
        closed.in_flight.close();
        assert!(closed.acquire().await.is_err());
        let poisoned = Arc::new(TenantRateLimiter::new("poisoned".into()));
        let state = &poisoned.state;
        assert!(std::panic::catch_unwind(|| {
            let _guard = state.lock().unwrap();
            panic!("fixture poisoned state");
        })
        .is_err());
        assert!(poisoned.acquire().await.is_err());
        assert!(poisoned.defer_for(Duration::from_secs(1)).await.is_err());
        assert!(poisoned.reserve_daily_call_at(Instant::now()).is_err());
        assert_eq!(poisoned.proactive_wait(), None);
        assert!(poisoned
            .update_from_headers(&headers_with_daily("100"))
            .await
            .is_ok());
        assert!(poisoned.acquire().await.is_err());
    }

    #[test]
    fn registry_returns_same_arc_for_same_tenant() {
        let a = for_tenant("t1");
        let b = for_tenant("t1");
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn registry_returns_distinct_for_different_tenants() {
        let a = for_tenant("t-a");
        let b = for_tenant("t-b");
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn update_from_headers_parses_known_keys() {
        let lim = TenantRateLimiter::new("unit".into());
        let mut h = HeaderMap::new();
        h.insert("x-minlimit-remaining", HeaderValue::from_static("3"));
        h.insert("x-daylimit-remaining", HeaderValue::from_static("4990"));
        h.insert("x-appminlimit-remaining", HeaderValue::from_static("9"));
        lim.update_from_headers_at(&h, Instant::now());
        let s = lim.state.lock().expect("state poisoned");
        assert_eq!(s.min_remaining, Some(3));
        assert_eq!(s.day_remaining, Some(4990));
        assert_eq!(lim.app_budget.state.lock().unwrap().remaining, Some(9));
    }

    #[test]
    fn proactive_wait_kicks_in_below_threshold() {
        let lim = TenantRateLimiter::new("unit".into());
        let mut h = HeaderMap::new();
        h.insert("x-minlimit-remaining", HeaderValue::from_static("2"));
        lim.update_from_headers_at(&h, Instant::now());
        assert!(lim.proactive_wait().is_some());
    }

    #[test]
    fn proactive_wait_silent_above_threshold() {
        let lim = TenantRateLimiter::new("unit".into());
        let mut h = HeaderMap::new();
        h.insert("x-minlimit-remaining", HeaderValue::from_static("30"));
        lim.update_from_headers_at(&h, Instant::now());
        assert!(lim.proactive_wait().is_none());
    }

    #[tokio::test]
    async fn sixth_request_waits_for_a_permit() {
        let lim = Arc::new(TenantRateLimiter::new("concurrency".into()));
        let mut permits = Vec::new();
        for _ in 0..5 {
            permits.push(lim.acquire().await.unwrap());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), lim.acquire())
                .await
                .is_err()
        );
        permits.pop();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), lim.acquire())
                .await
                .unwrap()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn observed_daily_quota_is_reserved_before_a_request() {
        let lim = Arc::new(TenantRateLimiter::new("daily".into()));
        let mut headers = HeaderMap::new();
        headers.insert("x-daylimit-remaining", HeaderValue::from_static("1"));
        lim.update_from_headers(&headers).await.unwrap();
        drop(lim.acquire().await.unwrap());
        let error = lim.acquire().await.err().expect("daily budget exhausted");
        assert!(error.to_string().contains("daily"));
    }

    #[tokio::test]
    async fn closed_app_gate_does_not_consume_tenant_daily_budget() {
        let lim = Arc::new(TenantRateLimiter::new("app-gated".into()));
        let mut headers = HeaderMap::new();
        headers.insert("x-daylimit-remaining", HeaderValue::from_static("2"));
        headers.insert("x-appminlimit-remaining", HeaderValue::from_static("0"));
        lim.update_from_headers(&headers).await.unwrap();
        assert!(lim.acquire().await.is_err());
        assert_eq!(lim.state.lock().unwrap().day_remaining, Some(2));
    }

    #[tokio::test]
    async fn exhausted_tenant_does_not_spend_a_shared_app_reservation() {
        let shared_app = Arc::new(AppBudget::default());
        shared_app.observe_at(1, Instant::now()).unwrap();
        let mut exhausted = TenantRateLimiter::new("exhausted".into());
        exhausted.app_budget = shared_app.clone();
        exhausted.update_from_headers_at(&headers_with_daily("0"), Instant::now());
        let mut healthy = TenantRateLimiter::new("healthy".into());
        healthy.app_budget = shared_app;

        assert!(Arc::new(exhausted).acquire().await.is_err());
        assert!(Arc::new(healthy).acquire().await.is_ok());
    }

    #[test]
    fn failed_remote_reservation_rolls_back_local_budgets() {
        let app_budget = Arc::new(AppBudget::default());
        app_budget.observe_at(1, Instant::now()).unwrap();
        let limiter = TenantRateLimiter {
            state: Mutex::new(RateLimitState {
                day_remaining: Some(1),
                ..RateLimitState::default()
            }),
            in_flight: Arc::new(Semaphore::new(MAX_CONCURRENT_PER_TENANT)),
            tenant_id: "rollback".into(),
            app_budget: app_budget.clone(),
            distributed: None,
            configuration_error: false,
        };
        let local = limiter.reserve_local(Instant::now()).unwrap();
        limiter.rollback_local(local);
        assert_eq!(limiter.state.lock().unwrap().day_remaining, Some(1));
        assert_eq!(app_budget.state.lock().unwrap().remaining, Some(1));
        assert!(app_budget.state.lock().unwrap().reservations.is_empty());
    }
}
