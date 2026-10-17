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
//! - It is a filter on the *log writer*. It does not stop code from computing or
//!   returning the data, and it does not protect the OTLP span attributes of
//!   spans that declare a sensitive field (the request span declares none).
//!
//! `QUARRY_ZDR_LOG_FILTER=off` turns the filter off (an emergency lever for a
//! debugging session on a non-production edge; read once at start).

use axum::extract::Request;
use axum::http::Request as HttpRequest;
use axum::middleware::Next;
use axum::response::Response;
use quarry_core::error::QuarryError;
use quarry_core::zdr::ZDR_SPAN_FIELD;
use tracing::span::{Attributes, Id, Record};
use tracing::{field::Visit, Metadata, Span, Subscriber};
use tracing_subscriber::filter::DynFilterFn;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Filter};
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
/// format or export events; it adds no output of its own.
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
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(ZdrMarked);
            }
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = ZdrVisitor(false);
        values.record(&mut visitor);
        if !visitor.0 {
            // A `false` (or any other value) never lowers the mark.
            return;
        }
        if let Some(span) = ctx.span(id) {
            // Look first, then insert: the guard of the first borrow is gone
            // before the second is taken, and a second mark is not an error.
            let already = span.extensions().get::<ZdrMarked>().is_some();
            if !already {
                span.extensions_mut().insert(ZdrMarked);
            }
        }
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

/// Whether an event with these fields may be written under a ZDR span.
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

/// `filter` with [`CAPPED_TARGETS`] applied on top. Directives added last win,
/// so an operator's `RUST_LOG=debug` still turns the edge's own crates up but
/// cannot turn these on.
pub fn harden(mut filter: EnvFilter) -> EnvFilter {
    for (target, level) in CAPPED_TARGETS {
        if let Ok(directive) = format!("{target}={level}").parse() {
            filter = filter.add_directive(directive);
        }
    }
    filter
}

fn filter_disabled() -> bool {
    std::env::var("QUARRY_ZDR_LOG_FILTER")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "off" | "0" | "false"
            )
        })
        .unwrap_or(false)
}

/// The per-layer filter: drops events with a sensitive field while a ZDR span
/// is in scope, keeps everything else. Spans always pass.
pub fn event_filter<S>() -> impl Filter<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let disabled = filter_disabled();
    DynFilterFn::new(move |meta: &Metadata<'_>, ctx: &Context<'_, S>| {
        if disabled || !meta.is_event() || !carries_sensitive_field(meta) {
            return true;
        }
        !in_zdr_scope(ctx)
    })
}

/// The JSON log layer the edge writes to `writer`, with the ZDR filter applied.
/// Used by `telemetry` and by the tests, so they exercise the same layer.
pub fn build_json_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(event_filter())
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
}
