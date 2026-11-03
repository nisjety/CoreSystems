//! Retry/block/escalation policy executor. Enforces `RunPolicy::{retry, block}`.
//!
//! E3.8.3: the synchronous scrape path runs its fetches through
//! [`execute_with_retry`], behind `QUARRY_EDGE__SCRAPE_RETRIES` (off by default).
//! It uses one merged config ([`ScrapeRetry`]), jittered waits, `Retry-After`
//! honoured, and no retry that could reach a proxy beyond the egress broker's
//! bounded plan (ADR-5).

use std::collections::hash_map::{DefaultHasher, RandomState};
use std::future::Future;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::{sleep, Instant};
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::output::DriverKind;
use quarry_core::policy::{BackoffKind, Retry};
use quarry_tls::TlsProfile;

use crate::driver::{BrowserMeta, Driver, FetchHints, ScrapeCapabilities};
use crate::fetch::FetchResponse;
use crate::policy::RetryPolicy;

/// Extra attempts the sync scrape path may add after the first. Two is the most
/// E3.8.1 allows a DNS fault, and it matches the attempt cap of `RetryPolicy`.
pub const MAX_SCRAPE_RETRIES: u32 = 2;

/// The longest one retry wait may be, `Retry-After` included. A longer ask ends
/// the retries instead of holding the request.
pub const MAX_RETRY_WAIT_MS: u64 = 10_000;

/// The environment flag the sync scrape path reads its retry count from.
pub const SCRAPE_RETRIES_ENV: &str = "QUARRY_EDGE__SCRAPE_RETRIES";

/// The backoff curve of the sync scrape path: exponential from 500 ms. Its `max`
/// is the most attempts the curve allows. [`ScrapeRetry::merge`] takes the smaller
/// of that and the classifier's own cap.
pub const SCRAPE_CURVE: Retry = Retry {
    max: MAX_SCRAPE_RETRIES + 1,
    backoff: BackoffKind::Exp,
    base_ms: 500,
};

pub fn backoff_delay(retry: &Retry, attempt: u32) -> Duration {
    Duration::from_millis(delay_ms(retry.backoff, u64::from(retry.base_ms), attempt))
}

pub async fn wait(retry: &Retry, attempt: u32) {
    sleep(backoff_delay(retry, attempt)).await;
}

/// The delay of the `attempt`th wait (0 for the first) on a backoff curve.
fn delay_ms(backoff: BackoffKind, base_ms: u64, attempt: u32) -> u64 {
    match backoff {
        BackoffKind::Fixed => base_ms,
        BackoffKind::Linear => base_ms.saturating_mul(u64::from(attempt) + 1),
        BackoffKind::Exp => base_ms.saturating_mul(1u64 << attempt.min(10)),
    }
}

/// Classification of an error for retry purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    /// Transient — safe to retry.
    Transient,
    /// Permanent — do not retry.
    Permanent,
}

/// Classify an error into retry classes based on its `retryable` flag.
pub fn classify(err: &QuarryError) -> RetryClass {
    if err.retryable {
        RetryClass::Transient
    } else {
        RetryClass::Permanent
    }
}

/// The one retry config the synchronous scrape path reads (E3.8.3). It merges
/// the two shapes that carried retry knobs before: `quarry_core::policy::Retry`
/// (the backoff curve and its attempt cap) and `policy::RetryPolicy` (its own
/// attempt cap and the HTTP codes it calls transient). Neither source type
/// changes, so no policy fingerprint moves.
#[derive(Debug, Clone)]
pub struct ScrapeRetry {
    /// Attempts including the first. `1` means no retry.
    pub max_attempts: u32,
    /// The backoff curve, from `Retry::backoff`.
    pub backoff: BackoffKind,
    /// The base wait in ms, from `Retry::base_ms`.
    pub base_ms: u64,
    /// The longest one wait may be, `Retry-After` included.
    pub max_wait_ms: u64,
    /// Equal jitter: each wait is drawn from `[delay / 2, delay]`.
    pub jitter: bool,
    /// Status codes of a response that are retried, from `RetryPolicy::transient_codes`.
    pub transient_codes: Vec<u16>,
}

