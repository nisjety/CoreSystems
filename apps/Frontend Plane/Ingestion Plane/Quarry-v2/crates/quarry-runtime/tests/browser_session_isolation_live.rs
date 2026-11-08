//! Live proof of WS13.1 session isolation: two concurrent scrapes of one host,
//! for two different organizations, must not share cookies, with a real
//! Chromium behind the real pinned egress proxy.
//!
//! The cookie page is `https://httpbin.org/cookies/set?probe=<value>`, which
//! sets a cookie named `probe` and redirects to `/cookies`, a JSON listing of
//! the cookies the browser sent. Both fetches set the same cookie name with a
//! different value, so a shared cookie jar shows up as one org's page carrying
//! the other org's value. The cookie page can be changed with
//! `QUARRY_TEST_COOKIE_PAGE` (it must set a cookie and echo the jar back).
//!
//! Needs Chromium and outbound HTTPS. Skips without `CHROMIUMOXIDE_TEST=1`; with
//! it, FAILS when no Chromium is found (the shared gate, see
//! `quarry_browser::live_gate`).
//!
//! Run in the pinned Chromium lane:
//!
//!   CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//!     cargo test -p quarry-runtime --features chromiumoxide \
//!     --test browser_session_isolation_live -- --test-threads=1
//!
//! Not run in the build that added it (WS13.1 wave 2 defers test runs to the
//! wave's test phase). The kill-switch proof (`QUARRY_SCRAPE_SESSION_ISOLATION=0`
//! must make this test fail) is still owed; see the WS13.1 depth note.

#![cfg(feature = "chromiumoxide")]

use std::sync::Arc;

use quarry_browser::chromiumoxide::ChromiumoxideDriver;
use quarry_runtime::browser_driver::BrowserDriverAdapter;
use quarry_runtime::driver::{Driver, FetchHints};
use quarry_runtime::lease_pool::RuntimeLeasePool;
use quarry_runtime::PinnedBrowserEgressProxy;
use url::Url;

/// Default cookie page: sets the probe cookie, then echoes the jar.
const DEFAULT_COOKIE_PAGE: &str = "https://httpbin.org/cookies/set";

fn cookie_page(probe: &str) -> Url {
    let base = std::env::var("QUARRY_TEST_COOKIE_PAGE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_COOKIE_PAGE.to_owned());
    let mut url = Url::parse(&base).expect("QUARRY_TEST_COOKIE_PAGE is a URL");
    url.query_pairs_mut().append_pair("probe", probe);
    url
}

fn hints_for(org_id: &str) -> FetchHints {
    FetchHints {
        org_id: org_id.to_owned(),
        ..FetchHints::default()
    }
}

#[tokio::test]
async fn concurrent_same_host_scrapes_do_not_share_cookies() {
    if !quarry_browser::live_gate::live_enabled() {
        return;
    }

    let proxy = Arc::new(PinnedBrowserEgressProxy::new());
    let driver = ChromiumoxideDriver::new().with_pinned_egress_proxy(proxy);
    let pool = Arc::new(RuntimeLeasePool::new(2));
    let adapter = Arc::new(
        BrowserDriverAdapter::new(Arc::new(driver), pool).with_session_isolation(true),
    );

    let probe_a = format!("org-a-{}", uuid::Uuid::new_v4());
    let probe_b = format!("org-b-{}", uuid::Uuid::new_v4());
    let page_a = cookie_page(&probe_a);
    let page_b = cookie_page(&probe_b);

    let hints_a = hints_for("org_a");
    let hints_b = hints_for("org_b");
    let (first, second) = tokio::join!(
        adapter.fetch_conditional(&page_a, &hints_a),
        adapter.fetch_conditional(&page_b, &hints_b),
    );
    let body_a = String::from_utf8_lossy(&first.expect("org a scrape").body).into_owned();
    let body_b = String::from_utf8_lossy(&second.expect("org b scrape").body).into_owned();

    assert!(
        body_a.contains(&probe_a),
        "org a's page does not show its own cookie"
    );
    assert!(
        !body_a.contains(&probe_b),
        "org b's cookie reached org a's page: the browser session is shared"
    );
    assert!(
        body_b.contains(&probe_b),
        "org b's page does not show its own cookie"
    );
    assert!(
        !body_b.contains(&probe_a),
        "org a's cookie reached org b's page: the browser session is shared"
    );
}
