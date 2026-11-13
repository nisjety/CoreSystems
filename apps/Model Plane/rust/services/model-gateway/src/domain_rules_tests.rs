//! F-f10-04 / FU-29: the org's web domain rules apply to the page a web call ENDED on. org-core (wiremock) allows
//! `example.org` and denies `blocked.example`. `final_url_refusal` refuses a final URL on the blocked host under
//! `enforce` only, passes the same host and an empty final URL without asking org-core, and reads but refuses
//! nothing under `shadow`. The inline `fetch_url` tool drops the redirected page unread: the model gets the typed
//! refusal and none of the page's text, while `off` and `shadow` read the page as before.

use mp_contracts::model_plane::v1::ToolCall;
use mp_contracts::site_policy::WebDomainPolicyMode;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::site_policy_client::{
    final_url_refusal, test_seam, SiteChecker, SiteOwner, SITE_BLOCKED_CODE,
};
use crate::state::AppState;

const MARKER: &str = "F10-04-PAGE-MARKER-91c2";

async fn org_core(org_id: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/internal/orgs/{org_id}/browser-sites/resolve")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "organization_id": org_id,
            "revision": 1,
            "decisions": [
                {"host": "example.org", "effective": "read", "cap": "read", "source": "default"},
                {"host": "blocked.example", "effective": "deny", "cap": "none", "source": "org_block"}
            ]
        })))
        .mount(&server)
        .await;
    server
}

fn owner(base: &str) -> SiteOwner<'_> {
    SiteOwner {
        base_url: base,
        service_id: "model-gateway",
        service_token: "test-token",
    }
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap_or_default().len()
}

#[tokio::test]
async fn final_url_on_a_denied_host_is_refused_under_enforce_only() {
    let server = org_core("org-f1004").await;
    let base = server.uri();
    let refusal = final_url_refusal(
        WebDomainPolicyMode::Enforce,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1004",
        "https://example.org/start",
        "https://blocked.example/landing?q=secret",
        "fetch_url",
    )
    .await
    .expect("a redirect onto a denied host is refused");
    assert!(refusal.starts_with(SITE_BLOCKED_CODE), "{refusal}");
    assert!(
        refusal.contains("blocked.example"),
        "names the FINAL host: {refusal}"
    );
    assert!(
        !refusal.contains("/landing"),
        "no path in the refusal: {refusal}"
    );
    assert!(
        !refusal.contains("secret"),
        "no query in the refusal: {refusal}"
    );

    // A final URL on an allowed host passes.
    let allowed = final_url_refusal(
        WebDomainPolicyMode::Enforce,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1004",
        "https://blocked.example/start",
        "https://example.org/ok",
        "fetch_url",
    )
    .await;
    assert_eq!(allowed, None);

    // Shadow reads the rules and refuses nothing.
    let before = requests(&server).await;
    let shadow = final_url_refusal(
        WebDomainPolicyMode::Shadow,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1004",
        "https://example.org/start",
        "https://blocked.example/landing",
        "fetch_url",
    )
    .await;
    assert_eq!(shadow, None);
    assert!(
        requests(&server).await > before,
        "shadow still asks org-core"
    );
}

#[tokio::test]
async fn same_host_empty_final_url_and_off_ask_org_core_nothing() {
    let server = org_core("org-f1004-pass").await;
    let base = server.uri();
    for (mode, requested, final_url) in [
        (
            WebDomainPolicyMode::Enforce,
            "https://example.org/a",
            "https://EXAMPLE.org/b",
        ),
        (WebDomainPolicyMode::Enforce, "https://example.org/a", ""),
        (WebDomainPolicyMode::Enforce, "https://example.org/a", "   "),
        (
            WebDomainPolicyMode::Off,
            "https://example.org/a",
            "https://blocked.example/x",
        ),
    ] {
        let refusal = final_url_refusal(
            mode,
            &SiteChecker::default(),
            &owner(&base),
            "org-f1004-pass",
            requested,
            final_url,
            "fetch_url",
        )
        .await;
        assert_eq!(refusal, None, "{mode:?} {requested} -> {final_url:?}");
    }
    assert_eq!(requests(&server).await, 0, "none of these needs org-core");
}