impl ScrapeRetry {
    /// Merge the curve and the classifier. `retries` is the extra attempts asked
    /// for, clamped to [`MAX_SCRAPE_RETRIES`]. The attempts never exceed the
    /// smaller of the two sources' caps.
    pub fn merge(curve: &Retry, classifier: &RetryPolicy, retries: u32) -> Self {
        let cap = curve.max.min(classifier.max_attempts).max(1);
        Self {
            max_attempts: (1 + retries.min(MAX_SCRAPE_RETRIES)).min(cap),
            backoff: curve.backoff,
            base_ms: u64::from(curve.base_ms),
            max_wait_ms: MAX_RETRY_WAIT_MS,
            jitter: true,
            transient_codes: classifier.transient_codes.clone(),
        }
    }
}

/// `QUARRY_EDGE__SCRAPE_RETRIES` as the sync scrape path reads it: the extra
/// attempts after the first. Unset, blank, not a whole number or `0` is off
/// (`None`). A value above [`MAX_SCRAPE_RETRIES`] is clamped to it.
pub fn scrape_retry_from_flag(raw: Option<&str>) -> Option<ScrapeRetry> {
    let retries = raw
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0)
        .min(MAX_SCRAPE_RETRIES);
    (retries > 0).then(|| ScrapeRetry::merge(&SCRAPE_CURVE, &RetryPolicy::default(), retries))
}

/// Whether the scrape path may try a failed fetch again (E3.8.3). The error's own
/// `retryable` flag decides, with two exceptions. A block is never retried
/// (ADR-5: a block is classified and refused, never escaped). `INTERNAL` is a
/// fault in this process or its configuration, which a repeat does not clear.
pub fn scrape_retryable(err: &QuarryError) -> bool {
    classify(err) == RetryClass::Transient
        && !matches!(err.code, ErrorCode::UpstreamBlocked | ErrorCode::Internal)
}

/// Whether a failure went through a proxy identity: any `"egress": "proxy"`
/// record in its details, `egress_attempts` or nested. The egress broker already
/// rotates within its bounded plan, so a retry would reach proxies beyond that
/// plan. Such a failure is final on the scrape path (ADR-5).
pub fn routed_through_proxy(err: &QuarryError) -> bool {
    err.details.as_ref().is_some_and(mentions_proxy_egress)
}

fn mentions_proxy_egress(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.get("egress").and_then(serde_json::Value::as_str) == Some("proxy")
                || map.values().any(mentions_proxy_egress)
        }
        serde_json::Value::Array(items) => items.iter().any(mentions_proxy_egress),
        _ => false,
    }
}

/// `Retry-After` in delta-seconds, from a response's headers. An HTTP-date is not
/// read, so it gives `None` and the backoff curve decides the wait.
fn retry_after_from_headers(headers: &[(String, String)]) -> Option<u64> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, value)| value.trim().parse::<u64>().ok())
}

/// `details.retry_after_seconds` of an error, when the code that raised it set one.
fn retry_after_from_error(err: &QuarryError) -> Option<u64> {
    err.details.as_ref()?.get("retry_after_seconds")?.as_u64()
}

/// Equal jitter: a random wait in `[ms / 2, ms]`. The randomness comes from the
/// std hasher's per-instance keys, so no RNG crate is needed.
fn jitter_ms(ms: u64) -> u64 {
    let half = ms / 2;
    if half == 0 {
        return ms;
    }
    let mut hasher = RandomState::new().build_hasher();
    ms.hash(&mut hasher);
    half + hasher.finish() % (half + 1)
}

/// The wait before the next try, or `None` when the retries end here: the wait
/// would exceed `max_wait_ms`. A `Retry-After` longer than the backoff raises the
/// wait to that many seconds.
fn wait_ms(cfg: &ScrapeRetry, index: u32, retry_after_s: Option<u64>) -> Option<u64> {
    let curve = delay_ms(cfg.backoff, cfg.base_ms, index).min(cfg.max_wait_ms);
    let paced = if cfg.jitter { jitter_ms(curve) } else { curve };
    let wait = match retry_after_s {
        Some(seconds) => paced.max(seconds.saturating_mul(1000)),
        None => paced,
    };
    (wait <= cfg.max_wait_ms).then_some(wait)
}

