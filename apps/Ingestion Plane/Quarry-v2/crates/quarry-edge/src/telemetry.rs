//! OpenTelemetry / OTLP tracing setup.
//!
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, exports spans via gRPC.
//! Otherwise falls back to tracing-subscriber's JSON fmt layer only.
//!
//! Before adding counters anywhere in this crate: the `metrics` and
//! `metrics-exporter-prometheus` dependencies in `Cargo.toml` are inert. No
//! recorder is ever installed and no `/metrics` endpoint is served, so
//! `metrics::counter!` compiles fine, records nothing, and leaves the call
//! site reading as instrumented — which is how the search path shipped with
//! no per-provider observability at all. Until a recorder is installed here,
//! per-request observability goes out as `tracing` events with explicit
//! low-cardinality fields (see the `search.upstream` / `search.result_shape`
//! events in `search_routes`), which the layers below already export.
//!
//! This module is declared in `main.rs` only, not in `lib.rs`, so the source
//! files shared by both targets cannot reference it — hence those helpers
//! living next to their call site rather than here.

use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init_telemetry() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().json();

    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty())
        .is_some()
    {
        match try_init_otel() {
            Ok(provider) => {
                let tracer = provider.tracer("quarry-edge");
                let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(fmt_layer)
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

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

fn try_init_otel() -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    let exporter = SpanExporter::builder().with_tonic().build()?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    Ok(provider)
}
