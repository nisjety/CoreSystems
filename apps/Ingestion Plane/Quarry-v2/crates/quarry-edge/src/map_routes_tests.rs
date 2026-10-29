use super::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use quarry_core::error::ErrorCode;

struct MapDriver {
    responses: HashMap<String, String>,
    calls: Mutex<Vec<String>>,
    error: Option<ErrorCode>,
}

#[async_trait]
impl quarry_runtime::driver::Driver for MapDriver {
    fn kind(&self) -> quarry_core::output::DriverKind {
        quarry_core::output::DriverKind::Static
    }
    async fn fetch(
        &self,
        _: &Url,
    ) -> quarry_core::QuarryResult<quarry_runtime::fetch::FetchResponse> {
        panic!("map must never use an unbounded driver fetch")
    }
    async fn fetch_discovery_hop(
        &self,
        url: &Url,
        max_bytes: usize,
    ) -> quarry_core::QuarryResult<quarry_runtime::fetch::FetchResponse> {
        assert_eq!(max_bytes, 1024 * 1024);
        self.calls.lock().unwrap().push(url.to_string());
        if let Some(error) = self.error {
            return Err(quarry_core::error::QuarryError::new(error, "test failure"));
        }
        let body = self
            .responses
            .get(url.as_str())
            .cloned()
            .unwrap_or_default();
        Ok(quarry_runtime::fetch::FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![],
            body: body.into_bytes(),
            duration_ms: 1,
        })
    }
}

async fn map_response(driver: Arc<MapDriver>, url: &str) -> axum::response::Response {
    map(
        State(crate::test_support::test_state(driver)),
        Extension(crate::test_support::claims_for_org("org_alpha")),
        Json(MapRequest {
            url: url.into(),
            search: None,
            limit: Some(1000),
            include_subdomains: false,
        }),
    )
    .await
    .into_response()
}

