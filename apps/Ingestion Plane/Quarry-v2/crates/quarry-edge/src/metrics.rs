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
//!   (the router's matched route template or `unmatched`, a status class, a
//!   pool name, an action class, a driver or error code from an enum). A URL, host, organization, user, run id or
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
    extract::{MatchedPath, Request, State},
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
                _ => {
                    return Err(format!(
                        "{ENV_ENABLED} must be 1/0 (true/false), got {value:?}"
                    ))
                }
            },
        };
        let addr = get(ENV_ADDR)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_METRICS_ADDR.to_owned());
        if enabled && addr.parse::<SocketAddr>().is_err() {
            return Err(format!(
                "{ENV_ADDR} must be a socket address like 0.0.0.0:9092"
            ));
        }
        let token = get(ENV_TOKEN)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        Ok(Self {
            enabled,
            addr,
            token,
        })
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

/// The route label of a request that matched no route.
pub const UNMATCHED_ROUTE: &str = "unmatched";

/// The route label for a request: the router's matched template (for example
/// `/v1/agent/runs/:run_id/step`), never the raw path.
///
/// A template can only be one the route table registered, so the label set is
/// as large as the route table and no larger: an attacker probing random URLs,
/// or a path that embeds an id, matches no route and becomes `unmatched`. A
/// template that does not look like one (empty, not rooted, absurdly long) is
/// also `unmatched`.
pub fn route_label(matched: Option<&str>) -> String {
    match matched {
        Some(template) if template.starts_with('/') && template.len() <= 160 => {
            template.to_owned()
        }
        _ => UNMATCHED_ROUTE.to_owned(),
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
    let route = route_label(
        request
            .extensions()
            .get::<MatchedPath>()
            .map(MatchedPath::as_str),
    );
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
    metrics::counter!("quarry_http_requests_total", "route" => route.clone(), "method" => method, "status_class" => status)
        .increment(1);
    metrics::histogram!("quarry_http_request_duration_seconds", "route" => route, "status_class" => status)
        .record(started.elapsed().as_secs_f64());
    response
}

/// A run reached a budget bound. `dimension` is steps, attempts, runtime or
/// cost; `class` is interactive or agent; `mode` is `on` (refused) or
/// `shadow` (would have been). All three come from closed sets.
pub fn record_budget_exhausted(dimension: &'static str, class: &'static str, mode: &'static str) {
    metrics::counter!(
        "quarry_agent_budget_exhausted_total",
        "dimension" => dimension,
        "class" => class,
        "mode" => mode
    )
    .increment(1);
}

/// The run classes a series may carry; anything else is `other`.
fn run_class_label(class: &str) -> &'static str {
    match class {
        "interactive" => "interactive",
        "agent" => "agent",
        _ => "other",
    }
}

/// An agent run start. `outcome` is `started`, `resumed`, `denied` (a cap or an
/// allowance refused it), `draining` (the edge is shutting down) or `aborted`
/// (admitted, then failed before the run was registered). WS6.I.
pub fn record_run_start(class: &str, outcome: &str) {
    let outcome = match outcome {
        "started" => "started",
        "resumed" => "resumed",
        "denied" => "denied",
        "draining" => "draining",
        "aborted" => "aborted",
        _ => "other",
    };
    metrics::counter!(
        "quarry_agent_run_starts_total",
        "class" => run_class_label(class),
        "outcome" => outcome
    )
    .increment(1);
}

/// An agent run start refused. `reason` is a `details.reason` of the typed 429/402
/// (the closed list in `docs/ERROR_REASONS.md`); an unknown reason is `other`.
pub fn record_run_denied(reason: &str) {
    let reason = match reason {
        "global_run_cap" => "global_run_cap",
        "org_run_cap" => "org_run_cap",
        "org_interactive_run_cap" => "org_interactive_run_cap",
        "org_plan_cap" => "org_plan_cap",
        "org_allowance" => "org_allowance",
        "draining" => "draining",
        _ => "other",
    };
    metrics::counter!("quarry_agent_run_denied_total", "reason" => reason).increment(1);
}

