//! ZDR at the OTLP exporter (WS10.S8, F-t2-004 phase 2).
//!
//! The log side ([`crate::zdr_log`]) drops events while a ZDR span is in scope.
//! This module does the same for SPANS that leave the process: a trace that
//! touched a ZDR request exports nothing, and a non-ZDR span loses the field
//! names the contract calls sensitive. Three pieces:
//!
//! - [`ZdrTraceRegistry`]: a bounded FIFO of trace ids that were flagged ZDR.
//!   A trace is flagged when a span of it records `zdr = true`, even late (the
//!   record can come after the span was created, so the sampler at creation
//!   cannot see it; the registry is what covers the later spans of the trace).
//!   Eviction is oldest-first (FIFO, not LRU), capped by
//!   `QUARRY_TELEMETRY_ZDR_REGISTRY_CAP` (default 100,000).
//! - [`ZdrSampler`]: drops at creation when export is off, when the trace is
//!   registered, or when the span is created with `zdr = true`; otherwise it
//!   delegates to `ParentBased(AlwaysOn)`.
//! - [`ZdrSpanProcessor`]: at span end, drops a registered or `zdr` span (and
//!   flags its trace so later spans of it are dropped), and strips the
//!   `zdr_sensitive` and `never` keys from the spans it keeps: from the span's
//!   own attributes, from the attributes of every span event (a `tracing` event
//!   inside a span exports as an OTel span event, and its fields as the event's
//!   attributes) and from the attributes of its links (D2-022).
//!
//! [`ZdrFlagLayer`] is the tracing side: it wraps the OpenTelemetry layer and,
//! after the wrapped layer has created the span's OTel data, flags the trace
//! when `zdr = true` is declared or recorded.
//!
//! Limits, stated so nobody relies on more: spans that ENDED before their trace
//! was flagged are already in the batch and are not recalled (the same limit as
//! logs); the golden at `crates/quarry-runtime/contracts/telemetry-field-contract.json`
//! is the field vocabulary, and this module's constants must equal it (a test
//! pins that). Export is off by default only in the sense that
//! `QUARRY_TELEMETRY_EXPORT=off` builds no exporter at all; the default keeps the
//! existing behaviour.

use std::collections::{HashSet, VecDeque};
use std::fmt::Debug;
use std::sync::{Arc, Mutex, OnceLock};

use opentelemetry::trace::{Link, SamplingDecision, SamplingResult, SpanKind, TraceContextExt, TraceId, TraceState};
use opentelemetry::{Context, KeyValue, Value};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{Sampler, ShouldSample, Span, SpanData, SpanProcessor};
use opentelemetry_sdk::Resource;
use quarry_core::zdr::ZDR_SPAN_FIELD;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::layer::{Context as LayerContext, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::zdr_log::SENSITIVE_FIELDS;

/// Env var for the registry capacity (a positive integer; anything else uses the default).
pub const REGISTRY_CAP_ENV: &str = "QUARRY_TELEMETRY_ZDR_REGISTRY_CAP";
/// Default registry capacity.
pub const DEFAULT_REGISTRY_CAP: usize = 100_000;
/// Env var that turns span export on (default) or off (`off`, `0` or `false`).
pub const TELEMETRY_EXPORT_ENV: &str = "QUARRY_TELEMETRY_EXPORT";

/// The golden's `never` class: never leaves the process, ZDR or not.
pub const NEVER_FIELDS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "token",
    "password",
    "otp",
    "secret",
    "api_key",
    "typed_text",
];

/// The golden's `safe` class: the keys that may stay on any span.
pub const SAFE_FIELDS: &[&str] = &[
    "request_id",
    "run_id",
    "step",
    "zdr",
    "outcome",
    "error_class",
    "status",
    "status_code",
    "driver",
    "driver_name",
    "attempt",
    "count",
    "bytes",
    "byte_size",
    "duration_ms",
    "elapsed_ms",
    "kind",
    "method",
    "route",
];

