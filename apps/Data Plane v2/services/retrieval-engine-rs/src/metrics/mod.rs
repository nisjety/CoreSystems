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
