use super::*;
use async_trait::async_trait;
use quarry_core::{output::DriverKind, QuarryResult};
use quarry_runtime::{driver::Driver, fetch::FetchResponse};
use std::sync::Arc;

struct RedirectDriver {
    calls: std::sync::Mutex<Vec<String>>,
    location: String,
}

#[async_trait]
impl Driver for RedirectDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }
    async fn fetch(&self, _: &Url) -> QuarryResult<FetchResponse> {
        panic!("unbounded")
    }
    async fn fetch_discovery_hop(&self, url: &Url, _: usize) -> QuarryResult<FetchResponse> {
        self.calls.lock().unwrap().push(url.to_string());
        let redirect = url.host_str() == Some("example.com");
        Ok(FetchResponse {
            status: if redirect { 308 } else { 200 },
            final_url: url.clone(),
            headers: if redirect {
                vec![(
                    "location".into(),
                    self.location.replace("{path}", url.path()),
                )]
            } else {
                vec![]
            },
            body: if redirect {
                vec![]
            } else if url.path() == "/robots.txt" {
                b"User-agent: *\nDisallow: /private\n".to_vec()
            } else {
                b"<a href='/public'>Public</a><a href='/private'>Private</a>".to_vec()
            },
            duration_ms: 1,
        })
    }
}

#[tokio::test]
async fn canonical_www_redirect_discovers_links_and_applies_effective_origin_robots() {
    let driver = Arc::new(RedirectDriver {
        calls: Default::default(),
        location: "https://www.example.com{path}".into(),
    });
    let response = map(
        State(crate::test_support::test_state(driver.clone())),
        Extension(crate::test_support::claims_for_org("alpha")),
        Json(request()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = crate::test_support::response_json(response).await;
    assert_eq!(body["url"], "https://example.com/");
    assert_eq!(body["links"][0]["url"], "https://www.example.com/public");
    assert_eq!(body["count"], 1);
}

#[tokio::test]
async fn redirected_discovery_rejects_scope_and_sensitive_targets_before_fetch() {
    for location in [
        "https://evil.test/",
        "https://docs.example.com/",
        "https://www.example.com:444/",
        "http://www.example.com/",
        "https://www.example.com/login",
        "https://www.example.com/?token=secret",
        "https://user:secret@www.example.com/",
        "https://127.0.0.1/",
    ] {
        let driver = Arc::new(RedirectDriver {
            calls: Default::default(),
            location: location.into(),
        });
        let response = map(
            State(crate::test_support::test_state(driver.clone())),
            Extension(crate::test_support::claims_for_org("alpha")),
            Json(request()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{location}");
        assert_eq!(driver.calls.lock().unwrap().len(), 1, "{location}");
    }
}

#[tokio::test]
async fn canonical_redirect_still_obeys_configured_target_blocklist() {
    let driver = Arc::new(RedirectDriver {
        calls: Default::default(),
        location: "https://www.example.com{path}".into(),
    });
    let mut state = crate::test_support::test_state(driver.clone());
    let security = quarry_security::preflight::DefaultEngine::new();
    security.block_host("www.example.com").await;
    state.security = Arc::new(security);
    let response = map(
        State(state),
        Extension(crate::test_support::claims_for_org("alpha")),
        Json(request()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        *driver.calls.lock().unwrap(),
        vec!["https://example.com/robots.txt"]
    );
}

#[tokio::test]
async fn redirected_discovery_detects_cycles_before_another_fetch() {
    let driver = Arc::new(RedirectDriver {
        calls: Default::default(),
        location: "https://example.com{path}".into(),
    });
    let response = map(
        State(crate::test_support::test_state(driver.clone())),
        Extension(crate::test_support::claims_for_org("alpha")),
        Json(request()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(driver.calls.lock().unwrap().len(), 1);
}

#[test]
fn discovery_redirect_scope_allows_only_origin_or_exact_www_pair() {
    let seed = Url::parse("https://example.com/public").unwrap();
    for target in ["https://example.com/other", "https://www.example.com/other"] {
        assert!(redirect_in_scope(&seed, &Url::parse(target).unwrap()));
    }
    for target in [
        "https://www.www.example.com/",
        "https://example.com.evil.test/",
        "https://docs.example.com/",
        "https://www.example.com:8443/",
        "https://www.example.com/api/data",
        "https://www.example.com/%61dmin/",
    ] {
        assert!(
            !redirect_in_scope(&seed, &Url::parse(target).unwrap()),
            "{target}"
        );
    }
    assert!(!redirect_in_scope(
        &Url::parse("https://www.example.com/").unwrap(),
        &Url::parse("https://www.www.example.com/").unwrap()
    ));
}

struct HopDriver {
    calls: std::sync::Mutex<Vec<String>>,
    disallow: bool,
}
#[async_trait]
impl Driver for HopDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }
    async fn fetch(&self, _: &Url) -> QuarryResult<FetchResponse> {
        panic!("unbounded")
    }
    async fn fetch_discovery_hop(&self, url: &Url, _: usize) -> QuarryResult<FetchResponse> {
        self.calls.lock().unwrap().push(url.to_string());
        let robots = url.path() == "/robots.txt";
        let n = url
            .path()
            .trim_start_matches('/')
            .parse::<usize>()
            .unwrap_or(0);
        Ok(FetchResponse {
            status: if robots { 200 } else { 308 },
            final_url: url.clone(),
            headers: if robots {
                vec![]
            } else {
                vec![(
                    "location".into(),
                    format!("https://www.example.com/{}", n + 1),
                )]
            },
            body: if robots {
                if self.disallow {
                    b"User-agent: *\nDisallow: /1".to_vec()
                } else {
                    vec![]
                }
            } else {
                vec![]
            },
            duration_ms: 1,
        })
    }
}

#[tokio::test]
async fn discovery_hops_are_bounded_and_recheck_robots_before_each_target() {
    let driver = Arc::new(HopDriver {
        calls: Default::default(),
        disallow: false,
    });
    let state = crate::test_support::test_state(driver.clone());
    let error = fetch_document(&state, &Url::parse("https://example.com/0").unwrap())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, quarry_core::error::ErrorCode::SecurityBlocked);
    let calls = driver.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|url| !url.ends_with("robots.txt"))
            .count(),
        4
    );
    assert_eq!(
        calls
            .iter()
            .filter(|url| url.ends_with("robots.txt"))
            .count(),
        3
    );
    drop(calls);
    let driver = Arc::new(HopDriver {
        calls: Default::default(),
        disallow: true,
    });
    let state = crate::test_support::test_state(driver.clone());
    let error = fetch_document(&state, &Url::parse("https://example.com/0").unwrap())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, quarry_core::error::ErrorCode::SecurityBlocked);
    assert_eq!(
        *driver.calls.lock().unwrap(),
        vec![
            "https://example.com/0",
            "https://www.example.com/robots.txt"
        ]
    );
}

struct HangingDriver;

#[async_trait]
impl Driver for HangingDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }
    async fn fetch(&self, _: &Url) -> QuarryResult<FetchResponse> {
        panic!("unbounded fallback must never run")
    }
    async fn fetch_discovery_hop(&self, _: &Url, _: usize) -> QuarryResult<FetchResponse> {
        std::future::pending().await
    }
}

