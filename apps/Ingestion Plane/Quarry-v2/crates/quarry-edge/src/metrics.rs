//! Prometheus metrics: the recorder, an internal-only listener, an HTTP layer
//! and a pool sampler.
//!
//! Until this module the `metrics` and `metrics-exporter-prometheus`
//! dependencies were inert: no recorder was installed and no `/metrics` was
//! served, so any `metrics::counter!` compiled to a silent no-op. Turning the
//! recorder on is therefore opt-in and guarded:
//!
//! - **Separate listener.** `/metrics` is served on `QUARRY_EDGE__METRICS_ADDR`
//!   (default `0.0.0.0:9092`), never on the public router, so the public port
//!   answers 404 for it and the ZDR and OpenAPI scans of the public routes do
//!   not see it. The port is `expose`d inside the compose network, never
//!   `ports`-published.
//! - **Bearer token.** `QUARRY_EDGE__METRICS_TOKEN`, compared in constant
//!   time. In production an enabled listener without a token of at least
//!   [`MIN_PRODUCTION_TOKEN_LEN`] characters refuses to start.
//! - **Closed label sets.** Every label value comes from a fixed, small set
//!   (a route class from an allowlist, a status class, a pool name, a driver
//!   or error code from an enum). A URL, host, organization, user, run id or
//!   message is never a label: those belong in traces, and a label built from
//!   them grows the series count without bound and leaks what a tenant
//!   browses.
//!
//! The recorder is process-global, so tests that install it live in their own
//! test binary (`tests/metrics_recorder.rs`).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use quarry_runtime::lease_pool::RuntimeLeasePool;
use sha2::{Digest, Sha256};

pub const DEFAULT_METRICS_ADDR: &str = "0.0.0.0:9092";
pub const MIN_PRODUCTION_TOKEN_LEN: usize = 16;
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

const ENV_ENABLED: &str = "QUARRY_EDGE__METRICS_ENABLED";
const ENV_ADDR: &str = "QUARRY_EDGE__METRICS_ADDR";
const ENV_TOKEN: &str = "QUARRY_EDGE__METRICS_TOKEN";

/// Where and whether to serve metrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub addr: String,
    pub token: Option<String>,
}

impl MetricsConfig {
    /// Read the configuration through `get` (the process environment in
    /// `main`). Off unless explicitly enabled, so a rollback is
    /// `QUARRY_EDGE__METRICS_ENABLED=0` and an unset variable changes nothing.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let enabled = match get(ENV_ENABLED).as_deref().map(str::trim) {
            None | Some("") => false,
            Some(value) => match value.to_ascii_lowercase().as_str() {
                "1" | "true" | "on" | "yes" => true,
                "0" | "false" | "off" | "no" => false,
                _ => return Err(format!("{ENV_ENABLED} must be 1/0 (true/false), got {value:?}")),
            },
        };
        let addr = get(ENV_ADDR)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_METRICS_ADDR.to_owned());
        if enabled && addr.parse::<SocketAddr>().is_err() {
            return Err(format!("{ENV_ADDR} must be a socket address like 0.0.0.0:9092"));
        }
        let token = get(ENV_TOKEN)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        Ok(Self { enabled, addr, token })
    }

    /// Production must not expose an unauthenticated or guessable endpoint.
    pub fn validate(&self, production: bool) -> Result<(), String> {
        if !(self.enabled && production) {
            return Ok(());
        }
        match self.token.as_deref() {
            None => Err(format!(
                "{ENV_TOKEN} is required in production when {ENV_ENABLED}=1"
            )),
            Some(token) if token.len() < MIN_PRODUCTION_TOKEN_LEN => Err(format!(
                "{ENV_TOKEN} must be at least {MIN_PRODUCTION_TOKEN_LEN} characters in production"
            )),
            Some(_) => Ok(()),
        }
    }
}

/// The route label for a request path: a fixed set, never the raw path.
///
/// An allowlist of the first path segments (under `/v1/` where it applies) so
/// an attacker probing random URLs, or a path that embeds an id, cannot mint a
/// new series. Anything not listed is `other`.
pub fn route_class(path: &str) -> &'static str {
    const V1: &[&str] = &[
        "agent",
        "ai",
        "answer",
        "artifacts",
        "audio",
        "batch",
        "benchmarks",
        "change",
        "crawl",
        "extract",
        "fleets",
        "internal",
        "jobs",
        "map",
        "profiles",
        "request-queues",
        "runs",
        "schedules",
        "scrape",
        "search",
        "snapshots",
        "sources",
        "team",
        "webhooks",
    ];
    const ROOT: &[&str] = &["health", "ready", "version", "graphql", "jwks", "frames"];

    let mut segments = path.trim_start_matches('/').split('/');
    match segments.next() {
        Some("v1") => {
            let second = segments.next().unwrap_or("");
            V1.iter().find(|name| **name == second).copied().unwrap_or("v1_other")
        }
        Some(first) => ROOT.iter().find(|name| **name == first).copied().unwrap_or("other"),
        None => "other",
    }
}