/// `count` runs ended without a client closing them. `reason` is the
/// `agent.expired` reason: `idle`, `ttl`, `takeover_idle`, `max_age` or `shutdown`.
pub fn record_run_expired(reason: &str, count: u64) {
    let reason = match reason {
        "idle" => "idle",
        "ttl" => "ttl",
        "takeover_idle" => "takeover_idle",
        "max_age" => "max_age",
        "shutdown" => "shutdown",
        _ => "other",
    };
    metrics::counter!("quarry_agent_run_expired_total", "reason" => reason).increment(count);
}

/// The coarse class of an agent action, from its wire name
/// (`browser_action_name`): a closed set of eleven, so no step metric grows
/// with the action vocabulary.
pub fn action_class(action: &str) -> &'static str {
    match action {
        "navigate" | "back" | "forward" | "reload" => "navigate",
        "click" | "click_ref" | "frame_click_ref" | "click_semantic" | "click_point" => "click",
        "type" | "type_ref" | "frame_type_ref" | "type_semantic" => "type",
        "press" | "respond_dialog" => "press",
        "scroll" | "mouse_wheel" => "scroll",
        "select" | "select_ref" | "frame_select_ref" | "select_semantic" => "select",
        "wait" | "wait_for" | "wait_for_ref" | "frame_wait_for_ref" | "wait_for_semantic" => {
            "wait"
        }
        "screenshot" | "pdf" | "get_content" => "capture",
        "evaluate" => "evaluate",
        "upload_ref" | "frame_upload_ref" | "download_ref" | "frame_download_ref" => "file",
        _ => "other",
    }
}

/// One executed agent step: its count and duration by action class and outcome
/// (`ok` when the driver completed it, `error` when it failed). WS6.I.
pub fn record_agent_step(action: &str, ok: bool, elapsed: Duration) {
    let class = action_class(action);
    let outcome = if ok { "ok" } else { "error" };
    metrics::counter!("quarry_agent_steps_total", "action_class" => class, "outcome" => outcome)
        .increment(1);
    metrics::histogram!(
        "quarry_agent_step_duration_seconds",
        "action_class" => class,
        "outcome" => outcome
    )
    .record(elapsed.as_secs_f64());
}

/// A cache lookup. `cache` is `page` or `search`; `outcome` is `hit`, `miss` or
/// `error` (backend or decode failure, served as a miss). WS6.I.
pub fn record_cache(cache: &'static str, outcome: &'static str) {
    let cache = match cache {
        "page" => "page",
        "search" => "search",
        _ => "other",
    };
    let outcome = match outcome {
        "hit" => "hit",
        "miss" => "miss",
        "error" => "error",
        _ => "other",
    };
    metrics::counter!("quarry_cache_requests_total", "cache" => cache, "outcome" => outcome)
        .increment(1);
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
            &[
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
            ],
        )?
        .install_recorder()?;
    describe_all();
    metrics::gauge!("quarry_build_info", "version" => env!("CARGO_PKG_VERSION")).set(1.0);
    Ok(handle)
}

