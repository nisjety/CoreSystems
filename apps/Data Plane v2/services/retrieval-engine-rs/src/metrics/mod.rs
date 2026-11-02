use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;

pub fn init_metrics() -> PrometheusHandle {
    let builder = PrometheusBuilder::new();
    let handle = builder
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    describe_metrics();
    PrometheusHandle(handle)
}

fn describe_metrics() {
    metrics::describe_counter!("dpv2_retrieval_requests_total", "Total retrieval requests");
    metrics::describe_histogram!(
        "dpv2_retrieval_duration_seconds",
        "Retrieval request latency in seconds"
    );
    metrics::describe_counter!("dpv2_retrieval_errors_total", "Total retrieval errors");
    metrics::describe_counter!(
        "dpv2_trace_persist_failures_total",
        "Trace persistence failures (retrieval succeeded but trace write failed)"
    );
    metrics::describe_counter!(
        "dpv2_retrieval_zero_results_total",
        "Retrieval requests that returned zero candidates"
    );
    metrics::describe_gauge!(
        "dpv2_retrieval_candidates_count",
        "Number of candidates returned"
    );
    metrics::describe_counter!("dpv2_embed_requests_total", "Total embedding API calls");
    metrics::describe_counter!("dpv2_embed_cache_hits_total", "Embedding cache hits");
    metrics::describe_counter!("dpv2_embed_retries_total", "Embedding retry attempts");
    metrics::describe_counter!("dpv2_rerank_requests_total", "Total rerank API calls");
    metrics::describe_counter!(
        "dpv2_grpc_requests_total",
        "Total gRPC requests by service and method"
    );
    metrics::describe_histogram!(
        "dpv2_grpc_duration_seconds",
        "gRPC request latency in seconds"
    );
    metrics::describe_gauge!("dpv2_active_connections", "Current active connections");
    metrics::describe_counter!(
        "dpv2_rate_limit_denied_total",
        "Requests rejected (429) by the per-org rate limiter"
    );
    metrics::describe_counter!(
        "dpv2_rate_limiter_backend_unavailable_total",
        "Rate limiter checks that failed open because Dragonfly was unreachable or the script errored"
    );
    metrics::describe_counter!(
        "dpv2_authz_denials_total",
        "Authenticated requests refused at admission, by org and cause"
    );
    metrics::describe_counter!(
        "dpv2_service_principal_requests_total",
        "Service-principal requests by service id and admission outcome (admitted or the denial cause)"
    );
    metrics::describe_counter!(
        "dpv2_web_rerank_total",
        "POST /v1/rerank outcomes by outcome (ok, skipped, degraded) and a closed reason list"
    );
    metrics::describe_histogram!(
        "dpv2_web_rerank_duration_seconds",
        "POST /v1/rerank request latency in seconds"
    );
    metrics::describe_histogram!(
        "dpv2_web_rerank_documents",
        "Candidate documents per POST /v1/rerank request"
    );
    metrics::describe_counter!(
        "dpv2_web_rerank_search_units_total",
        "Search units the provider reported as billed for POST /v1/rerank"
    );
    metrics::describe_counter!(
        "dpv2_web_rerank_budget_exceeded_total",
        "Orgs whose daily web-rerank unit total passed the alert threshold (alert only)"
    );
    metrics::describe_counter!(
        "dpv2_web_rerank_cost_event_failures_total",
        "Successful web reranks whose signed cost event could not be published"
    );
    metrics::describe_counter!(
        "dpv2_rerank_provider_refused_total",
        "Provider calls the rerank gate refused before the provider saw them, by lane (web, tenant) and reason (saturated, throttled, circuit_open, tenant_wait_expired)"
    );
    metrics::describe_gauge!(
        "dpv2_rerank_provider_calls_in_flight",
        "Rerank provider calls admitted and not yet settled, by lane (web, tenant)"
    );
    metrics::describe_counter!(
        "dpv2_rerank_breaker_opened_total",
        "Times the rerank provider breaker opened on this replica"
    );
    metrics::describe_gauge!(
        "dpv2_rerank_breaker_state",
        "Rerank provider breaker state on this replica: 0 closed, 1 half-open, 2 open"
    );
}

pub struct PrometheusHandle(metrics_exporter_prometheus::PrometheusHandle);

impl PrometheusHandle {
    pub fn render(&self) -> String {
        self.0.render()
    }
}

pub fn record_retrieval(duration_secs: f64, candidate_count: usize, org_id: &str) {
    counter!("dpv2_retrieval_requests_total", "org_id" => org_id.to_string()).increment(1);
    histogram!("dpv2_retrieval_duration_seconds", "org_id" => org_id.to_string())
        .record(duration_secs);
    gauge!("dpv2_retrieval_candidates_count", "org_id" => org_id.to_string())
        .set(candidate_count as f64);
}