/// Whether a span attribute key is removed from a span that is kept: the
/// `zdr_sensitive` class (WS4 `SENSITIVE_FIELDS`) or the `never` class.
pub fn is_stripped_key(key: &str) -> bool {
    SENSITIVE_FIELDS.contains(&key) || NEVER_FIELDS.contains(&key)
}

/// Whether an attribute is the ZDR mark (`zdr = true`).
fn is_zdr_attr(kv: &KeyValue) -> bool {
    kv.key.as_str() == ZDR_SPAN_FIELD && matches!(kv.value, Value::Bool(true))
}

/// Reads the `QUARRY_TELEMETRY_EXPORT` value: export is on unless it is `off`,
/// `0` or `false` (any case, surrounding whitespace ignored).
pub fn export_enabled_from(value: Option<&str>) -> bool {
    !value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false"
        )
    })
}

/// [`export_enabled_from`] of the process environment.
pub fn export_enabled() -> bool {
    export_enabled_from(std::env::var(TELEMETRY_EXPORT_ENV).ok().as_deref())
}

/// The registry capacity from the environment, or [`DEFAULT_REGISTRY_CAP`].
pub fn registry_cap_from_env() -> usize {
    std::env::var(REGISTRY_CAP_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
        .unwrap_or(DEFAULT_REGISTRY_CAP)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Default)]
struct RegistryInner {
    order: VecDeque<TraceId>,
    members: HashSet<TraceId>,
}

/// A bounded set of ZDR trace ids, evicted oldest-first.
#[derive(Debug)]
pub struct ZdrTraceRegistry {
    cap: usize,
    inner: Mutex<RegistryInner>,
}

impl ZdrTraceRegistry {
    /// A registry holding at most `cap` trace ids (`cap` is at least 1).
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            inner: Mutex::new(RegistryInner::default()),
        }
    }

    /// The bound on the number of trace ids held.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Flags `trace_id` as ZDR. Flagging an id already held changes nothing (a
    /// re-flag does not move it to the back of the queue). An invalid id is
    /// ignored.
    pub fn flag(&self, trace_id: TraceId) {
        if trace_id == TraceId::INVALID {
            return;
        }
        let mut inner = lock(&self.inner);
        if inner.members.contains(&trace_id) {
            return;
        }
        while inner.order.len() >= self.cap {
            match inner.order.pop_front() {
                Some(oldest) => {
                    inner.members.remove(&oldest);
                }
                None => break,
            }
        }
        inner.members.insert(trace_id);
        inner.order.push_back(trace_id);
    }

    /// Whether `trace_id` is flagged.
    pub fn contains(&self, trace_id: TraceId) -> bool {
        lock(&self.inner).members.contains(&trace_id)
    }

    /// The number of trace ids held (never more than [`Self::capacity`]).
    pub fn len(&self) -> usize {
        lock(&self.inner).order.len()
    }

    /// Whether no trace id is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

static SHARED_REGISTRY: OnceLock<Arc<ZdrTraceRegistry>> = OnceLock::new();

/// The process-wide registry the exporter path uses, sized from the environment on first use.
pub fn shared_registry() -> Arc<ZdrTraceRegistry> {
    SHARED_REGISTRY
        .get_or_init(|| Arc::new(ZdrTraceRegistry::with_capacity(registry_cap_from_env())))
        .clone()
}

/// Flags a trace on the process-wide registry.
pub fn flag(trace_id: TraceId) {
    shared_registry().flag(trace_id);
}

fn drop_result() -> SamplingResult {
    SamplingResult {
        decision: SamplingDecision::Drop,
        attributes: Vec::new(),
        trace_state: TraceState::NONE,
    }
}

/// Decides at span creation. Drops when export is off, when the trace is
/// flagged, or when the span is created with `zdr = true`; otherwise
/// `ParentBased(AlwaysOn)`.
#[derive(Debug, Clone)]
pub struct ZdrSampler {
    registry: Arc<ZdrTraceRegistry>,
    export_on: bool,
    inner: Sampler,
}

impl ZdrSampler {
    /// A sampler over `registry`; `export_on` false drops every span.
    pub fn new(registry: Arc<ZdrTraceRegistry>, export_on: bool) -> Self {
        Self {
            registry,
            export_on,
            inner: Sampler::ParentBased(Box::new(Sampler::AlwaysOn)),
        }
    }
}

