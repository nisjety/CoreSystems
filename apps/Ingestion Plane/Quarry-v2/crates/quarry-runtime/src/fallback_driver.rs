//! Fallback driver — tries drivers in order until one succeeds.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use quarry_core::error::ErrorCode;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_tls::TlsProfile;
use tracing::warn;
use url::Url;

use crate::driver::{Driver, FetchHints};
use crate::fetch::FetchResponse;

pub struct FallbackDriver {
    primary: DriverKind,
    chain: Vec<DriverKind>,
    drivers: HashMap<DriverKind, Arc<dyn Driver>>,
}

impl FallbackDriver {
    pub fn new(
        primary: DriverKind,
        chain: Vec<DriverKind>,
        drivers: HashMap<DriverKind, Arc<dyn Driver>>,
    ) -> Self {
        Self {
            primary,
            chain,
            drivers,
        }
    }

    pub fn from_plan(
        plan: &crate::driver_plan::DriverPlan,
        drivers: HashMap<DriverKind, Arc<dyn Driver>>,
    ) -> Self {
        Self {
            primary: plan.driver,
            chain: plan.fallback_chain.clone(),
            drivers,
        }
    }

    fn attempt_order(&self) -> Vec<DriverKind> {
        let mut order = vec![self.primary];
        for k in &self.chain {
            if !order.contains(k) {
                order.push(*k);
            }
        }
        order
    }

    async fn fetch_inner(
        &self,
        url: &Url,
        hints: Option<&FetchHints>,
    ) -> QuarryResult<FetchResponse> {
        let order = self.attempt_order();
        // Aggregate per-driver attempts so when the entire chain fails the
        // operator sees `[static: timeout (5s), tls: ja3-mismatch, browser:
        // 502]` instead of a single mystery error from the last attempt.
        let mut attempts: Vec<(DriverKind, String)> = Vec::with_capacity(order.len());
        let mut last_err: Option<quarry_core::QuarryError> = None;
        // 2D — on a block STATUS (403/429/503/…), fall through to the next
        // driver (a different TLS fingerprint / transport) instead of
        // returning the block. Bounded naturally by the chain length.
        let mut best_block: Option<FetchResponse> = None;

        for (idx, kind) in order.iter().enumerate() {
            let driver = match self.drivers.get(kind) {
                Some(d) => d,
                None => {
                    attempts.push((*kind, "driver not registered".into()));
                    continue;
                }
            };
            let result = match hints {
                Some(hints) => driver.fetch_conditional(url, hints).await,
                None => driver.fetch(url).await,
            };
            match result {
                Ok(resp) => {
                    if crate::fingerprint_rotation::is_block_status(resp.status)
                        && idx < order.len() - 1
                    {
                        attempts.push((*kind, format!("blocked: HTTP {}", resp.status)));
                        warn!(
                            driver = ?kind,
                            status = resp.status,
                            next = ?order.get(idx + 1),
                            "block status — rotating fingerprint/driver"
                        );
                        best_block = Some(resp);
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    let is_retryable = is_retryable_driver_error(e.code);
                    attempts.push((
                        *kind,
                        format!("{}: {}", error_code_short(e.code), e.message),
                    ));
                    if is_retryable && idx < order.len() - 1 {
                        warn!(
                            driver = ?kind,
                            next = ?order.get(idx + 1),
                            error = %e,
                            "driver failed, falling back"
                        );
                        last_err = Some(e);
                        continue;
                    }
                    return Err(e.with_details(serde_json::json!({
                        "fallback_attempts": attempts
                            .iter()
                            .map(|(k, msg)| serde_json::json!({"driver": format!("{k:?}"), "error": msg}))
                            .collect::<Vec<_>>(),
                    })));
                }
            }
        }

        // All drivers rotated through; if every one was blocked, surface the
        // last block response (the caller's scheduler reads the status) rather
        // than a generic error.
        if let Some(resp) = best_block {
            return Ok(resp);
        }

        let aggregate_msg = attempts
            .iter()
            .map(|(k, e)| format!("{k:?}={e}"))
            .collect::<Vec<_>>()
            .join("; ");

        Err(last_err
            .map(|e| {
                e.with_details(serde_json::json!({
                    "fallback_attempts": attempts
                        .iter()
                        .map(|(k, msg)| serde_json::json!({"driver": format!("{k:?}"), "error": msg}))
                        .collect::<Vec<_>>(),
                }))
            })
            .unwrap_or_else(|| {
                quarry_core::QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("no drivers succeeded in fallback chain: {aggregate_msg}"),
                )
            }))
    }
}

