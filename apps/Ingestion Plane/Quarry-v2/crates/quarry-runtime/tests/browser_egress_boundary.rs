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
//! # What each test proves, and why each is needed
//!
//! `guard_page_request_target` and `guard_navigation_target` are already
//! unit-tested against loopback and RFC1918 addresses in quarry-browser. Those
//! prove the DECISION. Neither proves the decision is actually wired into a
//! running browser, or that the transport layer would refuse a destination the
//! decision missed. That is what these do:
//!
//! 1. `the_driver_refuses_a_loopback_goto_before_sending_anything_to_chromium` —
//!    the driver's own Rust guard. A caller-supplied loopback URL is refused with
//!    `SecurityBlocked` by `ChromiumoxideDriver::goto` before any CDP command, so
//!    Chromium is never asked. It also carries the positive control that a real
//!    browser is up and answering. It does NOT show Chromium itself being refused;
//!    tests 2 and 3 do.
//! 2. `chromium_page_requests_to_loopback_are_refused_by_the_cdp_boundary_*` — a
//!    real browser, launched behind the real proxy, whose PAGE sends the request
//!    (an `<img>` and a `fetch`) to a live loopback listener. Chromium itself
//!    emits the request, the CDP Fetch boundary aborts it, and the refusal is
//!    read back from the driver's receipts, once per layer: the domain grant, and
//!    (with a grant that wrongly names the loopback address) the SSRF guard.
//!    The proxy can write the same policy string for the same request, so the
//!    tests also read the pinned proxy's OWN receipts and require none for the
//!    listener: that is what shows the CDP boundary, not the proxy, refused it.
//!    Needs Chromium, so it is gated on `CHROMIUMOXIDE_TEST=1`.
//! 3. `the_pinned_proxy_refuses_loopback_*` — a genuine HTTP request through the
//!    proxy's own endpoint to a live loopback listener, once per layer, with the
//!    refusal read from the proxy's receipts (`domain_grant_blocked`, and
//!    `dns_or_ssrf_blocked` when the grant names the loopback address). This is
//!    the TRANSPORT half, which CDP interception is only defence in depth for:
//!    Chromium would otherwise re-resolve an approved hostname when it opens a
//!    socket, and this is the thing that stops the socket. Needs no browser, so
//!    it runs everywhere.
//!
//! Why a grant that names `127.0.0.1` is used: `canonical_broker_domains`
//! rejects IP literals, so a governed run can never hold such a grant. The
//! policy type itself does not reject them, and that is the point of the
//! "grant names it" cases: with the grant layer out of the way, only the SSRF
//! layer stands between the page and the listener, so removing that check turns
//! the test red. With a restricted grant naming an unrelated domain the grant
//! layer refuses first and the SSRF check would never be reached.
//!
//! Every test that asserts an empty listener log also contacts the listener
//! directly afterwards and requires that it DOES log (`require_witness_records`),
//! so emptiness means "refused" rather than "not watching".
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
use quarry_core::ids::kinds;
use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};
use quarry_runtime::PinnedBrowserEgressProxy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::MockServer;

