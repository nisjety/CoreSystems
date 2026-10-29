//! Live proof of Quarry's browser egress boundary, with a real Chromium and the
//! real pinned proxy.
//!
//! # Why this lives here
//!
//! The boundary has two halves and they are in different crates:
//! `ChromiumoxideDriver` (quarry-browser) and `PinnedBrowserEgressProxy`
//! (quarry-runtime). quarry-runtime depends on quarry-browser one-way, so
//! quarry-runtime is the only place the two can meet. A quarry-browser unit test
//! cannot construct the real proxy without a circular dependency — which is
//! exactly why the version that lived there could only ever fail with
//! "chromium browser navigation requires a pinned egress proxy".
//!
//! # What each test proves, and why both are needed
//!
//! `guard_page_request_target` and `guard_navigation_target` are already
//! unit-tested against loopback and RFC1918 addresses in quarry-browser. Those
//! prove the DECISION. Neither proves the decision is actually wired into a
//! running browser, or that the transport layer would refuse a destination the
//! decision missed. That is what these two do:
//!
//! 1. `chromium_refuses_a_loopback_navigation` — a real browser, launched, with
//!    the real proxy installed and a restricted grant, asked to visit a live
//!    loopback listener. The refusal must happen and the listener must log
//!    nothing. Needs Chromium, so it is gated on `CHROMIUMOXIDE_TEST=1`.
//! 2. `the_pinned_proxy_refuses_loopback_over_a_real_connection` — a genuine
//!    HTTP request through the proxy's own endpoint to a live loopback listener.
//!    This is the TRANSPORT half, which CDP interception is only defence in depth
//!    for: Chromium would otherwise re-resolve an approved hostname when it opens
//!    a socket, and this is the thing that stops the socket. Needs no browser, so
//!    it runs everywhere.
//!
//! # Running the browser half
//!
//!   CHROMIUMOXIDE_TEST=1 cargo test -p quarry-runtime \
//!     --features chromiumoxide --test browser_egress_boundary

#![cfg(feature = "chromiumoxide")]

use std::sync::Arc;
use std::time::Duration;

use quarry_browser::chromiumoxide::ChromiumoxideDriver;
use quarry_browser::{BrowserDriver, BrowserEgressPolicy, BrowserEgressProxyProvider};
use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};
use quarry_core::ids::kinds;
use quarry_runtime::PinnedBrowserEgressProxy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::MockServer;

fn browser_available() -> bool {
    std::env::var("CHROMIUMOXIDE_TEST").ok().as_deref() == Some("1")
}

fn lease(key: &str) -> BrowserLease {
    BrowserLease {
        lease_id: kinds::LeaseKind::new(),
        profile_id: kinds::ProfileKind::new(),
        session_affinity_key: key.into(),
        proxy_affinity: ProxyAffinity {
            pool: "p".into(),
            sticky_key: None,
        },
        ttl_s: 30,
        capabilities: vec![Capability::Js],
        artifact_bucket: "b".into(),
        persist_profile: false,
        viewport: None,
        org_id: "test_org".into(),
    }
}

/// A restricted grant naming one unrelated domain. An IP literal could not be
/// added to it even deliberately — `canonical_broker_domains` rejects those — so
/// there is no policy under which the loopback target becomes reachable.
fn restricted_policy() -> BrowserEgressPolicy {
    BrowserEgressPolicy::from_allowed_domains(&["example.com".to_owned()])
}

