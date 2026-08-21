//! Transport-level fallback for opportunistic protocol upgrades.
//!
//! Unlike [`crate::fallback_driver::FallbackDriver`], this wrapper is not
//! keyed by [`DriverKind`]. It is used when the logical driver stays the same
//! but the transport changes first, e.g. "try HTTP/3, then fall back to the
//! planned static/TLS driver if UDP/QUIC is unavailable".

use std::sync::Arc;

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::{QuarryError, QuarryResult};
use quarry_tls::TlsProfile;
use serde_json::json;
use tracing::warn;
use url::Url;

use crate::driver::{BrowserMeta, Driver, FetchHints};
use crate::fallback_driver::{error_code_short, is_retryable_driver_error};
use crate::fetch::FetchResponse;

pub struct TransportFallbackDriver {
    primary_name: &'static str,
    primary: Arc<dyn Driver>,
    fallback_name: &'static str,
    fallback: Arc<dyn Driver>,
}

impl TransportFallbackDriver {
    pub fn new(
        primary_name: &'static str,
        primary: Arc<dyn Driver>,
        fallback_name: &'static str,
        fallback: Arc<dyn Driver>,
    ) -> Self {
        Self {
            primary_name,
            primary,
            fallback_name,
            fallback,
        }
    }

    pub fn prefer_http3(http3: Arc<dyn Driver>, fallback: Arc<dyn Driver>) -> Self {
        Self::new("http3", http3, "planned", fallback)
    }

    async fn fetch_inner(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        match self.primary.fetch_conditional(url, hints).await {
            Ok(resp) => Ok(resp),
            Err(primary_err) if is_retryable_driver_error(primary_err.code) => {
                warn!(
                    primary = self.primary_name,
                    fallback = self.fallback_name,
                    error = %primary_err,
                    "preferred transport failed, falling back"
                );
                match self.fallback.fetch_conditional(url, hints).await {
                    Ok(resp) => Ok(resp),
                    Err(fallback_err) => Err(with_transport_attempts(
                        fallback_err,
                        self.primary_name,
                        self.primary.kind(),
                        &primary_err,
                        self.fallback_name,
                        self.fallback.kind(),
                    )),
                }
            }
            Err(err) => Err(err),
        }
    }
}

