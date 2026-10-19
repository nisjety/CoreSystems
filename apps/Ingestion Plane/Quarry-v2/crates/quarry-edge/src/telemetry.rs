//! OpenTelemetry / OTLP tracing setup.
//!
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, exports spans via gRPC.
//! Otherwise falls back to tracing-subscriber's JSON fmt layer only.
//!
//! Before adding counters anywhere in this crate: the `metrics` facade only
//! records when a recorder is installed, and `crate::metrics` installs one only
//! when `QUARRY_EDGE__METRICS_ENABLED=1` (and serves it on a separate internal
//! listener, never the public port). With it off, `metrics::counter!` compiles
//! fine, records nothing, and leaves the call site reading as instrumented —
//! which is how the search path shipped with no per-provider observability at
//! all. The series that exist are the ones described in `crate::metrics`;
//! anything else (the `search.upstream` / `search.result_shape` events in
//! `search_routes`, for one) still goes out as `tracing` events with explicit
//! low-cardinality fields, which the layers below already export. Label values
//! must come from a closed set: never a URL, host, org, user or run id.
//!
//! This module is declared in `main.rs` only, not in `lib.rs`, so the source
//! files shared by both targets cannot reference it — hence those helpers
//! living next to their call site rather than here.

use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// `RUST_LOG` (default `info`), with the dependencies that print page content or
/// URLs at debug capped regardless (see `crate::zdr_log::harden`).
///
/// This is handed to each OUTPUT layer as its own filter and never added to the
/// registry. A level filter on the registry decides which spans exist: at
/// `RUST_LOG=error` the info-level request span was never created, the ZDR mark
/// had nowhere to live, and every sensitive-named error event was written for a
/// ZDR caller. (The gateway had the same wiring; commit d8a9708d fixed it there.)
fn env_filter() -> EnvFilter {
    crate::zdr_log::harden(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
    )
}

pub fn init_telemetry() {
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty())
        .is_some()
    {
        match try_init_otel() {
            Ok(provider) => {
                let tracer = provider.tracer("quarry-edge");
                // The exporter gets the same filter as the log writer (level
                // filter, caps and ZDR rule), as its own per-layer filter.
                let otel_layer = tracing_opentelemetry::layer()
                    .with_tracer(tracer)
                    .with_filter(crate::zdr_log::export_filter(env_filter()));
                // ZdrMarkLayer makes the mark on a request span sticky (see
                // `crate::zdr_log`); it carries no filter.
                tracing_subscriber::registry()
                    .with(crate::zdr_log::ZdrMarkLayer)
                    .with(crate::zdr_log::log_layer(std::io::stdout, env_filter()))
                    .with(otel_layer)
                    .init();
                tracing::info!("OTEL tracing enabled");
                return;
            }
            Err(e) => {
                eprintln!("OTEL init failed, falling back to fmt: {e}");
            }
        }
    }

    crate::zdr_log::production_subscriber(std::io::stdout, env_filter()).init();
}

fn try_init_otel() -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    let exporter = SpanExporter::builder().with_tonic().build()?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    Ok(provider)
}
