//! WS13.5, ADR-6 (decision 3.8): the `user` mode of the scrape-actions switch, with real tokens.
//!
//! Evidence, not CI: every test here is `#[ignore]`d and returns at once unless
//! `QUARRY_EVIDENCE_STACK=1` is set (the flag of `scrape_actions_live`). Run them with
//! `QUARRY_EVIDENCE_STACK=1 cargo test -p quarry-edge --test <this binary> -- --ignored`.
//! They need no stack and no Chromium.
//!
//! Decision 3.8 says `user` admits user tokens only: a service principal is refused even when
//! it holds `scrape:interact`. That is checked here on the real-auth harness. The switch is read
//! once per process, which is why this is its own binary; the `on` mode is in
//! `tests/scrape_actions_real_token.rs`.

mod support;

#[path = "support/zdr_harness.rs"]
mod zdr_harness;

#[path = "support/actions_real_auth.rs"]
mod actions_real_auth;

use actions_real_auth::{
    click, enable, evidence_lane, post, scrape_body, start, SCOPE_SCRAPE_INTERACT,
};
use zdr_harness::{token_with_scopes, user_in_standard_org};

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn user_mode_admits_a_user_token_with_real_auth() {
    if !evidence_lane() {
        return;
    }
    enable("user");
    let harness = start().await;

    let (status, body) = post(&harness, &user_in_standard_org(), scrape_body(click())).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["actions"][0]["type"], "click", "{body}");
}

#[tokio::test]
#[ignore = "ADR-6 evidence: set QUARRY_EVIDENCE_STACK=1 and run with --ignored"]
async fn user_mode_refuses_a_service_principal_even_with_scrape_interact() {
    if !evidence_lane() {
        return;
    }
    enable("user");
    let harness = start().await;
    let token = token_with_scopes("service_execution_core", &[SCOPE_SCRAPE_INTERACT]);

    let (status, body) = post(&harness, &token, scrape_body(click())).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(
        body["error"]["details"]["reason"], "scrape_interact_scope_required",
        "{body}"
    );
}
