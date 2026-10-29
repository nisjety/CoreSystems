//! Policy-aware egress broker.
//!
//! Owns the decision between direct egress and configured proxy identities.
//! The fetch driver asks this broker for an ordered egress plan per request,
//! then reports block/transport outcomes back so later requests avoid unhealthy
//! `(host, proxy)` pairs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quarry_core::QuarryResult;
use url::Url;

use crate::driver::FetchHints;
use crate::proxy_pool::ProxyPool;

const DEFAULT_BLOCK_COOLDOWN: Duration = Duration::from_secs(60);
const DEFAULT_MAX_PROXY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressIdentity {
    Direct,
    Proxy {
        uri: String,
        processor_id: Option<String>,
    },
}

impl EgressIdentity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Proxy { .. } => "proxy",
        }
    }

    pub fn proxy_uri(&self) -> Option<&str> {
        match self {
            Self::Direct => None,
            Self::Proxy { uri, .. } => Some(uri.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDecision {
    pub identity: EgressIdentity,
    pub attempt: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
struct EgressHealth {
    failures: u32,
    blocked_until: Option<Instant>,
    last_status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HealthKey {
    host: String,
    proxy_uri: String,
}

#[derive(Debug, Clone)]
pub struct EgressBroker {
    pool: ProxyPool,
    proxy_processor_id: Option<String>,
    health: Arc<Mutex<HashMap<HealthKey, EgressHealth>>>,
    block_cooldown: Duration,
    max_proxy_attempts: usize,
}

impl EgressBroker {
    pub fn new(pool: ProxyPool, proxy_processor_id: Option<String>) -> Self {
        Self {
            pool,
            proxy_processor_id,
            health: Arc::new(Mutex::new(HashMap::new())),
            block_cooldown: DEFAULT_BLOCK_COOLDOWN,
            max_proxy_attempts: DEFAULT_MAX_PROXY_ATTEMPTS,
        }
    }

    #[cfg(test)]
    pub fn with_limits(
        pool: ProxyPool,
        proxy_processor_id: Option<String>,
        block_cooldown: Duration,
        max_proxy_attempts: usize,
    ) -> Self {
        Self {
            pool,
            proxy_processor_id,
            health: Arc::new(Mutex::new(HashMap::new())),
            block_cooldown,
            max_proxy_attempts: max_proxy_attempts.max(1),
        }
    }

    pub fn plan(&self, hints: &FetchHints, url: &Url) -> QuarryResult<Vec<EgressDecision>> {
        let host = url.host_str().unwrap_or("").to_string();
        if self.pool.is_empty() {
            return Ok(vec![EgressDecision {
                identity: EgressIdentity::Direct,
                attempt: 0,
                reason: "no_proxy_pool".into(),
            }]);
        }

        if let Some(processor_id) = self.proxy_processor_id.as_deref() {
            hints
                .privacy
                .guard_third_party_processing("proxy", processor_id)?;
        }

        let ordered = self.pool.ordered_candidates(&hints.org_id, &host);
        let now = Instant::now();
        let mut healthy = Vec::new();
        let mut blocked = Vec::new();
        for entry in ordered {
            if self.is_blocked(&host, &entry.uri, now) {
                blocked.push(entry);
            } else {
                healthy.push(entry);
            }
        }

        // Prefer healthy candidates, but if every proxy is currently cooling
        // down still return the ordered list so the caller can make bounded
        // progress rather than deadlocking on stale health.
        let candidates = if healthy.is_empty() { blocked } else { healthy };
        let max_attempts = self.max_proxy_attempts.min(candidates.len()).max(1);

        Ok(candidates
            .into_iter()
            .take(max_attempts)
            .enumerate()
            .map(|(attempt, entry)| EgressDecision {
                identity: EgressIdentity::Proxy {
                    uri: entry.uri,
                    processor_id: self.proxy_processor_id.clone(),
                },
                attempt,
                reason: if attempt == 0 {
                    "sticky_primary".into()
                } else {
                    "block_rotation".into()
                },
            })
            .collect())
    }

    pub fn mark_http_status(&self, host: &str, identity: &EgressIdentity, status: u16) {
        let Some(uri) = identity.proxy_uri() else {
            return;
        };
        if crate::fingerprint_rotation::is_block_status(status) {
            self.mark_block(host, uri, Some(status));
            return;
        }
        self.mark_ok(host, uri);
    }

    pub fn mark_transport_error(&self, host: &str, identity: &EgressIdentity) {
        let Some(uri) = identity.proxy_uri() else {
            return;
        };
        self.mark_block(host, uri, None);
    }

    fn is_blocked(&self, host: &str, proxy_uri: &str, now: Instant) -> bool {
        let key = HealthKey {
            host: host.to_string(),
            proxy_uri: proxy_uri.to_string(),
        };
        let Ok(health) = self.health.lock() else {
            return false;
        };
        health
            .get(&key)
            .and_then(|h| h.blocked_until)
            .is_some_and(|until| until > now)
    }

    fn mark_block(&self, host: &str, proxy_uri: &str, status: Option<u16>) {
        let key = HealthKey {
            host: host.to_string(),
            proxy_uri: proxy_uri.to_string(),
        };
        let Ok(mut health) = self.health.lock() else {
            return;
        };
        let entry = health.entry(key).or_default();
        entry.failures = entry.failures.saturating_add(1);
        entry.last_status = status;
        entry.blocked_until = Some(Instant::now() + self.block_cooldown);
    }

    fn mark_ok(&self, host: &str, proxy_uri: &str) {
        let key = HealthKey {
            host: host.to_string(),
            proxy_uri: proxy_uri.to_string(),
        };
        let Ok(mut health) = self.health.lock() else {
            return;
        };
        health.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use quarry_core::privacy::PrivacyPolicy;

    fn approved_hints() -> FetchHints {
        FetchHints {
            org_id: "org_a".into(),
            privacy: PrivacyPolicy {
                allow_third_party_processing: true,
                processor_id: Some("quarry_proxy_pool".into()),
                ..PrivacyPolicy::default()
            },
            ..FetchHints::default()
        }
    }

    #[test]
    fn empty_pool_plans_direct_egress() {
        let broker = EgressBroker::new(ProxyPool::empty(), None);
        let url = Url::parse("https://example.com/").unwrap();

        let plan = broker.plan(&FetchHints::default(), &url).unwrap();

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].identity, EgressIdentity::Direct);
    }

    #[test]
    fn proxy_pool_requires_privacy_approval() {
        let pool = ProxyPool::from_env_string("http://proxy-a:8080");
        let broker = EgressBroker::new(pool, Some("quarry_proxy_pool".into()));
        let url = Url::parse("https://example.com/").unwrap();

        let err = broker.plan(&FetchHints::default(), &url).unwrap_err();

        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[test]
    fn proxy_pool_plans_approved_candidates() {
        let pool = ProxyPool::from_env_string("http://proxy-a:8080;http://proxy-b:8080");
        let broker = EgressBroker::new(pool, Some("quarry_proxy_pool".into()));
        let url = Url::parse("https://example.com/").unwrap();

        let plan = broker.plan(&approved_hints(), &url).unwrap();

        assert_eq!(plan.len(), 2);
        assert!(matches!(plan[0].identity, EgressIdentity::Proxy { .. }));
    }

    #[test]
    fn blocked_proxy_is_deprioritized_for_same_host() {
        let pool = ProxyPool::from_env_string("http://proxy-a:8080;http://proxy-b:8080");
        let broker = EgressBroker::with_limits(
            pool,
            Some("quarry_proxy_pool".into()),
            Duration::from_secs(60),
            2,
        );
        let hints = approved_hints();
        let url = Url::parse("https://example.com/").unwrap();
        let first = broker.plan(&hints, &url).unwrap();
        let first_identity = first[0].identity.clone();

        broker.mark_http_status("example.com", &first_identity, 429);
        let second = broker.plan(&hints, &url).unwrap();

        assert_ne!(second[0].identity, first_identity);
    }
}
