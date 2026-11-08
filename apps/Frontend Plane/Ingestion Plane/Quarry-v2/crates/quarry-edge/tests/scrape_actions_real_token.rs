//! WS13.5, ADR-6 (decision 3.8, step 4): the real-token agent-label test, in the `on` mode.
//!
//! Beside `zdr_claim_floor.rs`, on its real-auth harness (`support/zdr_harness.rs`): no dev
//! bypass, every request runs the real `require_auth` against a JWKS endpoint, and the claim
//! sets are the golden Auth Core minted. The tests in `scrape_actions.rs` run on the dev bypass,
//! whose stub identity cannot show what a real service principal may do; these cannot be
//! mistaken for that.
//!
//! Evidence, not CI: every test here is `#[ignore]`d and returns at once unless
//! `QUARRY_EVIDENCE_STACK=1` is set (the flag of `scrape_actions_live`). Run them with
//! `QUARRY_EVIDENCE_STACK=1 cargo test -p quarry-edge --test <this binary> -- --ignored`.
//! They need no stack and no Chromium.
//!
//! The decision in force (3.8): a request labelled `initiated_by: "agent"` is refused in every
//! mode whatever the token; a service principal needs `scrape:interact`; `on` admits a service
//! principal that holds it. The switch is read once per process, so this binary runs `on`
//! and `tests/scrape_actions_real_token_user_mode.rs` runs `user`.

mod support;

#[path = "support/zdr_harness.rs"]
mod zdr_harness;

#[path = "support/actions_real_auth.rs"]
mod actions_real_auth;

use actions_real_auth::{
    click, enable, evidence_lane, labelled, post, scrape_body, start, SCOPE_SCRAPE_INTERACT,
};
use zdr_harness::{golden_token, token_with_scopes, user_in_standard_org};

const REFUSED: &str = "scrape_interact_scope_required";

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn a_user_token_with_the_agent_label_is_refused_and_the_same_body_without_it_is_served() {
    if !evidence_lane() {
        return;
    }
    enable("on");
    let harness = start().await;
    let token = user_in_standard_org();

    let (status, body) = post(&harness, &token, labelled(scrape_body(click()), "agent")).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["details"]["reason"], REFUSED, "{body}");

    let (status, body) = post(&harness, &token, scrape_body(click())).await;
    assert_eq!(
        status, 200,
        "the same body without the label is served: {body}"
    );
    assert_eq!(body["data"]["actions"][0]["type"], "click", "{body}");

    // An unknown label is ignored: it never refuses and never widens anything.
    let (status, body) = post(&harness, &token, labelled(scrape_body(click()), "somebody")).await;
    assert_eq!(status, 200, "an unknown label is ignored: {body}");
}

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn an_agent_label_is_refused_for_a_service_principal_that_holds_scrape_interact() {
    if !evidence_lane() {
        return;
    }
    enable("on");
    let harness = start().await;
    let token = token_with_scopes("service_execution_core", &[SCOPE_SCRAPE_INTERACT]);

    let (status, body) = post(&harness, &token, labelled(scrape_body(click()), "agent")).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["details"]["reason"], REFUSED, "{body}");
}

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn a_service_principal_without_scrape_interact_is_refused_with_actions() {
    if !evidence_lane() {
        return;
    }
    enable("on");
    let harness = start().await;
    // Both golden service principals hold a scrape scope that reaches /v1/scrape; neither holds
    // scrape:interact, so neither may send actions.
    for case in ["service_model_gateway", "service_execution_core"] {
        let (status, body) = post(&harness, &golden_token(case), scrape_body(click())).await;
        assert_eq!(status, 403, "{case}: {body}");
        assert_eq!(
            body["error"]["details"]["reason"], REFUSED,
            "{case}: {body}"
        );
    }
}

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn a_service_principal_that_holds_scrape_interact_is_served_in_on_mode() {
    if !evidence_lane() {
        return;
    }
    enable("on");
    let harness = start().await;
    let token = token_with_scopes("service_execution_core", &[SCOPE_SCRAPE_INTERACT]);

    let (status, body) = post(&harness, &token, scrape_body(click())).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["actions"][0]["type"], "click", "{body}");
}

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn a_token_that_does_not_verify_never_reaches_the_action_gate() {
    if !evidence_lane() {
        return;
    }
    enable("on");
    let harness = start().await;

    let (status, body) = post(&harness, "not-a-signed-token", scrape_body(click())).await;
    assert_eq!(status, 401, "{body}");
}