/// `2xx`, `3xx`, `4xx`, `5xx`, or `other`.
pub fn status_class(code: u16) -> &'static str {
    match code {
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

/// Count and time every request by route class and status class.
///
/// Layered with `Router::layer` (outermost) so a request rejected by an
/// authentication layer is counted too.
pub async fn http_middleware(request: Request, next: Next) -> Response {
    let route = route_class(request.uri().path());
    let method = match request.method().as_str() {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "PATCH" => "PATCH",
        "DELETE" => "DELETE",
        _ => "OTHER",
    };
    let started = Instant::now();
    let response = next.run(request).await;
    let status = status_class(response.status().as_u16());
    metrics::counter!("quarry_http_requests_total", "route" => route, "method" => method, "status_class" => status)
        .increment(1);
    metrics::histogram!("quarry_http_request_duration_seconds", "route" => route, "status_class" => status)
        .record(started.elapsed().as_secs_f64());
    response
}

/// Record the result of one `/v1/scrape`. `outcome` is `ok` or the error
/// code's wire name (a closed enum).
pub fn record_scrape(outcome: &str) {
    metrics::counter!("quarry_scrape_outcomes_total", "outcome" => outcome.to_owned()).increment(1);
}

/// The wire name of an error code (for [`record_scrape`]); `unknown` if it
/// cannot be serialized.
pub fn error_label(code: quarry_core::error::ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_ascii_lowercase))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Install the process-global recorder. Fails if one is already installed.
pub fn install() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Suffix("_seconds".to_owned()),
            &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0],
        )?
        .install_recorder()?;
    describe_all();
    metrics::gauge!("quarry_build_info", "version" => env!("CARGO_PKG_VERSION")).set(1.0);
    Ok(handle)
}

fn describe_all() {
    use metrics::{describe_counter, describe_gauge, describe_histogram, Unit};
    describe_counter!("quarry_http_requests_total", "HTTP requests by route class, method and status class.");
    describe_histogram!(
        "quarry_http_request_duration_seconds",
        Unit::Seconds,
        "HTTP request duration by route class and status class."
    );
    describe_counter!("quarry_scrape_outcomes_total", "Single-page scrapes by outcome (ok or an error code).");
    describe_gauge!("quarry_lease_pool_in_flight", "Leases currently held, by pool.");
    describe_gauge!("quarry_lease_pool_peak_in_flight", "Highest in-flight lease count seen, by pool.");
    describe_gauge!("quarry_lease_pool_capacity", "Concurrent leases a pool allows.");
    describe_counter!("quarry_lease_pool_acquired_total", "Leases handed out, by pool.");
    describe_gauge!("quarry_build_info", "Always 1; the version is the label.");
}

fn pools() -> &'static Mutex<Vec<(&'static str, RuntimeLeasePool)>> {
    static POOLS: OnceLock<Mutex<Vec<(&'static str, RuntimeLeasePool)>>> = OnceLock::new();
    POOLS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Make a lease pool visible to the sampler. `name` is a fixed label.
pub fn register_pool(name: &'static str, pool: RuntimeLeasePool) {
    if let Ok(mut registered) = pools().lock() {
        registered.push((name, pool));
    }
}

/// Publish every registered pool's state once.
pub fn sample_pools() {
    let snapshot: Vec<_> = match pools().lock() {
        Ok(registered) => registered.clone(),
        Err(_) => return,
    };
    for (name, pool) in snapshot {
        let counters = pool.metrics_snapshot();
        metrics::gauge!("quarry_lease_pool_in_flight", "pool" => name).set(counters.in_flight as f64);
        metrics::gauge!("quarry_lease_pool_peak_in_flight", "pool" => name)
            .set(counters.peak_in_flight as f64);
        metrics::gauge!("quarry_lease_pool_capacity", "pool" => name).set(pool.max_concurrent() as f64);
        metrics::counter!("quarry_lease_pool_acquired_total", "pool" => name).absolute(counters.acquired);
    }
}

/// Sample the pools every [`SAMPLE_INTERVAL`] until the process exits.
pub fn spawn_sampler() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            sample_pools();
        }
    })
}

