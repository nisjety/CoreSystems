//! What the process log may say while a request runs under Zero Data Retention.
//!
//! A log line is a durable copy of whatever it names. WS4.D removed the known
//! leaks; this module makes the rule structural. A request that runs under ZDR
//! carries `zdr = true` on its span (set by the earliest code that learns the
//! posture, see [`quarry_core::zdr::ZdrMode::mark_current_span`]); the filter
//! below drops every event, under that span or any child of it, that carries a
//! field whose name says it holds a URL, a query, a title or an error text
//! ([`SENSITIVE_FIELDS`]). Everything else (counts, status, class, ids) is kept.
//!
//! What this does and does not do:
//! - It drops whole events. A request-path `error!(error = %e, ...)` disappears
//!   for a ZDR caller until it is rewritten to log `error_class = class(&e)`;
//!   that rewrite is the log-field lint's job, not this filter's.
//! - It reads span context. An event emitted from a spawned task that did not
//!   inherit the span is not covered; spawn with `.in_current_span()`.
//! - The mark is sticky: set once, it stays for the life of the span, and a later
//!   `record(zdr, false)` cannot lower it. Events logged before the posture was
//!   known are not recalled; late ZDR cannot be undone for logs.
//! - The mark does not depend on `RUST_LOG`. The operator's level filter lives on
//!   each output layer ([`ZdrLogFilter`]), never on the registry, so the request
//!   span is created, marked and visible to the lookups below even at
//!   `RUST_LOG=error`, `RUST_LOG=off` or a per-target directive that silences
//!   the module that creates it. (Round 1 found the gateway had a global
//!   `EnvFilter` in the registry; this edge had the same wiring in
//!   `telemetry.rs`.) A span that declares the `zdr` field always passes the
//!   level filter for that reason; every other span follows `RUST_LOG`.
//! - The third-party caps ([`CAPPED_TARGETS`]) are enforced per event, so a more
//!   specific operator directive (`reqwest::connect=trace`) cannot lift them.
//! - A span that carries a sensitive field name (`info_span!("x", url = ..)`)
//!   and is created under a ZDR span is withheld from the log too, so its
//!   fields do not ride along in the context of other events. `Span::record`
//!   of a sensitive name after creation is not covered; the lint reads span
//!   macros.
//! - It is a filter on the *output layers* (the JSON log and the OTLP exporter).
//!   It does not stop code from computing or returning the data.
//!
//! `QUARRY_ZDR_LOG_FILTER=off` turns the sensitive-field filter off (an
//! emergency lever for a debugging session on a non-production edge; read once
//! at start). The third-party caps stay on.

