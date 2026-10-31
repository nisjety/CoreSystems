//! INNH-10: the API-key route fence, through the real router and real token verification
//! (the harness is in `support/agent_harness.rs`).
//!
//! What it pins:
//! - with `QUARRY_EDGE__API_KEY_AGENT_RUNS` off (the default) a key token is refused on EVERY
//!   route, and a person's and a service's requests are served exactly as before;
//! - with it on, a key token without `browser:agent` is refused on every route;
//! - with it on and the scope, every route outside the allow-list is a 403
//!   `api_key_route_refused` and every route on it reaches its handler;
//! - the fence does not touch a user or a service token on the same routes;
//! - a key token that is not canonical never authenticates.
//!
//! The switch is read on each request, so every test is `#[serial]` and sets it.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_edge::run_limits::{RunLimits, ENV_API_KEY_AGENT_RUNS};
use reqwest::Method;
use serde_json::{json, Value};
use serial_test::serial;

const RUN: &str = "run_00000000000000000000000000";

fn switch(value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(ENV_API_KEY_AGENT_RUNS, value),
        None => std::env::remove_var(ENV_API_KEY_AGENT_RUNS),
    }
}

/// The reasons the fence itself gives.
fn is_fence_reason(reason: &str) -> bool {
    matches!(
        reason,
        "api_key_agent_runs_disabled" | "api_key_scope_missing" | "api_key_route_refused"
    )
}

fn reason(reply: &Reply) -> String {
    reply.details()["reason"].as_str().unwrap_or("").to_owned()
}