#[allow(dead_code)] // surfaced from error paths still being wired up
pub fn record_retrieval_error(org_id: &str) {
    counter!("dpv2_retrieval_errors_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_trace_persist_failure(org_id: &str) {
    counter!("dpv2_trace_persist_failures_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_zero_results(org_id: &str) {
    counter!("dpv2_retrieval_zero_results_total", "org_id" => org_id.to_string()).increment(1);
}

/// Retrieval-result cache outcomes, counted per org so a tenant whose hit rate
/// collapses (churning corpus bumping `org_version`, or a per-user grant set
/// that fragments the scope) is visible without reading logs. Hits and misses
/// are separate counters rather than a ratio gauge so both the rate and the
/// absolute volume survive aggregation.
pub fn record_retrieval_cache_hit(org_id: &str) {
    counter!("dpv2_retrieval_cache_hits_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_retrieval_cache_miss(org_id: &str) {
    counter!("dpv2_retrieval_cache_misses_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_audit_write_failure(org_id: &str) {
    counter!("dpv2_audit_write_failures_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_authz_denial(org_id: &str, cause: &str) {
    counter!(
        "dpv2_authz_denials_total",
        "org_id" => org_id.to_string(),
        "cause" => cause.to_string()
    )
    .increment(1);
}

/// One service-principal request at the admission guard. `outcome` is
/// `admitted` or the denial cause (`denied:service_route_not_allowed`,
/// `denied:service_scope_missing`, ...). The label set is bounded: service ids
/// come from Control's registry and the outcomes are a closed list.
pub fn record_service_principal_request(service_id: &str, outcome: &str) {
    counter!(
        "dpv2_service_principal_requests_total",
        "service_id" => service_id.to_string(),
        "outcome" => outcome.to_string()
    )
    .increment(1);
}

pub fn record_embed_request() {
    counter!("dpv2_embed_requests_total").increment(1);
}

pub fn record_embed_cache_hit() {
    counter!("dpv2_embed_cache_hits_total").increment(1);
}

#[allow(dead_code)] // called once embed retry loop is enabled
pub fn record_embed_retry() {
    counter!("dpv2_embed_retries_total").increment(1);
}

pub fn record_rerank_request() {
    counter!("dpv2_rerank_requests_total").increment(1);
}

pub fn record_rate_limit_denied(org_id: &str) {
    counter!("dpv2_rate_limit_denied_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_rate_limit_backend_unavailable() {
    counter!("dpv2_rate_limiter_backend_unavailable_total").increment(1);
}

/// `POST /v1/rerank` outcome. `outcome` is `ok`, `skipped` or `degraded`; `reason` is a
/// closed list (`none`, `replay`, a skip reason or a degrade reason), so the labels stay bounded.
pub fn record_web_rerank(outcome: &str, reason: &str) {
    counter!(
        "dpv2_web_rerank_total",
        "outcome" => outcome.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

pub fn record_web_rerank_duration(duration_secs: f64) {
    histogram!("dpv2_web_rerank_duration_seconds").record(duration_secs);
}

pub fn record_web_rerank_documents(documents: u64) {
    histogram!("dpv2_web_rerank_documents").record(documents as f64);
}

pub fn record_web_rerank_search_units(units: u64) {
    counter!("dpv2_web_rerank_search_units_total").increment(units);
}

pub fn record_web_rerank_budget_exceeded(org_id: &str) {
    counter!("dpv2_web_rerank_budget_exceeded_total", "org_id" => org_id.to_string()).increment(1);
}

pub fn record_web_rerank_cost_failure() {
    counter!("dpv2_web_rerank_cost_event_failures_total").increment(1);
}

/// A provider call the rerank gate refused before the provider saw it. Both labels are closed
/// lists: `lane` is `web` or `tenant`, `reason` one of the gate's refusals.
pub fn record_provider_refused(lane: &'static str, reason: &'static str) {
    counter!(
        "dpv2_rerank_provider_refused_total",
        "lane" => lane,
        "reason" => reason
    )
    .increment(1);
}

pub fn record_provider_call_started(lane: &'static str) {
    gauge!("dpv2_rerank_provider_calls_in_flight", "lane" => lane).increment(1.0);
}

pub fn record_provider_call_finished(lane: &'static str) {
    gauge!("dpv2_rerank_provider_calls_in_flight", "lane" => lane).decrement(1.0);
}

pub fn record_breaker_opened() {
    counter!("dpv2_rerank_breaker_opened_total").increment(1);
}

/// The breaker state as its gauge value: 0 closed, 1 half-open, 2 open.
pub fn record_breaker_state(code: f64) {
    gauge!("dpv2_rerank_breaker_state").set(code);
}

#[allow(dead_code)] // gRPC client interceptor will call this; instrumentation in progress
pub fn record_grpc_request(service: &str, method: &str, duration_secs: f64) {
    counter!(
        "dpv2_grpc_requests_total",
        "service" => service.to_string(),
        "method" => method.to_string()
    )
    .increment(1);
    histogram!(
        "dpv2_grpc_duration_seconds",
        "service" => service.to_string(),
        "method" => method.to_string()
    )
    .record(duration_secs);
}