#[async_trait]
impl Driver for FallbackDriver {
    fn kind(&self) -> DriverKind {
        self.primary
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetch_inner(url, None).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.fetch_inner(url, Some(hints)).await
    }

    fn tls_profile(&self) -> Option<TlsProfile> {
        self.drivers
            .get(&self.primary)
            .and_then(|d: &Arc<dyn Driver>| d.tls_profile())
    }
}

pub(crate) fn is_retryable_driver_error(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Timeout | ErrorCode::UpstreamBlocked | ErrorCode::DriverFailed
    )
}

pub(crate) fn error_code_short(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::BadRequest => "bad_request",
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::Forbidden => "forbidden",
        ErrorCode::NotFound => "not_found",
        ErrorCode::Conflict => "conflict",
        ErrorCode::RateLimited => "rate_limited",
        ErrorCode::Timeout => "timeout",
        ErrorCode::SecurityBlocked => "security_blocked",
        ErrorCode::DriverFailed => "driver_failed",
        ErrorCode::UpstreamBlocked => "upstream_blocked",
        ErrorCode::Unsupported => "unsupported",
        ErrorCode::Internal => "internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::QuarryError;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct OkDriver {
        kind: DriverKind,
        calls: AtomicU32,
    }

    impl OkDriver {
        fn new(kind: DriverKind) -> Self {
            Self {
                kind,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Driver for OkDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: b"ok".to_vec(),
                duration_ms: 1,
            })
        }
    }

    struct FailDriver {
        kind: DriverKind,
        calls: AtomicU32,
    }

    impl FailDriver {
        fn new(kind: DriverKind) -> Self {
            Self {
                kind,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Driver for FailDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "simulated failure",
            ))
        }
    }

    struct BlockDriver {
        kind: DriverKind,
        calls: AtomicU32,
    }

    impl BlockDriver {
        fn new(kind: DriverKind) -> Self {
            Self { kind, calls: AtomicU32::new(0) }
        }
    }

    #[async_trait]
    impl Driver for BlockDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(FetchResponse {
                status: 403,
                final_url: url.clone(),
                headers: vec![],
                body: b"blocked".to_vec(),
                duration_ms: 1,
            })
        }
    }

    #[tokio::test]
    async fn block_status_rotates_to_next_fingerprint() {
        // primary returns a 403 block → fall through to the next (different
        // fingerprint) driver, which succeeds.
        let primary = Arc::new(BlockDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn all_blocked_returns_last_block_response() {
        let primary = Arc::new(BlockDriver::new(DriverKind::Static));
        let fallback = Arc::new(BlockDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(resp.status, 403); // exhausted rotation → surface the block
    }

    #[tokio::test]
    async fn primary_success_skips_fallback() {
        let primary = Arc::new(OkDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn falls_back_on_primary_failure() {
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn all_fail_returns_last_error() {
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let fallback = Arc::new(FailDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let err = fb.fetch(&url).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn all_fail_aggregates_per_driver_errors_in_details() {
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let mid = Arc::new(FailDriver::new(DriverKind::Tls));
        let last = Arc::new(FailDriver::new(DriverKind::Browser));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, mid.clone());
        drivers.insert(DriverKind::Browser, last.clone());

        let fb = FallbackDriver::new(
            DriverKind::Static,
            vec![DriverKind::Tls, DriverKind::Browser],
            drivers,
        );
        let url: Url = "https://example.com".parse().unwrap();
        let err = fb.fetch(&url).await.unwrap_err();
        let details = err.details.as_ref().expect("details should be set");
        let attempts = details
            .get("fallback_attempts")
            .and_then(|v| v.as_array())
            .expect("fallback_attempts array");
        assert_eq!(attempts.len(), 3, "all three drivers should have entries");

        // Each driver tried.
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(mid.calls.load(Ordering::Relaxed), 1);
        assert_eq!(last.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn missing_driver_skipped() {
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
