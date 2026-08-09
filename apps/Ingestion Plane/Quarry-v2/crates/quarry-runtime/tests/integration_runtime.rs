use quarry_core::error::ErrorCode;
use quarry_runtime::{Driver, StaticDriver};
use std::time::Duration;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// wiremock always binds to loopback, and StaticDriver's direct-egress preflight
// now unconditionally blocks private/loopback targets (see fetch.rs's
// client_for_decision), so a real end-to-end fetch against a wiremock server can
// no longer reach the network -- this replaces two tests that used to assert a
// successful 200/500 round trip. HTTP response parsing (status/body/headers)
// is still covered via the proxy-egress path in fetch.rs's own unit tests,
// which isn't subject to this preflight.
#[tokio::test]
async fn static_driver_blocks_direct_fetch_to_loopback_mock() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>hi</html>"))
        .mount(&server)
        .await;

    let driver = StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap();
    let url = Url::parse(&server.uri()).unwrap();
    let err = driver.fetch(&url).await.unwrap_err();

    assert_eq!(err.code, ErrorCode::SecurityBlocked);
    assert!(server.received_requests().await.unwrap().is_empty());
}