fn describe_all() {
    use metrics::{describe_counter, describe_gauge, describe_histogram, Unit};
    describe_counter!(
        "quarry_http_requests_total",
        "HTTP requests by route class, method and status class."
    );
    describe_histogram!(
        "quarry_http_request_duration_seconds",
        Unit::Seconds,
        "HTTP request duration by route class and status class."
    );
    describe_counter!(
        "quarry_scrape_outcomes_total",
        "Single-page scrapes by outcome (ok or an error code)."
    );
    describe_counter!(
        "quarry_agent_budget_exhausted_total",
        "Agent runs that reached a budget bound, by dimension, class and mode (shadow = would have been refused)."
    );
    describe_counter!(
        "quarry_agent_run_starts_total",
        "Agent run starts by class and outcome (started, resumed, denied, draining, aborted)."
    );
    describe_counter!(
        "quarry_agent_run_denied_total",
        "Agent run starts refused, by the typed reason (a cap or an allowance)."
    );
    describe_counter!(
        "quarry_agent_run_expired_total",
        "Agent runs ended without a client closing them, by reason (idle, ttl, takeover_idle, max_age, shutdown)."
    );
    describe_gauge!(
        "quarry_agent_runs_active",
        "Live agent runs now, by class (interactive or agent)."
    );
    describe_gauge!(
        "quarry_agent_runs_pending",
        "Agent run starts admitted and not yet registered."
    );
    describe_counter!(
        "quarry_agent_steps_total",
        "Agent steps executed, by action class and outcome."
    );
    describe_histogram!(
        "quarry_agent_step_duration_seconds",
        Unit::Seconds,
        "Agent step duration by action class and outcome."
    );
    describe_histogram!(
        "quarry_agent_run_duration_seconds",
        Unit::Seconds,
        "How long a run lived, by class."
    );
    describe_counter!(
        "quarry_cache_requests_total",
        "Cache lookups by cache (page, search) and outcome (hit, miss, error)."
    );
    describe_gauge!(
        "quarry_edge_draining",
        "1 once the edge has begun draining for shutdown, otherwise 0."
    );
    describe_counter!(
        "quarry_lease_pool_refused_total",
        "Lease requests refused, by pool and reason (pool_full, org_render_cap, wait_timeout)."
    );
    describe_counter!(
        "quarry_lease_pool_waits_total",
        "Waits for a lease permit that ended, by pool."
    );
    describe_gauge!(
        "quarry_lease_pool_wait_seconds_sum",
        "Cumulative seconds spent waiting for a lease permit, by pool (divide by the waits counter for the mean)."
    );
    describe_gauge!(
        "quarry_lease_pool_in_flight",
        "Leases currently held, by pool."
    );
    describe_gauge!(
        "quarry_lease_pool_peak_in_flight",
        "Highest in-flight lease count seen, by pool."
    );
    describe_gauge!(
        "quarry_lease_pool_capacity",
        "Concurrent leases a pool allows."
    );
    describe_counter!(
        "quarry_lease_pool_acquired_total",
        "Leases handed out, by pool."
    );
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
        metrics::gauge!("quarry_lease_pool_in_flight", "pool" => name)
            .set(counters.in_flight as f64);
        metrics::gauge!("quarry_lease_pool_peak_in_flight", "pool" => name)
            .set(counters.peak_in_flight as f64);
        metrics::gauge!("quarry_lease_pool_capacity", "pool" => name)
            .set(pool.max_concurrent() as f64);
        metrics::counter!("quarry_lease_pool_acquired_total", "pool" => name)
            .absolute(counters.acquired);
        metrics::counter!("quarry_lease_pool_refused_total", "pool" => name, "reason" => "pool_full")
            .absolute(counters.refused_pool_full);
        metrics::counter!("quarry_lease_pool_refused_total", "pool" => name, "reason" => "org_render_cap")
            .absolute(counters.refused_org_cap);
        metrics::counter!("quarry_lease_pool_refused_total", "pool" => name, "reason" => "wait_timeout")
            .absolute(counters.wait_timeouts);
        metrics::counter!("quarry_lease_pool_waits_total", "pool" => name)
            .absolute(counters.waits);
        metrics::gauge!("quarry_lease_pool_wait_seconds_sum", "pool" => name)
            .set(counters.wait_nanos as f64 / 1_000_000_000.0);
    }
}

/// What the run registry reports, as the sampler needs it. The sampler takes a
/// closure instead of the registry so this module does not depend on the
/// `browser-agent` feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStats {
    pub interactive: usize,
    pub agent: usize,
    pub pending: usize,
}

type RunStatsFn = Arc<dyn Fn() -> RunStats + Send + Sync>;

fn run_stats_source() -> &'static Mutex<Option<RunStatsFn>> {
    static SOURCE: OnceLock<Mutex<Option<RunStatsFn>>> = OnceLock::new();
    SOURCE.get_or_init(|| Mutex::new(None))
}

/// Make the live-run counts visible to the sampler (`main` passes
/// `RunRegistry::stats`). A later call replaces the earlier one.
pub fn register_run_stats(source: impl Fn() -> RunStats + Send + Sync + 'static) {
    if let Ok(mut registered) = run_stats_source().lock() {
        *registered = Some(Arc::new(source));
    }
}