#[tokio::test]
async fn chromium_refuses_a_loopback_navigation() {
    if !browser_available() {
        eprintln!("skipping: set CHROMIUMOXIDE_TEST=1 and provide a Chromium binary");
        return;
    }

    // A real listener on loopback. Anything it records is a boundary failure.
    let target = MockServer::start().await;

    // The real transport boundary, not a stub: a local CONNECT proxy that
    // resolves every destination through Quarry's SSRF guard before dialling.
    let proxy = Arc::new(PinnedBrowserEgressProxy::new());
    let driver = ChromiumoxideDriver::new().with_pinned_egress_proxy(proxy);

    let session = driver
        .acquire(&lease("egress-boundary-browser"))
        .await
        .expect("acquire session (this launches Chromium)");

    // Without an installed policy the driver refuses to navigate at all, so the
    // refusal below has to come from the target, not from a missing grant.
    driver
        .configure_egress_policy(&session, restricted_policy())
        .await
        .expect("install egress policy");

    // `about:blank` is the one non-network target the API admits. Navigating it
    // AND reading the document back is the positive control for this test: it
    // proves a real Chromium is up and answering CDP, so the refusal below is a
    // decision rather than a browser that never started.
    driver
        .goto(&session, "about:blank")
        .await
        .expect("about:blank must navigate with the boundary installed");
    let document = driver
        .content(&session)
        .await
        .expect("a live browser must return its document");
    let document = String::from_utf8_lossy(&document).to_ascii_lowercase();
    assert!(
        document.contains("<html"),
        "expected real HTML from a live browser, got {document:?}"
    );

    // The driver only reports isolated egress when a pinned proxy is installed,
    // so this pins that the browser half really is running behind the boundary.
    assert!(
        driver.capabilities().isolated_egress,
        "the browser must report isolated egress with the pinned proxy installed"
    );

    let refusal = driver
        .goto(&session, &format!("{}/private", target.uri()))
        .await
        .expect_err("a loopback navigation must be refused");
    assert_eq!(
        refusal.code,
        quarry_core::error::ErrorCode::SecurityBlocked,
        "refusal must be a security decision, not an incidental failure: {refusal:?}"
    );

    // The refusal must have happened BEFORE a socket, not after a response was
    // read and discarded.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let received = target.received_requests().await.expect("request log");
    assert!(
        received.is_empty(),
        "the browser reached a loopback target: {:?}",
        received
            .iter()
            .map(|request| request.url.to_string())
            .collect::<Vec<_>>()
    );

    driver.release(session).await.expect("release session");
}

/// The transport half, over a real TCP connection to the real proxy.
///
/// Chromium is not involved: this is the guard that holds even if a request
/// passed every URL check and only then resolved to a private address. It speaks
/// the proxy's own wire protocol (absolute-form HTTP, as a browser configured
/// with an HTTP proxy does) so the assertion is about the proxy's behaviour, not
/// about a client library's.
#[tokio::test]
async fn the_pinned_proxy_refuses_loopback_over_a_real_connection() {
    let target = MockServer::start().await;
    let proxy = PinnedBrowserEgressProxy::new();
    let session_key = "egress-boundary-transport";

    let endpoint = proxy
        .endpoint_for_session(session_key)
        .await
        .expect("bind a session endpoint");
    proxy
        .configure_policy(session_key, restricted_policy())
        .await
        .expect("install egress policy");

    let address = endpoint
        .as_str()
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let mut stream = tokio::net::TcpStream::connect(&address)
        .await
        .expect("the proxy must be listening on its advertised endpoint");

    // Absolute-form request line: what a browser sends to an HTTP proxy.
    let request = format!(
        "GET {}/private HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.uri(),
        target.address(),
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write proxied request");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read proxy response");
    let response = String::from_utf8_lossy(&response);

    // Whatever the proxy answers, it must not be a success it fetched from
    // loopback. An empty response (connection closed) is also a refusal.
    assert!(
        !response.starts_with("HTTP/1.1 200"),
        "the proxy served a loopback destination: {response}"
    );

    let received = target.received_requests().await.expect("request log");
    assert!(
        received.is_empty(),
        "the pinned proxy dialled a loopback destination: {:?}",
        received
            .iter()
            .map(|request| request.url.to_string())
            .collect::<Vec<_>>()
    );

    // POSITIVE CONTROL. Everything above asserts an empty request log, which is
    // also what a broken witness produces — a mock server that never records, or
    // a `received_requests` call used wrongly, would make both tests in this file
    // pass for the wrong reason. Contact the listener directly and require that
    // it DOES log, so the emptiness above means "refused" rather than "not
    // watching".
    let direct = tokio::net::TcpStream::connect(target.address()).await;
    let mut direct = direct.expect("the listener must be reachable without the proxy");
    direct
        .write_all(
            format!(
                "GET /control HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                target.address()
            )
            .as_bytes(),
        )
        .await
        .expect("write direct request");
    let mut discard = Vec::new();
    let _ = direct.read_to_end(&mut discard).await;

    let after_control = target.received_requests().await.expect("request log");
    assert_eq!(
        after_control.len(),
        1,
        "the witness itself is not recording, so the assertions above prove nothing"
    );
    assert!(after_control[0].url.path().ends_with("/control"));
}
