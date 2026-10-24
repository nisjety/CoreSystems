//! WS6.G: plan-aware run caps, through the real router and real token
//! verification (the harness is `support/agent_harness.rs`).
//!
//! A stub limits resolver stands in for billing-core (only the static source is
//! built, and it reports no plan caps). The tests pin what a caller is told and
//! what the registry admits: an organization whose plan allows one agent run
//! gets a typed 429 `org_plan_cap` with `Retry-After` on the second, a
//! neighbouring organization is untouched, a plan can raise an organization
//! above the static per-organization cap but never above the global one, and a
//! person's session never fails because the limits source is down.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::collections::HashMap;
use std::sync::Arc;

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::error::QuarryError;
use quarry_edge::limits::{
    Allowance, BackendError, FailMode, LimitsBackend, LimitsOrigin, LimitsResolver, OrgLimits,
    PlanCaps, WithFailMode,
};
use quarry_edge::run_limits::RunLimits;
use quarry_edge::state::AppState;
use serde_json::json;

/// The organization of `service_token()` and `member_token()`.
const ORG_A: &str = "org-standard-golden";

/// A resolver that answers by organization id; an organization it does not know
/// gets the floor.
struct PerOrg(HashMap<String, OrgLimits>);

#[async_trait]
impl LimitsResolver for PerOrg {
    async fn org_limits(&self, org_id: &str) -> Result<OrgLimits, QuarryError> {
        Ok(self
            .0
            .get(org_id)
            .cloned()
            .unwrap_or_else(OrgLimits::floor))
    }
}

/// A source that is always down.
struct Down;

#[async_trait]
impl LimitsBackend for Down {
    async fn read(&self, _org_id: &str) -> Result<OrgLimits, BackendError> {
        Err(BackendError("unreachable"))
    }
}

/// An entitlement carrying only the plan's two run caps.
fn plan(agent_runs: Option<u32>, interactive_runs: Option<u32>) -> OrgLimits {
    OrgLimits {
        origin: LimitsOrigin::Entitlement,
        crawl_pages: Allowance::Unconfigured,
        batch_urls: Allowance::Unconfigured,
        browser_seconds_agent: Allowance::Unconfigured,
        caps: PlanCaps {
            agent_runs,
            interactive_runs,
            ..PlanCaps::default()
        },
    }
}

fn with_limits(resolver: Arc<dyn LimitsResolver>) -> impl FnOnce(AppState) -> AppState {
    move |state| AppState {
        limits: resolver,
        ..state
    }
}

fn per_org(entries: &[(&str, OrgLimits)]) -> Arc<dyn LimitsResolver> {
    Arc::new(PerOrg(
        entries
            .iter()
            .map(|(org, limits)| ((*org).to_owned(), limits.clone()))
            .collect(),
    ))
}

/// Caps off, so only the plan (and the global cap a test sets) decides.
fn open_limits() -> RunLimits {
    RunLimits {
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    }
}

/// A capacity refusal: 429, the closed reason, the numbers, and the wait both in
/// the body and as a `Retry-After` header.
fn assert_refusal(reply: &Reply, reason: &str, scope: &str, limit: u64, active: u64) {
    assert_eq!(reply.status, 429, "{}", reply.body);
    assert_eq!(reply.code(), "RATE_LIMITED", "{}", reply.body);
    assert_eq!(reply.details()["reason"], reason, "{}", reply.body);
    assert_eq!(reply.details()["scope"], scope, "{}", reply.body);
    assert_eq!(reply.details()["limit"], limit, "{}", reply.body);
    assert_eq!(reply.details()["active"], active, "{}", reply.body);
    assert_eq!(reply.details()["retry_after_seconds"], 10, "{}", reply.body);
    assert_eq!(
        reply
            .headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("10"),
        "the wait is also a Retry-After header"
    );
}