/// Publish the live-run gauges once.
pub fn sample_runs() {
    let source = match run_stats_source().lock() {
        Ok(registered) => registered.clone(),
        Err(_) => return,
    };
    if let Some(source) = source {
        let stats = source();
        metrics::gauge!("quarry_agent_runs_active", "class" => "interactive")
            .set(stats.interactive as f64);
        metrics::gauge!("quarry_agent_runs_active", "class" => "agent").set(stats.agent as f64);
        metrics::gauge!("quarry_agent_runs_pending").set(stats.pending as f64);
    }
}

fn robots_gate() -> &'static Mutex<Option<Arc<quarry_runtime::RobotsGate>>> {
    static GATE: OnceLock<Mutex<Option<Arc<quarry_runtime::RobotsGate>>>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(None))
}

/// Make the robots.txt gate's counts visible to the sampler (WS14.S4). A later
/// call replaces the earlier one.
pub fn register_robots(gate: Arc<quarry_runtime::RobotsGate>) {
    if let Ok(mut registered) = robots_gate().lock() {
        *registered = Some(gate);
    }
}

/// Publish the robots.txt gate's counts once. Content-free: the labels come from a
/// closed set (the mode and the outcome), never a host, a path or a URL.
pub fn sample_robots() {
    let gate = match robots_gate().lock() {
        Ok(registered) => registered.clone(),
        Err(_) => return,
    };
    let Some(gate) = gate else {
        return;
    };
    let stats = gate.stats();
    for (mode, outcome, value) in [
        ("any", "allowed", stats.allowed),
        ("enforce", "denied", stats.denied),
        ("enforce", "unreachable", stats.denied_unreachable),
        ("observe", "would_deny", stats.would_deny),
        ("observe", "would_unreachable", stats.would_deny_unreachable),
    ] {
        metrics::counter!("quarry_robots_decisions_total", "mode" => mode, "outcome" => outcome)
            .absolute(value);
    }
    metrics::gauge!("quarry_robots_cache_entries").set(gate.len() as f64);
}

/// Publish every sampled series once: the pools, the live runs, the drain flag.
pub fn sample_all() {
    sample_pools();
    sample_runs();
    sample_robots();
    metrics::gauge!("quarry_edge_draining").set(if crate::drain::is_draining() { 1.0 } else { 0.0 });
}

/// Sample the pools every [`SAMPLE_INTERVAL`] until the process exits.
pub fn spawn_sampler() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            sample_all();
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
    left.iter()
        .zip(right)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
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
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
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
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
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
        assert!(cfg(&[(ENV_ENABLED, "maybe")])
            .unwrap_err()
            .contains(ENV_ENABLED));
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
    fn route_label_is_the_matched_template_not_the_raw_path() {
        assert_eq!(
            route_label(Some("/v1/agent/runs/:run_id/step")),
            "/v1/agent/runs/:run_id/step"
        );
        assert_eq!(route_label(Some("/health")), "/health");
        // A request that matched no route, or a template that is not one.
        for bad in [None, Some(""), Some("no-slash"), Some(&"/x".repeat(100)[..])] {
            assert_eq!(route_label(bad), UNMATCHED_ROUTE, "{bad:?}");
        }
    }

    #[test]
    fn action_classes_are_a_closed_set_of_eleven() {
        let known = [
            "navigate", "back", "forward", "reload", "click", "click_ref", "frame_click_ref",
            "click_semantic", "click_point", "type", "type_ref", "frame_type_ref",
            "type_semantic", "press", "respond_dialog", "scroll", "mouse_wheel", "select",
            "select_ref", "frame_select_ref", "select_semantic", "wait", "wait_for",
            "wait_for_ref", "frame_wait_for_ref", "wait_for_semantic", "screenshot", "pdf",
            "get_content", "evaluate", "upload_ref", "frame_upload_ref", "download_ref",
            "frame_download_ref",
        ];
        let classes: std::collections::BTreeSet<_> =
            known.iter().map(|name| action_class(name)).collect();
        assert!(!classes.contains("other"), "every known action has a class: {classes:?}");
        assert!(classes.len() <= 10, "{classes:?}");
        assert_eq!(action_class("https://probe.example/x"), "other");
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
        assert_eq!(
            error_label(ErrorCode::UnsupportedMedia),
            "unsupported_media"
        );
    }
}
