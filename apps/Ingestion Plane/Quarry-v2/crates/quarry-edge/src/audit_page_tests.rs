use super::*;
use async_trait::async_trait;
use quarry_core::{output::DriverKind, QuarryResult};
use quarry_runtime::{driver::Driver, fetch::FetchResponse};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;

struct PageDriver {
    calls: Mutex<Vec<String>>,
    body: String,
    status: u16,
}
#[async_trait]
impl Driver for PageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }
    async fn fetch(&self, _: &Url) -> QuarryResult<FetchResponse> {
        panic!("unbounded transport forbidden")
    }
    async fn fetch_discovery_hop(
        &self,
        url: &Url,
        max_bytes: usize,
    ) -> QuarryResult<FetchResponse> {
        assert_eq!(max_bytes, 1048576);
        self.calls.lock().unwrap().push(url.to_string());
        let robots = url.path() == "/robots.txt";
        Ok(FetchResponse {
            status: if robots { 200 } else { self.status },
            final_url: url.clone(),
            headers: vec![(
                "content-type".into(),
                if robots {
                    "text/plain"
                } else {
                    "text/html; charset=utf-8"
                }
                .into(),
            )],
            body: if robots {
                b"User-agent: *\nDisallow: /private\n".to_vec()
            } else {
                self.body.as_bytes().to_vec()
            },
            duration_ms: 1,
        })
    }
}

async fn capture_response(driver: Arc<PageDriver>, url: &str) -> Response {
    capture(
        State(crate::test_support::test_state(driver)),
        Extension(crate::test_support::claims_for_org("alpha")),
        Json(AuditPageRequest { url: url.into() }),
    )
    .await
}

#[tokio::test]
async fn capture_exposes_only_bounded_document_evidence_not_html_or_artifacts() {
    let driver=Arc::new(PageDriver{calls:Default::default(),status:200,body:"<title>Actual title</title><meta property='og:title' content='Not actual title'><meta name='description' content='Description'><link rel='canonical' href='/canonical'><h1>Heading</h1><a href='/public'>Public</a><a href='/account'>Private</a><script>secretRawHtmlMarker</script>".into()});
    let response = capture_response(driver.clone(), "https://example.com/").await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = crate::test_support::response_json(response).await;
    assert_eq!(body["version"], "bounded_page_v1");
    assert_eq!(body["metadata"]["title"], "Actual title");
    assert_eq!(body["metadata"]["description"], "Description");
    assert_eq!(body["canonical"], "https://example.com/canonical");
    assert_eq!(body["links"].as_array().unwrap().len(), 1);
    assert_eq!(body["headings"][0]["level"], 1);
    assert!(body["run_id"].as_str().unwrap().starts_with("run_"));
    assert!(body["fingerprint"].as_str().unwrap().starts_with("blake3:"));
    let serialized = body.to_string();
    for forbidden in ["secretRawHtmlMarker", "artifact_id", "markdown", "<title>"] {
        assert!(!serialized.contains(forbidden));
    }
    assert_eq!(driver.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn capture_rejects_private_sensitive_and_non_https_urls_before_transport() {
    for url in [
        "http://example.com/",
        "https://127.0.0.1/",
        "https://example.com/admin",
        "https://example.com/?token=secret",
    ] {
        let driver = Arc::new(PageDriver {
            calls: Default::default(),
            status: 200,
            body: String::new(),
        });
        assert!(!capture_response(driver.clone(), url)
            .await
            .status()
            .is_success());
        assert!(driver.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn capture_respects_initial_robots_and_rejects_oversized_document() {
    let driver = Arc::new(PageDriver {
        calls: Default::default(),
        status: 200,
        body: String::new(),
    });
    assert_eq!(
        capture_response(driver.clone(), "https://example.com/private")
            .await
            .status(),
        axum::http::StatusCode::FORBIDDEN
    );
    assert_eq!(driver.calls.lock().unwrap().len(), 1);
    let driver = Arc::new(PageDriver {
        calls: Default::default(),
        status: 200,
        body: "x".repeat(1048577),
    });
    assert_eq!(
        capture_response(driver, "https://example.com/")
            .await
            .status(),
        axum::http::StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn capture_reports_upstream_404_without_turning_it_into_transport_failure() {
    let driver = Arc::new(PageDriver {
        calls: Default::default(),
        status: 404,
        body: "<title>Not found</title>".into(),
    });
    let response = capture_response(driver, "https://example.com/missing").await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        crate::test_support::response_json(response).await["status"],
        404
    );
}

#[test]
fn capture_request_has_no_unbounded_or_browser_knobs() {
    assert!(serde_json::from_value::<AuditPageRequest>(
        serde_json::json!({"url":"https://example.com/","browser":true})
    )
    .is_err());
}

struct HangingDriver;
#[async_trait]
impl Driver for HangingDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }
    async fn fetch(&self, _: &Url) -> QuarryResult<FetchResponse> {
        panic!("unbounded")
    }
    async fn fetch_discovery_hop(&self, _: &Url, _: usize) -> QuarryResult<FetchResponse> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn capture_deadline_cancels_stalled_transport_without_fallback() {
    let response = capture_with_budget(
        crate::test_support::test_state(Arc::new(HangingDriver)),
        AuditPageRequest {
            url: "https://example.com/".into(),
        },
        Duration::from_millis(10),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test]
async fn saturated_capture_capacity_rejects_without_queue_or_transport() {
    let driver = Arc::new(PageDriver {
        calls: Default::default(),
        status: 200,
        body: String::new(),
    });
    let response = capture_with_capacity(
        crate::test_support::test_state(driver.clone()),
        AuditPageRequest {
            url: "https://example.com/".into(),
        },
        &tokio::sync::Semaphore::new(0),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert!(driver.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn capture_rejects_overlong_metadata_instead_of_inventing_missing_fields() {
    let driver = Arc::new(PageDriver {
        calls: Default::default(),
        status: 200,
        body: format!("<title>{}</title>", "x".repeat(513)),
    });
    assert_eq!(
        capture_response(driver, "https://example.com/")
            .await
            .status(),
        axum::http::StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn capture_caps_link_evidence_and_labels_partial_results() {
    let driver = Arc::new(PageDriver {
        calls: Default::default(),
        status: 200,
        body: (0..150)
            .map(|index| format!("<a href='/page-{index}'>Page</a>"))
            .collect(),
    });
    let response = capture_response(driver, "https://example.com/").await;
    let body = crate::test_support::response_json(response).await;
    assert_eq!(body["links"].as_array().unwrap().len(), 100);
    assert_eq!(body["partial"], true);
    assert_eq!(
        body["limits"],
        serde_json::json!({"body_bytes":1048576,"timeout_ms":10000,"max_links":100})
    );
}
