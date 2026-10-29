//! OpenTelemetry / OTLP tracing setup.
//!
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, exports spans via gRPC.
//! Otherwise falls back to tracing-subscriber's JSON fmt layer only.

use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init_telemetry() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().json();

    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok() {
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
