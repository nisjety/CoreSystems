//! Telemetry initialization for Model Plane services.
//!
//! Configures `tracing-subscriber` with JSON formatting and an optional
//! OpenTelemetry OTLP exporter controlled by environment variables.
//!
//! # Environment variables
//!
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` — OTLP gRPC endpoint (e.g. `http://localhost:4317`).
//!   If unset, the OTLP layer is skipped.
//! - `OTEL_SERVICE_NAME` — override the service name reported to the collector.
//! - `RUST_LOG` — controls the env-filter (default: `info`).

use anyhow::Result;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize the global tracing subscriber.
///
/// Call once at service startup. Returns the `TracerProvider` guard
/// that must be kept alive for the duration of the process; dropping
/// it flushes pending spans.
///
/// # Errors
///
/// Returns an error if the OTLP exporter cannot be configured.
pub fn init(service_name: &str) -> Result<Option<opentelemetry_sdk::trace::TracerProvider>> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_thread_ids(true);

    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    let (otel_layer, provider) = if let Some(endpoint) = otel_endpoint {
        let exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(endpoint);

        let svc_name =
            std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| service_name.to_owned());

        let trace_config = opentelemetry_sdk::trace::Config::default().with_resource(
            opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                "service.name",
                svc_name,
            )]),
        );

        let provider = opentelemetry_sdk::trace::TracerProvider::builder()
            .with_batch_exporter(
                exporter.build_span_exporter()?,
                opentelemetry_sdk::runtime::Tokio,
            )
            .with_config(trace_config)
            .build();

        let tracer = provider.tracer(service_name.to_owned());
        let layer = tracing_opentelemetry::layer().with_tracer(tracer);

        (Some(layer), Some(provider))
    } else {
        (None, None)
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();

    Ok(provider)
}
