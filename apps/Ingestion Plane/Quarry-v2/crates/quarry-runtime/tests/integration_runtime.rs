use quarry_core::error::ErrorCode;
use quarry_core::privacy::PrivacyPolicy;
use quarry_runtime::driver::FetchHints;
use quarry_runtime::proxy_pool::ProxyPool;
use quarry_runtime::{Driver, StaticDriver};
use std::time::Duration;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// StaticDriver's direct-egress client only ever connects to an address that
// has passed the security preflight (see `quarry_runtime::dns_guard`), and
// `wiremock::MockServer` always binds to loopback -- so a direct fetch here
// would be correctly rejected as SSRF-blocked before any HTTP happens. Route
// through the proxy-egress identity instead: it owns its own connection (the
// pinned-DNS guard is a Direct-only contract) and exercises the same
// request-send / response-parse path (`StaticDriver::send_once`) that a
// direct fetch would.
/// The comment above explains that every other test here routes through proxy
/// egress BECAUSE a direct fetch to a loopback mock is refused. That premise
/// was load-bearing and untested — if the preflight ever regressed, the
/// workaround would keep passing and quietly stop proving anything.
///
/// So assert the refusal itself. The second assertion is the one that matters:
/// the mock must have received NOTHING, which is what distinguishes "blocked
/// before any connection" from "connected, then errored".
#[tokio::test]
async fn a_direct_fetch_to_a_loopback_target_is_refused_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>hi</html>"))
        .mount(&server)
        .await;

    let driver = StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap();
    let url = Url::parse(&server.uri()).unwrap();

    let error = driver
        .fetch(&url)
        .await
        .expect_err("a loopback target must not be fetchable over direct egress");

    assert_eq!(error.code, ErrorCode::SecurityBlocked);
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "the guard must refuse before connecting, not after"
    );
}

fn approved_proxy_hints() -> FetchHints {
    FetchHints {
        org_id: "quarry_integration_test".to_string(),
        privacy: PrivacyPolicy {
            allow_third_party_processing: true,
            processor_id: Some("quarry_proxy_pool".into()),
            ..PrivacyPolicy::default()
        },
        ..FetchHints::default()
    }
}

fn driver_proxied_through(server: &MockServer) -> StaticDriver {
    let pool = ProxyPool::from_env_string(&server.uri());
    StaticDriver::with_proxy_pool_and_processor(
        Duration::from_secs(5),
        "quarry-test",
        pool,
        Some("quarry_proxy_pool".into()),
    )
    .unwrap()
}

#[tokio::test]
async fn static_driver_fetches_from_wiremock() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string("<html>hi</html>"),
        )
        .mount(&server)
        .await;

    let driver = driver_proxied_through(&server);
    let url: Url = "http://example.com/".parse().unwrap();
    let resp = driver
        .fetch_conditional(&url, &approved_proxy_hints())
        .await
        .unwrap();

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"<html>hi</html>");
    assert!(resp
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type")));
}

#[tokio::test]
async fn static_driver_returns_500_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let driver = driver_proxied_through(&server);
    let url: Url = "http://example.com/".parse().unwrap();
    let resp = driver
        .fetch_conditional(&url, &approved_proxy_hints())
        .await
        .unwrap();

    assert_eq!(resp.status, 500);
}
