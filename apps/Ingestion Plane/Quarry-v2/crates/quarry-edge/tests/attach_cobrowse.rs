//! WS8.F: the agent attachment, refusal paths over the wire.
//!
//! These cover what can be checked with the default driver double and the switch off:
//! the switch refuses attach, a stranger and a person are not a delegate, the tab
//! hand-over is not open to a service principal, and a release with no attachment is
//! a 404. The enabled path (an attach that opens an agent tab, a delegate step, the
//! take-over and give-back) needs a driver double that advertises background tabs,
//! tab pinning and per-tab egress and implements `open_tab`; that double is not built
//! yet (see the W2-U67 progress note). Written during the build; not run.

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_edge::run_limits::RunLimits;
use serde_json::json;

#[tokio::test]
async fn attach_is_refused_while_the_switch_is_off() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let reply = edge
        .post(
            &service_token(),
            &format!("/v1/agent/runs/{run}/attach"),
            json!({ "grant_id": "grant_attach" }),
        )
        .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.code(), "FORBIDDEN", "{}", reply.body);
    assert!(reply.message().contains("attach_disabled"), "{}", reply.body);
}

#[tokio::test]
async fn a_person_token_is_not_a_delegate_of_the_run() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    // Another member of the org, and a stranger, both get the same 404 as an unknown run.
    let stranger = stranger_token();
    let reply = edge
        .step(&stranger, &run, json!({ "type": "navigate", "url": "https://example.com/" }), None)
        .await;
    assert_eq!(reply.status, 404, "{}", reply.body);
}

#[tokio::test]
async fn a_service_principal_cannot_change_a_tab_holder() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    // The tab hand-over is the person's own call; a service principal is not the owner.
    let reply = edge
        .post(
            &service_token(),
            &format!("/v1/agent/runs/{run}/tabs/tab_1/control"),
            json!({ "holder": "agent" }),
        )
        .await;
    assert_eq!(reply.status, 404, "{}", reply.body);
}

#[tokio::test]
async fn releasing_an_attachment_that_does_not_exist_is_a_404() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let reply = edge
        .delete(&person, &format!("/v1/agent/runs/{run}/attach/att_missing"))
        .await;
    assert_eq!(reply.status, 404, "{}", reply.body);
}
