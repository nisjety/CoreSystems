//! WS14.S4: the robots.txt fetch is not a way around the SSRF guard.
//!
//! The gate fetches `/robots.txt` through the guarded `Driver` chain, never
//! through a client of its own. With the real `StaticDriver` that means a
//! loopback, link-local or private origin is refused before any request, and the
//! gate reads the refusal as "unreachable", never as "allowed".
//!
//! Written in the build wave and compile-checked only. Not run yet.

use std::sync::Arc;
use std::time::Duration;

use quarry_runtime::{RobotsDecision, RobotsGate, RobotsGateConfig, StaticDriver};
use url::Url;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gate() -> RobotsGate {
    let driver = StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap();
    RobotsGate::new(Arc::new(driver), RobotsGateConfig::default())
}

/// `wiremock::MockServer` binds to loopback, so a direct fetch to it is exactly the
/// private-target case: the guard must refuse it before the mock sees a request.
#[tokio::test]
async fn a_loopback_robots_txt_is_refused_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow:\n"))
        .mount(&server)
        .await;

    let page = Url::parse(&format!("{}/docs/a", server.uri())).unwrap();
    let decision = gate().check(&page, "org_ssrf", false).await;

    assert!(
        matches!(decision, RobotsDecision::Unreachable { .. }),
        "a refused target is unreachable, never allowed: {decision:?}"
    );
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "the guard refuses before connecting"
    );
}

/// A robots.txt that redirects to the cloud metadata address. In this test the
/// first hop is already a loopback mock, so the refusal happens there; the guard
/// that re-runs the public-address check on every redirect hop is pinned by
/// `redirect_to_a_private_target_is_blocked_before_contact` in `fetch.rs`.
#[tokio::test]
async fn a_robots_txt_redirect_to_the_metadata_address_is_unreachable() {
    let redirector = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "http://169.254.169.254/robots.txt"),
        )
        .mount(&redirector)
        .await;

    let page = Url::parse(&format!("{}/page", redirector.uri())).unwrap();
    let decision = gate().check(&page, "org_ssrf", false).await;

    assert!(
        matches!(decision, RobotsDecision::Unreachable { .. }),
        "{decision:?}"
    );
    assert!(redirector.received_requests().await.unwrap().is_empty());
}

/// A literal private address in the page URL never reaches the network either.
#[tokio::test]
async fn a_private_literal_origin_is_unreachable() {
    for page in [
        "http://10.0.0.5/page",
        "http://169.254.169.254/latest/meta-data",
        "http://[::1]/page",
    ] {
        let decision = gate()
            .check(&Url::parse(page).unwrap(), "org_ssrf", false)
            .await;
        assert!(
            matches!(decision, RobotsDecision::Unreachable { .. }),
            "{page}: {decision:?}"
        );
    }
}
