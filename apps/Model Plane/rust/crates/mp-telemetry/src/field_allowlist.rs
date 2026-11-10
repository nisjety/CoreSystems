//! Field allow-list for Model Plane telemetry (F-t3-075, W5-075-a).
//!
//! Behind `MP_TELEMETRY_FIELD_ALLOWLIST` (only the literal `true` turns it on; off by default), a
//! field whose NAME is not on `mp_contracts::telemetry_labels::TRACE_FIELDS` is dropped from the
//! JSON log lines and from every span, span event and span link attribute the OTLP exporter sends.
//! The number of names dropped is recorded as `scrubbed_fields`; the dropped names and their values
//! are never recorded. Off, [`crate::init`] builds the same subscriber it always built.
//!
//! Two pieces make the JSON side work:
//!
//! - [`AllowlistFields`] is the span-field formatter. Span fields are kept as JSON text and read back
//!   by the JSON envelope, so it writes a JSON object on the first record and on every later one.
//! - [`AllowlistJson`] is the event formatter. `tracing-subscriber`'s `Format<Json>` serialises event
//!   fields itself and never asks the layer's `FormatFields`, so a `.fmt_fields` filter alone would
//!   leave every event field in place. This formatter writes the same envelope as the stock `json()`
//!   layer (`timestamp`, `level`, `fields`, `target`, `span`, `spans`, `threadId`) with the fields of
//!   the event and of its spans filtered.
//!
//! The exporter side is [`AllowlistSpanExporter`], which wraps the OTLP exporter. tracing-opentelemetry
//! builds attributes inside its own visitor and offers no hook there, so the filter sits where every
//! exported span leaves the process.

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use mp_contracts::telemetry_labels::is_allowed_trace_field;
use opentelemetry::trace::Status;
use opentelemetry::KeyValue;
use opentelemetry_sdk::export::trace::{ExportResult, SpanData, SpanExporter};
use opentelemetry_sdk::Resource;
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::span::Record;
use tracing::{Event, Subscriber};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::{LookupSpan, SpanRef};

/// The switch that turns the allow-list on. Only the literal `true` enables it.
pub const SWITCH_ENV: &str = "MP_TELEMETRY_FIELD_ALLOWLIST";

/// The field that records how many names were dropped from a span, an event or a link. It holds a
/// count, never a name and never a value.
pub const SCRUBBED_FIELDS: &str = "scrubbed_fields";

/// Whether `raw` (the value of [`SWITCH_ENV`], if set) is the literal on-word.
#[must_use]
pub fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the allow-list is switched on in this process.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(SWITCH_ENV).ok().as_deref())
}

/// The fields of one span or one event, keyed by name, with only the allowed names kept.
#[derive(Default)]
struct Allowed {
    values: Map<String, Value>,
}

impl Allowed {
    /// Whether a field may be kept. A name that may not be kept is counted in [`SCRUBBED_FIELDS`],
    /// and its value is never formatted.
    fn admits(&mut self, field: &Field) -> bool {
        if is_allowed_trace_field(field.name()) {
            return true;
        }
        let dropped = self
            .values
            .get(SCRUBBED_FIELDS)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        self.values
            .insert(SCRUBBED_FIELDS.to_owned(), Value::from(dropped + 1));
        false
    }

    /// Stores a field that [`Allowed::admits`] let through.
    fn keep(&mut self, field: &Field, value: Value) {
        self.values.insert(field.name().to_owned(), value);
    }

    /// The collected fields as one JSON object.
    fn into_json(self) -> Result<String, fmt::Error> {
        serde_json::to_string(&self.values).map_err(|_| fmt::Error)
    }
}