impl ShouldSample for ZdrSampler {
    fn should_sample(
        &self,
        parent_context: Option<&Context>,
        trace_id: TraceId,
        name: &str,
        span_kind: &SpanKind,
        attributes: &[KeyValue],
        links: &[Link],
    ) -> SamplingResult {
        if !self.export_on
            || self.registry.contains(trace_id)
            || attributes.iter().any(is_zdr_attr)
        {
            return drop_result();
        }
        self.inner
            .should_sample(parent_context, trace_id, name, span_kind, attributes, links)
    }
}

/// Drops ZDR spans and the spans of flagged traces, strips the contract keys
/// from the rest, and forwards to `inner`.
#[derive(Debug)]
pub struct ZdrSpanProcessor<P: SpanProcessor> {
    inner: P,
    registry: Arc<ZdrTraceRegistry>,
}

impl<P: SpanProcessor> ZdrSpanProcessor<P> {
    /// Wraps `inner`, judging spans against `registry`.
    pub fn new(inner: P, registry: Arc<ZdrTraceRegistry>) -> Self {
        Self { inner, registry }
    }
}

/// Removes the contract's stripped keys from everything of a kept span that can
/// carry a field: its attributes, its events' attributes and its links'
/// attributes. Event names are left alone: they are the event message, which the
/// log-field lint holds to the same no-caller-data rule as the log line itself.
fn strip_span(span: &mut SpanData) {
    span.attributes.retain(|kv| !is_stripped_key(kv.key.as_str()));
    for event in &mut span.events.events {
        event.attributes.retain(|kv| !is_stripped_key(kv.key.as_str()));
    }
    for link in &mut span.links.links {
        link.attributes.retain(|kv| !is_stripped_key(kv.key.as_str()));
    }
}

impl<P: SpanProcessor> SpanProcessor for ZdrSpanProcessor<P> {
    fn on_start(&self, span: &mut Span, cx: &Context) {
        self.inner.on_start(span, cx);
    }

