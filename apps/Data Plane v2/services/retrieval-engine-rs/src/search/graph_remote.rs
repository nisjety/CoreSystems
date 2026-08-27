//! HTTP client for graph-index-rs's `POST /v1/graph/traverse` — the Neo4j-backed
//! native multi-hop traversal (with its own transparent Postgres-BFS fallback
//! server-side). This is how the fused graph arm reaches DEEP multi-hop
//! neighbourhoods: Neo4j stays encapsulated inside graph-index-rs (no direct
//! Bolt from this service — plane rule), and this service re-grounds the
//! returned entity ids through its own org-visible Postgres chunk mapping.
//!
//! Auth: the caller's ALREADY-VERIFIED bearer is forwarded; graph-index
//! re-verifies it and pins the org itself (defense in depth — two independent
//! verifications of the same principal). No service-key fallback: without a
//! bearer the arm uses the in-process SQL grounding instead.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;

/// Consecutive failures before the breaker opens.
const BREAKER_FAILURE_THRESHOLD: u32 = 3;
/// How long the breaker stays open (skipping the hop) once tripped.
const BREAKER_COOLDOWN_MS: u64 = 10_000;

/// Circuit breaker so a persistently-down/slow graph-index isn't re-called
/// (and re-timed-out) on every single retrieval. Shared across the process via
/// `Arc`; all state is atomic. Time is measured against `origin` so the pure
/// decision helpers are unit-testable with an injected clock.
struct Breaker {
    origin: Instant,
    consecutive_failures: AtomicU32,
    open_until_ms: AtomicU64,
}

impl Breaker {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            consecutive_failures: AtomicU32::new(0),
            open_until_ms: AtomicU64::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }

    /// True when the breaker is open (skip the hop) at `now_ms`.
    fn is_open(&self, now_ms: u64) -> bool {
        now_ms < self.open_until_ms.load(Ordering::Relaxed)
    }

    fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.open_until_ms.store(0, Ordering::Relaxed);
    }

    /// Counts a failure and opens the breaker once the threshold is reached.
    fn record_failure(&self, now_ms: u64) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= BREAKER_FAILURE_THRESHOLD {
            self.open_until_ms
                .store(now_ms + BREAKER_COOLDOWN_MS, Ordering::Relaxed);
        }
    }
}

/// Thin client over graph-index's traverse endpoint. Cheap to clone (shares
/// the reqwest pool and the circuit breaker).
#[derive(Clone)]
pub struct GraphTraverseClient {
    http: reqwest::Client,
    base_url: String,
    breaker: Arc<Breaker>,
}

impl GraphTraverseClient {
    /// `None` when `base_url` is empty (feature off — e.g. unit tests, bare
    /// metal). The timeout is deliberately short: this runs inside the
    /// retrieval hot path (concurrent with the other arms) and is non-fatal,
    /// so a slow graph-index must degrade, never stall the query.
    pub fn from_config(base_url: &str, timeout_ms: u64) -> Option<Self> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return None;
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms.max(100)))
            .build()
            .ok()?;
        Some(Self {
            http,
            base_url: base_url.to_string(),
            breaker: Arc::new(Breaker::new()),
        })
    }

    /// Multi-hop traversal from `seed_ids` within `org_id`. Returns
    /// `(entity_id, hops)` pairs for the reached entities. The org in the body
    /// must match the bearer's verified org or graph-index rejects with 403 —
    /// callers always pass the same org the bearer was verified for.
    pub async fn traverse(
        &self,
        bearer: &str,
        org_id: &str,
        seed_ids: &[String],
        max_hops: u8,
        max_entities: u32,
    ) -> anyhow::Result<Vec<(String, u8)>> {
        // Circuit breaker: after repeated failures, skip the hop entirely for a
        // cooldown so a down/slow graph-index doesn't impose its timeout on
        // every query. The caller degrades to the in-process 1-hop tier.
        let now_ms = self.breaker.now_ms();
        if self.breaker.is_open(now_ms) {
            anyhow::bail!("graph traverse breaker open; skipping remote hop");
        }

        let result = self
            .traverse_inner(bearer, org_id, seed_ids, max_hops, max_entities)
            .await;
        // The breaker exists to stop us re-timing-out against a graph-index that
        // is DOWN or SLOW. It must therefore count reachability, not request
        // outcome — see `breaker_verdict`. Counting every `Err` alike (which is
        // what this did) meant one under-scoped caller's 403 tripped the breaker
        // and disabled the deep tier for EVERY caller, including end users whose
        // tokens are authorized for it, for the whole 10s cooldown. Observed
        // live: 18 x 403 from one service principal, then 39 x "breaker open"
        // affecting unrelated queries.
        match breaker_verdict(&result) {
            BreakerVerdict::Healthy => self.breaker.record_success(),
            BreakerVerdict::Unhealthy => self.breaker.record_failure(self.breaker.now_ms()),
        }
        result.map_err(|e| match e {
            TraverseError::Status(status) => {
                // Never echo the body — mirrors the rerank client's no-leak rule.
                anyhow::anyhow!("graph traverse returned {status}")
            }
            TraverseError::Unreachable(e) => e,
        })
    }

    async fn traverse_inner(
        &self,
        bearer: &str,
        org_id: &str,
        seed_ids: &[String],
        max_hops: u8,
        max_entities: u32,
    ) -> Result<Vec<(String, u8)>, TraverseError> {
        let url = format!("{}/v1/graph/traverse", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(bearer)
            .json(&serde_json::json!({
                "org_id": org_id,
                "seed_entity_ids": seed_ids,
                "max_hops": max_hops,
                "max_entities": max_entities,
            }))
            .send()
            .await
            .context("graph traverse request failed")
            .map_err(TraverseError::Unreachable)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(TraverseError::Status(status));
        }
        // A body we cannot decode means graph-index answered but is speaking a
        // shape we do not understand — a real service fault, so `Unreachable`
        // (which counts against the breaker), not `Status`.
        let body: serde_json::Value = resp
            .json()
            .await
            .context("graph traverse decode")
            .map_err(TraverseError::Unreachable)?;
        Ok(parse_traverse_entities(&body))
    }
}

