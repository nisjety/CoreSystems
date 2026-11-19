//! F-f10-04 / FU-29: the org's web domain rules apply to the page a web call ENDED on. org-core (wiremock) allows
//! `example.org` and denies `blocked.example`. `final_url_refusal` refuses a final URL on the blocked host under
//! `enforce` only, passes the same host without asking org-core, and reads but refuses nothing under `shadow`. An
//! empty, blank or absent final URL is no final URL, so it is unknown: under `enforce` and `shadow` the requested host
//! is read in its place (under `shadow` a would-be refusal is logged and nothing is refused), and under `off` nothing
//! is read. The inline `fetch_url` tool drops the redirected page unread: the model gets the typed refusal and none of
//! the page's text, while `off` and `shadow` read the page as before. When the final URL is unknown, the `fetch_url`
//! header and failure name the requested URL under its own label.

use mp_contracts::model_plane::v1::ToolCall;
use mp_contracts::site_policy::WebDomainPolicyMode;
use serde_json::{json, Value};
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

/// An owner whose org-core is not configured (no service token), so no host's answer can be read.
fn unconfigured(base: &str) -> SiteOwner<'_> {
    SiteOwner {
        base_url: base,
        service_id: "model-gateway",
        service_token: "",
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
async fn same_host_final_url_and_off_ask_org_core_nothing() {
    let server = org_core("org-f1004-pass").await;
    let base = server.uri();
    for (mode, requested, final_url) in [
        (
            WebDomainPolicyMode::Enforce,
            "https://example.org/a",
            "https://EXAMPLE.org/b",
        ),
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
async fn a_blank_final_url_judges_the_requested_host_under_enforce() {
    let server = org_core("org-f1004-blank").await;
    let base = server.uri();
    for final_url in ["", "   ", "\t\n"] {
        let before = requests(&server).await;
        let refusal = final_url_refusal(
            WebDomainPolicyMode::Enforce,
            &SiteChecker::default(),
            &owner(&base),
            "org-f1004-blank",
            "https://example.org/a",
            final_url,
            "fetch_url",
        )
        .await;
        assert_eq!(
            refusal, None,
            "an allowed requested host passes: {final_url:?}"
        );
        assert_eq!(
            requests(&server).await,
            before + 1,
            "the requested host is read once: {final_url:?}"
        );
    }
}

#[tokio::test]
async fn a_blank_final_url_on_a_denied_requested_host_is_refused_under_enforce() {
    let server = org_core("org-f1004-blank-deny").await;
    let base = server.uri();
    for final_url in ["", "  "] {
        let refusal = final_url_refusal(
            WebDomainPolicyMode::Enforce,
            &SiteChecker::default(),
            &owner(&base),
            "org-f1004-blank-deny",
            "https://blocked.example/start?q=secret",
            final_url,
            "extract_structured",
        )
        .await
        .expect("the requested host is denied, so the page is refused");
        assert!(refusal.starts_with(SITE_BLOCKED_CODE), "{refusal}");
        assert!(
            refusal.contains("blocked.example"),
            "names the requested host: {refusal}"
        );
        assert!(
            !refusal.contains("secret"),
            "no query in the refusal: {refusal}"
        );
    }
}

#[tokio::test]
async fn a_blank_final_url_is_refused_when_the_requested_host_cannot_be_read() {
    let server = org_core("org-f1004-blank-unread").await;
    let base = server.uri();
    let refusal = final_url_refusal(
        WebDomainPolicyMode::Enforce,
        &SiteChecker::default(),
        &unconfigured(&base),
        "org-f1004-blank-unread",
        "https://example.org/a",
        "",
        "fetch_url",
    )
    .await
    .expect("an answer that cannot be read refuses the page (fail closed)");
    assert!(refusal.starts_with(SITE_BLOCKED_CODE), "{refusal}");
    assert!(refusal.contains("cannot be read"), "{refusal}");
    assert_eq!(
        requests(&server).await,
        0,
        "org-core is not configured, so it is not asked"
    );
}

#[tokio::test]
async fn a_blank_final_url_for_a_requested_url_with_no_host_is_refused_under_enforce() {
    let server = org_core("org-f1004-blank-nohost").await;
    let base = server.uri();
    let refusal = final_url_refusal(
        WebDomainPolicyMode::Enforce,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1004-blank-nohost",
        "not a url",
        "",
        "fetch_url",
    )
    .await
    .expect("a page with no host the rules can name is refused");
    assert!(refusal.starts_with(SITE_BLOCKED_CODE), "{refusal}");
    assert!(refusal.contains("cannot be checked"), "{refusal}");
    assert_eq!(requests(&server).await, 0, "no host, so nothing is asked");
}

/// A blank final URL is no final URL. Under `off` nothing is read. Under `shadow` the requested host is judged, so a
/// would-be refusal is logged, but nothing is refused: a denied requested host passes here, as every shadow judgement
/// passes.
#[tokio::test]
async fn a_blank_final_url_is_not_read_under_off_and_is_judged_under_shadow() {
    let server = org_core("org-f1004-blank-quiet").await;
    let base = server.uri();
    for (requested, final_url) in [
        ("https://example.org/a", ""),
        ("https://example.org/a", "  "),
        ("https://blocked.example/a", ""),
        ("not a url", ""),
    ] {
        let refusal = final_url_refusal(
            WebDomainPolicyMode::Off,
            &SiteChecker::default(),
            &owner(&base),
            "org-f1004-blank-quiet",
            requested,
            final_url,
            "fetch_url",
        )
        .await;
        assert_eq!(refusal, None, "off {requested} -> {final_url:?}");
    }
    assert_eq!(
        requests(&server).await,
        0,
        "off asks org-core nothing for a blank final URL"
    );

    // Shadow asks org-core about the requested host (none for a URL with no host) and refuses nothing.
    for (requested, final_url, asked) in [
        ("https://example.org/a", "", 1),
        ("https://example.org/a", "  ", 1),
        ("https://blocked.example/a", "", 1),
        ("not a url", "", 0),
    ] {
        let before = requests(&server).await;
        let refusal = final_url_refusal(
            WebDomainPolicyMode::Shadow,
            &SiteChecker::default(),
            &owner(&base),
            "org-f1004-blank-quiet",
            requested,
            final_url,
            "fetch_url",
        )
        .await;
        assert_eq!(refusal, None, "shadow {requested} -> {final_url:?}");
        assert_eq!(
            requests(&server).await,
            before + asked,
            "shadow judges the requested host: {requested} -> {final_url:?}"
        );
    }
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

/// The URL `fetch_url` asks Quarry for in these tests.
const REQUESTED: &str = "https://example.org/start";

/// The gateway with Quarry and org-core on mock servers; Quarry's page was asked for [`REQUESTED`] and ended on
/// `final_url`, in the edge's `UrlTriple` shape (`url.requested` beside `url.final_url`).
async fn fetch_state(org: &MockServer, final_url: &str) -> (AppState, MockServer) {
    fetch_state_with_url(org, json!({"requested": REQUESTED, "final_url": final_url})).await
}

/// The same gateway, with Quarry's reply carrying `url` as given. A test names the keys it means: `final_url`, the
/// legacy `final`, or neither.
async fn fetch_state_with_url(org: &MockServer, url: Value) -> (AppState, MockServer) {
    fetch_state_with_page(
        org,
        url,
        &format!("Page text {MARKER} with enough words to be readable prose here."),
    )
    .await
}

/// The same gateway, with Quarry's reply carrying `url` as given and `markdown` as the page's text. An empty `markdown`
/// is a page with nothing to read.
async fn fetch_state_with_page(
    org: &MockServer,
    url: Value,
    markdown: &str,
) -> (AppState, MockServer) {
    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/scrape"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "status": 200,
                "url": url,
                "title": "Landing",
                "formats": {"markdown": markdown}
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
        arguments_json: json!({ "url": REQUESTED }).to_string(),
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

/// An edge that predates `final_url` answers with the legacy `final` key alone. That key still names where the page
/// ended, so a page that ended on a denied host is refused. This is the one refusal test that reads only the legacy key.
#[tokio::test]
async fn fetch_url_refuses_a_legacy_final_key_naming_a_denied_host() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    let org = org_core("org-f1004-legacy").await;
    let (state, _quarry) = fetch_state_with_url(
        &org,
        json!({"requested": REQUESTED, "final": "https://blocked.example/landing"}),
    )
    .await;
    let outcome = fetch_url(&state, "org-f1004-legacy").await;
    let error = outcome
        .error
        .clone()
        .expect("the legacy key names a denied host, so the page is refused");
    assert!(error.starts_with(SITE_BLOCKED_CODE), "{error}");
    assert!(error.contains("blocked.example"), "{error}");
    assert!(
        !outcome.output.contains(MARKER),
        "the page text reached the model: {}",
        outcome.output
    );
}

/// `final_url` is the edge's key and wins over the legacy `final` when both are present. The two name different hosts,
/// so each verdict shows which key was read: the denied host refuses the page, and the allowed host reads it.
#[tokio::test]
async fn fetch_url_judges_final_url_before_the_legacy_final_key_under_enforce() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    let org = org_core("org-f1004-precedence-deny").await;
    let (state, _quarry) = fetch_state_with_url(
        &org,
        json!({
            "requested": REQUESTED,
            "final_url": "https://blocked.example/landing",
            "final": "https://example.org/ok",
        }),
    )
    .await;
    let outcome = fetch_url(&state, "org-f1004-precedence-deny").await;
    let error = outcome
        .error
        .clone()
        .expect("final_url names a denied host, so the page is refused");
    assert!(error.starts_with(SITE_BLOCKED_CODE), "{error}");
    assert!(error.contains("blocked.example"), "{error}");
    assert!(!outcome.output.contains(MARKER), "{}", outcome.output);

    let org = org_core("org-f1004-precedence-allow").await;
    let (state, _quarry) = fetch_state_with_url(
        &org,
        json!({
            "requested": REQUESTED,
            "final_url": "https://example.org/landing",
            "final": "https://blocked.example/landing",
        }),
    )
    .await;
    let outcome = fetch_url(&state, "org-f1004-precedence-allow").await;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.output.contains(MARKER), "{}", outcome.output);
}

/// A blank `final_url` is no final URL, and it stays blank: the legacy `final` beside it is not read. The requested host
/// (allowed) is judged in its place, so the page is read. A `final` naming a denied host would refuse it if it were read.
#[tokio::test]
async fn fetch_url_keeps_a_blank_final_url_blank_and_never_reads_the_legacy_final_key() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    let org = org_core("org-f1004-blank-legacy").await;
    let (state, _quarry) = fetch_state_with_url(
        &org,
        json!({
            "requested": REQUESTED,
            "final_url": "",
            "final": "https://blocked.example/landing",
        }),
    )
    .await;
    let outcome = fetch_url(&state, "org-f1004-blank-legacy").await;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.output.contains(MARKER), "{}", outcome.output);
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

/// Quarry's `url` object for a page with no final URL: `final_url` absent, blank, or whitespace-only, or no keys at all.
/// Each case gets an org id named for `tag`, so the process-wide verdict cache cannot carry one test's answers into
/// another.
fn no_final_url_replies(tag: &str) -> Vec<(String, Value)> {
    vec![
        (
            format!("org-f1004-{tag}-absent"),
            json!({"requested": REQUESTED}),
        ),
        (format!("org-f1004-{tag}-empty"), json!({})),
        (
            format!("org-f1004-{tag}-blank"),
            json!({"requested": REQUESTED, "final_url": ""}),
        ),
        (
            format!("org-f1004-{tag}-spaces"),
            json!({"requested": REQUESTED, "final_url": "   "}),
        ),
    ]
}

#[tokio::test]
async fn fetch_url_reads_a_page_with_an_absent_or_blank_final_url_under_enforce() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    for (org_id, url) in no_final_url_replies("enforce") {
        let org = org_core(&org_id).await;
        let (state, _quarry) = fetch_state_with_url(&org, url).await;
        let outcome = fetch_url(&state, &org_id).await;
        assert!(outcome.error.is_none(), "{org_id}: {:?}", outcome.error);
        assert!(
            outcome.output.contains(MARKER),
            "{org_id}: {}",
            outcome.output
        );
    }
}

