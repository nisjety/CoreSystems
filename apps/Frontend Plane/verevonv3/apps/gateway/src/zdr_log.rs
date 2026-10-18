//! What the gateway's process log may say while a request runs under Zero Data
//! Retention.
//!
//! A log line is a durable copy of whatever it names. A ZDR request carries
//! `zdr = true` on its HTTP span (set by the earliest code that learns the
//! posture, see [`mark_current_span`]); the filter below drops every event,
//! under that span or any child of it, that carries a field whose name says it
//! holds a URL, a query, a title or an error text ([`SENSITIVE_FIELDS`]).
//! Everything else (counts, status, class, ids) is kept.
//!
//! This is the gateway half of a contract shared with Quarry's edge
//! (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/zdr_log.rs`). It is a
//! documented contract, not shared code: the two planes ship separately. The
//! field list is pinned by `tests/fixtures/zdr-sensitive-fields.golden.json`,
//! and a test below fails when it differs from Quarry's own golden.
//!
//! What this does and does not do:
//! - It drops whole events. A request-path `error!(error = %e, ...)` disappears
//!   for a ZDR caller until it is rewritten to log an `error_class`; that
//!   rewrite is the log-field lint's job (`tests/log_field_discipline.rs`).
//! - It reads span context. An event emitted from a spawned task that did not
//!   inherit the span is not covered; spawn with `.in_current_span()`.
//! - The mark is sticky: set once, it stays for the life of the span, and a
//!   later `record(zdr, false)` cannot lower it. Events logged before the
//!   posture was known are not recalled; late ZDR cannot be undone for logs.
//! - The mark does not depend on `RUST_LOG`. The level filter lives on the JSON
//!   layer only ([`ZdrLogFilter`]), so the registry and [`ZdrMarkLayer`] see the
//!   request span even when `RUST_LOG=error` would never create it.
//! - A span that carries a sensitive field name (`info_span!("x", url = ..)`)
//!   and is created under a ZDR span is withheld from the log too, so its
//!   fields do not ride along in the context of other events. A span created
//!   with an explicit `parent:` is not looked at; the lint reads span macros.
//!   `Span::record` of a sensitive name after creation is not covered.
//! - The third-party caps ([`CAPPED_TARGETS`]) are enforced per event, so a more
//!   specific operator directive (`reqwest::connect=trace`) cannot lift them.
//! - It is a filter on the log writer. It does not stop code from computing or
//!   returning the data.
//!
//! `GATEWAY_ZDR_LOG_FILTER=off` turns the sensitive-field filter off (an
//! emergency lever for a debugging session on a non-production gateway; read
//! once at start). The third-party caps stay on.
//!
//! This file depends only on `tracing`, `tracing-subscriber` and the standard
//! library, so it can be read without the rest of the crate.

use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{field::Visit, Level, Metadata, Span, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Filter, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// The span field a request records `true` into once it is known to be ZDR.
pub(crate) const ZDR_SPAN_FIELD: &str = "zdr";

/// Field names that say "this holds what the caller asked about". An event with
/// any of them is dropped while a ZDR span is in scope.
///
/// Identical to Quarry's list; pinned by
/// `tests/fixtures/zdr-sensitive-fields.golden.json`.
pub(crate) const SENSITIVE_FIELDS: &[&str] = &[
    "url",
    "uri",
    "source_url",
    "target_url",
    "target",
    "query",
    "q",
    "selector",
    "title",
    "host",
    "domain",
    "origin",
    "snippet",
    "text",
    "markdown",
    "excerpt",
    "error",
    "err",
    "reason_text",
];

/// Marker kept in a span's extensions once it (or a record on it) said ZDR.
struct ZdrMarked;

/// Reads `zdr = true` out of span attributes and records.
struct ZdrVisitor(bool);

impl Visit for ZdrVisitor {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == ZDR_SPAN_FIELD && value {
            self.0 = true;
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
}

/// Makes the ZDR mark sticky. Add it to the registry beneath the layers that
/// format or export events; it adds no output of its own, and it carries no
/// level filter, so it sees every span whatever `RUST_LOG` says.
#[derive(Default, Clone, Copy)]
pub(crate) struct ZdrMarkLayer;

impl<S> Layer<S> for ZdrMarkLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = ZdrVisitor(false);
        attrs.record(&mut visitor);
        if visitor.0 {
            mark(id, &ctx);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = ZdrVisitor(false);
        values.record(&mut visitor);
        // A `false` (or any other value) never lowers the mark.
        if visitor.0 {
            mark(id, &ctx);
        }
    }
}

/// Sets the marker under ONE write lock. Two tasks that inherit the same span
/// can both record `zdr = true` at once; `replace` makes the second a no-op,
/// where a look-then-`insert` would panic in `Extensions::insert`.
fn mark<S>(id: &Id, ctx: &Context<'_, S>)
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    if let Some(span) = ctx.span(id) {
        let _ = span.extensions_mut().replace(ZdrMarked);
    }
}

/// Records `zdr = true` on the current span. A no-op when there is no current
/// span or it does not declare the field, so it is safe to call from code that
/// also runs outside a request (tests, background jobs).
///
/// Call it from the earliest code that learns a request is ZDR. It can only
/// raise the mark; there is no way to lower it.
pub(crate) fn mark_current_span() {
    Span::current().record(ZDR_SPAN_FIELD, true);
}

/// Whether the current span, or any span above it, is marked ZDR.
fn in_zdr_scope<S>(ctx: &Context<'_, S>) -> bool
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    ctx.lookup_current().is_some_and(|span| {
        span.scope()
            .any(|ancestor| ancestor.extensions().get::<ZdrMarked>().is_some())
    })
}

/// Whether an event or span with these fields is one the ZDR filter looks at.
fn carries_sensitive_field(meta: &Metadata<'_>) -> bool {
    meta.fields()
        .iter()
        .any(|field| SENSITIVE_FIELDS.contains(&field.name()))
}

/// Dependencies whose own debug logging prints page content, request URLs or
/// protocol traffic. They reach the log through the `log` bridge with only a
/// `message`, so the sensitive-field filter cannot see what is inside, and they
/// are capped here whatever `RUST_LOG` says. The same list as Quarry's edge
/// (some of these crates are not in the gateway's tree; capping an absent
/// target costs nothing and keeps the two lists identical).
pub(crate) const CAPPED_TARGETS: &[(&str, &str)] = &[
    ("html5ever", "warn"),
    ("markup5ever", "warn"),
    ("selectors", "warn"),
    ("reqwest", "info"),
    ("hyper_util", "info"),
    ("hyper", "info"),
    ("chromiumoxide", "info"),
];

/// Whether this event or span is more verbose than the cap on its target (the
/// target itself or any module under it). Compared per event, so no directive
/// an operator writes, however specific, can lift a cap.
fn exceeds_cap(meta: &Metadata<'_>) -> bool {
    let target = meta.target();
    CAPPED_TARGETS.iter().any(|(name, level)| {
        let under = target == *name
            || target
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with("::"));
        under && *meta.level() > level.parse::<Level>().unwrap_or(Level::ERROR)
    })
}