/// Skips without `CHROMIUMOXIDE_TEST=1`; with it, FAILS when no Chromium is found
/// (the shared gate, see `quarry_browser::live_gate`).
fn browser_available() -> bool {
    quarry_browser::live_gate::live_enabled()
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
async fn the_driver_refuses_a_loopback_goto_before_sending_anything_to_chromium() {
    if !browser_available() {
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
    // (The refusal itself is the driver's Rust guard, before CDP; see the
    // header for the tests where Chromium itself sends the request.)
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
    require_witness_records(&target).await;

    driver.release(session).await.expect("release session");
}

/// POSITIVE CONTROL for every "the listener saw nothing" assertion. An empty
/// request log is also what a broken witness produces (a mock server that never
/// records, `received_requests` used wrongly), which would make the tests below
/// pass for the wrong reason. Contact the listener directly and require that it
/// DOES log exactly that one request.
async fn require_witness_records(target: &MockServer) {
    let mut direct = tokio::net::TcpStream::connect(target.address())
        .await
        .expect("the listener must be reachable without the proxy");
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
        "the witness itself is not recording, so an empty log proves nothing"
    );
    assert!(after_control[0].url.path().ends_with("/control"));
}

async fn assert_listener_saw_nothing(target: &MockServer, what: &str) {
    tokio::time::sleep(Duration::from_millis(250)).await;
    let received = target.received_requests().await.expect("request log");
    assert!(
        received.is_empty(),
        "{what}: the loopback listener was reached: {:?}",
        received
            .iter()
            .map(|request| request.url.to_string())
            .collect::<Vec<_>>()
    );
}

/// A real Chromium behind the real proxy, whose page sends a request to a live
/// loopback listener. Chromium emits it; the CDP Fetch boundary must abort it,
/// and the driver's receipts must show that decision for the listener's origin.
async fn chromium_page_request_to_loopback_is_refused(
    grant: &[&str],
    session_key: &str,
    expected_policy: &str,
) {
    let target = MockServer::start().await;
    let proxy = Arc::new(PinnedBrowserEgressProxy::new());
    let driver = ChromiumoxideDriver::new().with_pinned_egress_proxy(proxy.clone());

    let session = driver
        .acquire(&lease(session_key))
        .await
        .expect("acquire session (this launches Chromium)");
    let grant: Vec<String> = grant.iter().map(|domain| (*domain).to_owned()).collect();
    driver
        .configure_egress_policy(&session, BrowserEgressPolicy::from_allowed_domains(&grant))
        .await
        .expect("install egress policy");

    // Positive control: a real browser is up and answering CDP.
    driver
        .goto(&session, "about:blank")
        .await
        .expect("about:blank must navigate with the boundary installed");

    // The PAGE asks Chromium to fetch the loopback origin, two ways. The
    // driver's own URL guard is not involved: nothing calls `goto` with it.
    let origin = target.uri();
    let script = format!(
        "(() => {{ const img = new Image(); img.src = \"{origin}/private.png\"; \
         document.body.appendChild(img); \
         fetch(\"{origin}/private.json\").catch(() => {{}}); return true; }})()"
    );
    driver
        .evaluate(&session, &script)
        .await
        .expect("the page script must run");

    // Wait until the boundary has reported a decision for the listener, then
    // let any second request settle.
    let host = target.address().to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut after = 0_u64;
    let mut seen = Vec::new();
    loop {
        let batch = driver
            .egress_receipts(&session, after, 100)
            .await
            .expect("read egress receipts");
        if let Some(last) = batch.last() {
            after = last.sequence;
        }
        seen.extend(batch);
        if seen.iter().any(|receipt| receipt.url.contains(&host)) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Chromium never sent the page's loopback request to the boundary \
             (receipts: {seen:?}); the test would otherwise prove nothing"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    seen.extend(
        driver
            .egress_receipts(&session, after, 100)
            .await
            .expect("read egress receipts"),
    );

    let for_listener: Vec<_> = seen
        .iter()
        .filter(|receipt| receipt.url.contains(&host))
        .collect();
    assert!(!for_listener.is_empty());
    for receipt in &for_listener {
        assert_eq!(
            receipt.decision,
            quarry_core::contracts::BrowserEgressDecision::Block,
            "a loopback request was not blocked: {receipt:?}"
        );
        assert_eq!(
            receipt.policy, expected_policy,
            "blocked, but by the wrong layer: {receipt:?}"
        );
    }

    // Which layer refused. The driver merges the proxy's receipts into the
    // stream read above, and both layers can write the same policy string for
    // the same request (`domain_grant_blocked`), so the receipts alone cannot
    // tell a CDP refusal from a proxy refusal. A CDP refusal happens before
    // Chromium routes anything, so the proxy must hold NO receipt for the
    // listener; if the CDP interception were removed, the request would reach
    // the proxy, which would record (and refuse) it, and this fails. That the
    // proxy does record loopback refusals is proven by the
    // `the_pinned_proxy_refuses_loopback_*` tests, so an empty list is
    // meaningful. The proxy is read straight from the provider, not through
    // the driver, so nothing is merged in.
    let proxy_receipts = proxy
        .receipts_after(session_key, 0, 100)
        .await
        .expect("read the pinned proxy's own receipts");
    let proxy_for_listener: Vec<_> = proxy_receipts
        .iter()
        .filter(|receipt| receipt.url.contains(&host))
        .collect();
    assert!(
        proxy_for_listener.is_empty(),
        "the request reached the pinned proxy, so the CDP boundary did not refuse it first:          {proxy_for_listener:?}"
    );

    assert_listener_saw_nothing(&target, "page-initiated request").await;
    require_witness_records(&target).await;
    driver.release(session).await.expect("release session");
}

#[tokio::test]
async fn chromium_page_requests_to_loopback_are_refused_by_the_cdp_boundary_domain_grant() {
    if !browser_available() {
        return;
    }
    chromium_page_request_to_loopback_is_refused(
        &["example.com"],
        "egress-boundary-page-grant",
        "domain_grant_blocked",
    )
    .await;
}

#[tokio::test]
async fn chromium_page_requests_to_loopback_are_refused_by_the_cdp_boundary_ssrf_guard() {
    if !browser_available() {
        return;
    }
    // The grant names the loopback address, so the grant layer lets the request
    // through and only the SSRF guard can refuse it.
    chromium_page_request_to_loopback_is_refused(
        &["127.0.0.1"],
        "egress-boundary-page-ssrf",
        "url_or_dns_blocked",
    )
    .await;
}

/// The transport half, over a real TCP connection to the real proxy.
///
/// Chromium is not involved: this is the guard that holds even if a request
/// passed every URL check and only then resolved to a private address. It speaks
/// the proxy's own wire protocol (absolute-form HTTP, as a browser configured
/// with an HTTP proxy does) so the assertion is about the proxy's behaviour, not
/// about a client library's. The refusal is read from the proxy's receipts, so
/// the test also says WHICH layer refused.
async fn the_pinned_proxy_refuses_loopback(
    grant: &[&str],
    session_key: &str,
    expected_policy: &str,
) {
    let target = MockServer::start().await;
    let proxy = PinnedBrowserEgressProxy::new();

    let endpoint = proxy
        .endpoint_for_session(session_key)
        .await
        .expect("bind a session endpoint");
    let grant: Vec<String> = grant.iter().map(|domain| (*domain).to_owned()).collect();
    proxy
        .configure_policy(
            session_key,
            BrowserEgressPolicy::from_allowed_domains(&grant),
        )
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

    let receipts = proxy
        .receipts_after(session_key, 0, 10)
        .await
        .expect("proxy receipts");
    assert_eq!(receipts.len(), 1, "one request, one receipt: {receipts:?}");
    assert_eq!(
        receipts[0].decision,
        quarry_core::contracts::BrowserEgressDecision::Block
    );
    assert_eq!(
        receipts[0].policy, expected_policy,
        "blocked, but by the wrong layer: {:?}",
        receipts[0]
    );

    assert_listener_saw_nothing(&target, "pinned proxy").await;
    require_witness_records(&target).await;
}

#[tokio::test]
async fn the_pinned_proxy_refuses_loopback_by_domain_grant() {
    the_pinned_proxy_refuses_loopback(
        &["example.com"],
        "egress-boundary-transport-grant",
        "domain_grant_blocked",
    )
    .await;
}

#[tokio::test]
async fn the_pinned_proxy_refuses_loopback_by_ssrf_guard_even_when_the_grant_names_it() {
    the_pinned_proxy_refuses_loopback(
        &["127.0.0.1"],
        "egress-boundary-transport-ssrf",
        "dns_or_ssrf_blocked",
    )
    .await;
}