#[tokio::test]
async fn fetch_url_reads_a_page_with_an_absent_or_blank_final_url_under_off() {
    let _mode = test_seam::set(WebDomainPolicyMode::Off);
    for (org_id, url) in no_final_url_replies("off") {
        let org = org_core(&org_id).await;
        let (state, _quarry) = fetch_state_with_url(&org, url).await;
        let outcome = fetch_url(&state, &org_id).await;
        assert!(outcome.error.is_none(), "{org_id}: {:?}", outcome.error);
        assert!(
            outcome.output.contains(MARKER),
            "{org_id}: {}",
            outcome.output
        );
        assert_eq!(
            requests(&org).await,
            0,
            "{org_id}: off asks org-core nothing"
        );
    }
}

/// V3d-go-1, display: a page whose final URL is unknown has a header `final_url` of null, and the URL asked for under its
/// own label, never an empty address. A known final URL is shown as it always was, with no `requested_url` key.
#[tokio::test]
async fn fetch_url_header_names_the_requested_url_when_the_final_url_is_unknown() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    for (org_id, url) in no_final_url_replies("header") {
        let org = org_core(&org_id).await;
        let (state, _quarry) = fetch_state_with_url(&org, url).await;
        let outcome = fetch_url(&state, &org_id).await;
        assert!(outcome.error.is_none(), "{org_id}: {:?}", outcome.error);
        let header: Value = serde_json::from_str(&outcome.output).expect("the page is JSON");
        assert!(
            header["final_url"].is_null(),
            "{org_id}: {}",
            outcome.output
        );
        assert_eq!(
            header["requested_url"], REQUESTED,
            "{org_id}: {}",
            outcome.output
        );
        assert!(outcome.output.contains(MARKER), "{org_id}");
    }

    let org = org_core("org-f1004-header-known").await;
    let (state, _quarry) = fetch_state(&org, "https://example.org/landing").await;
    let outcome = fetch_url(&state, "org-f1004-header-known").await;
    let header: Value = serde_json::from_str(&outcome.output).expect("the page is JSON");
    assert_eq!(
        header["final_url"], "https://example.org/landing",
        "{}",
        outcome.output
    );
    assert!(header.get("requested_url").is_none(), "{}", outcome.output);
}

