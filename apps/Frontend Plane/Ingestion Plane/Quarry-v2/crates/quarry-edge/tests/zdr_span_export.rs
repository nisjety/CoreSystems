//! WS10.S8: ZDR spans and their trace never reach the OTLP exporter, and the
//! log of a ZDR request carries no probe URL. Built on the real stack
//! (`zdr_log::otel_subscriber` with the OTel layer wrapped by `ZdrFlagLayer`,
//! `ZdrSampler`, `ZdrSpanProcessor`), with an in-memory exporter standing in for
//! OTLP. Each test owns its registry, so tests do not share the process-wide one.
//!
//! Written in the W2 build; first run in D-B2-zdr-pins, which found the harness
//! itself was empty: dropping the last reference to an `SdkTracerProvider` shuts
//! it down, and `InMemorySpanExporter::shutdown` clears what it holds, so every
//! test read zero spans whatever the stack did (the three "exports nothing" tests
//! passed vacuously). `Rig` now keeps the provider alive until the spans are read.

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
    /// Held so the exporter is not shut down (and emptied) before the test reads it.
    provider: SdkTracerProvider,
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
    let provider_alive = provider.clone();
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
        provider: provider_alive,
    }
}

fn run(rig: Rig, body: impl FnOnce()) -> (Vec<opentelemetry_sdk::trace::SpanData>, String) {
    let Rig {
        subscriber,
        exporter,
        logs,
        provider,
    } = rig;
    tracing::subscriber::with_default(subscriber, body);
    let spans = exporter.get_finished_spans().expect("spans readable");
    drop(provider);
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

/// D2-022: a `tracing` event inside a kept span exports as a span event whose
/// fields are the event's attributes; the contract's sensitive keys are stripped
/// from those too, not only from the span's own attributes. The canary values
/// below must appear nowhere in what the exporter received.
#[test]
fn a_kept_spans_events_lose_the_sensitive_keys_too() {
    const EVENT_URL: &str = "https://event-canary.example.test/secret";
    let (spans, _log) = run(rig(true), || {
        let request = tracing::info_span!("request", request_id = "req-plain");
        let _request = request.enter();
        tracing::info!(url = EVENT_URL, error = EVENT_URL, status_code = 502_i64, "fetch finished");
    });
    assert_eq!(spans.len(), 1, "the non-ZDR span is exported");
    let events = &spans[0].events.events;
    assert!(!events.is_empty(), "the event rides on the span, so the strip is exercised");
    let dump = format!("{:?}", spans[0]);
    assert!(!dump.contains("event-canary"), "an event field reached the exporter: {dump}");
    let keys: Vec<&str> = events[0].attributes.iter().map(|kv| kv.key.as_str()).collect();
    assert!(!keys.contains(&"url") && !keys.contains(&"error"), "{keys:?}");
    assert!(keys.contains(&"status_code"), "the safe field stays: {keys:?}");
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
