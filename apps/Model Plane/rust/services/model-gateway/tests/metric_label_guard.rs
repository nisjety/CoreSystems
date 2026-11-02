//! Metric-label guard for model-gateway (WS06-S3).
//!
//! Walks this crate's `src` tree and asserts that no `counter!`, `gauge!` or `histogram!`
//! carries a label key that is not on `mp_contracts::telemetry_labels::METRIC_LABELS`. A label
//! whose value is request data (a thread id, a run id, a space id, a raw path) would put that
//! identifier into the metrics store of every scraper.

use std::fs;
use std::path::{Path, PathBuf};

use mp_contracts::telemetry_labels::disallowed_metric_labels;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn src_files() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    files
}

#[test]
fn no_metric_uses_a_label_key_outside_the_closed_list() {
    let files = src_files();
    assert!(!files.is_empty(), "the src walk found no files");
    let mut offences = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("read source file");
        for found in disallowed_metric_labels(&source) {
            offences.push(format!(
                "{}: metric `{}` uses label `{}`",
                file.display(),
                found.metric,
                found.label
            ));
        }
    }
    assert!(
        offences.is_empty(),
        "metric labels outside METRIC_LABELS (add the key to the reviewed list or drop the label):\n{}",
        offences.join("\n")
    );
}

#[test]
fn a_synthetic_thread_id_label_fails_the_scan() {
    let synthetic = r#"counter!("synthetic_total", "thread_id" => id.to_string()).increment(1);"#;
    let found = disallowed_metric_labels(synthetic);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "thread_id");
    assert_eq!(found[0].metric, "synthetic_total");
}

// ---------------------------------------------------------------------------------------------
// The request metrics label the route TEMPLATE, never the raw URI path.
// ---------------------------------------------------------------------------------------------

mod route_label {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use metrics_exporter_prometheus::PrometheusBuilder;
    use model_gateway::gateway_metrics::{metrics_middleware, UNMATCHED_ROUTE};
    use tower::ServiceExt;

    async fn ok() -> &'static str {
        "ok"
    }

    /// The same layering as `http_routes::router`: routes first, then the metrics middleware as
    /// a `Router::layer`, so axum has set `MatchedPath` before the middleware runs.
    fn app() -> Router {
        Router::new()
            .route("/v1/threads/:thread_id", get(ok))
            .route("/v1/runs/:run_id/events", get(ok))
            .layer(middleware::from_fn(metrics_middleware))
    }

    async fn get_status(app: &Router, uri: &str) -> StatusCode {
        app.clone()
            .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
            .await
            .expect("response")
            .status()
    }

    #[tokio::test]
    async fn route_label_is_the_template_not_the_path() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        // Thread-local recorder: the default test runtime is current-thread, so the middleware
        // polls on this thread and records here without touching the global recorder.
        let _guard = metrics::set_default_local_recorder(&recorder);

        let app = app();
        let thread_id = "thr_0123456789abcdef";
        let run_id = "run_fedcba9876543210";
        assert_eq!(
            get_status(&app, &format!("/v1/threads/{thread_id}")).await,
            StatusCode::OK
        );
        assert_eq!(
            get_status(&app, &format!("/v1/runs/{run_id}/events")).await,
            StatusCode::OK
        );
        assert_eq!(
            get_status(&app, "/v1/unknown/secret-space-id").await,
            StatusCode::NOT_FOUND
        );

        let rendered = handle.render();
        assert!(
            rendered.contains(r#"route="/v1/threads/:thread_id""#),
            "template label missing:\n{rendered}"
        );
        assert!(
            rendered.contains(r#"route="/v1/runs/:run_id/events""#),
            "template label missing:\n{rendered}"
        );
        assert!(
            rendered.contains(&format!(r#"route="{UNMATCHED_ROUTE}""#)),
            "unmatched label missing:\n{rendered}"
        );
        // No raw id and no `path` label anywhere in the exposition.
        assert!(!rendered.contains(thread_id), "raw thread id leaked:\n{rendered}");
        assert!(!rendered.contains(run_id), "raw run id leaked:\n{rendered}");
        assert!(
            !rendered.contains("secret-space-id"),
            "raw unknown path leaked:\n{rendered}"
        );
        assert!(!rendered.contains("path="), "a path label exists:\n{rendered}");
        assert!(rendered.contains("gateway_requests_total"));
        assert!(rendered.contains("gateway_request_duration_seconds"));
    }
}