use axum::extract::Request;
use axum::http::Request as HttpRequest;
use axum::middleware::Next;
use axum::response::Response;
use quarry_core::error::QuarryError;
use quarry_core::zdr::ZDR_SPAN_FIELD;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{field::Visit, Level, Metadata, Span, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Filter, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

/// Field names that say "this holds what the caller asked about". An event with
/// any of them is dropped while a ZDR span is in scope.
///
/// One list, pinned by `tests/fixtures/zdr-sensitive-fields.golden.json` so a
/// sibling plane can vendor it rather than guess.
pub const SENSITIVE_FIELDS: &[&str] = &[
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
pub struct ZdrMarkLayer;

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
/// CDP traffic. They reach the log through the `log` bridge with only a
/// `message`, so the sensitive-field filter cannot see what is inside, and they
/// are capped here whatever `RUST_LOG` says. Found by the debug-level capture
/// test: `html5ever` at debug logs every text node it parses, title and body
/// included, for every request, ZDR or not.
pub const CAPPED_TARGETS: &[(&str, &str)] = &[
    ("html5ever", "warn"),
    ("markup5ever", "warn"),
    ("selectors", "warn"),
    ("reqwest", "info"),
    ("hyper_util", "info"),
    ("hyper", "info"),
    ("chromiumoxide", "info"),
];

/// Whether this event or span is more verbose than the cap on its target (the
/// target itself or any module under it). Compared per event, so no directive an
/// operator writes, however specific, can lift a cap.
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
/// common case cheap (the callsite is disabled once, up front); the guarantee is
/// [`exceeds_cap`], which [`ZdrLogFilter`] applies to every event: a more
/// specific directive such as `reqwest::connect=trace` outranks `reqwest=info`
/// inside an `EnvFilter`, so the directives alone cannot hold the cap.
pub fn harden(mut filter: EnvFilter) -> EnvFilter {
    for (target, level) in CAPPED_TARGETS {
        if let Ok(directive) = format!("{target}={level}").parse() {
            filter = filter.add_directive(directive);
        }
    }
    filter
}

/// Whether the `QUARRY_ZDR_LOG_FILTER` value turns the sensitive-field filter
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
    disabled_from(std::env::var("QUARRY_ZDR_LOG_FILTER").ok().as_deref())
}

/// The per-layer filter every output layer (JSON log, OTLP exporter) carries. It
/// combines, in this order:
///
/// 1. the third-party caps ([`exceeds_cap`]), always;
/// 2. the operator's `RUST_LOG` filter. A span that declares the `zdr` field
///    passes it unconditionally, so the request span exists, is marked and is
///    visible to the scope lookups below even at `RUST_LOG=error`. Events and
///    all other spans follow it;
/// 3. the ZDR rule: an event or span with a sensitive field name is dropped while
///    a ZDR span is in scope (unless the lever is off).
///
/// A span an output layer does not enable is skipped by that layer's scope walk,
/// not treated as the end of the chain, so a disabled child span cannot hide the
/// marked request span above it.
pub struct ZdrLogFilter {
    env: Option<EnvFilter>,
    zdr_rule_off: bool,
}

impl ZdrLogFilter {
    /// `env` is the operator's level filter; `None` lets every level through.
    pub fn new(env: Option<EnvFilter>, zdr_rule_off: bool) -> Self {
        Self { env, zdr_rule_off }
    }

    fn sensitive_under_zdr<S>(&self, meta: &Metadata<'_>, ctx: &Context<'_, S>) -> bool
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        !self.zdr_rule_off && carries_sensitive_field(meta) && in_zdr_scope(ctx)
    }
}

/// A span that can carry the ZDR mark (the request span declares `zdr`).
fn declares_mark(meta: &Metadata<'_>) -> bool {
    meta.is_span() && meta.fields().field(ZDR_SPAN_FIELD).is_some()
}

impl<S> Filter<S> for ZdrLogFilter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(&self, meta: &Metadata<'_>, ctx: &Context<'_, S>) -> bool {
        if exceeds_cap(meta) {
            return false;
        }
        if !declares_mark(meta) {
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
        // Consulted for every callsite so the env filter registers its dynamic
        // (span-field) directives, but it never hides a span that can carry the mark.
        let from_env = self
            .env
            .as_ref()
            .map(|env| Filter::<S>::callsite_enabled(env, meta))
            .filter(|_| !declares_mark(meta));
        // Which side of the ZDR rule a callsite falls on depends on the request,
        // so a callsite with a sensitive field is decided per call.
        let per_call = !self.zdr_rule_off && carries_sensitive_field(meta);
        match from_env {
            Some(interest) if interest.is_never() => Interest::never(),
            Some(interest) if !per_call => interest,
            None if !per_call => Interest::always(),
            _ => Interest::sometimes(),
        }
    }

    // No `max_level_hint`: a span that declares the mark passes at every level,
    // so no ceiling holds.

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

/// The filter for the OTLP exporter layer: the same rules as the log writer, with
/// its own copy of the operator's level filter (an `EnvFilter` is not `Clone`).
pub fn export_filter(env: EnvFilter) -> ZdrLogFilter {
    ZdrLogFilter::new(Some(env), filter_disabled())
}

/// The JSON log layer the edge writes to `writer`, filtered by `env` (the
/// operator's `RUST_LOG`, already passed through [`harden`]) and by the ZDR rule.
/// The level filter sits on this layer, not on the registry.
pub fn log_layer<S, W>(writer: W, env: EnvFilter) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(ZdrLogFilter::new(Some(env), filter_disabled()))
}

/// The JSON log layer with the ZDR filter and the caps but no `RUST_LOG` level
/// filter (every level passes), for tests that only care about the ZDR rule.
#[cfg(test)]
pub fn build_json_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(ZdrLogFilter::new(None, filter_disabled()))
}