#[tokio::test]
async fn map_rejects_non_https_sensitive_and_credential_urls_before_fetch() {
    for url in [
        "http://example.com/",
        "https://a:b@example.com/",
        "https://example.com/?token=x",
        "https://example.com/#secret",
        "https://example.com/admin",
        "https://example.com/%61dmin",
        "https://example.com/login",
    ] {
        let driver = Arc::new(MapDriver {
            responses: HashMap::new(),
            calls: Mutex::new(vec![]),
            error: None,
        });
        assert_eq!(
            map_response(driver.clone(), url).await.status(),
            StatusCode::BAD_REQUEST,
            "{url}"
        );
        assert!(driver.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn map_traverses_only_safe_exact_origin_nested_sitemaps() {
    let nested = [
        "https://evil.test/a.xml",
        "https://example.com:444/a.xml",
        "http://example.com/a.xml",
        "https://example.com/admin",
        "https://example.com/a.xml?token=x",
        "https://user@example.com/a.xml",
        "https://example.com/a.xml#x",
        "https://example.com/s1.xml",
        "https://example.com/s2.xml",
        "https://example.com/s3.xml",
        "https://example.com/s4.xml",
        "https://example.com/s5.xml",
        "https://example.com/s6.xml",
    ];
    let index = format!(
        "<sitemapindex>{}</sitemapindex>",
        nested
            .iter()
            .map(|u| format!("<sitemap><loc>{u}</loc></sitemap>"))
            .collect::<String>()
    );
    let driver = Arc::new(MapDriver {
        responses: HashMap::from([
            ("https://example.com/sitemap.xml".into(), index),
            (
                "https://example.com/".into(),
                "<a href='/public'>Public</a>".into(),
            ),
        ]),
        calls: Mutex::new(vec![]),
        error: None,
    });
    assert_eq!(
        map_response(driver.clone(), "https://example.com/")
            .await
            .status(),
        StatusCode::OK
    );
    let calls = driver.calls.lock().unwrap();
    assert_eq!(calls.len(), 9);
    assert!(calls.iter().all(|u| u.starts_with("https://example.com/")));
    assert!(calls.contains(&"https://example.com/s5.xml".into()));
    assert!(calls.contains(&"https://example.com/s6.xml".into()));
}

#[test]
fn map_candidates_are_capped_and_unsafe_links_are_dropped() {
    let mut input = vec![
        "https://example.com/admin".into(),
        "http://example.com/http".into(),
        "https://u@example.com/secret".into(),
        "https://example.com/a?token=x".into(),
        "https://example.com/a#x".into(),
    ];
    input.extend((0..5000).map(|i| format!("https://example.com/{i}")));
    let output = merge_and_scope("example.com", false, input);
    assert!(output.len() <= 2000);
    assert!(output.iter().all(|u| !u.contains("admin")
        && !u.contains('?')
        && !u.contains('#')
        && !u.contains('@')
        && u.starts_with("https://")));
}

#[tokio::test]
async fn map_timeout_is_a_typed_failure_not_an_empty_success() {
    let driver = Arc::new(MapDriver {
        responses: HashMap::new(),
        calls: Mutex::new(vec![]),
        error: Some(ErrorCode::Timeout),
    });
    let response = map_response(driver, "https://example.com/").await;
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        crate::test_support::response_json(response).await["code"],
        "TIMEOUT"
    );
}

#[tokio::test]
async fn map_rejects_oversized_body_even_if_another_bounded_driver_misbehaves() {
    let driver = Arc::new(MapDriver {
        responses: HashMap::from([(
            "https://example.com/robots.txt".into(),
            "x".repeat(MAX_BODY_BYTES + 1),
        )]),
        calls: Mutex::new(vec![]),
        error: None,
    });
    let response = map_response(driver.clone(), "https://example.com/").await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        crate::test_support::response_json(response).await["code"],
        "UPSTREAM_LIMIT"
    );
    assert_eq!(driver.calls.lock().unwrap().len(), 1);
}

#[test]
fn same_host_in_scope() {
    assert!(host_in_scope("a.com", "a.com", false));
}

#[test]
fn external_host_rejected() {
    assert!(!host_in_scope("a.com", "b.com", false));
    assert!(!host_in_scope("a.com", "evil-a.com", true));
}

#[test]
fn subdomain_only_with_flag() {
    assert!(!host_in_scope("a.com", "docs.a.com", false));
    assert!(host_in_scope("a.com", "docs.a.com", true));
}

#[test]
fn merge_dedups_and_scopes() {
    let got = merge_and_scope(
        "a.com",
        false,
        vec![
            "https://a.com/x".into(),
            "https://a.com/x".into(),      // dup
            "https://b.com/y".into(),      // external
            "https://docs.a.com/z".into(), // subdomain, flag off
            "ftp://a.com/bad".into(),      // non-http
        ],
    );
    assert_eq!(got, vec!["https://a.com/x".to_string()]);
}

#[test]
fn merge_includes_subdomains_when_flagged() {
    let got = merge_and_scope(
        "a.com",
        true,
        vec!["https://a.com/x".into(), "https://docs.a.com/z".into()],
    );
    assert_eq!(got.len(), 2);
}

#[test]
fn limit_clamps() {
    assert_eq!(clamp_limit(None), DEFAULT_LIMIT);
    assert_eq!(clamp_limit(Some(0)), 1);
    assert_eq!(clamp_limit(Some(99999)), MAX_LIMIT);
    assert_eq!(clamp_limit(Some(25)), 25);
}

#[tokio::test]
async fn bad_url_returns_structured_envelope_without_fetching() {
    let driver = crate::test_support::StubDriver::ok();
    let state = crate::test_support::test_state(driver.clone());
    let response = map(
        State(state),
        Extension(crate::test_support::claims_for_org("org_alpha")),
        Json(MapRequest {
            url: "not a url".into(),
            search: None,
            limit: None,
            include_subdomains: false,
        }),
    )
    .await
    .into_response();

    assert_eq!(driver.calls.load(Ordering::Relaxed), 0);
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = crate::test_support::response_json(response).await;
    // Structured envelope: exactly {error, code} — no rate-limit fields.
    assert_eq!(crate::test_support::json_keys(&body), vec!["code", "error"]);
    assert_eq!(body["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn rate_limited_driver_maps_to_api_error_envelope() {
    let state = crate::test_support::test_state(Arc::new(MapDriver {
        responses: HashMap::new(),
        calls: Mutex::new(vec![]),
        error: Some(ErrorCode::RateLimited),
    }));
    let response = map(
        State(state),
        Extension(crate::test_support::claims_for_org("org_alpha")),
        Json(MapRequest {
            url: "https://example.com".into(),
            search: None,
            limit: None,
            include_subdomains: false,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = crate::test_support::response_json(response).await;
    assert_eq!(body["code"], "RATE_LIMITED");
    assert_eq!(body["retry_after_seconds"], 60);
    assert_eq!(
        crate::test_support::json_keys(&body),
        vec![
            "code",
            "error",
            "hint",
            "next_actions",
            "retry_after_seconds",
            "window"
        ]
    );
}