impl Visit for Allowed {
    fn record_i64(&mut self, field: &Field, value: i64) {
        if self.admits(field) {
            self.keep(field, Value::from(value));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if self.admits(field) {
            self.keep(field, Value::from(value));
        }
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        if self.admits(field) {
            self.keep(field, Value::String(value.to_string()));
        }
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        if self.admits(field) {
            self.keep(field, Value::String(value.to_string()));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if self.admits(field) {
            self.keep(field, Value::Bool(value));
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if self.admits(field) {
            let number = serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number);
            self.keep(field, number);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if self.admits(field) {
            self.keep(field, Value::String(value.to_owned()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if self.admits(field) {
            self.keep(field, Value::String(format!("{value:?}")));
        }
    }
}

/// The span-field formatter of the allow-list layer. A span's fields are stored as one JSON object,
/// so the first write and every later `record` keep that object whole.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowlistFields;

impl<'writer> FormatFields<'writer> for AllowlistFields {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut collected = Allowed::default();
        fields.record(&mut collected);
        writer.write_str(&collected.into_json()?)
    }

    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &Record<'_>,
    ) -> fmt::Result {
        // The stored object is parsed, extended and written back whole, as the stock JSON formatter
        // does; appending text would leave two objects side by side.
        let values: Map<String, Value> = if current.fields.is_empty() {
            Map::new()
        } else {
            serde_json::from_str(&current.fields).map_err(|_| fmt::Error)?
        };
        let mut collected = Allowed { values };
        fields.record(&mut collected);
        current.fields = collected.into_json()?;
        Ok(())
    }
}

/// The event formatter of the allow-list layer. It writes the envelope of `tracing-subscriber`'s
/// `json()` layer, with the event's fields and its spans' fields filtered.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowlistJson;

impl<S> FormatEvent<S, AllowlistFields> for AllowlistJson
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, AllowlistFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        let mut timestamp = String::new();
        SystemTime.format_time(&mut Writer::new(&mut timestamp))?;

        let mut fields = Allowed::default();
        event.record(&mut fields);

        let mut line: Map<String, Value> = Map::new();
        line.insert("timestamp".to_owned(), Value::String(timestamp));
        line.insert(
            "level".to_owned(),
            Value::String(meta.level().as_str().to_owned()),
        );
        line.insert("fields".to_owned(), Value::Object(fields.values));
        line.insert("target".to_owned(), Value::String(meta.target().to_owned()));

        // The stock layer's current span: the event's parent span if it has one, else the current span.
        let current = event
            .parent()
            .and_then(|id| ctx.span(id))
            .or_else(|| ctx.lookup_current());
        if let Some(span) = current.as_ref() {
            line.insert("span".to_owned(), Value::Object(span_object(span)));
            let spans: Vec<Value> = ctx.lookup_current().map_or_else(Vec::new, |leaf| {
                leaf.scope()
                    .from_root()
                    .map(|scoped| Value::Object(span_object(&scoped)))
                    .collect()
            });
            line.insert("spans".to_owned(), Value::Array(spans));
        }

        line.insert(
            "threadId".to_owned(),
            Value::String(format!("{:?}", std::thread::current().id())),
        );

        let json = serde_json::to_string(&Value::Object(line)).map_err(|_| fmt::Error)?;
        writer.write_str(&json)?;
        writer.write_str("\n")
    }
}

/// One span as a JSON object: its filtered fields and its name.
fn span_object<S>(span: &SpanRef<'_, S>) -> Map<String, Value>
where
    S: for<'lookup> LookupSpan<'lookup>,
{
    let mut object = span
        .extensions()
        .get::<FormattedFields<AllowlistFields>>()
        .and_then(|stored| serde_json::from_str::<Map<String, Value>>(&stored.fields).ok())
        .unwrap_or_default();
    object.insert(
        "name".to_owned(),
        Value::String(span.metadata().name().to_owned()),
    );
    object
}

/// The JSON log layer with the allow-list on. Span fields go through [`AllowlistFields`] and event
/// lines through [`AllowlistJson`]. ANSI is off, as it is in the stock JSON layer.
pub fn json_layer<S>() -> tracing_subscriber::fmt::Layer<S, AllowlistFields, AllowlistJson>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .fmt_fields(AllowlistFields)
        .event_format(AllowlistJson)
}

/// Wraps an OTLP span exporter. Every batch passes through [`scrub_span`] before the inner exporter
/// sees it. Shutdown, flush and resource calls are forwarded unchanged.
#[derive(Debug)]
pub struct AllowlistSpanExporter<E> {
    inner: E,
}

impl<E> AllowlistSpanExporter<E> {
    /// Wraps `inner`, which exports the filtered batches.
    #[must_use]
    pub fn new(inner: E) -> Self {
        Self { inner }
    }
}

impl<E: SpanExporter> SpanExporter for AllowlistSpanExporter<E> {
    fn export(
        &mut self,
        batch: Vec<SpanData>,
    ) -> Pin<Box<dyn Future<Output = ExportResult> + Send + 'static>> {
        self.inner
            .export(batch.into_iter().map(scrub_span).collect())
    }

    fn shutdown(&mut self) {
        self.inner.shutdown();
    }

    fn force_flush(&mut self) -> Pin<Box<dyn Future<Output = ExportResult> + Send + 'static>> {
        self.inner.force_flush()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

/// Drops every attribute whose name is not on `TRACE_FIELDS`, and returns how many were dropped.
fn drop_unlisted(attributes: &mut Vec<KeyValue>) -> usize {
    let before = attributes.len();
    attributes.retain(|attribute| is_allowed_trace_field(attribute.key.as_str()));
    before - attributes.len()
}

/// Records, on one span, event or link, how many attributes were dropped from it.
fn record_dropped(attributes: &mut Vec<KeyValue>, dropped: usize) {
    if dropped > 0 {
        let count = i64::try_from(dropped).unwrap_or(i64::MAX);
        attributes.push(KeyValue::new(SCRUBBED_FIELDS, count));
    }
}

/// The span as the exporter may send it: no attribute outside `TRACE_FIELDS` on the span, on its
/// events or on its links. The status stays an error, but its description (the debug text of the
/// error value, which can carry content) is cleared.
fn scrub_span(mut span: SpanData) -> SpanData {
    let dropped = drop_unlisted(&mut span.attributes);
    record_dropped(&mut span.attributes, dropped);

    for event in &mut span.events.events {
        let dropped = drop_unlisted(&mut event.attributes);
        record_dropped(&mut event.attributes, dropped);
    }

    for link in &mut span.links.links {
        let dropped = drop_unlisted(&mut link.attributes);
        record_dropped(&mut link.attributes, dropped);
    }

    if let Status::Error { description } = &mut span.status {
        *description = Cow::Borrowed("");
    }

    span
}