/// The routes on a key's allow-list, with a body where the method takes one.
fn allowed_routes() -> Vec<(Method, String, Option<Value>)> {
    vec![
        (Method::POST, "/v1/agent/runs".to_owned(), Some(json!({}))),
        (Method::DELETE, format!("/v1/agent/runs/{RUN}"), None),
        (Method::POST, format!("/v1/agent/runs/{RUN}/step"), Some(json!({ "action": wait_action() }))),
        (Method::GET, format!("/v1/agent/runs/{RUN}/receipts"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/egress-receipts"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/tabs"), None),
        (Method::POST, format!("/v1/agent/runs/{RUN}/tabs"), Some(json!({}))),
        (Method::POST, format!("/v1/agent/runs/{RUN}/tabs/tab_1/select"), None),
        (Method::DELETE, format!("/v1/agent/runs/{RUN}/tabs/tab_1"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/events/stream"), None),
    ]
}

/// Routes a person or a service can reach that a key cannot.
fn refused_routes() -> Vec<(Method, String, Option<Value>)> {
    vec![
        (Method::POST, format!("/v1/agent/runs/{RUN}/interact"), Some(json!({ "action": wait_action() }))),
        (Method::GET, format!("/v1/agent/runs/{RUN}/browser-session"), None),
        (Method::POST, format!("/v1/agent/runs/{RUN}/browser-session/control"), Some(json!({}))),
        (Method::GET, format!("/v1/agent/runs/{RUN}/browser-session/timeline"), None),
        (Method::POST, format!("/v1/agent/runs/{RUN}/attach"), Some(json!({}))),
        (Method::DELETE, format!("/v1/agent/runs/{RUN}/attach/att_1"), None),
        (Method::POST, format!("/v1/agent/runs/{RUN}/procedure"), Some(json!({}))),
        (Method::POST, "/v1/agent/procedures/replay-check".to_owned(), Some(json!({}))),
        (Method::POST, "/v1/agent/procedures/quality-check".to_owned(), Some(json!({}))),
        (Method::POST, "/v1/agent/procedures/impact-check".to_owned(), Some(json!({}))),
        (Method::POST, format!("/v1/agent/runs/{RUN}/tabs/tab_1/control"), Some(json!({}))),
        (Method::GET, format!("/v1/agent/runs/{RUN}/devtools"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/frames/ws"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/frames/stream"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/frame"), None),
        (Method::GET, format!("/v1/agent/runs/{RUN}/live-view"), None),
        (Method::GET, "/v1/agent/runs".to_owned(), None),
        (Method::PUT, format!("/v1/agent/runs/{RUN}"), Some(json!({}))),
        (Method::GET, format!("/v1/agent/runs/{RUN}/step"), None),
        (Method::GET, "/v1/profiles".to_owned(), None),
        (Method::POST, "/v1/profiles".to_owned(), Some(json!({}))),
        (Method::GET, "/v1/artifacts".to_owned(), None),
        (Method::GET, "/v1/artifacts/art_1".to_owned(), None),
        (Method::POST, "/v1/scrape".to_owned(), Some(json!({ "url": "https://example.com" }))),
        (Method::POST, "/v1/crawl".to_owned(), Some(json!({}))),
        (Method::POST, "/v1/batch".to_owned(), Some(json!({}))),
        (Method::POST, "/v1/search".to_owned(), Some(json!({}))),
        (Method::POST, "/v1/extract".to_owned(), Some(json!({}))),
        (Method::GET, "/v1/jobs/job_1".to_owned(), None),
        (Method::GET, "/v1/team/credit-usage".to_owned(), None),
        (Method::POST, "/graphql".to_owned(), Some(json!({ "query": "{ __typename }" }))),
        (Method::POST, "/v1/schedules".to_owned(), Some(json!({}))),
        (Method::GET, "/v1/change/latest".to_owned(), None),
        (Method::POST, "/v1/fleets".to_owned(), Some(json!({}))),
        (Method::POST, "/v1/agent/runs/".to_owned(), Some(json!({}))),
        (Method::POST, format!("/v1/agent/runs/{RUN}/step/"), Some(json!({}))),
    ]
}

/// The agent, profile and artifact routes of both tables: what a person or a service can call
/// without the edge fetching anything from the network.
fn browser_routes() -> Vec<(Method, String, Option<Value>)> {
    refused_routes()
        .into_iter()
        .chain(allowed_routes())
        .filter(|route| {
            route.1.starts_with("/v1/agent/")
                || route.1.starts_with("/v1/artifacts")
                || route.1 == "/v1/profiles"
        })
        .collect()
}

async fn call(edge: &Edge, bearer: &str, route: &(Method, String, Option<Value>)) -> Reply {
    edge.send(route.0.clone(), bearer, &route.1, route.2.clone()).await
}

/// With the switch off (the default) a key token is refused on every route, the allowed ones
/// too, and nothing is acquired.
#[tokio::test]
#[serial]
async fn switch_off_refuses_a_key_on_every_route() {
    switch(None);
    let edge = Edge::start(RunLimits::default()).await;
    let key = key_token();
    let routes: Vec<_> = allowed_routes().into_iter().chain(refused_routes()).collect();
    assert!(routes.len() >= 40, "the table is the whole surface a test can name");
    for route in &routes {
        let reply = call(&edge, &key, route).await;
        assert_eq!(reply.status, 403, "{} {}: {}", route.0, route.1, reply.body);
        assert_eq!(reply.code(), "FORBIDDEN", "{} {}", route.0, route.1);
        assert_eq!(
            reason(&reply),
            "api_key_agent_runs_disabled",
            "{} {}: {}",
            route.0,
            route.1,
            reply.body
        );
    }
    assert!(edge.driver.acquired().is_empty(), "no browser is acquired for a refused key");

    // An explicit 0 and an unreadable value are off as well.
    for value in ["0", "false", "off", "enabled"] {
        switch(Some(value));
        let reply = edge
            .post(&key, "/v1/agent/runs", json!({ "grant_id": "g1" }))
            .await;
        assert_eq!(reason(&reply), "api_key_agent_runs_disabled", "value {value:?}: {}", reply.body);
    }
    switch(None);
}

/// With the switch off a person and a service are served exactly as they were: the same
/// start, step and close, the same answers. (The existing agent suites, which never set the
/// switch, are the broad proof; this pins the fence layer itself.)
#[tokio::test]
#[serial]
async fn switch_off_leaves_users_and_services_as_they_were() {
    switch(None);
    let edge = Edge::start(RunLimits::default()).await;
    for (label, bearer) in [("person", member_token()), ("service", service_token())] {
        let run = edge.start_run(&bearer, json!({})).await;
        let step = edge.step(&bearer, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "{label}: {}", step.body);
        let _receipts = edge.receipts(&bearer, &run).await;
        let closed = edge.delete(&bearer, &format!("/v1/agent/runs/{run}")).await;
        assert_eq!(closed.status, 200, "{label}: {}", closed.body);
    }
    // The refused-for-keys routes answer a person and a service without a fence reason.
    // (Only the browser routes: a person's scrape or search would fetch from the network.)
    for bearer in [member_token(), service_token()] {
        for route in browser_routes() {
            let reply = call(&edge, &bearer, &route).await;
            assert!(
                !is_fence_reason(&reason(&reply)),
                "{} {}: {}",
                route.0,
                route.1,
                reply.body
            );
        }
    }
}

/// Switch on, but the token lacks `browser:agent`: refused on every route. A key token that
/// holds some other scope (the Model Plane's `browser:execute` included) gets nothing from it.
#[tokio::test]
#[serial]
async fn switch_on_without_the_agent_scope_refuses_every_route() {
    switch(Some("1"));
    let edge = Edge::start(RunLimits::default()).await;
    let tokens = [
        api_key_token("key-1", "user-golden", |claims| {
            claims.insert("scopes".into(), json!([]));
        }),
        api_key_token("key-1", "user-golden", |claims| {
            claims.insert("scopes".into(), json!(["browser:execute", "scrape:read", "search:read"]));
        }),
        api_key_token("key-1", "user-golden", |claims| {
            claims.remove("scopes");
        }),
    ];
    let routes: Vec<_> = allowed_routes().into_iter().chain(refused_routes()).collect();
    for bearer in &tokens {
        for route in &routes {
            let reply = call(&edge, bearer, route).await;
            assert_eq!(reply.status, 403, "{} {}: {}", route.0, route.1, reply.body);
            assert_eq!(reason(&reply), "api_key_scope_missing", "{} {}: {}", route.0, route.1, reply.body);
        }
    }
    assert!(edge.driver.acquired().is_empty());
    switch(None);
}

/// Switch on with the scope: every route off the allow-list is `api_key_route_refused`.
#[tokio::test]
#[serial]
async fn switch_on_with_the_scope_refuses_every_route_off_the_allow_list() {
    switch(Some("1"));
    let edge = Edge::start(RunLimits::default()).await;
    let key = key_token();
    for route in &refused_routes() {
        let reply = call(&edge, &key, route).await;
        assert_eq!(reply.status, 403, "{} {}: {}", route.0, route.1, reply.body);
        assert_eq!(reply.code(), "FORBIDDEN");
        assert_eq!(reason(&reply), "api_key_route_refused", "{} {}: {}", route.0, route.1, reply.body);
    }
    assert!(edge.driver.acquired().is_empty());
    switch(None);
}

/// Switch on with the scope: every route on the allow-list gets past the fence to its handler
/// (an unknown run is a 400 or 404 there, a start with no grant is the key rule's refusal).
#[tokio::test]
#[serial]
async fn switch_on_with_the_scope_lets_the_allow_list_reach_its_handlers() {
    switch(Some("1"));
    let edge = Edge::start(RunLimits::default()).await;
    let key = key_token();
    for route in &allowed_routes() {
        // The event stream of an unknown run is a plain 404; nothing is left open.
        let reply = call(&edge, &key, route).await;
        assert!(
            !is_fence_reason(&reason(&reply)),
            "{} {} was stopped by the fence: {}",
            route.0,
            route.1,
            reply.body
        );
        assert_ne!(reply.status, 401, "{} {}", route.0, route.1);
    }
    // The start reaches the key rule, not the fence.
    let start = edge.post(&key, "/v1/agent/runs", json!({})).await;
    assert_eq!(reason(&start), "api_key_grant_required", "{}", start.body);
    switch(None);
}

/// The fence leaves a user and a service alone with the switch on.
#[tokio::test]
#[serial]
async fn the_fence_does_not_touch_user_or_service_tokens() {
    switch(Some("1"));
    let edge = Edge::start(RunLimits::default()).await;
    for bearer in [member_token(), service_token()] {
        for route in browser_routes() {
            let reply = call(&edge, &bearer, &route).await;
            assert!(
                !is_fence_reason(&reason(&reply)),
                "{} {}: {}",
                route.0,
                route.1,
                reply.body
            );
        }
    }
    switch(None);
}

/// A key token that is not canonical never authenticates (401), switch or not.
#[tokio::test]
#[serial]
async fn a_non_canonical_key_token_is_unauthorized() {
    switch(Some("1"));
    let edge = Edge::start(RunLimits::default()).await;
    let bad: Vec<(&str, String)> = vec![
        ("sub does not match the key", api_key_token("key-1", "user-golden", |c| {
            c.insert("sub".into(), json!("apikey:key-2"));
        })),
        ("no acting user", api_key_token("key-1", "user-golden", |c| {
            c.remove("acting_user_id");
        })),
        ("a user_id beside the key", api_key_token("key-1", "user-golden", |c| {
            c.insert("user_id".into(), json!("user-golden"));
        })),
        ("a service id beside the key", api_key_token("key-1", "user-golden", |c| {
            c.insert("service_id".into(), json!("service:execution-core"));
        })),
        ("no key id", api_key_token("key-1", "user-golden", |c| {
            c.remove("key_id");
        })),
        ("a key id over the bound", {
            let long = "k".repeat(129);
            api_key_token(&long, "user-golden", |_| {})
        }),
        ("a key id on a user token", member_token_with_key_id()),
    ];
    for (label, bearer) in &bad {
        let reply = edge.post(bearer, "/v1/agent/runs", json!({})).await;
        assert_eq!(reply.status, 401, "{label}: {}", reply.body);
    }
    switch(None);
}

fn member_token_with_key_id() -> String {
    // A person's token that also names a key is ambiguous.
    api_key_token("key-1", "user-golden", |c| {
        c.insert("principal_type".into(), json!("user"));
        c.insert("sub".into(), json!("user-golden"));
        c.insert("user_id".into(), json!("user-golden"));
    })
}