/// Why a traverse attempt failed, split by what it says about graph-index's
/// HEALTH rather than about this request's outcome.
enum TraverseError {
    /// graph-index answered with a non-success status. It is up and serving.
    Status(reqwest::StatusCode),
    /// No usable answer: transport error, timeout, or an undecodable body.
    Unreachable(anyhow::Error),
}

/// What one attempt should tell the circuit breaker.
#[derive(Debug, PartialEq, Eq)]
enum BreakerVerdict {
    /// graph-index is serving; do not penalise it.
    Healthy,
    /// graph-index is down, overloaded, or broken; count toward opening.
    Unhealthy,
}

/// Map an attempt to a breaker verdict.
///
/// The rule: an answered request proves reachability, so it must not count
/// toward a breaker whose entire purpose is to stop calling an unreachable
/// service. Two deliberate carve-outs stay `Unhealthy`, because they describe
/// graph-index's own capacity rather than the caller's request:
///
/// * **5xx** — the server is failing.
/// * **429** — the server is shedding load; backing off is the cooperative
///   response, and hammering a rate-limited dependency is what breakers are for.
///
/// Everything else in 4xx (403 insufficient scope, 400 bad seeds, 404) is a
/// per-request fault. That caller degrades to the in-process 1-hop tier, which
/// is correct for it, and no other caller is affected.
fn breaker_verdict<T>(result: &Result<T, TraverseError>) -> BreakerVerdict {
    match result {
        Ok(_) => BreakerVerdict::Healthy,
        Err(TraverseError::Unreachable(_)) => BreakerVerdict::Unhealthy,
        Err(TraverseError::Status(status)) => {
            if status.is_server_error() || *status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                BreakerVerdict::Unhealthy
            } else {
                BreakerVerdict::Healthy
            }
        }
    }
}