/// `filter` with [`CAPPED_TARGETS`] added as directives. This only makes the
/// common case cheap (the callsite is disabled once, up front); the guarantee
/// is [`exceeds_cap`], which [`ZdrLogFilter`] applies to every event.
pub(crate) fn harden(mut filter: EnvFilter) -> EnvFilter {
    for (target, level) in CAPPED_TARGETS {
        if let Ok(directive) = format!("{target}={level}").parse() {
            filter = filter.add_directive(directive);
        }
    }
    filter
}

/// Whether the `GATEWAY_ZDR_LOG_FILTER` value turns the sensitive-field filter
/// off: `off`, `0` or `false`, any case, surrounding whitespace ignored.
fn disabled_from(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false"
        )
    })
}

fn filter_disabled() -> bool {
    disabled_from(std::env::var("GATEWAY_ZDR_LOG_FILTER").ok().as_deref())
}

/// The one per-layer filter of the JSON log. It combines, in this order:
///
/// 1. the third-party caps ([`exceeds_cap`]), always;
/// 2. the operator's `RUST_LOG` filter, for events only: a span always passes,
///    so the HTTP span exists, is marked and is visible to the lookups below
///    even at `RUST_LOG=error`;
/// 3. the ZDR rule: an event or span with a sensitive field name is dropped
///    while a ZDR span is in scope (unless the lever is off).
pub(crate) struct ZdrLogFilter {
    env: Option<EnvFilter>,
    zdr_rule_off: bool,
}

impl ZdrLogFilter {
    /// `env` is the operator's level filter; `None` lets every level through.
    pub(crate) fn new(env: Option<EnvFilter>, zdr_rule_off: bool) -> Self {
        Self { env, zdr_rule_off }
    }

    fn sensitive_under_zdr<S>(&self, meta: &Metadata<'_>, ctx: &Context<'_, S>) -> bool
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        !self.zdr_rule_off && carries_sensitive_field(meta) && in_zdr_scope(ctx)
    }
}

impl<S> Filter<S> for ZdrLogFilter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(&self, meta: &Metadata<'_>, ctx: &Context<'_, S>) -> bool {
        if exceeds_cap(meta) {
            return false;
        }
        if !meta.is_span() {
            if let Some(env) = &self.env {
                if !Filter::<S>::enabled(env, meta, ctx) {
                    return false;
                }
            }
        }
        !self.sensitive_under_zdr(meta, ctx)
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        if exceeds_cap(meta) {
            return Interest::never();
        }
        let from_env = self
            .env
            .as_ref()
            .map(|env| Filter::<S>::callsite_enabled(env, meta));
        // Which side of the ZDR rule a callsite falls on depends on the request,
        // so a callsite with a sensitive field is decided per call.
        let per_call = !self.zdr_rule_off && carries_sensitive_field(meta);
        if meta.is_span() {
            // The env filter is consulted (above) so dynamic directives register,
            // but it never hides a span.
            return if per_call {
                Interest::sometimes()
            } else {
                Interest::always()
            };
        }
        match from_env {
            Some(interest) if interest.is_never() => Interest::never(),
            Some(interest) if !per_call => interest,
            None if !per_call => Interest::always(),
            _ => Interest::sometimes(),
        }
    }

    // No `max_level_hint`: spans pass at every level, so no ceiling holds.

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(env) = &self.env {
            Filter::<S>::on_new_span(env, attrs, id, ctx);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(env) = &self.env {
            Filter::<S>::on_record(env, id, values, ctx);
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(env) = &self.env {
            Filter::<S>::on_enter(env, id, ctx);
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(env) = &self.env {
            Filter::<S>::on_exit(env, id, ctx);
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        if let Some(env) = &self.env {
            Filter::<S>::on_close(env, id, ctx);
        }
    }
}

fn json_layer<S, W>(writer: W, filter: ZdrLogFilter) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(filter)
}

/// The JSON log layer the gateway writes to `writer`, with the ZDR filter and
/// the third-party caps but no `RUST_LOG` level filter (every level passes).
/// [`production_subscriber`] adds the level filter; tests that only care about
/// the ZDR rule use this.
#[cfg(test)]
pub(crate) fn build_json_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    json_layer(writer, ZdrLogFilter::new(None, filter_disabled()))
}