/// The subscriber the edge runs without an OTLP exporter: the sticky ZDR mark and
/// a JSON log on `writer`. `telemetry::init_telemetry` installs it; the capture
/// tests build it with the `RUST_LOG` they want to probe.
pub fn production_subscriber<W>(writer: W, env: EnvFilter) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(ZdrMarkLayer)
        .with(log_layer(writer, env))
}

/// The subscriber the edge runs with an OTLP exporter: the sticky ZDR mark, the
/// JSON log on `writer` and `exporter` (the OpenTelemetry layer), each output
/// layer behind its own [`ZdrLogFilter`] and nothing level-filtered on the
/// registry. `telemetry::init_telemetry` installs it with the real exporter; a
/// test installs it with a recording layer, so the OTLP wiring is held to the
/// same rules as [`production_subscriber`]. Two level filters because an
/// `EnvFilter` is not `Clone`.
pub fn otel_subscriber<W, E>(
    writer: W,
    log_env: EnvFilter,
    export_env: EnvFilter,
    exporter: E,
) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
    E: Layer<tracing_subscriber::layer::Layered<ZdrMarkLayer, tracing_subscriber::Registry>>
        + Send
        + Sync
        + 'static,
{
    tracing_subscriber::registry()
        .with(ZdrMarkLayer)
        .with(exporter.with_filter(export_filter(export_env)))
        .with(log_layer(writer, log_env))
}

/// The span for one HTTP request: method and path only. tower-http's default
/// span records the full request URI, query string included, and a query string
/// can hold the very URL a ZDR caller asked about. `zdr` starts empty and is
/// recorded by `mark_floor` or by the handler that learns the posture.
pub fn make_request_span<B>(request: &HttpRequest<B>) -> Span {
    tracing::info_span!(
        "request",
        method = %request.method(),
        path = %request.uri().path(),
        zdr = tracing::field::Empty,
    )
}

/// Marks the request span ZDR for a user of a ZDR organization before the
/// handler runs. Layer it after `require_auth` (which inserts the claims).
pub async fn mark_floor(request: Request, next: Next) -> Response {
    let floor = request
        .extensions()
        .get::<crate::auth::Claims>()
        .is_some_and(crate::auth::Claims::zdr_floor);
    if floor {
        Span::current().record(ZDR_SPAN_FIELD, true);
    }
    next.run(request).await
}

/// A closed-vocabulary label for an error, safe to log under ZDR: the error's
/// wire code (`TIMEOUT`, `DRIVER_FAILED`, ...), never its message.
pub fn class(error: &QuarryError) -> &'static str {
    error.code.wire_name()
}

