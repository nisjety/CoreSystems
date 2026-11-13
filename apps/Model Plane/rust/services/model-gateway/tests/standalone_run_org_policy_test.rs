//! F-f06-21 (D2-1119): a standalone browser run is a Do run, so the org's pause, lockdown bundle and mode cap
//! bind it. Under `enforce` a paused, lockdown, read-only or unreadable-policy org is refused with the closed
//! codes and the refusal answer the Do turn uses; `ask_first` / `auto` orgs and the off / shadow modes start as
//! before, and off reads nothing. The route reads the policy after the ZDR refusal and before any grant,
//! thread or run write, pinned on its source.

use axum::http::StatusCode;
use model_gateway::org_agent_policy::{
    admit_standalone_run, refusal_answer, OrgPolicyCache, PolicyMode, AGENTS_PAUSED_CODE,
    POLICY_UNAVAILABLE_CODE,
};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const POLICY_PATH: &str = "/api/v1/org-agent-policy";
const LOCKDOWN_CODE: &str = mp_contracts::agent_mode::LOCKDOWN_CODE;

fn document(org: &str, settings: Value) -> Value {
    json!({ "org_id": org, "revision": 1, "digest": "sha256:1", "settings": settings })
}

async fn registry(org: &str, settings: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(POLICY_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(document(org, settings)))
        .mount(&server)
        .await;
    server
}

async fn admit(mode: PolicyMode, server: &MockServer, org: &str) -> Result<(), &'static str> {
    admit_standalone_run(
        mode,
        &OrgPolicyCache::new(),
        &reqwest::Client::new(),
        &server.uri(),
        org,
        "token",
    )
    .await
}

#[tokio::test]
async fn enforced_standalone_run_is_refused_for_paused_lockdown_and_read_only_orgs() {
    for (org, settings, want) in [
        (
            "org-paused",
            json!({ "agents.paused": true }),
            AGENTS_PAUSED_CODE,
        ),
        // A paused org is refused as paused whatever its cap says.
        (
            "org-paused-auto",
            json!({ "agents.paused": true, "mode_cap": "auto" }),
            AGENTS_PAUSED_CODE,
        ),
        (
            "org-lockdown",
            json!({ "mode_cap": "lockdown" }),
            LOCKDOWN_CODE,
        ),
        (
            "org-bundle",
            json!({ "mode_cap": "auto", "lockdown": true }),
            LOCKDOWN_CODE,
        ),
        // A browser run clicks and types: it cannot be held to read-only, so the cap forbids it.
        (
            "org-read-only",
            json!({ "mode_cap": "read_only" }),
            LOCKDOWN_CODE,
        ),
        // A cap this build cannot read is the strictest one.
        (
            "org-odd-cap",
            json!({ "mode_cap": "full_access" }),
            LOCKDOWN_CODE,
        ),
    ] {
        let server = registry(org, settings.clone()).await;
        assert_eq!(
            admit(PolicyMode::Enforce, &server, org).await,
            Err(want),
            "{org} {settings}"
        );
    }
}

#[tokio::test]
async fn enforced_standalone_run_fails_closed_when_the_policy_is_unreadable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(POLICY_PATH))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    assert_eq!(
        admit(PolicyMode::Enforce, &server, "org-unread").await,
        Err(POLICY_UNAVAILABLE_CODE)
    );
    // No capability bearer forwarded: the policy cannot be read, and the run is refused, not admitted.
    let empty_bearer = admit_standalone_run(
        PolicyMode::Enforce,
        &OrgPolicyCache::new(),
        &reqwest::Client::new(),
        &server.uri(),
        "org-unread",
        "",
    )
    .await;
    assert_eq!(empty_bearer, Err(POLICY_UNAVAILABLE_CODE));
}

#[tokio::test]
async fn enforced_standalone_run_admits_ask_first_auto_and_default_orgs() {
    for (org, settings) in [
        ("org-ask-first", json!({ "mode_cap": "ask_first" })),
        ("org-auto", json!({ "mode_cap": "auto" })),
        ("org-default", json!({})),
    ] {
        let server = registry(org, settings.clone()).await;
        assert_eq!(
            admit(PolicyMode::Enforce, &server, org).await,
            Ok(()),
            "{org} {settings}"
        );
    }
}

#[tokio::test]
async fn off_reads_nothing_and_shadow_refuses_nothing() {
    let server = registry(
        "org-locked",
        json!({ "agents.paused": true, "mode_cap": "lockdown" }),
    )
    .await;
    assert_eq!(admit(PolicyMode::Off, &server, "org-locked").await, Ok(()));
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty(),
        "off makes no call to the registry"
    );
    assert_eq!(
        admit(PolicyMode::Shadow, &server, "org-locked").await,
        Ok(())
    );
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        1,
        "shadow reads the policy and applies nothing"
    );
}

#[test]
fn refusals_answer_as_a_do_turn_does() {
    assert_eq!(refusal_answer(AGENTS_PAUSED_CODE).0, StatusCode::FORBIDDEN);
    assert_eq!(refusal_answer(LOCKDOWN_CODE).0, StatusCode::FORBIDDEN);
    assert_eq!(
        refusal_answer(POLICY_UNAVAILABLE_CODE).0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_ne!(
        refusal_answer(LOCKDOWN_CODE).1,
        refusal_answer(POLICY_UNAVAILABLE_CODE).1
    );
}

/// The route reads the policy after the ZDR refusal and before any grant, thread or run write: the
/// admission call sits between `browser_run_zdr_blocked` and the first broker, session or execution call
/// in `browser_run_start_inner`.
#[test]
fn the_browser_route_admits_after_zdr_and_before_any_write() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/browser_run.rs"),
    )
    .expect("browser_run.rs");
    let start = source
        .find("async fn browser_run_start_inner(")
        .expect("browser_run_start_inner");
    let body = &source[start..];
    let end = body[1..]
        .find("\nasync fn ")
        .map_or(body.len(), |at| at + 1);
    let body = &body[..end];
    let zdr = body
        .find("browser_run_zdr_blocked(")
        .expect("the ZDR refusal");
    let admit = body
        .find("admit_standalone_browser_run(")
        .expect("the org policy admission");
    assert!(zdr < admit, "the ZDR refusal stays first");
    // Nothing before the admission awaits: no grant, thread, run or broker call can precede it.
    let first_await = body.find(".await").expect("the route awaits");
    assert!(
        admit < first_await,
        "an await (an upstream call) precedes the org policy admission"
    );
}