#[async_trait]
impl Driver for TransportFallbackDriver {
    fn kind(&self) -> DriverKind {
        self.fallback.kind()
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetch_inner(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.fetch_inner(url, hints).await
    }

    fn tls_profile(&self) -> Option<TlsProfile> {
        self.fallback.tls_profile()
    }

    fn browser_meta(&self) -> Option<BrowserMeta> {
        self.fallback.browser_meta()
    }
}

fn with_transport_attempts(
    fallback_err: QuarryError,
    primary_name: &'static str,
    primary_kind: DriverKind,
    primary_err: &QuarryError,
    fallback_name: &'static str,
    fallback_kind: DriverKind,
) -> QuarryError {
    let fallback_code = fallback_err.code;
    let fallback_message = fallback_err.message.clone();
    fallback_err.with_details(json!({
        "fallback_attempts": [
            {
                "transport": primary_name,
                "driver": format!("{primary_kind:?}"),
                "error": format!(
                    "{}: {}",
                    error_code_short(primary_err.code),
                    primary_err.message
                ),
            },
            {
                "transport": fallback_name,
                "driver": format!("{fallback_kind:?}"),
                "error": format!(
                    "{}: {}",
                    error_code_short(fallback_code),
                    fallback_message
                ),
            },
        ],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use quarry_core::QuarryError;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    struct StubDriver {
        kind: DriverKind,
        outcome: StubOutcome,
        calls: AtomicU32,
        hints_seen: Mutex<Vec<FetchHints>>,
    }

    enum StubOutcome {
        Ok(&'static [u8]),
        Err(ErrorCode),
    }

    impl StubDriver {
        fn ok(kind: DriverKind, body: &'static [u8]) -> Self {
            Self {
                kind,
                outcome: StubOutcome::Ok(body),
                calls: AtomicU32::new(0),
                hints_seen: Mutex::new(Vec::new()),
            }
        }

        fn err(kind: DriverKind, code: ErrorCode) -> Self {
            Self {
                kind,
                outcome: StubOutcome::Err(code),
                calls: AtomicU32::new(0),
                hints_seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Driver for StubDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.fetch_conditional(url, &FetchHints::default()).await
        }

        async fn fetch_conditional(
            &self,
            url: &Url,
            hints: &FetchHints,
        ) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.hints_seen.lock().unwrap().push(hints.clone());
            match self.outcome {
                StubOutcome::Ok(body) => Ok(FetchResponse {
                    status: 200,
                    final_url: url.clone(),
                    headers: vec![],
                    body: body.to_vec(),
                    duration_ms: 1,
                    served_by: self.kind,
                }),
                StubOutcome::Err(code) => Err(QuarryError::new(code, "simulated failure")),
            }
        }
    }

    #[tokio::test]
    async fn primary_success_skips_fallback() {
        let primary = Arc::new(StubDriver::ok(DriverKind::Static, b"h3"));
        let fallback = Arc::new(StubDriver::ok(DriverKind::Static, b"h2"));
        let driver = TransportFallbackDriver::prefer_http3(primary.clone(), fallback.clone());
        let url: Url = "https://example.com".parse().unwrap();

        let resp = driver.fetch(&url).await.unwrap();

        assert_eq!(resp.body, b"h3");
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn retryable_primary_failure_uses_fallback() {
        let primary = Arc::new(StubDriver::err(DriverKind::Static, ErrorCode::DriverFailed));
        let fallback = Arc::new(StubDriver::ok(DriverKind::Tls, b"h2"));
        let driver = TransportFallbackDriver::prefer_http3(primary.clone(), fallback.clone());
        let url: Url = "https://example.com".parse().unwrap();

        let resp = driver.fetch(&url).await.unwrap();

        assert_eq!(resp.body, b"h2");
        // served_by passes through the wrapper untouched — the fallback
        // (Tls) served, so the response says Tls even though the primary
        // stub was Static-kinded.
        assert_eq!(resp.served_by, DriverKind::Tls);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn non_retryable_primary_failure_does_not_fallback() {
        let primary = Arc::new(StubDriver::err(
            DriverKind::Static,
            ErrorCode::SecurityBlocked,
        ));
        let fallback = Arc::new(StubDriver::ok(DriverKind::Tls, b"h2"));
        let driver = TransportFallbackDriver::prefer_http3(primary.clone(), fallback.clone());
        let url: Url = "https://example.com".parse().unwrap();

        let err = driver.fetch(&url).await.unwrap_err();

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn conditional_hints_flow_to_fallback() {
        let primary = Arc::new(StubDriver::err(DriverKind::Static, ErrorCode::DriverFailed));
        let fallback = Arc::new(StubDriver::ok(DriverKind::Tls, b"h2"));
        let driver = TransportFallbackDriver::prefer_http3(primary.clone(), fallback.clone());
        let url: Url = "https://example.com".parse().unwrap();
        let hints = FetchHints {
            if_none_match: Some("\"etag\"".into()),
            org_id: "org_123".into(),
            ..FetchHints::default()
        };

        let resp = driver.fetch_conditional(&url, &hints).await.unwrap();

        assert_eq!(resp.body, b"h2");
        let seen = fallback.hints_seen.lock().unwrap();
        assert_eq!(seen[0].if_none_match.as_deref(), Some("\"etag\""));
        assert_eq!(seen[0].org_id, "org_123");
    }

    #[tokio::test]
    async fn all_failures_include_transport_attempts() {
        let primary = Arc::new(StubDriver::err(DriverKind::Static, ErrorCode::DriverFailed));
        let fallback = Arc::new(StubDriver::err(DriverKind::Tls, ErrorCode::Timeout));
        let driver = TransportFallbackDriver::prefer_http3(primary.clone(), fallback.clone());
        let url: Url = "https://example.com".parse().unwrap();

        let err = driver.fetch(&url).await.unwrap_err();

        assert_eq!(err.code, ErrorCode::Timeout);
        let attempts = err
            .details
            .as_ref()
            .and_then(|v| v.get("fallback_attempts"))
            .and_then(|v| v.as_array())
            .expect("fallback attempts");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("transport").and_then(|v| v.as_str()),
            Some("http3")
        );
    }
}