/// A closed-vocabulary label for a transport error: one of `timeout`,
/// `connect`, `status`, `decode`, `request`. `reqwest::Error`'s own text embeds
/// the request URL, query string included.
pub fn class_reqwest(error: &reqwest::Error) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

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
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

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
    fn mark_current_span_is_a_no_op_off_and_without_a_field() {
        use quarry_core::zdr::ZdrMode;
        let log = captured(|| {
            // Off never marks.
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            let _guard = span.enter();
            ZdrMode::Off.mark_current_span();
            tracing::info!(url = "https://off.test/", "zdr off");
            drop(_guard);
            // A span without the field ignores the mark and does not panic.
            let plain = tracing::info_span!("plain");
            let _plain = plain.enter();
            ZdrMode::On.mark_current_span();
            tracing::info!(url = "https://nofield.test/", "no field");
            drop(_plain);
            // On with the field marks.
            let marked = tracing::info_span!("request", zdr = tracing::field::Empty);
            let _marked = marked.enter();
            ZdrMode::On.mark_current_span();
            tracing::info!(url = "https://on.test/", "zdr on");
        });
        assert!(log.contains("off.test"), "{log}");
        assert!(log.contains("nofield.test"), "{log}");
        assert!(!log.contains("on.test/"), "{log}");
    }

    #[test]
    fn the_request_span_never_prints_the_query_string() {
        let log = captured(|| {
            let request = HttpRequest::builder()
                .uri("/v1/change/latest?url=https://secret-query.test/page&q=hunter")
                .body(())
                .unwrap();
            let span = make_request_span(&request);
            let _guard = span.enter();
            tracing::info!(status = 200, "handled");
        });
        assert!(log.contains("/v1/change/latest"), "{log}");
        assert!(!log.contains("secret-query"), "{log}");
        assert!(!log.contains("hunter"), "{log}");
    }

    #[tokio::test]
    async fn the_floor_middleware_marks_a_zdr_org_user_before_the_handler_runs() {
        use axum::body::Body;
        use axum::routing::get;
        use axum::{middleware, Extension, Router};
        use tower::ServiceExt;
        use tower_http::trace::TraceLayer;

        fn claims(zdr: bool) -> crate::auth::Claims {
            let mut claims = crate::test_support::claims_for_org("org-floor");
            claims.zdr = Some(zdr);
            claims
        }

        async fn handler() -> &'static str {
            tracing::info!(url = "https://handler.test/", "in the handler");
            tracing::info!(count = 1, "safe handler event");
            "ok"
        }

        let app = |zdr: bool| {
            Router::new()
                .route("/probe", get(handler))
                .layer(middleware::from_fn(mark_floor))
                .layer(Extension(claims(zdr)))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &HttpRequest<_>| make_request_span(request)),
                )
        };

        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(build_json_layer(capture.clone()));
        let _default = tracing::subscriber::set_default(subscriber);

        let request = || {
            HttpRequest::builder()
                .uri("/probe?url=https://floor-query.test/")
                .body(Body::empty())
                .unwrap()
        };
        app(true).oneshot(request()).await.unwrap();
        let zdr_log = capture.text();
        assert!(!zdr_log.contains("handler.test"), "{zdr_log}");
        assert!(zdr_log.contains("safe handler event"), "{zdr_log}");

        capture.0.lock().unwrap().clear();
        app(false).oneshot(request()).await.unwrap();
        let plain_log = capture.text();
        assert!(plain_log.contains("handler.test"), "{plain_log}");
        assert!(
            !plain_log.contains("floor-query"),
            "the query string never reaches the span: {plain_log}"
        );
    }

    /// The list is a contract with the planes that vendor it.
    #[test]
    fn the_sensitive_field_list_matches_its_golden() {
        let golden: Vec<String> = serde_json::from_str(include_str!(
            "../tests/fixtures/zdr-sensitive-fields.golden.json"
        ))
        .expect("golden is a JSON array of strings");
        let current: Vec<&str> = SENSITIVE_FIELDS.to_vec();
        assert_eq!(
            golden.iter().map(String::as_str).collect::<Vec<_>>(),
            current,
            "SENSITIVE_FIELDS changed: update tests/fixtures/zdr-sensitive-fields.golden.json \
             and tell the planes that vendor it"
        );
    }

    #[test]
    fn harden_caps_the_crates_that_print_page_content_even_at_debug() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(super::harden(EnvFilter::new("debug")))
            .with(build_json_layer(capture.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "html5ever::tree_builder", "processing Characters page text");
            tracing::debug!(target: "reqwest::connect", "starting new connection");
            tracing::debug!(target: "chromiumoxide::conn", "Sending CDP message");
            tracing::warn!(target: "html5ever::tokenizer", "html5ever still warns");
            tracing::debug!(target: "quarry_runtime::pipeline", "our own debug stays on");
        });
        let log = capture.text();
        assert!(!log.contains("page text"), "{log}");
        assert!(!log.contains("starting new connection"), "{log}");
        assert!(!log.contains("Sending CDP message"), "{log}");
        assert!(log.contains("html5ever still warns"), "{log}");
        assert!(log.contains("our own debug stays on"), "{log}");
    }

    /// A body whose text embeds a URL, so a leak of the error text is visible.
    const SECRET_URL: &str = "http://127.0.0.1:1/secret-path?q=hunter";

    #[tokio::test]
    async fn class_reqwest_labels_a_connect_failure() {
        let error = reqwest::get(SECRET_URL).await.unwrap_err();
        assert!(error.to_string().contains("secret-path"), "premise: the raw text embeds the URL");
        assert_eq!(class_reqwest(&error), "connect");
    }

    #[tokio::test]
    async fn class_reqwest_labels_a_timeout() {
        // A listener that accepts and never answers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _accepting = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(50))
            .build()
            .unwrap();
        let error = client
            .get(format!("http://{address}/secret-path?q=hunter"))
            .send()
            .await
            .unwrap_err();
        assert_eq!(class_reqwest(&error), "timeout");
    }

    #[test]
    fn class_reqwest_labels_a_status_error() {
        let response = reqwest::Response::from(
            axum::http::Response::builder().status(500).body("").unwrap(),
        );
        let error = response.error_for_status().unwrap_err();
        assert_eq!(class_reqwest(&error), "status");
    }

    #[tokio::test]
    async fn class_reqwest_labels_a_decode_failure() {
        let response = reqwest::Response::from(
            axum::http::Response::builder().status(200).body("not json").unwrap(),
        );
        let error = response.json::<serde_json::Value>().await.unwrap_err();
        assert_eq!(class_reqwest(&error), "decode");
    }

    #[test]
    fn class_reqwest_labels_a_malformed_request_as_request() {
        let error = reqwest::Client::new().get("not a url").build().unwrap_err();
        assert_eq!(class_reqwest(&error), "request");
    }

    #[test]
    fn classes_are_closed_vocabularies_that_never_carry_text() {
        use quarry_core::error::ErrorCode;
        let error = QuarryError::new(
            ErrorCode::DriverFailed,
            "GET https://secret.test/?q=hunter failed",
        );
        assert_eq!(class(&error), "DRIVER_FAILED");
        assert!(!class(&error).contains("secret"));
    }

    // ---- the production stack: the mark and the filter do not depend on RUST_LOG

    /// The production stack (`production_subscriber`) with `env` as `RUST_LOG`,
    /// run through `harden` as `telemetry::init_telemetry` does.
    fn production_log(env: &str, body: impl FnOnce()) -> String {
        let capture = Capture::default();
        let subscriber = production_subscriber(capture.clone(), harden(EnvFilter::new(env)));
        tracing::subscriber::with_default(subscriber, body);
        capture.text()
    }

    /// One request under the edge's own request span, with the production stack
    /// and `env` as `RUST_LOG`: a ZDR (or not) request that logs error, warn and
    /// info events with sensitive field names, and one safe error event.
    fn request_log(env: &str, zdr: bool) -> String {
        production_log(env, || {
            let request = HttpRequest::builder()
                .uri("/v1/scrape?url=SECRET-QUERY-STRING")
                .body(())
                .unwrap();
            let span = make_request_span(&request);
            let _guard = span.enter();
            if zdr {
                span.record(ZDR_SPAN_FIELD, true);
            }
            tracing::error!(error = "SECRET-ERROR-TEXT", "boom");
            tracing::warn!(url = "https://secret.test/page", "warned");
            tracing::info!(query = "SECRET-QUERY", "info with a query");
            tracing::error!(status = 500, "SAFE-ERROR-EVENT");
        })
    }

    /// The defect this pins: the level filter sat in the registry, so the
    /// info-level request span was never created at `RUST_LOG=error` or `warn`,
    /// nothing was marked, and every error event was written in full for a ZDR
    /// caller.
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

    /// A per-target directive that silences the module creating the request span
    /// (`quarry_edge=off`) must not hide the span from the mark either.
    #[test]
    fn a_per_target_directive_cannot_hide_the_request_span() {
        for env in [
            "error,quarry_edge=off",
            "quarry_edge::zdr_log=off,quarry_edge=trace",
            "off,quarry_runtime=trace",
        ] {
            let log = production_log(env, || {
                let request = HttpRequest::builder().uri("/v1/scrape").body(()).unwrap();
                let span = make_request_span(&request);
                span.record(ZDR_SPAN_FIELD, true);
                let _guard = span.enter();
                tracing::error!(target: "quarry_runtime::pipeline", error = "SECRET-ERROR-TEXT", "boom");
                tracing::error!(target: "quarry_runtime::pipeline", status = 500, "SAFE-ERROR-EVENT");
            });
            assert!(!log.contains("SECRET"), "RUST_LOG={env}: {log}");
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

        // `off` writes nothing at all, ZDR or not (and does not panic).
        assert!(request_log("off", true).is_empty());
        assert!(request_log("off", false).is_empty());
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
    /// that merely starts with it.
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

    // ---- the exporter (OTLP) layer gets the same rules ---------------------

    /// Records the message of every event the layer under test is shown, and
    /// the name of every span.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl<S> Layer<S> for Recorder
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
            self.0.lock().unwrap().push(format!("span:{}", attrs.metadata().name()));
        }

        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            struct Message(String);
            impl Visit for Message {
                fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    if field.name() == "message" {
                        self.0 = format!("{value:?}");
                    }
                }
            }
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    /// The exporter stack as `telemetry` builds it: the mark layer plus a
    /// layer filtered by `export_filter`.
    fn exported(env: &str, zdr: bool) -> Vec<String> {
        let recorder = Recorder::default();
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(recorder.clone().with_filter(export_filter(harden(EnvFilter::new(env)))));
        tracing::subscriber::with_default(subscriber, || {
            let request = HttpRequest::builder().uri("/v1/scrape").body(()).unwrap();
            let span = make_request_span(&request);
            if zdr {
                span.record(ZDR_SPAN_FIELD, true);
            }
            let _guard = span.enter();
            tracing::error!(error = "SECRET-ERROR-TEXT", "EXPORT-SENSITIVE");
            tracing::error!(status = 500, "EXPORT-SAFE");
            tracing::debug!(target: "reqwest::connect", "EXPORT-CAPPED");
        });
        let seen = recorder.0.lock().unwrap().clone();
        seen
    }

    #[test]
    fn the_exporter_layer_drops_sensitive_events_for_a_zdr_request_at_every_level() {
        for env in ["error", "warn", "info", "trace,reqwest::connect=trace"] {
            let seen = exported(env, true);
            assert!(!seen.iter().any(|m| m.contains("EXPORT-SENSITIVE")), "RUST_LOG={env}: {seen:?}");
            assert!(!seen.iter().any(|m| m.contains("EXPORT-CAPPED")), "RUST_LOG={env}: {seen:?}");
            assert!(seen.iter().any(|m| m.contains("EXPORT-SAFE")), "RUST_LOG={env}: {seen:?}");
        }
        let plain = exported("error", false);
        assert!(plain.iter().any(|m| m.contains("EXPORT-SENSITIVE")), "{plain:?}");
    }

    /// The OTLP stack exactly as `telemetry::init_telemetry` composes it
    /// (`otel_subscriber`), with a recording layer where the exporter goes.
    /// Returns what the exporter layer saw and what the JSON log wrote.
    fn otlp_stack(env: &str, zdr: bool) -> (Vec<String>, String) {
        let recorder = Recorder::default();
        let capture = Capture::default();
        let subscriber = otel_subscriber(
            capture.clone(),
            harden(EnvFilter::new(env)),
            harden(EnvFilter::new(env)),
            recorder.clone(),
        );
        tracing::subscriber::with_default(subscriber, || {
            let request = HttpRequest::builder().uri("/v1/scrape").body(()).unwrap();
            let span = make_request_span(&request);
            if zdr {
                span.record(ZDR_SPAN_FIELD, true);
            }
            let _guard = span.enter();
            tracing::error!(error = "SECRET-ERROR-TEXT", "EXPORT-SENSITIVE");
            tracing::error!(status = 500, "EXPORT-SAFE");
            tracing::debug!(target: "reqwest::connect", "EXPORT-CAPPED");
        });
        let seen = recorder.0.lock().unwrap().clone();
        (seen, capture.text())
    }

    /// The defect 9a2bc002 fixed, pinned on the OTLP wiring too: with the
    /// operator's level filter set to `error` (or anything), a ZDR request's
    /// sensitive events reach neither the exporter nor the log, its safe events
    /// do, and the third-party caps hold.
    #[test]
    fn the_otlp_stack_drops_sensitive_events_for_a_zdr_request_at_every_level() {
        for env in ["error", "warn", "info", "trace,reqwest::connect=trace"] {
            let (seen, log) = otlp_stack(env, true);
            assert!(!seen.iter().any(|m| m.contains("EXPORT-SENSITIVE")), "RUST_LOG={env}: {seen:?}");
            assert!(!log.contains("EXPORT-SENSITIVE") && !log.contains("SECRET-ERROR-TEXT"), "RUST_LOG={env}: {log}");
            assert!(!seen.iter().any(|m| m.contains("EXPORT-CAPPED")), "RUST_LOG={env}: {seen:?}");
            assert!(seen.iter().any(|m| m.contains("EXPORT-SAFE")), "RUST_LOG={env}: {seen:?}");
            assert!(log.contains("EXPORT-SAFE"), "RUST_LOG={env}: {log}");
        }
        let (plain, plain_log) = otlp_stack("error", false);
        assert!(plain.iter().any(|m| m.contains("EXPORT-SENSITIVE")), "{plain:?}");
        assert!(plain_log.contains("EXPORT-SENSITIVE"), "{plain_log}");
    }

    /// The rule the declares-mark exception states: the request span passes the
    /// level filter at every level, `RUST_LOG=off` included, so the mark always
    /// has a span to live on (and the exporter shows the request span), while
    /// every event still follows the level filter.
    #[test]
    fn the_otlp_stack_passes_the_request_span_even_at_rust_log_off() {
        for zdr in [true, false] {
            let (seen, log) = otlp_stack("off", zdr);
            assert!(seen.iter().any(|m| m == "span:request"), "zdr={zdr}: {seen:?}");
            assert!(!seen.iter().any(|m| m.starts_with("EXPORT-")), "zdr={zdr}: {seen:?}");
            assert!(!log.contains("EXPORT-"), "zdr={zdr}: {log}");
        }
    }

    // ---- spawned tasks -----------------------------------------------------

    /// Why lint rule R4 exists: a task spawned without the request span has no
    /// ZDR mark in scope, so its sensitive-named events are written; one spawned
    /// with `.in_current_span()` is filtered like the request itself. (The
    /// streamed-scrape canary in `tests/zdr_log_capture.rs` checks the real
    /// route; this pins the mechanism on the production stack.)
    #[tokio::test]
    async fn a_spawned_task_keeps_the_mark_only_when_it_carries_the_span() {
        use tracing::Instrument;

        let capture = Capture::default();
        let _default = tracing::subscriber::set_default(production_subscriber(
            capture.clone(),
            harden(EnvFilter::new("info")),
        ));
        let request = HttpRequest::builder().uri("/v1/scrape/stream").body(()).unwrap();
        let span = make_request_span(&request);
        span.record(ZDR_SPAN_FIELD, true);
        async {
            tokio::spawn(async { tracing::info!(url = "https://bare-spawn.test/", "bare") })
                .await
                .unwrap();
            tokio::spawn(
                async { tracing::info!(url = "https://carried-spawn.test/", "carried") }
                    .in_current_span(),
            )
            .await
            .unwrap();
        }
        .instrument(span)
        .await;
        let log = capture.text();
        assert!(
            log.contains("bare-spawn.test"),
            "the documented limit: a task that lost the span is not covered: {log}"
        );
        assert!(!log.contains("carried-spawn.test"), "{log}");
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

    /// The span a mark is recorded on may sit under a child span that the
    /// operator's directive disables (a debug span at `RUST_LOG=error`). The
    /// events inside it must still see the mark through the scope.
    #[test]
    fn a_disabled_child_span_does_not_hide_the_mark_from_its_events() {
        let log = production_log("error", || {
            let request = HttpRequest::builder().uri("/v1/scrape").body(()).unwrap();
            let span = make_request_span(&request);
            span.record(ZDR_SPAN_FIELD, true);
            let _guard = span.enter();
            let child = tracing::debug_span!("fetch");
            let _child = child.enter();
            tracing::error!(error = "SECRET-ERROR-TEXT", "boom in a disabled span");
            tracing::error!(status = 500, "SAFE-IN-CHILD");
        });
        assert!(!log.contains("SECRET"), "{log}");
        assert!(log.contains("SAFE-IN-CHILD"), "{log}");
    }
}