#[tokio::test]
async fn a_final_url_with_no_host_is_refused_under_enforce() {
    let server = org_core("org-f1004-nohost").await;
    let base = server.uri();
    let refusal = final_url_refusal(
        WebDomainPolicyMode::Enforce,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1004-nohost",
        "https://example.org/a",
        "not a url",
        "fetch_url",
    )
    .await;
    assert!(
        refusal
            .as_deref()
            .is_some_and(|r| r.starts_with(SITE_BLOCKED_CODE)),
        "an unnamable final host counts as refused: {refusal:?}"
    );
}

/// The gateway with Quarry and org-core on mock servers; Quarry's page asked for `example.org` and ended on
/// `final_url`.
async fn fetch_state(org: &MockServer, final_url: &str) -> (AppState, MockServer) {
    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/scrape"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "status": 200,
                "url": {"final": final_url},
                "title": "Landing",
                "formats": {"markdown": format!("Page text {MARKER} with enough words to be readable prose here.")}
            }
        })))
        .mount(&quarry)
        .await;
    let mut state = AppState::new();
    state.quarry = crate::quarry::Client::new(crate::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: std::time::Duration::from_secs(5),
    });
    state.org_core_base_url = org.uri();
    state.org_core_service_token = "test-token".to_owned();
    (state, quarry)
}

async fn fetch_url(state: &AppState, org_id: &str) -> crate::tool_loop::ToolOutcome {
    let call = ToolCall {
        id: "call-1".to_owned(),
        name: "fetch_url".to_owned(),
        arguments_json: json!({ "url": "https://example.org/start" }).to_string(),
    };
    crate::tool_loop::dispatch_tool(
        state,
        "run",
        org_id,
        "user",
        "thread",
        "",
        &[],
        "",
        None,
        None,
        "",
        "",
        None,
        false,
        false,
        false,
        &call,
        None,
        None,
        None,
    )
    .await
}

#[tokio::test]
async fn fetch_url_drops_a_page_that_ended_on_a_denied_host_under_enforce() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    let org = org_core("org-f1004-fetch").await;
    let (state, _quarry) = fetch_state(&org, "https://blocked.example/landing").await;
    let outcome = fetch_url(&state, "org-f1004-fetch").await;
    let error = outcome
        .error
        .clone()
        .expect("the redirected page is refused");
    assert!(error.starts_with(SITE_BLOCKED_CODE), "{error}");
    assert!(error.contains("blocked.example"), "{error}");
    assert!(
        !outcome.output.contains(MARKER),
        "the page text reached the model: {}",
        outcome.output
    );
    assert!(!error.contains(MARKER));
}

#[tokio::test]
async fn fetch_url_reads_the_page_under_shadow_and_off() {
    for (mode, org_id) in [
        (WebDomainPolicyMode::Shadow, "org-f1004-shadow"),
        (WebDomainPolicyMode::Off, "org-f1004-off"),
    ] {
        let _mode = test_seam::set(mode);
        let org = org_core(org_id).await;
        let (state, _quarry) = fetch_state(&org, "https://blocked.example/landing").await;
        let outcome = fetch_url(&state, org_id).await;
        assert!(outcome.error.is_none(), "{mode:?}: {:?}", outcome.error);
        assert!(
            outcome.output.contains(MARKER),
            "{mode:?}: {}",
            outcome.output
        );
        if mode == WebDomainPolicyMode::Off {
            assert_eq!(requests(&org).await, 0, "off asks org-core nothing");
        } else {
            assert!(requests(&org).await > 0, "shadow reads the rules");
        }
    }
}