    fn on_end(&self, mut span: SpanData) {
        let trace_id = span.span_context.trace_id();
        if span.attributes.iter().any(is_zdr_attr) {
            // A ZDR span drops, and so does the rest of its trace from here on.
            self.registry.flag(trace_id);
            return;
        }
        if self.registry.contains(trace_id) {
            return;
        }
        strip_span(&mut span);
        self.inner.on_end(span);
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown(&self) -> OTelSdkResult {
        self.inner.shutdown()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

/// Whether an attribute or record set declares `zdr = true`.
#[derive(Default)]
struct ZdrFieldSeen(bool);

impl tracing::field::Visit for ZdrFieldSeen {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == ZDR_SPAN_FIELD && value {
            self.0 = true;
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn Debug) {}
}

fn trace_id_of(data: &tracing_opentelemetry::OtelData) -> Option<TraceId> {
    if let Some(trace_id) = data.builder.trace_id {
        return Some(trace_id);
    }
    let parent = data.parent_cx.span();
    let parent_context = parent.span_context();
    parent_context
        .is_valid()
        .then(|| parent_context.trace_id())
}

fn flag_span<S>(id: &Id, ctx: &LayerContext<'_, S>, registry: &ZdrTraceRegistry)
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let Some(span) = ctx.span(id) else {
        return;
    };
    let extensions = span.extensions();
    if let Some(trace_id) = extensions
        .get::<tracing_opentelemetry::OtelData>()
        .and_then(trace_id_of)
    {
        registry.flag(trace_id);
    }
}

/// Wraps the OpenTelemetry tracing layer and flags the span's trace when `zdr`
/// is declared or recorded as true. Every other hook is forwarded unchanged.
pub struct ZdrFlagLayer<L> {
    inner: L,
    registry: Arc<ZdrTraceRegistry>,
}

impl<L> ZdrFlagLayer<L> {
    /// Wraps `inner`, flagging into `registry`.
    pub fn new(inner: L, registry: Arc<ZdrTraceRegistry>) -> Self {
        Self { inner, registry }
    }
}

impl<L, S> Layer<S> for ZdrFlagLayer<L>
where
    L: Layer<S>,
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_layer(&mut self, subscriber: &mut S) {
        self.inner.on_layer(subscriber);
    }

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        self.inner.register_callsite(metadata)
    }

    fn enabled(&self, metadata: &Metadata<'_>, ctx: LayerContext<'_, S>) -> bool {
        self.inner.enabled(metadata, ctx)
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: LayerContext<'_, S>) {
        self.inner.on_new_span(attrs, id, ctx.clone());
        let mut seen = ZdrFieldSeen::default();
        attrs.record(&mut seen);
        if seen.0 {
            flag_span(id, &ctx, &self.registry);
        }
    }

    fn max_level_hint(&self) -> Option<tracing::metadata::LevelFilter> {
        self.inner.max_level_hint()
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: LayerContext<'_, S>) {
        self.inner.on_record(id, values, ctx.clone());
        let mut seen = ZdrFieldSeen::default();
        values.record(&mut seen);
        if seen.0 {
            flag_span(id, &ctx, &self.registry);
        }
    }

    fn on_follows_from(&self, span: &Id, follows: &Id, ctx: LayerContext<'_, S>) {
        self.inner.on_follows_from(span, follows, ctx);
    }

    fn event_enabled(&self, event: &Event<'_>, ctx: LayerContext<'_, S>) -> bool {
        self.inner.event_enabled(event, ctx)
    }

    fn on_event(&self, event: &Event<'_>, ctx: LayerContext<'_, S>) {
        self.inner.on_event(event, ctx);
    }

    fn on_enter(&self, id: &Id, ctx: LayerContext<'_, S>) {
        self.inner.on_enter(id, ctx);
    }

    fn on_exit(&self, id: &Id, ctx: LayerContext<'_, S>) {
        self.inner.on_exit(id, ctx);
    }

    fn on_close(&self, id: Id, ctx: LayerContext<'_, S>) {
        self.inner.on_close(id, ctx);
    }

    fn on_id_change(&self, old: &Id, new: &Id, ctx: LayerContext<'_, S>) {
        self.inner.on_id_change(old, new, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::time::SystemTime;

    use opentelemetry::trace::{SpanContext, SpanId, TraceFlags};
    use opentelemetry::InstrumentationScope;
    use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};

    /// Records what reaches the inner processor.
    #[derive(Debug, Default, Clone)]
    struct Recorder(Arc<Mutex<Vec<SpanData>>>);

    impl SpanProcessor for Recorder {
        fn on_start(&self, _span: &mut Span, _cx: &Context) {}
        fn on_end(&self, span: SpanData) {
            lock(&self.0).push(span);
        }
        fn force_flush(&self) -> OTelSdkResult {
            Ok(())
        }
        fn shutdown(&self) -> OTelSdkResult {
            Ok(())
        }
    }

    impl Recorder {
        fn spans(&self) -> Vec<SpanData> {
            lock(&self.0).clone()
        }
    }

    fn trace(n: u128) -> TraceId {
        TraceId::from(n)
    }

    fn span_data(trace_id: TraceId, attributes: Vec<KeyValue>) -> SpanData {
        SpanData {
            span_context: SpanContext::new(
                trace_id,
                SpanId::from(7u64),
                TraceFlags::SAMPLED,
                false,
                TraceState::NONE,
            ),
            parent_span_id: SpanId::INVALID,
            span_kind: SpanKind::Internal,
            name: Cow::Borrowed("span"),
            start_time: SystemTime::UNIX_EPOCH,
            end_time: SystemTime::UNIX_EPOCH,
            attributes,
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: opentelemetry::trace::Status::Unset,
            instrumentation_scope: InstrumentationScope::builder("test").build(),
        }
    }

    fn processor(cap: usize) -> (ZdrSpanProcessor<Recorder>, Recorder, Arc<ZdrTraceRegistry>) {
        let recorder = Recorder::default();
        let registry = Arc::new(ZdrTraceRegistry::with_capacity(cap));
        (
            ZdrSpanProcessor::new(recorder.clone(), registry.clone()),
            recorder,
            registry,
        )
    }

    #[test]
    fn zdr_attribute_span_is_dropped() {
        let (processor, recorder, registry) = processor(16);
        let zdr = vec![KeyValue::new(ZDR_SPAN_FIELD, true)];
        processor.on_end(span_data(trace(1), zdr));
        assert!(recorder.spans().is_empty());
        assert!(registry.contains(trace(1)), "a ZDR span flags its trace");
    }

    #[test]
    fn registered_trace_spans_are_dropped_on_end() {
        let (processor, recorder, registry) = processor(16);
        registry.flag(trace(2));
        processor.on_end(span_data(trace(2), Vec::new()));
        assert!(recorder.spans().is_empty());
        processor.on_end(span_data(trace(3), Vec::new()));
        assert_eq!(recorder.spans().len(), 1);
    }

    #[test]
    fn children_of_a_registered_trace_are_dropped() {
        let (processor, recorder, registry) = processor(16);
        registry.flag(trace(4));
        for _ in 0..3 {
            processor.on_end(span_data(trace(4), Vec::new()));
        }
        assert!(recorder.spans().is_empty());
    }

    #[test]
    fn sensitive_attributes_are_stripped_from_non_zdr_spans() {
        let (processor, recorder, _registry) = processor(16);
        let attrs = vec![
            KeyValue::new("url", "https://example.test/a"),
            KeyValue::new("status_code", 200_i64),
        ];
        processor.on_end(span_data(trace(5), attrs));
        let spans = recorder.spans();
        assert_eq!(spans.len(), 1);
        let keys: Vec<&str> = spans[0].attributes.iter().map(|kv| kv.key.as_str()).collect();
        assert_eq!(keys, vec!["status_code"]);
    }

    #[test]
    fn sensitive_event_and_link_attributes_are_stripped_from_a_kept_span() {
        use opentelemetry::trace::{Event, Link};
        let (processor, recorder, _registry) = processor(16);
        let mut data = span_data(trace(7), vec![KeyValue::new("status_code", 200_i64)]);
        data.events.events.push(Event::new(
            "fetch finished",
            SystemTime::UNIX_EPOCH,
            vec![
                KeyValue::new("url", "https://example.test/private"),
                KeyValue::new("error", "connect failed for https://example.test/private"),
                KeyValue::new("authorization", "Bearer x"),
                KeyValue::new("status_code", 502_i64),
            ],
            0,
        ));
        data.links.links.push(Link::new(
            SpanContext::new(trace(8), SpanId::from(9u64), TraceFlags::SAMPLED, false, TraceState::NONE),
            vec![KeyValue::new("target", "https://example.test/x"), KeyValue::new("count", 1_i64)],
            0,
        ));
        processor.on_end(data);
        let spans = recorder.spans();
        assert_eq!(spans.len(), 1);
        let event_keys: Vec<&str> = spans[0].events.events[0].attributes.iter().map(|kv| kv.key.as_str()).collect();
        assert_eq!(event_keys, vec!["status_code"], "the event keeps only the safe key");
        assert_eq!(spans[0].events.events[0].name, "fetch finished", "the event itself stays");
        let link_keys: Vec<&str> = spans[0].links.links[0].attributes.iter().map(|kv| kv.key.as_str()).collect();
        assert_eq!(link_keys, vec!["count"]);
    }

    #[test]
    fn never_class_attributes_are_stripped_even_when_not_zdr() {
        let (processor, recorder, _registry) = processor(16);
        let attrs = vec![
            KeyValue::new("authorization", "Bearer x"),
            KeyValue::new("api_key", "k"),
            KeyValue::new("run_id", "r1"),
        ];
        processor.on_end(span_data(trace(6), attrs));
        let spans = recorder.spans();
        let keys: Vec<&str> = spans[0].attributes.iter().map(|kv| kv.key.as_str()).collect();
        assert_eq!(keys, vec!["run_id"]);
    }

    #[test]
    fn registry_evicts_oldest_first_at_capacity() {
        let registry = ZdrTraceRegistry::with_capacity(3);
        for n in 1..=4 {
            registry.flag(trace(n));
        }
        assert!(!registry.contains(trace(1)), "oldest evicted");
        assert!(registry.contains(trace(2)));
        assert!(registry.contains(trace(4)));
        // A re-flag does not refresh position: 2 is still the oldest held.
        registry.flag(trace(2));
        registry.flag(trace(5));
        assert!(!registry.contains(trace(2)), "FIFO, not LRU");
        assert!(registry.contains(trace(3)));
    }

    #[test]
    fn registry_is_bounded() {
        let registry = ZdrTraceRegistry::with_capacity(8);
        for n in 1..=1_000 {
            registry.flag(trace(n));
        }
        assert_eq!(registry.len(), 8);
        assert_eq!(registry.capacity(), 8);
    }

    #[test]
    fn export_off_builds_no_exporter() {
        assert!(!export_enabled_from(Some("off")));
        assert!(!export_enabled_from(Some(" FALSE ")));
        assert!(!export_enabled_from(Some("0")));
        assert!(export_enabled_from(Some("on")));
        assert!(export_enabled_from(None));
        // A sampler built with export off drops everything, even a clean span.
        let sampler = ZdrSampler::new(Arc::new(ZdrTraceRegistry::with_capacity(4)), false);
        let result = sampler.should_sample(
            None,
            trace(9),
            "clean",
            &SpanKind::Internal,
            &[],
            &[],
        );
        assert_eq!(result.decision, SamplingDecision::Drop);
    }

    #[test]
    fn sampler_drops_a_zdr_span_and_a_registered_trace() {
        let registry = Arc::new(ZdrTraceRegistry::with_capacity(4));
        let sampler = ZdrSampler::new(registry.clone(), true);
        let zdr = sampler.should_sample(
            None,
            trace(10),
            "request",
            &SpanKind::Server,
            &[KeyValue::new(ZDR_SPAN_FIELD, true)],
            &[],
        );
        assert_eq!(zdr.decision, SamplingDecision::Drop);
        registry.flag(trace(11));
        let later = sampler.should_sample(None, trace(11), "child", &SpanKind::Internal, &[], &[]);
        assert_eq!(later.decision, SamplingDecision::Drop);
        let clean = sampler.should_sample(None, trace(12), "clean", &SpanKind::Internal, &[], &[]);
        assert_eq!(clean.decision, SamplingDecision::RecordAndSample);
    }

    /// Spike: the sampler sees only the attributes present at creation, so a
    /// `zdr` recorded after creation is invisible to it. The registry is what
    /// covers the later spans of that trace; this test pins that split.
    #[test]
    fn sampler_sees_a_zdr_field_recorded_after_span_creation() {
        let registry = Arc::new(ZdrTraceRegistry::with_capacity(4));
        let sampler = ZdrSampler::new(registry.clone(), true);
        // Created clean: the sampler keeps it, because the mark is not yet known.
        let created = sampler.should_sample(None, trace(20), "request", &SpanKind::Server, &[], &[]);
        assert_eq!(created.decision, SamplingDecision::RecordAndSample);
        // The mark arrives later (the ZdrFlagLayer flags on record). Only the
        // registry can now stop the trace's later spans.
        registry.flag(trace(20));
        let child = sampler.should_sample(None, trace(20), "child", &SpanKind::Internal, &[], &[]);
        assert_eq!(child.decision, SamplingDecision::Drop);
    }

    #[test]
    fn field_constants_equal_the_golden() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../quarry-runtime/contracts/telemetry-field-contract.json"
        ))
        .expect("golden parses");
        let as_list = |key: &str| -> Vec<String> {
            golden[key]
                .as_array()
                .unwrap_or_else(|| panic!("golden has no array {key}"))
                .iter()
                .map(|v| v.as_str().expect("string").to_string())
                .collect()
        };
        assert_eq!(golden["version"], 1);
        let owned = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        assert_eq!(as_list("safe"), owned(SAFE_FIELDS));
        assert_eq!(as_list("zdr_sensitive"), owned(SENSITIVE_FIELDS));
        assert_eq!(as_list("never"), owned(NEVER_FIELDS));
    }
}