/// Run one fetch under the scrape retry config (E3.8.3). `call` makes one try
/// through the driver. It is called again only when the outcome is retryable: a
/// response whose status is in `transient_codes`, or a failure that
/// [`scrape_retryable`] accepts and that did not go through a proxy.
///
/// The last outcome is returned when the attempts run out, when the wait would
/// pass `until` (the caller's deadline), or when the origin asks for longer than
/// `max_wait_ms`. A response stays a response and is never turned into an error,
/// so the caller sees what it would have seen without retries.
pub async fn execute_with_retry<F, Fut>(
    cfg: &ScrapeRetry,
    until: Option<Instant>,
    mut call: F,
) -> QuarryResult<FetchResponse>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = QuarryResult<FetchResponse>>,
{
    let mut made: u32 = 0;
    loop {
        let outcome = call().await;
        made += 1;
        let decision = match &outcome {
            Ok(resp) if cfg.transient_codes.contains(&resp.status) => {
                Some(retry_after_from_headers(&resp.headers))
            }
            Err(err) if scrape_retryable(err) && !routed_through_proxy(err) => {
                Some(retry_after_from_error(err))
            }
            _ => None,
        };
        let Some(retry_after_s) = decision else {
            return outcome;
        };
        if made >= cfg.max_attempts {
            return outcome;
        }
        let Some(pause) = wait_ms(cfg, made - 1, retry_after_s) else {
            return outcome;
        };
        if until.is_some_and(|until| Instant::now() + Duration::from_millis(pause) >= until) {
            return outcome;
        }
        tracing::debug!(attempt = made, wait_ms = pause, "scrape fetch retried");
        sleep(Duration::from_millis(pause)).await;
    }
}

/// A driver whose fetches run under a [`ScrapeRetry`] (E3.8.3). Only the two fetch
/// entry points retry. Every other method delegates to the inner driver, so the
/// page pipeline sees the same kind, capabilities and browser metadata as without
/// the wrapper.
pub struct RetryingDriver {
    inner: Arc<dyn Driver>,
    cfg: ScrapeRetry,
    until: Option<Instant>,
}

impl RetryingDriver {
    /// `inner` with its fetches retried under `cfg`. No retry waits past `until`.
    pub fn wrap(
        inner: Arc<dyn Driver>,
        cfg: ScrapeRetry,
        until: Option<Instant>,
    ) -> Arc<dyn Driver> {
        Arc::new(Self { inner, cfg, until })
    }
}

#[async_trait]
impl Driver for RetryingDriver {
    fn kind(&self) -> DriverKind {
        self.inner.kind()
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        execute_with_retry(&self.cfg, self.until, || self.inner.fetch(url)).await
    }

    async fn fetch_bounded(&self, url: &Url, max_bytes: usize) -> QuarryResult<FetchResponse> {
        self.inner.fetch_bounded(url, max_bytes).await
    }

    async fn fetch_discovery_hop(
        &self,
        url: &Url,
        max_bytes: usize,
    ) -> QuarryResult<FetchResponse> {
        self.inner.fetch_discovery_hop(url, max_bytes).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        execute_with_retry(&self.cfg, self.until, || {
            self.inner.fetch_conditional(url, hints)
        })
        .await
    }

    fn tls_profile(&self) -> Option<TlsProfile> {
        self.inner.tls_profile()
    }

    fn browser_meta(&self) -> Option<BrowserMeta> {
        self.inner.browser_meta()
    }

    async fn browser_health(&self) -> Option<quarry_browser::BrowserHealth> {
        self.inner.browser_health().await
    }

    fn scrape_capabilities(&self) -> ScrapeCapabilities {
        self.inner.scrape_capabilities()
    }
}

