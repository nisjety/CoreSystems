//! The org's web domain rules (EP-9 W3) on two paths the earlier tests did not reach. org-core (wiremock) allows
//! `example.org` and denies `blocked.example`.
//!
//! F-f10-05: the gRPC `WebSearch` RPC filters its response with `drop_denied_results_in`. Under `enforce` the hit on
//! the denied host is dropped and the others kept, in order; under `shadow` every hit is kept (the rules are still
//! read); under `off` org-core is never asked (its address is unreachable here) and every hit is kept.
//!
//! FU-29: the inline tool loop's `extract_structured` arm (`dispatch_tool`) under `enforce`. A URL on the denied host
//! is refused before the page reader is asked; a page that redirected onto the denied host is refused before any
//! inference call (the inference client is offline here, so reaching it would fail differently), and the model gets
//! the typed refusal and none of the page's text. A page that stayed on the allowed host passes the rules.
//!
//! This binary sets `WEB_DOMAIN_POLICY_MODE=enforce` and `MODEL_GATEWAY_EXTRACT_TOOL=true` itself, so no other test
//! binary sees them.

use model_gateway::site_policy_client::{
    drop_denied_results_in, SiteChecker, SiteOwner, SITE_BLOCKED_CODE,
};
use model_gateway::state::AppState;
use mp_contracts::model_plane::v1::{ToolCall, WebSearchResult};
use mp_contracts::site_policy::WebDomainPolicyMode;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

const MARKER: &str = "FU29-EXTRACT-PAGE-MARKER-5d1e";

async fn org_core(org_id: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/internal/orgs/{org_id}/browser-sites/resolve")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "organization_id": org_id,
            "revision": 1,
            "decisions": [
                {"host": "example.org", "effective": "read", "cap": "read", "source": "default"},
                {"host": "news.example.org", "effective": "read", "cap": "read", "source": "default"},
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

fn hit(url: &str) -> WebSearchResult {
    WebSearchResult {
        url: url.to_owned(),
        title: format!("title of {url}"),
        snippet: "snippet".to_owned(),
        source: "tavily".to_owned(),
        ..Default::default()
    }
}

fn hits() -> Vec<WebSearchResult> {
    vec![
        hit("https://example.org/a"),
        hit("https://blocked.example/b"),
        hit("https://news.example.org/c"),
    ]
}

fn urls(results: &[WebSearchResult]) -> Vec<&str> {
    results.iter().map(|hit| hit.url.as_str()).collect()
}

#[tokio::test]
async fn enforce_drops_the_denied_hosts_search_hit_and_keeps_the_rest() {
    let org = org_core("org-f1005-enforce").await;
    let base = org.uri();
    let mut results = hits();
    let dropped = drop_denied_results_in(
        WebDomainPolicyMode::Enforce,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1005-enforce",
        &mut results,
    )
    .await;
    assert_eq!(dropped, 1);
    assert_eq!(
        urls(&results),
        ["https://example.org/a", "https://news.example.org/c"]
    );
    assert!(requests(&org).await > 0, "enforce reads the rules");
}

#[tokio::test]
async fn shadow_keeps_every_search_hit_and_still_reads_the_rules() {
    let org = org_core("org-f1005-shadow").await;
    let base = org.uri();
    let mut results = hits();
    let counted = drop_denied_results_in(
        WebDomainPolicyMode::Shadow,
        &SiteChecker::default(),
        &owner(&base),
        "org-f1005-shadow",
        &mut results,
    )
    .await;
    assert_eq!(counted, 1, "shadow counts the hit it would drop");
    assert_eq!(urls(&results), urls(&hits()), "shadow drops nothing");
    assert!(requests(&org).await > 0, "shadow reads the rules");
}

#[tokio::test]
async fn off_keeps_every_search_hit_and_asks_org_core_nothing() {
    let mut results = hits();
    let dropped = drop_denied_results_in(
        WebDomainPolicyMode::Off,
        &SiteChecker::default(),
        // Nothing listens here: any request would fail, and `off` makes none.
        &owner("http://127.0.0.1:9"),
        "org-f1005-off",
        &mut results,
    )
    .await;
    assert_eq!(dropped, 0);
    assert_eq!(urls(&results), urls(&hits()));
}

/// The gateway with Quarry and org-core on mock servers and every gRPC downstream (inference included) offline.
/// Quarry's page ends on `final_url`.
async fn extract_state(org: &MockServer, final_url: &str) -> (AppState, MockServer) {
    std::env::set_var("WEB_DOMAIN_POLICY_MODE", "enforce");
    std::env::set_var("MODEL_GATEWAY_EXTRACT_TOOL", "true");
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
    let mut state = offline_downstreams::offline_state();
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: std::time::Duration::from_secs(5),
    });
    state.org_core_base_url = org.uri();
    state.org_core_service_token = "test-token".to_owned();
    (state, quarry)
}

async fn extract(
    state: &AppState,
    org_id: &str,
    url: &str,
) -> model_gateway::tool_loop::ToolOutcome {
    let call = ToolCall {
        id: "call-1".to_owned(),
        name: "extract_structured".to_owned(),
        arguments_json: json!({
            "urls": [url],
            "mode": "question",
            "question": "What are the opening hours?"
        })
        .to_string(),
    };
    model_gateway::tool_loop::dispatch_tool(
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
        "inference-bearer",
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
async fn extract_structured_refuses_a_url_on_a_denied_host_before_reading_it() {
    let org = org_core("org-fu29-asked").await;
    let (state, quarry) = extract_state(&org, "https://blocked.example/landing").await;
    let outcome = extract(&state, "org-fu29-asked", "https://blocked.example/landing").await;
    let error = outcome.error.clone().expect("the URL is refused");
    assert!(error.starts_with(SITE_BLOCKED_CODE), "{error}");
    assert_eq!(
        requests(&quarry).await,
        0,
        "the page reader was asked for a refused URL"
    );
    assert!(!outcome.output.contains(MARKER));
}

#[tokio::test]
async fn extract_structured_drops_a_page_that_redirected_onto_a_denied_host() {
    let org = org_core("org-fu29-redirect").await;
    let (state, quarry) = extract_state(&org, "https://blocked.example/landing").await;
    let outcome = extract(&state, "org-fu29-redirect", "https://example.org/start").await;
    let error = outcome
        .error
        .clone()
        .expect("the redirected page is refused");
    assert!(error.starts_with(SITE_BLOCKED_CODE), "{error}");
    assert!(error.contains("blocked.example"), "{error}");
    assert!(!error.contains(MARKER), "{error}");
    assert!(
        !outcome.output.contains(MARKER),
        "the page text reached the model: {}",
        outcome.output
    );
    assert!(requests(&quarry).await > 0, "the allowed URL was read");
}

#[tokio::test]
async fn extract_structured_on_an_allowed_page_passes_the_rules() {
    let org = org_core("org-fu29-allowed").await;
    let (state, quarry) = extract_state(&org, "https://example.org/landing").await;
    let outcome = extract(&state, "org-fu29-allowed", "https://example.org/start").await;
    // The extraction goes on to its inference call, which the offline client fails: not a domain refusal, and not a
    // refusal of the arguments either (the page was read).
    assert!(requests(&quarry).await > 0, "{:?}", outcome.error);
    assert!(
        !outcome
            .error
            .as_deref()
            .unwrap_or_default()
            .starts_with(SITE_BLOCKED_CODE),
        "{:?}",
        outcome.error
    );
}
