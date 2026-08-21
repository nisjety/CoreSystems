//! Retry/block/escalation policy executor. Enforces `RunPolicy::{retry, block}`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::time::sleep;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::policy::{BackoffKind, Retry};

use crate::driver::Driver;
use crate::fetch::FetchResponse;

pub fn backoff_delay(retry: &Retry, attempt: u32) -> Duration {
    let base = retry.base_ms as u64;
    let ms = match retry.backoff {
        BackoffKind::Fixed => base,
        BackoffKind::Linear => base.saturating_mul(attempt as u64 + 1),
        BackoffKind::Exp => base.saturating_mul(1u64 << attempt.min(10)),
    };
    Duration::from_millis(ms)
}

pub async fn wait(retry: &Retry, attempt: u32) {
    sleep(backoff_delay(retry, attempt)).await;
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

/// Execute a driver fetch with retry policy, honoring transient/permanent classification.
pub async fn execute_with_retry<D: Driver + ?Sized>(
    driver: &D,
    url: &Url,
    policy: &Retry,
) -> QuarryResult<FetchResponse> {
    let mut last_err: Option<QuarryError> = None;
    for attempt in 0..policy.max {
        match driver.fetch(url).await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                if classify(&err) == RetryClass::Permanent {
                    return Err(err);
                }
                last_err = Some(err);
                if attempt + 1 < policy.max {
                    wait(policy, attempt).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| QuarryError::new(ErrorCode::Internal, "retry loop exhausted")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use std::sync::atomic::{AtomicU32, Ordering};

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
                Ok(FetchResponse {
                    status: 200,
                    final_url: url.clone(),
                    headers: vec![],
                    body: b"ok".to_vec(),
                    duration_ms: 1,
                    served_by: DriverKind::Static,
                })
            }
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
        let url = Url::parse("https://example.com/").unwrap();
        let policy = Retry {
            max: 5,
            backoff: BackoffKind::Fixed,
            base_ms: 0,
        };
        let err = execute_with_retry(&driver, &url, &policy)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn transient_error_exhausts_after_max() {
        let driver = AlwaysFails(ErrorCode::Timeout);
        let url = Url::parse("https://example.com/").unwrap();
        let policy = Retry {
            max: 3,
            backoff: BackoffKind::Fixed,
            base_ms: 0,
        };
        let err = execute_with_retry(&driver, &url, &policy)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn transient_then_success() {
        let driver = FailsNThenOk {
            remaining: AtomicU32::new(1),
        };
        let url = Url::parse("https://example.com/").unwrap();
        let policy = Retry {
            max: 3,
            backoff: BackoffKind::Fixed,
            base_ms: 0,
        };
        let resp = execute_with_retry(&driver, &url, &policy).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
