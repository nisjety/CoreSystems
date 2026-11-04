//! The constraints of a person's open-web run, over the real router (WS8.B, D2-141: the
//! HTTP-level human-grant tests were missing). A `human_browse` grant is admitted only while
//! `QUARRY_EDGE__HUMAN_BROWSE` is on; an admitted run takes no host list, step, time or cost
//! bound from the request; the broker's deny list is the run's only host rule beyond "public
//! http(s)"; and a grant of any other kind never opens the web to a person's run.
//!
//! Every test is `#[serial]`: the switch is a process-wide environment variable that each
//! request reads. The pure decisions are unit-tested in `agent_routes/human_browse.rs`. The
//! keep-alive owner gate is not here: the renew route belongs to Model Gateway, and this edge
//! has no keep-alive route of its own. Built and compile-checked, not run (wave 2 rule).

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::collections::HashMap;
use std::sync::Arc;

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::QuarryResult;
use quarry_edge::run_limits::{RunLimits, ENV_HUMAN_BROWSE};
use quarry_runtime::{GrantValidation, GrantValidator};
use serde_json::{json, Value};
use serial_test::serial;

/// The broker, from memory: it knows the grants it was given and nothing else.
struct FakeBroker(HashMap<String, GrantValidation>);

#[async_trait]
impl GrantValidator for FakeBroker {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.0
            .get(grant_id)
            .cloned()
            .ok_or_else(|| QuarryError::new(ErrorCode::NotFound, "unknown browser grant"))
    }
}

/// An edge whose broker knows `grants`, in the production posture: every run needs a grant.
async fn edge_with(grants: Vec<GrantValidation>) -> Edge {
    let broker = FakeBroker(
        grants
            .into_iter()
            .map(|grant| (grant.grant_id.clone(), grant))
            .collect(),
    );
    Edge::start_with(RunLimits::default(), move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state.require_browser_grants = true;
        state
    })
    .await
}

/// Turn the human-browse switch on or off for the requests that follow.
fn switch(on: bool) {
    if on {
        std::env::set_var(ENV_HUMAN_BROWSE, "1");
    } else {
        std::env::remove_var(ENV_HUMAN_BROWSE);
    }
}

/// A grant of `kind` with `policy_mode`, owned by the standard org's member.
fn grant(kind: &str, policy_mode: &str, denied: &[&str]) -> GrantValidation {
    serde_json::from_value(json!({
        "grant_id": "grant-under-test",
        "active": true,
        "kind": kind,
        "owner_id": "user-golden",
        "policy_mode": policy_mode,
        "denied_domains": denied,
    }))
    .expect("a grant validation")
}

/// A person's open-web grant: the `human_browse` kind, open web minus `denied`.
fn human_grant(denied: &[&str]) -> GrantValidation {
    grant("human_browse", "open_web", denied)
}

/// The start body that names the grant only.
fn start_body() -> Value {
    json!({ "grant_id": "grant-under-test" })
}

/// A start that asks for bounds and hosts a person's run must not take.
fn greedy_start_body() -> Value {
    json!({
        "grant_id": "grant-under-test",
        "constraints": {
            "max_steps": 5,
            "allowed_domains": ["example.com"],
            "max_runtime_s": 600,
            "max_cost_usd": 3.0
        }
    })
}

fn navigate(url: &str) -> Value {
    json!({ "type": "navigate", "url": url })
}

/// The words a navigation outside the run's policy is refused with (the gateway maps them).
const OUTSIDE: &str = "outside the run's allowed domains";

/// With the switch off, a person's `human_browse` grant is refused before any browser is
/// acquired, whatever the grant says.
#[tokio::test]
#[serial]
async fn a_human_grant_is_refused_while_the_switch_is_off() {
    switch(false);
    let edge = edge_with(vec![human_grant(&[])]).await;
    let person = member_token();

    let refused = edge.post(&person, "/v1/agent/runs", start_body()).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.message(), "human_browse_disabled", "{}", refused.body);
    assert!(edge.driver.acquired().is_empty(), "no browser for a refused grant");
}

/// A person's open-web run takes no bounds and no hosts from the request: its step, time and
/// cost bounds are zero or absent and its host list is empty, whatever the caller asked for,
/// and the projection names it an open-web run.
#[tokio::test]
#[serial]
async fn a_persons_run_takes_no_bounds_or_hosts_from_the_request() {
    switch(true);
    let edge = edge_with(vec![human_grant(&["bank.example"])]).await;
    let person = member_token();

    let run = edge.start_run(&person, greedy_start_body()).await;
    {
        let handle = edge.live(&run).expect("the run is live");
        let entry = handle.entry.lock().await;
        assert!(entry.constraints.allowed_domains.is_empty());
        assert_eq!(entry.constraints.max_steps, 0);
        assert_eq!(entry.constraints.max_runtime_s, None);
        assert_eq!(entry.constraints.max_cost_usd, None);
    }
    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    assert_eq!(projection.data()["grantKind"], "human_browse", "{}", projection.body);
}