/// Idempotency key, used to de-duplicate retried operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    /// Build a stable key for a fetch operation.
    pub fn for_fetch(url: &Url, method: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        method.hash(&mut hasher);
        url.as_str().hash(&mut hasher);
        Self(format!("{:016x}", hasher.finish()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarry_core::error::driver_failed_reason;
    use quarry_core::output::DriverKind;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::time::Instant;

    struct AlwaysFails(ErrorCode);

    #[async_trait]
    impl Driver for AlwaysFails {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            Err(QuarryError::new(self.0, "mock failure"))
        }
    }

    struct FailsNThenOk {
        remaining: AtomicU32,
    }

    #[async_trait]
    impl Driver for FailsNThenOk {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            let prev = self.remaining.fetch_sub(1, Ordering::SeqCst);
            if prev > 0 {
                Err(QuarryError::new(ErrorCode::Timeout, "transient"))
            } else {
                Ok(page(200, vec![], url))
            }
        }
    }

    /// Fails with the same error on every call and counts the calls.
    struct FailsWith {
        error: QuarryError,
        calls: AtomicU32,
    }

    impl FailsWith {
        fn new(error: QuarryError) -> Self {
            Self {
                error,
                calls: AtomicU32::new(0),
            }
        }

        fn calls(&self) -> u32 {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Driver for FailsWith {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(self.error.clone())
        }
    }

    /// Answers every call with the same status and headers and counts the calls.
    struct AnswersWith {
        status: u16,
        headers: Vec<(String, String)>,
        calls: AtomicU32,
    }

    #[async_trait]
    impl Driver for AnswersWith {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(page(self.status, self.headers.clone(), url))
        }
    }

    fn page(status: u16, headers: Vec<(String, String)>, url: &Url) -> FetchResponse {
        FetchResponse {
            status,
            final_url: url.clone(),
            headers,
            body: b"page".to_vec(),
            duration_ms: 1,
            served_by: DriverKind::Static,
            captures: None,
        }
    }

    fn sample_url() -> Url {
        Url::parse("https://example.com/").unwrap()
    }

    /// A config with no real waits, so the tests run at once.
    fn retry_cfg(max_attempts: u32, transient_codes: Vec<u16>) -> ScrapeRetry {
        ScrapeRetry {
            max_attempts,
            backoff: BackoffKind::Fixed,
            base_ms: 0,
            max_wait_ms: MAX_RETRY_WAIT_MS,
            jitter: false,
            transient_codes,
        }
    }

    #[test]
    fn classify_permanent_vs_transient() {
        let perm = QuarryError::new(ErrorCode::BadRequest, "bad");
        let trans = QuarryError::new(ErrorCode::Timeout, "slow");
        assert_eq!(classify(&perm), RetryClass::Permanent);
        assert_eq!(classify(&trans), RetryClass::Transient);
    }

    #[test]
    fn backoff_delay_matches_kind() {
        let fixed = Retry {
            max: 3,
            backoff: BackoffKind::Fixed,
            base_ms: 10,
        };
        assert_eq!(backoff_delay(&fixed, 0), Duration::from_millis(10));
        assert_eq!(backoff_delay(&fixed, 5), Duration::from_millis(10));

        let linear = Retry {
            max: 3,
            backoff: BackoffKind::Linear,
            base_ms: 10,
        };
        assert_eq!(backoff_delay(&linear, 0), Duration::from_millis(10));
        assert_eq!(backoff_delay(&linear, 2), Duration::from_millis(30));

        let exp = Retry {
            max: 3,
            backoff: BackoffKind::Exp,
            base_ms: 10,
        };
        assert_eq!(backoff_delay(&exp, 0), Duration::from_millis(10));
        assert_eq!(backoff_delay(&exp, 3), Duration::from_millis(80));
    }

    #[test]
    fn idempotency_key_is_stable() {
        let url = Url::parse("https://example.com/a").unwrap();
        let k1 = IdempotencyKey::for_fetch(&url, "GET");
        let k2 = IdempotencyKey::for_fetch(&url, "GET");
        let k3 = IdempotencyKey::for_fetch(&url, "POST");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[tokio::test]
    async fn permanent_error_is_not_retried() {
        let driver = AlwaysFails(ErrorCode::BadRequest);
        let url = sample_url();
        let err = execute_with_retry(&retry_cfg(5, vec![]), None, || driver.fetch(&url))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn transient_error_exhausts_after_max() {
        let driver = FailsWith::new(QuarryError::new(ErrorCode::Timeout, "slow"));
        let url = sample_url();
        let err = execute_with_retry(&retry_cfg(3, vec![]), None, || driver.fetch(&url))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Timeout);
        assert_eq!(driver.calls(), 3);
    }

    #[tokio::test]
    async fn transient_then_success() {
        let driver = FailsNThenOk {
            remaining: AtomicU32::new(1),
        };
        let url = sample_url();
        let resp = execute_with_retry(&retry_cfg(3, vec![]), None, || driver.fetch(&url))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn merged_config_takes_the_smaller_cap_and_the_flag_count() {
        let curve = Retry {
            max: 3,
            backoff: BackoffKind::Exp,
            base_ms: 500,
        };
        let classifier = RetryPolicy::default();
        assert_eq!(ScrapeRetry::merge(&curve, &classifier, 0).max_attempts, 1);
        assert_eq!(ScrapeRetry::merge(&curve, &classifier, 1).max_attempts, 2);
        assert_eq!(ScrapeRetry::merge(&curve, &classifier, 2).max_attempts, 3);
        let short = RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        };
        assert_eq!(ScrapeRetry::merge(&curve, &short, 2).max_attempts, 2);
        let narrow = Retry {
            max: 1,
            backoff: BackoffKind::Fixed,
            base_ms: 10,
        };
        assert_eq!(ScrapeRetry::merge(&narrow, &classifier, 2).max_attempts, 1);
        let merged = ScrapeRetry::merge(&curve, &classifier, 2);
        assert_eq!(merged.base_ms, 500);
        assert!(merged.jitter);
        assert!(merged.transient_codes.contains(&503));
    }

    #[test]
    fn the_flag_is_off_by_default_and_clamped() {
        assert!(scrape_retry_from_flag(None).is_none());
        assert!(scrape_retry_from_flag(Some("")).is_none());
        assert!(scrape_retry_from_flag(Some("0")).is_none());
        assert!(scrape_retry_from_flag(Some("on")).is_none());
        assert_eq!(
            scrape_retry_from_flag(Some(" 1 ")).map(|cfg| cfg.max_attempts),
            Some(2)
        );
        assert_eq!(
            scrape_retry_from_flag(Some("9")).map(|cfg| cfg.max_attempts),
            Some(3)
        );
    }

    #[test]
    fn retry_after_sets_the_wait_and_a_long_one_ends_the_retries() {
        let cfg = ScrapeRetry {
            max_attempts: 3,
            backoff: BackoffKind::Fixed,
            base_ms: 100,
            max_wait_ms: 10_000,
            jitter: false,
            transient_codes: vec![],
        };
        assert_eq!(wait_ms(&cfg, 0, None), Some(100));
        assert_eq!(wait_ms(&cfg, 0, Some(3)), Some(3_000));
        assert_eq!(wait_ms(&cfg, 0, Some(11)), None);
        let headers = vec![("Retry-After".to_string(), " 7 ".to_string())];
        assert_eq!(retry_after_from_headers(&headers), Some(7));
        let dated = vec![(
            "retry-after".to_string(),
            "Wed, 21 Oct 2026 07:28:00 GMT".to_string(),
        )];
        assert_eq!(retry_after_from_headers(&dated), None);
    }

    #[test]
    fn jitter_stays_between_half_and_the_full_delay() {
        for ms in [0u64, 1, 2, 999, 1_000] {
            for _ in 0..64 {
                let waited = jitter_ms(ms);
                assert!(
                    ms / 2 <= waited && waited <= ms,
                    "{ms} jittered to {waited}"
                );
            }
        }
    }

    #[tokio::test]
    async fn transient_statuses_are_retried_and_the_last_response_is_returned() {
        let driver = AnswersWith {
            status: 503,
            headers: vec![],
            calls: AtomicU32::new(0),
        };
        let url = sample_url();
        let resp = execute_with_retry(&retry_cfg(3, vec![503]), None, || driver.fetch(&url))
            .await
            .unwrap();
        assert_eq!(resp.status, 503);
        assert_eq!(driver.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_status_outside_the_list_is_returned_at_once() {
        let driver = AnswersWith {
            status: 403,
            headers: vec![],
            calls: AtomicU32::new(0),
        };
        let url = sample_url();
        let resp = execute_with_retry(&retry_cfg(3, vec![503]), None, || driver.fetch(&url))
            .await
            .unwrap();
        assert_eq!(resp.status, 403);
        assert_eq!(driver.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_block_is_never_retried() {
        let driver = FailsWith::new(QuarryError::new(ErrorCode::UpstreamBlocked, "blocked"));
        let url = sample_url();
        let _ = execute_with_retry(&retry_cfg(3, vec![]), None, || driver.fetch(&url)).await;
        assert_eq!(driver.calls(), 1);
    }

    #[tokio::test]
    async fn sub_codes_retry_by_their_own_flag() {
        let cases: [(QuarryError, u32); 6] = [
            (
                QuarryError::driver_failed(driver_failed_reason::DNS, "lookup"),
                3,
            ),
            (
                QuarryError::driver_failed(driver_failed_reason::CONNECT, "refused"),
                3,
            ),
            (
                QuarryError::driver_failed(driver_failed_reason::TLS, "handshake"),
                1,
            ),
            (
                QuarryError::driver_failed(driver_failed_reason::ALL_DRIVERS_FAILED, "all"),
                1,
            ),
            (QuarryError::new(ErrorCode::DriverFailed, "plain"), 1),
            (QuarryError::new(ErrorCode::Internal, "bug"), 1),
        ];
        let url = sample_url();
        for (error, expected) in cases {
            let driver = FailsWith::new(error);
            let _ = execute_with_retry(&retry_cfg(3, vec![]), None, || driver.fetch(&url)).await;
            assert_eq!(driver.calls(), expected);
        }
    }

    #[tokio::test]
    async fn a_failure_through_a_proxy_is_final_and_a_direct_one_is_retried() {
        let through_proxy = QuarryError::new(ErrorCode::Timeout, "slow").with_details(json!({
            "egress_attempts": [{ "attempt": 0, "egress": "proxy", "reason": "transport_error" }]
        }));
        let direct = QuarryError::new(ErrorCode::Timeout, "slow").with_details(json!({
            "egress_attempts": [{ "attempt": 0, "egress": "direct", "reason": "transport_error" }]
        }));
        assert!(routed_through_proxy(&through_proxy));
        assert!(!routed_through_proxy(&direct));
        let url = sample_url();
        let proxied = FailsWith::new(through_proxy);
        let _ = execute_with_retry(&retry_cfg(3, vec![]), None, || proxied.fetch(&url)).await;
        assert_eq!(proxied.calls(), 1);
        let plain = FailsWith::new(direct);
        let _ = execute_with_retry(&retry_cfg(3, vec![]), None, || plain.fetch(&url)).await;
        assert_eq!(plain.calls(), 3);
    }

    #[tokio::test]
    async fn a_wait_that_passes_the_deadline_ends_the_retries() {
        let driver = FailsWith::new(QuarryError::new(ErrorCode::Timeout, "slow"));
        let url = sample_url();
        let cfg = ScrapeRetry {
            base_ms: 5_000,
            ..retry_cfg(3, vec![])
        };
        let until = Some(Instant::now() + Duration::from_secs(1));
        let _ = execute_with_retry(&cfg, until, || driver.fetch(&url)).await;
        assert_eq!(driver.calls(), 1);
    }

    #[tokio::test]
    async fn the_wrapper_retries_fetch_conditional_and_keeps_the_kind() {
        let inner: Arc<dyn Driver> = Arc::new(FailsNThenOk {
            remaining: AtomicU32::new(1),
        });
        let wrapped = RetryingDriver::wrap(inner, retry_cfg(3, vec![]), None);
        assert_eq!(wrapped.kind(), DriverKind::Static);
        let url = sample_url();
        let resp = wrapped
            .fetch_conditional(&url, &FetchHints::default())
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
    }
}
