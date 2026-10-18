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
//! - It is a filter on the log writer. It does not stop code from computing or
//!   returning the data.
//!
//! `GATEWAY_ZDR_LOG_FILTER=off` turns the filter off (an emergency lever for a
//! debugging session on a non-production gateway; read once at start).
//!
//! This file depends only on `tracing`, `tracing-subscriber` and the standard
//! library, so it can be read without the rest of the crate.

use tracing::span::{Attributes, Id, Record};
use tracing::{field::Visit, Metadata, Span, Subscriber};
use tracing_subscriber::filter::DynFilterFn;
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
/// format or export events; it adds no output of its own.
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

/// Whether an event with these fields may be written under a ZDR span.
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

/// `filter` with [`CAPPED_TARGETS`] applied on top. Directives added last win,
/// so an operator's `RUST_LOG=debug` still turns the gateway's own crate up but
/// cannot turn these on.
pub(crate) fn harden(mut filter: EnvFilter) -> EnvFilter {
    for (target, level) in CAPPED_TARGETS {
        if let Ok(directive) = format!("{target}={level}").parse() {
            filter = filter.add_directive(directive);
        }
    }
    filter
}

fn filter_disabled() -> bool {
    std::env::var("GATEWAY_ZDR_LOG_FILTER")
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
pub(crate) fn event_filter<S>() -> impl Filter<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    event_filter_with(filter_disabled())
}

fn event_filter_with<S>(disabled: bool) -> impl Filter<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    DynFilterFn::new(move |meta: &Metadata<'_>, ctx: &Context<'_, S>| {
        if disabled || !meta.is_event() || !carries_sensitive_field(meta) {
            return true;
        }
        !in_zdr_scope(ctx)
    })
}

/// The JSON log layer the gateway writes to `writer`, with the ZDR filter
/// applied. Used by [`init`] and by the tests, so they exercise the same layer.
pub(crate) fn build_json_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(event_filter())
}

/// Install the process-wide subscriber: `RUST_LOG` (default `error`, as before)
/// with the capped dependencies, the sticky ZDR mark and the filtered JSON log
/// on stdout.
pub(crate) fn init() {
    tracing_subscriber::registry()
        .with(ZdrMarkLayer)
        .with(harden(EnvFilter::from_default_env()))
        .with(build_json_layer(std::io::stdout))
        .init();
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
    fn the_lever_turns_the_filter_off() {
        let capture = Capture::default();
        let layer = tracing_subscriber::fmt::layer()
            .json()
            .with_writer(capture.clone())
            .with_filter(event_filter_with(true));
        let subscriber = tracing_subscriber::registry()
            .with(ZdrMarkLayer)
            .with(layer);
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("request", zdr = tracing::field::Empty);
            span.record("zdr", true);
            let _guard = span.enter();
            tracing::info!(url = "https://lever.test/", "kept with the filter off");
        });
        assert!(capture.text().contains("lever.test"), "{}", capture.text());
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

    #[test]
    fn harden_caps_the_crates_that_print_content_even_at_debug() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(harden(EnvFilter::new("debug")))
            .with(build_json_layer(capture.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "html5ever::tree_builder", "processing Characters page text");
            tracing::debug!(target: "reqwest::connect", "starting new connection");
            tracing::debug!(target: "hyper_util::client::legacy::connect::http", "connecting to a host");
            tracing::warn!(target: "html5ever::tokenizer", "html5ever still warns");
            tracing::debug!(target: "verevon_gateway_rs::upstream", "our own debug stays on");
        });
        let log = capture.text();
        assert!(!log.contains("page text"), "{log}");
        assert!(!log.contains("starting new connection"), "{log}");
        assert!(!log.contains("connecting to a host"), "{log}");
        assert!(log.contains("html5ever still warns"), "{log}");
        assert!(log.contains("our own debug stays on"), "{log}");
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
        assert!(!log.is_empty(), "the capture saw the request's own events");
        for needle in [
            "SEARCH-QUERY-IN-URL",
            "SEARCH-QUERY-IN-BODY",
            "SEARCH-URL-IN-BODY",
            "secret-site",
        ] {
            assert!(!log.contains(needle), "{needle} reached the log: {log}");
        }
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
