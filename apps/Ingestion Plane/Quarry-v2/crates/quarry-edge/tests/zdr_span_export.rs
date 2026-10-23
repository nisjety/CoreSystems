//! WS10.S8: ZDR spans and their trace never reach the OTLP exporter, and the
//! log of a ZDR request carries no probe URL. Built on the real stack
//! (`zdr_log::otel_subscriber` with the OTel layer wrapped by `ZdrFlagLayer`,
//! `ZdrSampler`, `ZdrSpanProcessor`), with an in-memory exporter standing in for
//! OTLP. Each test owns its registry, so tests do not share the process-wide one.
//!
//! Written in the W2 build; compiled, not yet run (the test phase runs it).

use std::io::Write;
use std::sync::{Arc, Mutex};

use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor};
use quarry_edge::otel_zdr::{ZdrFlagLayer, ZdrSampler, ZdrSpanProcessor, ZdrTraceRegistry};
use quarry_edge::zdr_log;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

const ZDR_PROBE: &str = "https://zdr-probe.example.test/private-page";
const PLAIN_PROBE: &str = "https://plain-probe.example.test/public-page";

/// Captures what the JSON log layer writes.
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log lock")).into_owned()
    }
}

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct Rig {
    subscriber: Box<dyn tracing::Subscriber + Send + Sync>,
    exporter: InMemorySpanExporter,
    logs: LogCapture,
}

/// The real subscriber shape, with `export_on` as the flag and a private registry.
fn rig(export_on: bool) -> Rig {
    let registry = Arc::new(ZdrTraceRegistry::with_capacity(64));
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_sampler(ZdrSampler::new(registry.clone(), export_on))
        .with_span_processor(ZdrSpanProcessor::new(
            SimpleSpanProcessor::new(exporter.clone()),
            registry.clone(),
        ))
        .build();
    let tracer = provider.tracer("zdr-span-export-test");
    let logs = LogCapture::default();
    let subscriber = zdr_log::otel_subscriber(
        logs.clone(),
        EnvFilter::new("info"),
        EnvFilter::new("info"),
        ZdrFlagLayer::new(
            tracing_opentelemetry::layer().with_tracer(tracer),
            registry,
        ),
    );
    Rig {
        subscriber: Box::new(subscriber),
        exporter,
        logs,
    }
}

fn run(rig: Rig, body: impl FnOnce()) -> (Vec<opentelemetry_sdk::trace::SpanData>, String) {
    let Rig {
        subscriber,
        exporter,
        logs,
    } = rig;
    tracing::subscriber::with_default(subscriber, body);
    let spans = exporter.get_finished_spans().expect("spans readable");
    (spans, logs.text())
}

#[test]
fn zdr_scrape_and_its_step_export_zero_spans_and_no_probe_url() {
    let (spans, log) = run(rig(true), || {
        let request = tracing::info_span!("request", zdr = true, request_id = "req-zdr");
        let _request = request.enter();
        tracing::info!(url = ZDR_PROBE, "scrape finished");
        let step = tracing::info_span!("step", run_id = "run-zdr");
        let _step = step.enter();
        tracing::info!(url = ZDR_PROBE, "step finished");
    });
    assert!(spans.is_empty(), "a ZDR trace exports no spans, got {}", spans.len());
    assert!(
        !log.contains("zdr-probe.example.test"),
        "no log line of a ZDR request names the probe URL"
    );
}

#[test]
fn non_zdr_twin_exports_spans_and_keeps_no_contract_key() {
    let (spans, _log) = run(rig(true), || {
        let request = tracing::info_span!("request", request_id = "req-plain", url = PLAIN_PROBE);
        let _request = request.enter();
        tracing::info!(status_code = 200_i64, "scrape finished");
    });
    assert_eq!(spans.len(), 1, "the non-ZDR twin exports its span");
    let attributes: Vec<&str> = spans[0]
        .attributes
        .iter()
        .map(|kv| kv.key.as_str())
        .collect();
    assert!(
        !attributes.contains(&"url"),
        "the zdr_sensitive key is stripped from a kept span: {attributes:?}"
    );
    assert!(attributes.contains(&"request_id"));
}

#[test]
fn late_zdr_record_drops_the_whole_trace_from_then_on() {
    let (spans, _log) = run(rig(true), || {
        let request = tracing::info_span!("request", zdr = tracing::field::Empty);
        let _request = request.enter();
        request.record("zdr", true);
        // Created after the mark: the sampler sees the flagged trace and drops it.
        let child = tracing::info_span!("child_after_mark");
        let _child = child.enter();
    });
    assert!(
        spans.is_empty(),
        "a span that records zdr late, and its trace, export nothing: {}",
        spans.len()
    );
}

#[test]
fn export_off_builds_a_sampler_that_drops_everything() {
    let (spans, _log) = run(rig(false), || {
        let plain = tracing::info_span!("plain_request");
        let _plain = plain.enter();
    });
    assert!(spans.is_empty(), "export off keeps no span");
}