/// Pulls `(entity_id, hops)` out of a traverse response. Tolerant by design:
/// entities missing an id are skipped, missing hops default to 1 (the server's
/// Postgres-fallback convention).
fn parse_traverse_entities(body: &serde_json::Value) -> Vec<(String, u8)> {
    body["entities"]
        .as_array()
        .map(|entities| {
            entities
                .iter()
                .filter_map(|e| {
                    let id = e["entity_id"].as_str()?;
                    if id.is_empty() {
                        return None;
                    }
                    let hops = e["hops"].as_u64().unwrap_or(1).clamp(1, u8::MAX as u64) as u8;
                    Some((id.to_string(), hops))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_disabled_on_empty_url_and_trims_slash() {
        assert!(GraphTraverseClient::from_config("", 1000).is_none());
        assert!(GraphTraverseClient::from_config("   ", 1000).is_none());
        let c = GraphTraverseClient::from_config("http://graph-index:9203/", 1000).unwrap();
        assert_eq!(c.base_url, "http://graph-index:9203");
    }

    #[test]
    fn parse_traverse_entities_reads_ids_and_hops() {
        let body = serde_json::json!({
            "entities": [
                {"entity_id": "e1", "hops": 1, "entity_text": "Ada"},
                {"entity_id": "e2", "hops": 3},
                {"entity_id": "e3"},            // missing hops → 1
                {"hops": 2},                     // missing id → skipped
                {"entity_id": "", "hops": 2},    // empty id → skipped
            ],
            "backend": "neo4j",
        });
        assert_eq!(
            parse_traverse_entities(&body),
            vec![
                ("e1".to_string(), 1),
                ("e2".to_string(), 3),
                ("e3".to_string(), 1),
            ]
        );
    }

    #[test]
    fn parse_traverse_entities_empty_on_malformed_body() {
        assert!(parse_traverse_entities(&serde_json::json!({})).is_empty());
        assert!(parse_traverse_entities(&serde_json::json!({"entities": "nope"})).is_empty());
    }

    /// The regression this split exists for.
    ///
    /// A 403 from one under-scoped service principal used to count as a breaker
    /// failure, so three of them disabled the deep graph tier for EVERY caller —
    /// including end users whose tokens are authorized for it (graph-index only
    /// enforces `graph:read` on service principals). Observed live: 18 x 403
    /// from one principal, then 39 x "breaker open" on unrelated queries.
    #[test]
    fn an_answered_request_never_penalises_the_breaker() {
        for code in [401u16, 403, 400, 404, 409, 422] {
            let status = reqwest::StatusCode::from_u16(code).expect("valid status");
            let result: Result<(), TraverseError> = Err(TraverseError::Status(status));
            assert_eq!(
                breaker_verdict(&result),
                BreakerVerdict::Healthy,
                "{code} is a per-request fault; graph-index answered it"
            );
        }
    }

    /// Capacity and health signals still open the breaker — that is its job.
    #[test]
    fn server_faults_and_rate_limits_still_open_the_breaker() {
        for code in [500u16, 502, 503, 504, 429] {
            let status = reqwest::StatusCode::from_u16(code).expect("valid status");
            let result: Result<(), TraverseError> = Err(TraverseError::Status(status));
            assert_eq!(
                breaker_verdict(&result),
                BreakerVerdict::Unhealthy,
                "{code} describes graph-index's own health/capacity"
            );
        }
    }

    #[test]
    fn unreachable_and_undecodable_open_the_breaker() {
        let result: Result<(), TraverseError> =
            Err(TraverseError::Unreachable(anyhow::anyhow!("timed out")));
        assert_eq!(breaker_verdict(&result), BreakerVerdict::Unhealthy);
        let ok: Result<(), TraverseError> = Ok(());
        assert_eq!(breaker_verdict(&ok), BreakerVerdict::Healthy);
    }

    /// End-to-end on the breaker itself: a burst of 403s leaves it closed, so a
    /// later authorized caller still reaches the remote tier.
    #[test]
    fn a_burst_of_403s_leaves_the_tier_available_to_other_callers() {
        let b = Breaker::new();
        for _ in 0..10 {
            let denied: Result<(), TraverseError> =
                Err(TraverseError::Status(reqwest::StatusCode::FORBIDDEN));
            match breaker_verdict(&denied) {
                BreakerVerdict::Healthy => b.record_success(),
                BreakerVerdict::Unhealthy => b.record_failure(0),
            }
        }
        assert!(
            !b.is_open(0),
            "an authz gap on one principal must not disable the tier globally"
        );
    }

    #[test]
    fn breaker_opens_after_threshold_failures_and_resets_on_success() {
        let b = Breaker::new();
        // Below threshold: stays closed.
        b.record_failure(0);
        b.record_failure(0);
        assert!(!b.is_open(0), "closed before threshold");
        // Threshold reached: opens for the cooldown window.
        b.record_failure(0);
        assert!(b.is_open(0), "open at trip time");
        assert!(b.is_open(BREAKER_COOLDOWN_MS - 1), "open within cooldown");
        assert!(
            !b.is_open(BREAKER_COOLDOWN_MS),
            "closed after cooldown elapses"
        );
        // A success clears the failure streak and any open window.
        b.record_failure(1000);
        b.record_failure(1000);
        b.record_success();
        b.record_failure(2000);
        b.record_failure(2000);
        assert!(
            !b.is_open(2000),
            "success reset the consecutive-failure count"
        );
    }
}