#[derive(Clone)]
struct MetricsState {
    handle: PrometheusHandle,
    token_digest: Option<Arc<[u8; 32]>>,
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// Constant-time equality over two fixed-size digests, so the comparison time
/// does not depend on how much of a guessed token is right or how long it is.
fn digests_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter().zip(right).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn authorized(state: &MetricsState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.token_digest.as_deref() else {
        return true;
    };
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|presented| digests_equal(&digest(presented.trim()), expected))
}

async fn metrics_handler(State(state): State<MetricsState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "unauthorized",
        )
            .into_response();
    }
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        state.handle.render(),
    )
        .into_response()
}

/// The internal metrics router: `/metrics` (bearer) and `/healthz`.
pub fn router(handle: PrometheusHandle, token: Option<&str>) -> Router {
    let state = MetricsState {
        handle,
        token_digest: token.map(|token| Arc::new(digest(token))),
    };
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(vars: &[(&str, &str)]) -> Result<MetricsConfig, String> {
        let vars: HashMap<String, String> =
            vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        MetricsConfig::from_env(|name| vars.get(name).cloned())
    }

    #[test]
    fn metrics_are_off_unless_explicitly_enabled() {
        let off = cfg(&[]).unwrap();
        assert!(!off.enabled);
        assert_eq!(off.addr, DEFAULT_METRICS_ADDR);
        assert!(!cfg(&[(ENV_ENABLED, "")]).unwrap().enabled);
        assert!(!cfg(&[(ENV_ENABLED, "0")]).unwrap().enabled);
        for on in ["1", "true", "ON", "Yes"] {
            assert!(cfg(&[(ENV_ENABLED, on)]).unwrap().enabled, "{on}");
        }
    }

    #[test]
    fn a_garbled_flag_or_address_stops_the_edge_naming_the_variable() {
        assert!(cfg(&[(ENV_ENABLED, "maybe")]).unwrap_err().contains(ENV_ENABLED));
        assert!(cfg(&[(ENV_ENABLED, "1"), (ENV_ADDR, "nine-oh-nine-two")])
            .unwrap_err()
            .contains(ENV_ADDR));
        // The address is only parsed when metrics are on.
        assert!(cfg(&[(ENV_ADDR, "garbage")]).is_ok());
    }

    #[test]
    fn production_refuses_to_start_without_a_metrics_token() {
        let enabled = cfg(&[(ENV_ENABLED, "1")]).unwrap();
        assert!(enabled.validate(true).unwrap_err().contains(ENV_TOKEN));
        let short = cfg(&[(ENV_ENABLED, "1"), (ENV_TOKEN, "short")]).unwrap();
        assert!(short.validate(true).is_err());
        let long_enough = "t".repeat(MIN_PRODUCTION_TOKEN_LEN);
        let ok = cfg(&[(ENV_ENABLED, "1"), (ENV_TOKEN, long_enough.as_str())]).unwrap();
        assert!(ok.validate(true).is_ok());
        // Development may run open; a disabled listener needs nothing.
        assert!(enabled.validate(false).is_ok());
        assert!(cfg(&[]).unwrap().validate(true).is_ok());
    }

    #[test]
    fn route_label_is_a_closed_set_never_the_raw_path() {
        for (path, want) in [
            ("/v1/scrape", "scrape"),
            ("/v1/agent/runs/run_abc123/step", "agent"),
            ("/v1/crawl", "crawl"),
            ("/v1/artifacts/art_1/content", "artifacts"),
            ("/health", "health"),
            ("/ready", "ready"),
            ("/graphql/playground", "graphql"),
            ("/v1/some-host.example.com/secret", "v1_other"),
            ("/v1", "v1_other"),
            ("/wp-admin/../../etc/passwd", "other"),
            ("/", "other"),
            ("", "other"),
        ] {
            assert_eq!(route_class(path), want, "{path}");
        }
    }

    #[test]
    fn status_classes_are_closed() {
        assert_eq!(status_class(200), "2xx");
        assert_eq!(status_class(302), "3xx");
        assert_eq!(status_class(404), "4xx");
        assert_eq!(status_class(503), "5xx");
        assert_eq!(status_class(99), "other");
        assert_eq!(status_class(700), "other");
    }

    #[test]
    fn digest_comparison_matches_only_equal_tokens() {
        let a = digest("token-one-aaaaaaaa");
        assert!(digests_equal(&a, &digest("token-one-aaaaaaaa")));
        assert!(!digests_equal(&a, &digest("token-one-aaaaaaab")));
        assert!(!digests_equal(&a, &digest("")));
    }

    #[test]
    fn error_labels_are_the_wire_names() {
        use quarry_core::error::ErrorCode;
        assert_eq!(error_label(ErrorCode::RateLimited), "rate_limited");
        assert_eq!(error_label(ErrorCode::UnsupportedMedia), "unsupported_media");
    }
}