/// The subscriber the gateway runs: the sticky ZDR mark, and a JSON log on
/// `writer` filtered by `env` (the operator's `RUST_LOG`, already passed through
/// [`harden`]) and by the ZDR rule. The level filter sits on the JSON layer, not
/// on the registry, so it cannot keep the request span from being created.
/// [`init`] installs it; tests build it with the filters they want to probe.
pub(crate) fn production_subscriber<W>(
    writer: W,
    env: EnvFilter,
) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(ZdrMarkLayer)
        .with(json_layer(
            writer,
            ZdrLogFilter::new(Some(env), filter_disabled()),
        ))
}

/// Install the process-wide subscriber: `RUST_LOG` (default `error`, as before)
/// with the capped dependencies, the sticky ZDR mark and the filtered JSON log
/// on stdout. See [`production_subscriber`].
pub(crate) fn init() {
    production_subscriber(std::io::stdout, harden(EnvFilter::from_default_env())).init();
}

/// A closed-vocabulary label for a transport error, safe to log under ZDR: one
/// of `timeout`, `connect`, `status`, `decode`, `request`. `reqwest::Error`'s own
/// text embeds the request URL, query string included.
pub(crate) fn class_reqwest(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_status() {
        "status"
    } else if error.is_decode() {
        "decode"
    } else {
        "request"
    }
}

/// The scheme, host and port of `url` and nothing else: no path, no query, no
/// fragment, no credentials. For logging which upstream service a call went to
/// without naming what was asked of it. Unparseable input yields `"invalid"`,
/// never the input.
pub(crate) fn upstream_origin(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => match (parsed.host_str(), parsed.port()) {
            (Some(host), Some(port)) => format!("{}://{host}:{port}", parsed.scheme()),
            (Some(host), None) => format!("{}://{host}", parsed.scheme()),
            _ => "invalid".to_owned(),
        },
        Err(_) => "invalid".to_owned(),
    }
}