/// V3d-go-1, display: a page with no readable text names the requested URL, labelled as requested, in its failure when
/// the final URL is unknown. The failure never reads "from" followed by an empty address. A known final URL is named as
/// it always was.
#[tokio::test]
async fn fetch_url_failure_names_the_requested_url_when_the_final_url_is_unknown() {
    let _mode = test_seam::set(WebDomainPolicyMode::Enforce);
    for (org_id, url) in no_final_url_replies("failure") {
        let org = org_core(&org_id).await;
        let (state, _quarry) = fetch_state_with_page(&org, url, "").await;
        let outcome = fetch_url(&state, &org_id).await;
        let error = outcome
            .error
            .clone()
            .expect("a page with no text is a failure");
        assert!(
            error.contains(&format!(
                "from the requested URL {REQUESTED}, whose final URL was not reported"
            )),
            "{org_id}: {error}"
        );
    }

    let org = org_core("org-f1004-failure-known").await;
    let (state, _quarry) = fetch_state_with_page(
        &org,
        json!({"requested": REQUESTED, "final_url": "https://example.org/landing"}),
        "",
    )
    .await;
    let outcome = fetch_url(&state, "org-f1004-failure-known").await;
    let error = outcome
        .error
        .clone()
        .expect("a page with no text is a failure");
    assert!(
        error.contains("from https://example.org/landing (the page is most likely"),
        "{error}"
    );
}