/// A person's open-web run is refused a denied host, and a host under one, whatever the case
/// of the name: the deny list is matched by suffix on the host.
#[tokio::test]
#[serial]
async fn an_open_web_run_refuses_a_denied_host_and_the_hosts_under_it() {
    switch(true);
    let edge = edge_with(vec![human_grant(&["bank.example"])]).await;
    let person = member_token();
    let run = edge.start_run(&person, start_body()).await;

    for url in [
        "https://bank.example/login",
        "https://www.bank.example/accounts",
        "https://BANK.example/",
    ] {
        let refused = edge.step(&person, &run, navigate(url), None).await;
        assert_eq!(refused.status, 403, "{url}: {}", refused.body);
        assert_eq!(refused.code(), "SECURITY_BLOCKED", "{url}: {}", refused.body);
        assert!(refused.message().contains(OUTSIDE), "{url}: {}", refused.body);
    }
}

/// An open-web run is no way into the host's own network: loopback, private, link-local and
/// metadata addresses and single-label names are refused whatever the deny list says. A public
/// address that the deny list does not name is reached.
#[tokio::test]
#[serial]
async fn an_open_web_run_refuses_local_and_metadata_hosts_and_reaches_a_public_one() {
    switch(true);
    let edge = edge_with(vec![human_grant(&[])]).await;
    let person = member_token();
    let run = edge.start_run(&person, start_body()).await;

    for url in [
        "http://127.0.0.1:8080/",
        "http://[::1]/",
        "http://10.0.0.5/",
        "http://169.254.169.254/latest/meta-data/",
        "http://localhost/",
    ] {
        let refused = edge.step(&person, &run, navigate(url), None).await;
        assert_eq!(refused.status, 403, "{url}: {}", refused.body);
        assert_eq!(refused.code(), "SECURITY_BLOCKED", "{url}: {}", refused.body);
        assert!(refused.message().contains(OUTSIDE), "{url}: {}", refused.body);
    }

    // The positive control: a public address with no deny entry is reached. The driver reports
    // that page as where the step landed, so the edge's check of the landed page passes it.
    let public = "https://93.184.216.34/";
    edge.driver.set_page_url(Some(public));
    let reached = edge.step(&person, &run, navigate(public), None).await;
    assert_eq!(reached.status, 200, "{}", reached.body);
}

/// A grant of another kind never opens the web to a person's run, whatever its policy mode
/// says: the run keeps the broker's host list, the projection names no open-web grant, and a
/// step to a public address the list does not name is refused.
#[tokio::test]
#[serial]
async fn a_run_grant_with_an_open_web_mode_does_not_open_the_web() {
    switch(true);
    let run_grant: GrantValidation = serde_json::from_value(json!({
        "grant_id": "grant-under-test",
        "active": true,
        "kind": "run",
        "owner_id": "user-golden",
        "policy_mode": "open_web",
        "allowed_domains": ["example.com"],
        "denied_domains": []
    }))
    .expect("a grant validation");
    let edge = edge_with(vec![run_grant]).await;
    let person = member_token();

    let started = edge.post(&person, "/v1/agent/runs", start_body()).await;
    assert_eq!(started.status, 200, "{}", started.body);
    let run = started.data()["run_id"]
        .as_str()
        .expect("the start answers a run id")
        .to_owned();

    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    assert!(projection.data().get("grantKind").is_none(), "{}", projection.body);

    let refused = edge
        .step(&person, &run, navigate("https://93.184.216.34/"), None)
        .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.code(), "SECURITY_BLOCKED", "{}", refused.body);
}

/// A person's own tab on an open-web run is the person's: the owner's projection names the
/// person as its opener, and the browser's own tab names none.
#[tokio::test]
#[serial]
async fn a_persons_tab_on_an_open_web_run_names_the_person_as_its_opener() {
    switch(true);
    let edge = edge_with(vec![human_grant(&[])]).await;
    let person = member_token();
    let run = edge.start_run(&person, start_body()).await;

    let opened = edge.new_tab(&person, &run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let tab_id = opened.data()["tab"]["tabId"]
        .as_str()
        .expect("the new tab's id")
        .to_owned();

    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let tabs = projection.data()["tabs"]
        .as_array()
        .expect("the projection lists the tabs")
        .clone();
    let opener = |id: &str| {
        tabs.iter()
            .find(|tab| tab["tabId"] == id)
            .map(|tab| tab["openedBy"].clone())
    };
    assert_eq!(opener(&tab_id), Some(json!({ "initiatedBy": "human" })), "{tabs:?}");
    assert_eq!(
        opener("tab_home"),
        Some(Value::Null),
        "the browser's own tab names no opener"
    );
}