#[cfg(test)]
pub(crate) mod test_capture {
    //! A capturing writer shared by the tests of this module and the handler
    //! tests that assert on what the gateway logged.
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    pub(crate) struct Capture(pub(crate) Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl Capture {
        pub(crate) fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_capture::Capture;
    use super::*;

    /// Run `body` under the real layer stack and return what was written.
    fn captured(body: impl FnOnce()) -> String {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(build_json_layer(capture.clone()));
        tracing::subscriber::with_default(subscriber, body);
        capture.text()
    }

    #[test]
    fn a_zdr_span_drops_events_with_sensitive_fields() {
        let log = captured(|| {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            span.record("zdr", true);
            let _guard = span.enter();
            tracing::info!(url = "https://secret.test/page", "fetched");
            tracing::warn!(error = "connect to secret.test failed", "upstream failed");
            tracing::info!(title = "Secret title", "extracted");
        });
        assert!(!log.contains("secret"), "{log}");
        assert!(log.is_empty(), "nothing should be written: {log}");
    }

    #[test]
    fn a_zdr_span_keeps_events_with_only_safe_fields() {
        let log = captured(|| {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            span.record("zdr", true);
            let _guard = span.enter();
            tracing::info!(word_count = 25, status = 200, "page extracted");
        });
        assert!(log.contains("page extracted"), "{log}");
        assert!(log.contains("word_count"), "{log}");
    }

    #[test]
    fn a_non_zdr_span_keeps_everything() {
        let log = captured(|| {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            let _guard = span.enter();
            tracing::info!(url = "https://visible.test/page", "fetched");
        });
        assert!(log.contains("visible.test"), "{log}");
        // And with no span at all.
        let bare = captured(|| tracing::info!(url = "https://bare.test/", "fetched"));
        assert!(bare.contains("bare.test"), "{bare}");
    }

    #[test]
    fn a_late_record_true_drops_later_events_only() {
        let log = captured(|| {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            let _guard = span.enter();
            tracing::info!(url = "https://before.test/", "before the posture was known");
            span.record("zdr", true);
            tracing::info!(url = "https://after.test/", "after");
        });
        assert!(log.contains("before.test"), "{log}");
        assert!(!log.contains("after.test"), "{log}");
    }

    #[test]
    fn children_inherit_the_marker() {
        let log = captured(|| {
            let parent = tracing::info_span!("request", zdr = tracing::field::Empty);
            parent.record("zdr", true);
            let _parent = parent.enter();
            let child = tracing::info_span!("fetch");
            let _child = child.enter();
            tracing::info!(url = "https://child.test/", "in a child span");
            tracing::info!(count = 1, "safe event in a child span");
        });
        assert!(!log.contains("child.test"), "{log}");
        assert!(log.contains("safe event in a child span"), "{log}");
    }

    #[test]
    fn a_later_record_false_cannot_lower_it() {
        let log = captured(|| {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            span.record("zdr", true);
            span.record("zdr", false);
            let _guard = span.enter();
            tracing::info!(url = "https://lowered.test/", "after a false record");
        });
        assert!(!log.contains("lowered.test"), "{log}");
    }

    #[test]
    fn mark_current_span_marks_a_span_with_the_field_and_ignores_one_without() {
        let log = captured(|| {
            // No current span at all: does not panic, marks nothing.
            mark_current_span();
            tracing::info!(url = "https://nospan.test/", "no span");

            // A span without the field ignores the mark and does not panic.
            let plain = tracing::info_span!("plain");
            let _plain = plain.enter();
            mark_current_span();
            tracing::info!(url = "https://nofield.test/", "no field");
            drop(_plain);

            // A span with the field is marked.
            let marked = tracing::info_span!("request", zdr = tracing::field::Empty);
            let _marked = marked.enter();
            mark_current_span();
            tracing::info!(url = "https://on.test/", "zdr on");
            tracing::info!(count = 1, "safe event kept");
        });
        assert!(log.contains("nospan.test"), "{log}");
        assert!(log.contains("nofield.test"), "{log}");
        assert!(!log.contains("on.test/"), "{log}");
        assert!(log.contains("safe event kept"), "{log}");
    }

    #[test]
    fn the_lever_value_is_parsed_as_documented() {
        for on in ["off", "OFF", " Off ", "0", "false", "False", "\tfalse\n"] {
            assert!(disabled_from(Some(on)), "{on:?} turns the rule off");
        }
        for keep in [
            None,
            Some(""),
            Some("on"),
            Some("1"),
            Some("true"),
            Some("offf"),
            Some("no"),
        ] {
            assert!(!disabled_from(keep), "{keep:?} leaves the rule on");
        }
    }

    #[test]
    fn the_lever_turns_the_sensitive_field_rule_off_but_not_the_caps() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(json_layer(capture.clone(), ZdrLogFilter::new(None, true)));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            span.record("zdr", true);
            let _guard = span.enter();
            tracing::info!(url = "https://lever.test/", "kept with the rule off");
            tracing::debug!(target: "reqwest::connect", "CAP-STILL-ON");
        });
        let log = capture.text();
        assert!(log.contains("lever.test"), "{log}");
        assert!(!log.contains("CAP-STILL-ON"), "{log}");
    }

    /// The list is a contract with Quarry's edge, and this plane's own golden
    /// pins it so a change here is a visible diff.
    #[test]
    fn the_sensitive_field_list_matches_its_golden() {
        let golden: Vec<String> = serde_json::from_str(include_str!(
            "../tests/fixtures/zdr-sensitive-fields.golden.json"
        ))
        .expect("golden is a JSON array of strings");
        assert_eq!(
            golden.iter().map(String::as_str).collect::<Vec<_>>(),
            SENSITIVE_FIELDS.to_vec(),
            "SENSITIVE_FIELDS changed: update tests/fixtures/zdr-sensitive-fields.golden.json \
             and Quarry's quarry-edge zdr_log.rs and golden with it"
        );
    }

    /// The same list as Quarry's edge. Read from disk (not `include_str!`) so a
    /// checkout that has only the gateway fails with a message rather than a
    /// compile error.
    #[test]
    fn the_sensitive_field_list_equals_quarrys() {
        let quarry = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../../Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/fixtures/zdr-sensitive-fields.golden.json",
        );
        let text = std::fs::read_to_string(&quarry).unwrap_or_else(|error| {
            panic!(
                "Quarry's field-list golden is not readable at {} ({error}); \
                 this test mirrors it on purpose",
                quarry.display()
            )
        });
        let theirs: Vec<String> = serde_json::from_str(&text).expect("Quarry's golden is JSON");
        assert_eq!(
            theirs.iter().map(String::as_str).collect::<Vec<_>>(),
            SENSITIVE_FIELDS.to_vec(),
            "the gateway's SENSITIVE_FIELDS differs from Quarry's edge golden"
        );
    }

    /// The production stack (`production_subscriber`) with `env` as `RUST_LOG`,
    /// run through `harden` as `init` does.
    fn production_log(env: &str, body: impl FnOnce()) -> String {
        let capture = Capture::default();
        let subscriber = production_subscriber(capture.clone(), harden(EnvFilter::new(env)));
        tracing::subscriber::with_default(subscriber, body);
        capture.text()
    }

    #[test]
    fn harden_caps_the_crates_that_print_content_even_at_debug() {
        let log = production_log("debug", || {
            tracing::debug!(target: "html5ever::tree_builder", "processing Characters page text");
            tracing::debug!(target: "reqwest::connect", "starting new connection");
            tracing::debug!(target: "hyper_util::client::legacy::connect::http", "connecting to a host");
            tracing::warn!(target: "html5ever::tokenizer", "html5ever still warns");
            tracing::debug!(target: "verevon_gateway_rs::upstream", "our own debug stays on");
        });
        assert!(!log.contains("page text"), "{log}");
        assert!(!log.contains("starting new connection"), "{log}");
        assert!(!log.contains("connecting to a host"), "{log}");
        assert!(log.contains("html5ever still warns"), "{log}");
        assert!(log.contains("our own debug stays on"), "{log}");
    }

    /// An operator directive more specific than a cap (`reqwest::connect=trace`)
    /// must not lift it, for any of the capped crates.
    #[test]
    fn a_more_specific_directive_cannot_lift_a_cap() {
        let directives = "trace,html5ever::tokenizer=trace,markup5ever::interface=trace,\
            selectors::matching=trace,reqwest::connect=trace,\
            hyper_util::client::legacy::connect::http=trace,hyper::proto::h1::io=trace,\
            chromiumoxide::conn=trace";
        let log = production_log(directives, || {
            tracing::debug!(target: "html5ever::tokenizer", "LEAK html5ever");
            tracing::trace!(target: "markup5ever::interface", "LEAK markup5ever");
            tracing::debug!(target: "selectors::matching", "LEAK selectors");
            tracing::debug!(target: "reqwest::connect", "LEAK reqwest");
            tracing::debug!(target: "hyper_util::client::legacy::connect::http", "LEAK hyper_util");
            tracing::trace!(target: "hyper::proto::h1::io", "LEAK hyper");
            tracing::trace!(target: "chromiumoxide::conn", "LEAK chromiumoxide");
            // At the cap, and below it, stays.
            tracing::warn!(target: "html5ever::tokenizer", "KEPT html5ever warn");
            tracing::info!(target: "reqwest::connect", "KEPT reqwest info");
        });
        assert!(!log.contains("LEAK"), "{log}");
        for kept in ["KEPT html5ever warn", "KEPT reqwest info"] {
            assert!(log.contains(kept), "{kept} missing: {log}");
        }
    }

    /// The per-event cap matches the target or a module under it, never a name
    /// that merely starts with it. (`harden`'s directives are plain prefixes
    /// and would also cap `hyperion`; this is the guarantee's own precision.)
    #[test]
    fn the_cap_matches_module_boundaries_only() {
        let capture = Capture::default();
        let subscriber = production_subscriber(capture.clone(), EnvFilter::new("trace"));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "hyperion::x", "KEPT hyperion");
            tracing::debug!(target: "reqwest_middleware", "KEPT reqwest_middleware");
            tracing::debug!(target: "reqwest", "LEAK reqwest root");
            tracing::debug!(target: "hyper::proto", "LEAK hyper module");
        });
        let log = capture.text();
        assert!(log.contains("KEPT hyperion"), "{log}");
        assert!(log.contains("KEPT reqwest_middleware"), "{log}");
        assert!(!log.contains("LEAK"), "{log}");
    }

    // ---- the mark and the filter do not depend on RUST_LOG -----------------

    /// One request under the gateway's own HTTP span, with the production stack
    /// and `env` as `RUST_LOG`: a ZDR (or not) request that logs error, warn and
    /// info events with sensitive field names, and one safe error event.
    fn request_log(env: &str, zdr: bool) -> String {
        production_log(env, || {
            let request = axum::http::Request::builder()
                .uri("/api/v1/search/web?q=SECRET-QUERY-STRING")
                .body(axum::body::Body::empty())
                .unwrap();
            let span = crate::make_http_trace_span(&request);
            let _guard = span.enter();
            if zdr {
                mark_current_span();
            }
            tracing::error!(error = "SECRET-ERROR-TEXT", "boom");
            tracing::warn!(url = "https://secret.test/page", "warned");
            tracing::info!(query = "SECRET-QUERY", "info with a query");
            tracing::error!(status = 500, "SAFE-ERROR-EVENT");
        })
    }

    /// The defect this pins: with a global `EnvFilter` in the registry, the
    /// info-level HTTP span was never created at `RUST_LOG=error` (the binary's
    /// default) or `warn`, so nothing was marked and every error event was
    /// written in full for a ZDR caller.
    #[test]
    fn a_zdr_request_drops_sensitive_events_at_every_rust_log_level() {
        for env in ["error", "warn", "info", "debug", "trace"] {
            let log = request_log(env, true);
            assert!(!log.contains("SECRET"), "RUST_LOG={env}: {log}");
            assert!(!log.contains("secret.test"), "RUST_LOG={env}: {log}");
            assert!(log.contains("SAFE-ERROR-EVENT"), "RUST_LOG={env}: {log}");
            assert!(
                log.contains("\"zdr\":true"),
                "RUST_LOG={env}: the span carries the mark: {log}"
            );
        }
    }

    /// The control: the level filter still works, and the ZDR rule only acts
    /// under a ZDR span.
    #[test]
    fn the_level_filter_still_applies_and_a_non_zdr_request_keeps_its_events() {
        let at_error = request_log("error", false);
        assert!(at_error.contains("SECRET-ERROR-TEXT"), "{at_error}");
        assert!(at_error.contains("SAFE-ERROR-EVENT"), "{at_error}");
        assert!(!at_error.contains("secret.test"), "warn is below error: {at_error}");
        assert!(!at_error.contains("SECRET-QUERY"), "info is below error: {at_error}");
        assert!(!at_error.contains("\"zdr\":true"), "{at_error}");

        let at_info = request_log("info", false);
        assert!(at_info.contains("secret.test"), "{at_info}");
        assert!(at_info.contains("SECRET-QUERY"), "{at_info}");
    }

    // ---- a second mark of the same span ------------------------------------

    /// Tasks that inherit one span can all learn the posture at once. A
    /// look-then-insert on the span's extensions panics on the second insert.
    #[test]
    fn concurrent_marks_of_one_span_do_not_panic() {
        use std::sync::Barrier;

        let capture = Capture::default();
        let dispatch = tracing::Dispatch::new(production_subscriber(
            capture.clone(),
            EnvFilter::new("error"),
        ));
        let span = tracing::dispatcher::with_default(&dispatch, || {
            tracing::info_span!("request", zdr = tracing::field::Empty)
        });
        for round in 0..1500 {
            let span = tracing::dispatcher::with_default(&dispatch, || {
                tracing::info_span!("request", zdr = tracing::field::Empty, round)
            });
            let barrier = Barrier::new(4);
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| {
                        barrier.wait();
                        span.record("zdr", true);
                    });
                }
            });
        }
        drop(span);
        assert!(capture.text().is_empty(), "no event was logged");
    }

    // ---- spans that carry a sensitive field --------------------------------

    #[test]
    fn a_span_with_a_sensitive_field_under_zdr_is_withheld_from_the_log() {
        let log = captured(|| {
            let parent = tracing::info_span!("request", zdr = tracing::field::Empty);
            parent.record("zdr", true);
            let _parent = parent.enter();
            let child = tracing::info_span!("upstream_call", url = "https://span-leak.test/?q=SPANQ");
            let _child = child.enter();
            tracing::info!(count = 1, "safe child event");
        });
        assert!(log.contains("safe child event"), "{log}");
        assert!(!log.contains("span-leak"), "{log}");
        assert!(!log.contains("SPANQ"), "{log}");
    }

    #[test]
    fn a_span_with_a_sensitive_field_outside_zdr_stays_in_the_log() {
        let log = captured(|| {
            let parent = tracing::info_span!("request", zdr = tracing::field::Empty);
            let _parent = parent.enter();
            let child = tracing::info_span!("upstream_call", url = "https://span-visible.test/");
            let _child = child.enter();
            tracing::info!(count = 1, "safe child event");
        });
        assert!(log.contains("span-visible.test"), "{log}");
    }

    #[test]
    fn capped_targets_equal_quarrys() {
        // The same seven crates at the same levels as Quarry's edge. Kept as a
        // literal here so a change on either side is a deliberate, visible diff.
        assert_eq!(
            CAPPED_TARGETS,
            &[
                ("html5ever", "warn"),
                ("markup5ever", "warn"),
                ("selectors", "warn"),
                ("reqwest", "info"),
                ("hyper_util", "info"),
                ("hyper", "info"),
                ("chromiumoxide", "info"),
            ]
        );
    }

    #[test]
    fn upstream_origin_keeps_the_service_and_drops_everything_else() {
        assert_eq!(
            upstream_origin("http://org-core:3000/api/v1/brreg/search?q=secret&size=8"),
            "http://org-core:3000"
        );
        assert_eq!(
            upstream_origin("https://user:pw@example.test/path#frag"),
            "https://example.test"
        );
        assert_eq!(upstream_origin("not a url secret-text"), "invalid");
    }

    // ---- every place that learns the posture marks the span ----------------

    /// Runs `future` inside the gateway's own HTTP span (built by
    /// `make_http_trace_span`, as the router does) and says whether the span
    /// ended up marked ZDR. Marked means the JSON log context carries
    /// `"zdr":true` on an event logged after the future finished. Needs the
    /// default single-threaded test runtime (the subscriber is thread-local).
    async fn span_marked_by<F: std::future::Future>(future: F) -> (F::Output, bool) {
        use tracing::Instrument;

        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(build_json_layer(capture.clone()));
        let _default = tracing::subscriber::set_default(subscriber);
        let request = axum::http::Request::builder()
            .uri("/p?q=secret")
            .body(axum::body::Body::empty())
            .unwrap();
        let span = crate::make_http_trace_span(&request);
        let output = async {
            let output = future.await;
            tracing::info!(count = 1, "after");
            output
        }
        .instrument(span)
        .await;
        (output, capture.text().contains("\"zdr\":true"))
    }

    fn member() -> crate::middleware::AuthenticatedUser {
        crate::middleware::AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// An org-core that answers `GET /api/v1/organizations/org-test` with `reply`.
    async fn org_core(reply: wiremock::ResponseTemplate) -> wiremock::MockServer {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org-test"))
            .respond_with(reply)
            .mount(&server)
            .await;
        server
    }

    fn zdr_org() -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "metadata": { "interactiveRetention": { "zdr": true } }
        }))
    }

    fn plain_org() -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "metadata": { "interactiveRetention": { "zdr": false } }
        }))
    }

    #[tokio::test]
    async fn the_header_marks_the_span() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-zdr", "true".parse().unwrap());
        let (zdr, marked) = span_marked_by(async { crate::zdr::request_zdr(&headers, None) }).await;
        assert!(zdr && marked);

        let headers = axum::http::HeaderMap::new();
        let (zdr, marked) = span_marked_by(async { crate::zdr::request_zdr(&headers, None) }).await;
        assert!(!zdr && !marked, "no header, no body flag: nothing to mark");
    }

    #[tokio::test]
    async fn a_body_flag_marks_the_span_in_request_zdr() {
        let headers = axum::http::HeaderMap::new();
        let body = serde_json::json!({ "zdr": true });
        let (zdr, marked) =
            span_marked_by(async { crate::zdr::request_zdr(&headers, Some(&body)) }).await;
        assert!(zdr && marked);

        let body = serde_json::json!({ "zdr": false });
        let (zdr, marked) =
            span_marked_by(async { crate::zdr::request_zdr(&headers, Some(&body)) }).await;
        assert!(!zdr && !marked);
    }

    #[tokio::test]
    async fn a_body_flag_marks_the_span_in_normalized_model_body() {
        let headers = axum::http::HeaderMap::new();
        let (body, marked) = span_marked_by(async {
            crate::domains::chat::shared::normalized_model_body(
                serde_json::json!({ "zdr": true }),
                &headers,
            )
        })
        .await;
        assert_eq!(body["zdr"], serde_json::json!(true));
        assert!(marked);

        let (body, marked) = span_marked_by(async {
            crate::domains::chat::shared::normalized_model_body(
                serde_json::json!({ "zdr": false }),
                &headers,
            )
        })
        .await;
        assert_eq!(body["zdr"], serde_json::json!(false));
        assert!(!marked);
    }

    #[tokio::test]
    async fn an_ag_ui_turn_marks_the_span_for_a_body_flag_and_for_the_header() {
        let headers = axum::http::HeaderMap::new();
        let body = serde_json::json!({ "zdr": true });
        let (zdr, marked) =
            span_marked_by(async { crate::domains::ag_ui::turn_requests_zdr(&headers, &body) })
                .await;
        assert!(zdr && marked);

        let body = serde_json::json!({});
        let (zdr, marked) =
            span_marked_by(async { crate::domains::ag_ui::turn_requests_zdr(&headers, &body) })
                .await;
        assert!(!zdr && !marked);

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-zdr", "1".parse().unwrap());
        let (zdr, marked) =
            span_marked_by(async { crate::domains::ag_ui::turn_requests_zdr(&headers, &body) })
                .await;
        assert!(zdr && marked);
    }

    #[tokio::test]
    async fn org_posture_marks_the_span_when_org_core_says_zdr_and_not_otherwise() {
        use crate::zdr::{org_posture, OrgPosture};

        let zdr = org_core(zdr_org()).await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = zdr.uri();
        let user = member();
        let (posture, marked) = span_marked_by(org_posture(&state, &user)).await;
        assert_eq!(posture, OrgPosture::Zdr);
        assert!(marked, "org-core said ZDR");

        let plain = org_core(plain_org()).await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = plain.uri();
        let (posture, marked) = span_marked_by(org_posture(&state, &user)).await;
        assert_eq!(posture, OrgPosture::NotZdr);
        assert!(!marked, "org-core said not ZDR");
    }

    /// The cached answer is a separate branch of `org_posture`.
    #[tokio::test]
    async fn a_cached_zdr_posture_marks_the_span_without_asking_org_core() {
        use crate::zdr::{org_posture, OrgPosture};

        for (cached, expected, expect_marked) in [
            (true, OrgPosture::Zdr, true),
            (false, OrgPosture::NotZdr, false),
        ] {
            let mut state = crate::tests::test_state(false);
            // Unroutable on purpose: a cache hit must not need org-core.
            state.org_core_url = "http://127.0.0.1:1".to_owned();
            state.cache = crate::cache::ResultCache::in_memory();
            let key = crate::cache::cache_key("org-zdr", &["org-test"]);
            state
                .cache
                .store_for_secs(&key, &serde_json::Value::Bool(cached), 60)
                .await;
            let user = member();
            let (posture, marked) = span_marked_by(org_posture(&state, &user)).await;
            assert_eq!(posture, expected);
            assert_eq!(marked, expect_marked, "cached zdr={cached}");
        }
    }

    /// `org_zdr_enabled` has its own mark: `org_posture` does not mark an
    /// answer it could not get, but the fail-closed caller does.
    #[tokio::test]
    async fn org_zdr_enabled_marks_the_span_for_zdr_and_for_the_fail_closed_case() {
        let user = member();

        let zdr = org_core(zdr_org()).await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = zdr.uri();
        let (enabled, marked) =
            span_marked_by(crate::zdr::org_zdr_enabled(&state, &user)).await;
        assert!(enabled && marked, "org-core said ZDR");

        let plain = org_core(plain_org()).await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = plain.uri();
        let (enabled, marked) =
            span_marked_by(crate::zdr::org_zdr_enabled(&state, &user)).await;
        assert!(!enabled && !marked, "org-core said not ZDR");

        let down = org_core(wiremock::ResponseTemplate::new(500)).await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = down.uri();
        let (posture, marked) =
            span_marked_by(crate::zdr::org_posture(&state, &user)).await;
        assert_eq!(posture, crate::zdr::OrgPosture::Unknown);
        assert!(!marked, "org_posture itself marks only a real ZDR answer");
        let (enabled, marked) =
            span_marked_by(crate::zdr::org_zdr_enabled(&state, &user)).await;
        assert!(enabled, "an org-core outage fails closed");
        assert!(marked, "and the fail-closed answer marks the span");
    }

    // ---- the request span and the real handlers ---------------------------

    /// The HTTP span the router builds records the path and never the query
    /// string, which can hold the very URL a ZDR caller asked about.
    #[test]
    fn the_request_span_never_prints_the_query_string() {
        let log = captured(|| {
            let request = axum::http::Request::builder()
                .uri("/api/v1/search/web?q=secret-query-text&url=https://hunter.test/page")
                .body(axum::body::Body::empty())
                .unwrap();
            let span = crate::make_http_trace_span(&request);
            let _guard = span.enter();
            tracing::info!(status = 200, "handled");
        });
        assert!(log.contains("/api/v1/search/web"), "{log}");
        assert!(!log.contains("secret-query-text"), "{log}");
        assert!(!log.contains("hunter"), "{log}");
    }

    /// A router with the gateway's own span and one handler that behaves like a
    /// ZDR-aware proxy: it resolves the posture with the real `request_zdr`,
    /// logs a query-named field, logs a safe field, then calls an upstream that
    /// is down with the caller's text in the URL.
    async fn probe_logs(zdr_header: bool) -> String {
        use axum::body::Body;
        use axum::extract::State;
        use axum::http::{HeaderMap, Request, StatusCode};
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;
        use tower_http::trace::TraceLayer;

        use crate::config::AppState;

        async fn probe(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
            let _zdr = crate::zdr::request_zdr(&headers, None);
            tracing::info!(query = "HANDLER-QUERY-TEXT", "handler saw the query");
            tracing::info!(count = 1, "safe handler event");
            let (status, _) = crate::upstream::proxy_json(
                &state,
                reqwest::Method::GET,
                "http://127.0.0.1:1/v1/search?q=UPSTREAM-URL-QUERY",
                None,
                None,
                None,
                None,
            )
            .await;
            status
        }

        let app = Router::new()
            .route("/probe", get(probe))
            .layer(TraceLayer::new_for_http().make_span_with(crate::make_http_trace_span))
            .with_state(crate::tests::test_state(false));

        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(build_json_layer(capture.clone()));
        let _default = tracing::subscriber::set_default(subscriber);

        let mut request = Request::builder().uri("/probe?q=REQUEST-QUERY-STRING");
        if zdr_header {
            request = request.header("x-zdr", "true");
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        capture.text()
    }

    #[tokio::test]
    async fn a_zdr_request_logs_no_query_url_or_error_text_through_the_real_stack() {
        let log = probe_logs(true).await;
        // The posture header marked the span, so the query-named event is gone.
        assert!(!log.contains("HANDLER-QUERY-TEXT"), "{log}");
        assert!(log.contains("\"zdr\":true"), "the span carries the mark: {log}");
        // The upstream failure survives, as a class and a service, with no URL.
        assert!(log.contains("upstream request failed before a response"), "{log}");
        assert!(log.contains("\"error_class\":\"connect\""), "{log}");
        assert!(log.contains("\"upstream\":\"http://127.0.0.1:1\""), "{log}");
        assert!(!log.contains("UPSTREAM-URL-QUERY"), "{log}");
        assert!(!log.contains("REQUEST-QUERY-STRING"), "{log}");
        assert!(log.contains("safe handler event"), "{log}");
    }

    #[tokio::test]
    async fn a_non_zdr_request_keeps_its_events_and_still_logs_no_url_text() {
        let log = probe_logs(false).await;
        // The control: capture works, and the filter only acts under ZDR.
        assert!(log.contains("HANDLER-QUERY-TEXT"), "{log}");
        assert!(!log.contains("\"zdr\":true"), "{log}");
        assert!(log.contains("\"error_class\":\"connect\""), "{log}");
        // The upstream failure no longer names the URL for anyone.
        assert!(!log.contains("UPSTREAM-URL-QUERY"), "{log}");
        assert!(!log.contains("REQUEST-QUERY-STRING"), "{log}");
    }

    /// A ZDR search through the real router: the header marks the request, the
    /// upstream is unreachable, and nothing the gateway logs contains the query
    /// text, the same text in the request's own query string, or the URL.
    #[tokio::test]
    async fn a_zdr_search_through_the_gateway_logs_no_event_with_the_query_text() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use serde_json::json;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "user-1", "email": "user@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-1" }
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1",
                "orgId": "org-1",
                "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let app = crate::build_router(state);

        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(build_json_layer(capture.clone()));
        let _default = tracing::subscriber::set_default(subscriber);

        for (uri, body) in [
            (
                "/api/v1/search/web?q=SEARCH-QUERY-IN-URL",
                json!({ "query": "SEARCH-QUERY-IN-BODY" }),
            ),
            (
                "/api/v1/search/web",
                json!({ "query": "https://secret-site.test/SEARCH-URL-IN-BODY" }),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("cookie", "idknuten.sid=session")
                        .header("content-type", "application/json")
                        .header("x-zdr", "true")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{uri}");
        }

        let log = capture.text();
        // The capture holds the gateway's own events, not only dependency noise,
        // and each of them sits under an HTTP span that carries the mark: the
        // x-zdr header reached `zdr_flag` through the real router.
        let own = own_events(&log);
        assert_eq!(own.len(), 2, "one authenticated-request event per request: {log}");
        for event in &own {
            assert_eq!(event["span"]["name"], "http_request", "{event}");
            assert_eq!(event["span"]["zdr"], true, "the span carries the mark: {event}");
        }
        for needle in [
            "SEARCH-QUERY-IN-URL",
            "SEARCH-QUERY-IN-BODY",
            "SEARCH-URL-IN-BODY",
            "secret-site",
        ] {
            assert!(!log.contains(needle), "{needle} reached the log: {log}");
        }

        // The control: the same router without the header leaves the span
        // unmarked, so the mark above is the header's doing.
        let before = log.len();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/search/web")
                    .header("cookie", "idknuten.sid=session")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&json!({ "query": "CONTROL" })).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let after = capture.text();
        let own = own_events(&after[before..]);
        assert_eq!(own.len(), 1, "{after}");
        assert!(own[0]["span"].get("zdr").is_none(), "not marked: {}", own[0]);
    }

    /// The JSON events the gateway itself logged for a request (the
    /// authenticated-request event), as opposed to its dependencies' output.
    fn own_events(log: &str) -> Vec<serde_json::Value> {
        log.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["fields"]["message"] == "gateway authenticated request")
            .collect()
    }

    #[tokio::test]
    async fn class_reqwest_is_a_closed_vocabulary_that_never_carries_the_url() {
        let error = reqwest::Client::new()
            .get("http://127.0.0.1:1/secret-path?q=hunter")
            .send()
            .await
            .expect_err("nothing listens on port 1");
        let class = class_reqwest(&error);
        assert!(
            ["timeout", "connect", "status", "decode", "request"].contains(&class),
            "{class}"
        );
        assert!(!class.contains("secret") && !class.contains("hunter"));
        // The probe that motivates the class: the error's own text carries the URL.
        assert!(error.to_string().contains("secret-path"), "{error}");
    }
}