fn request() -> MapRequest {
    MapRequest {
        url: "https://example.com/".into(),
        search: None,
        limit: None,
        include_subdomains: false,
    }
}

#[tokio::test]
async fn global_deadline_cancels_a_stalled_discovery_fetch() {
    let response = map_with_budget(
        crate::test_support::test_state(Arc::new(HangingDriver)),
        crate::test_support::claims_for_org("alpha"),
        request(),
        Duration::from_millis(10),
    )
    .await;
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        crate::test_support::response_json(response).await["code"],
        "TIMEOUT"
    );
}

#[tokio::test]
async fn unsupported_bounded_driver_never_falls_back_to_fetch() {
    let driver = crate::test_support::StubDriver::ok();
    let response = map(
        State(crate::test_support::test_state(driver.clone())),
        Extension(crate::test_support::claims_for_org("alpha")),
        Json(request()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(driver.calls.load(std::sync::atomic::Ordering::Relaxed), 0);
}

#[test]
fn nested_sitemaps_cannot_expand_scope_or_read_sensitive_routes() {
    let base = Url::parse("https://example.com/").unwrap();
    for raw in [
        "https://example.com:444/a.xml",
        "https://docs.example.com/a.xml",
        "https://evil.test/a.xml",
        "https://example.com/api/map.xml",
        "https://example.com/account/map.xml",
        "https://example.com/%61dmin/map.xml",
        "https://example.com/a.xml?token=x",
        "https://example.com/a.xml#token",
        "https://user:secret@example.com/a.xml",
    ] {
        assert!(!nested_in_scope(&base, &Url::parse(raw).unwrap()), "{raw}");
    }
    assert!(nested_in_scope(
        &base,
        &Url::parse("https://example.com/public-map.xml").unwrap()
    ));
}

#[test]
fn discovery_keeps_the_existing_request_and_response_wire_shape() {
    let request: MapRequest = serde_json::from_value(
        serde_json::json!({"url":"https://example.com/", "limit":25, "include_subdomains":false}),
    )
    .unwrap();
    assert_eq!(request.limit, Some(25));
    let value = serde_json::to_value(MapResponse {
        url: request.url,
        links: vec![],
        count: 0,
        ranked: false,
        coverage: MapCoverage::default(),
    })
    .unwrap();
    assert_eq!(
        crate::test_support::json_keys(&value),
        vec!["count", "coverage", "links", "ranked", "url"]
    );
}

#[tokio::test]
async fn configured_security_blocklist_cannot_be_bypassed_by_discovery() {
    let driver = crate::test_support::StubDriver::ok();
    let mut state = crate::test_support::test_state(driver.clone());
    let security = quarry_security::preflight::DefaultEngine::new();
    security.block_host("example.com").await;
    state.security = Arc::new(security);
    let response = map(
        State(state),
        Extension(crate::test_support::claims_for_org("alpha")),
        Json(request()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(driver.calls.load(std::sync::atomic::Ordering::Relaxed), 0);
}