/// Organization A's plan allows one agent run: the second concurrent start is a
/// typed 429 naming the plan, and organization B starts regardless.
#[tokio::test]
async fn org_at_its_plan_cap_is_refused_and_a_neighbour_is_not() {
    let edge = Edge::start_with(
        open_limits(),
        with_limits(per_org(&[(ORG_A, plan(Some(1), None))])),
    )
    .await;

    let first = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(first.status, 200, "{}", first.body);

    let second = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_refusal(&second, "org_plan_cap", "org", 1, 1);

    // The refused start admitted nothing and left no pending slot behind.
    assert_eq!(edge.driver.acquired().len(), 1);
    let stats = edge.state.agent_runs.stats();
    assert_eq!((stats.live, stats.pending), (1, 0));

    // Organization B has no plan bound and is unaffected.
    let neighbour = edge
        .post(&stranger_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(neighbour.status, 200, "{}", neighbour.body);
}

/// The agent bound counts agent runs: a person's own session in the same
/// organization neither counts against it nor is refused by it.
#[tokio::test]
async fn the_agent_plan_cap_does_not_count_or_block_a_persons_session() {
    let edge = Edge::start_with(
        open_limits(),
        with_limits(per_org(&[(ORG_A, plan(Some(1), None))])),
    )
    .await;

    let person = edge
        .post(&member_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(person.status, 200, "{}", person.body);
    let agent = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(agent.status, 200, "the person's session is not an agent run: {}", agent.body);
    let over = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_refusal(&over, "org_plan_cap", "org", 1, 1);
}

/// A closed run gives the plan capacity back.
#[tokio::test]
async fn a_closed_run_frees_plan_capacity() {
    let edge = Edge::start_with(
        open_limits(),
        with_limits(per_org(&[(ORG_A, plan(Some(1), None))])),
    )
    .await;
    let run = edge.start_run(&service_token(), json!({})).await;
    let full = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_refusal(&full, "org_plan_cap", "org", 1, 1);

    let closed = edge
        .delete(&service_token(), &format!("/v1/agent/runs/{run}"))
        .await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    let again = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
}

/// A plan that says three raises an organization above the static cap of one
/// and admits three; a fourth is refused by the global cap of three, not by the
/// plan (the plan never lifts the process-wide cap).
#[tokio::test]
async fn a_plan_raises_the_org_cap_but_the_global_cap_still_holds() {
    let limits = RunLimits {
        max_concurrent_runs: 3,
        max_runs_per_org: 1,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start_with(limits, with_limits(per_org(&[(ORG_A, plan(Some(3), None))]))).await;

    for _ in 0..3 {
        let reply = edge
            .post(&service_token(), "/v1/agent/runs", json!({}))
            .await;
        assert_eq!(reply.status, 200, "{}", reply.body);
    }
    let fourth = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_refusal(&fourth, "global_run_cap", "global", 3, 3);
    assert_eq!(edge.state.agent_runs.stats().live, 3);
}

/// Without a plan bound (the static source, or a plan that says nothing) the
/// static per-organization cap answers as it always did.
#[tokio::test]
async fn without_a_plan_bound_the_static_org_cap_answers() {
    let limits = RunLimits {
        max_concurrent_runs: 0,
        max_runs_per_org: 1,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start_with(limits, with_limits(per_org(&[(ORG_A, plan(None, None))]))).await;
    let first = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let second = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_refusal(&second, "org_run_cap", "org", 1, 1);
}

/// The plan's interactive key bounds a person's own sessions the same way.
#[tokio::test]
async fn the_interactive_plan_cap_bounds_a_persons_sessions() {
    let edge = Edge::start_with(
        open_limits(),
        with_limits(per_org(&[(ORG_A, plan(None, Some(1)))])),
    )
    .await;
    let first = edge
        .post(&member_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let second = edge
        .post(&member_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_refusal(&second, "org_plan_cap", "org", 1, 1);
}

/// With fail mode `closed` and the source down, an agent run is refused but a
/// person's session is not: it falls back to the floor.
#[tokio::test]
async fn a_person_is_not_refused_when_the_limits_source_is_down() {
    let resolver: Arc<dyn LimitsResolver> = Arc::new(WithFailMode::new(Down, FailMode::Closed));
    let edge = Edge::start_with(open_limits(), with_limits(resolver)).await;

    let person = edge
        .post(&member_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(person.status, 200, "{}", person.body);

    let agent = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_ne!(agent.status, 200, "{}", agent.body);
}
