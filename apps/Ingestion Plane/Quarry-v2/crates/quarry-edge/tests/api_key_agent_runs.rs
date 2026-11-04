//! INNH-10: what an API key's agent run does, through the real router and real token
//! verification, with a fake BrowserBroker (`FakeBroker`) in place of the Model Plane.
//!
//! The switch `QUARRY_EDGE__API_KEY_AGENT_RUNS` is on for this whole binary and run budgets are
//! `on` (both set before anything reads them); the opposite (switch off) is in
//! `api_key_fence.rs`. Rules pinned here, each with its negative case:
//!
//! - a key run needs a grant in every environment, issued to THIS key and its owner;
//! - the grant must be an allow-list session grant with hosts and a budget; open-web,
//!   human-browse, run, sensitive and derived grants are refused;
//! - the grant's hosts replace the request's, whatever the environment;
//! - no resume, no profile; a key's token raises ZDR, never lowers it;
//! - the run is an agent run, the grant's budget binds it whatever the claim switch says,
//!   and the run's receipts name the key;
//! - the run is scoped to the key: its owner's session cannot step it and it cannot step the
//!   owner's;
//! - every step revalidates the grant: revoked, or another key's, ends the run;
//! - no sensitive action, and no broker call is made for one;
//! - at most `MAX_CONCURRENT_RUNS_PER_KEY` live runs per key;
//! - an event stream a key opened ends when its token expires.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::QuarryResult;
use quarry_edge::budget::RunClass;
use quarry_edge::run_limits::{RunLimits, ENV_API_KEY_AGENT_RUNS};
use quarry_runtime::{GrantValidation, GrantValidator};
use serde_json::{json, Value};

/// The broker, from memory. A test can change what it answers between calls (a revocation, a
/// swapped grant), and the ids it was asked about are recorded.
#[derive(Clone, Default)]
struct FakeBroker {
    grants: Arc<Mutex<HashMap<String, GrantValidation>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl FakeBroker {
    fn set(&self, grant: GrantValidation) {
        self.grants.lock().unwrap().insert(grant.grant_id.clone(), grant);
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl GrantValidator for FakeBroker {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.asked.lock().unwrap().push(grant_id.to_owned());
        self.grants
            .lock()
            .unwrap()
            .get(grant_id)
            .cloned()
            .ok_or_else(|| QuarryError::new(ErrorCode::NotFound, "unknown browser grant"))
    }
}

/// The switch on, run budgets on (process-wide, read once), the claim switch at its default
/// (`ignore`): a key run must be bound by its grant's budget regardless.
fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var(ENV_API_KEY_AGENT_RUNS, "1");
        std::env::set_var("QUARRY_EDGE__BUDGET_ENFORCEMENT", "on");
        std::env::remove_var("QUARRY_EDGE__GRANT_BUDGET_CLAIM");
    });
}

/// A grant the broker would issue to key `key-1` (owner `user-golden`): an allow-list session
/// grant on `example.com` with a budget. `tweak` edits the wire fields.
fn key_grant(id: &str, tweak: impl FnOnce(&mut serde_json::Map<String, Value>)) -> GrantValidation {
    let mut fields = json!({
        "grant_id": id,
        "active": true,
        "kind": "session",
        "owner_id": "user-golden",
        "key_id": "key-1",
        "allowed_domains": ["example.com"],
        "budget": { "micro_usd": 1_000_000, "max_steps": 5, "max_runtime_s": 600,
                    "source": "request", "minted_at_unix": 1_760_000_000 },
    })
    .as_object()
    .cloned()
    .expect("an object");
    tweak(&mut fields);
    serde_json::from_value(Value::Object(fields)).expect("a grant validation")
}

fn plain_grant(id: &str) -> GrantValidation {
    key_grant(id, |_| {})
}

/// An edge whose broker knows `broker`'s grants; `require` is the edge's
/// `require_browser_grants` (the production posture). The page the driver double reports is
/// on `example.com`, so a step lands inside the grant's host list.
async fn edge_with(broker: FakeBroker, require: bool, limits: RunLimits) -> Edge {
    setup();
    let edge = Edge::start_with(limits, move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state.require_browser_grants = require;
        state
    })
    .await;
    edge.driver.set_page_url(Some("https://example.com/"));
    edge
}

fn no_caps() -> RunLimits {
    RunLimits {
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    }
}

fn broker_of(grants: Vec<GrantValidation>) -> FakeBroker {
    let broker = FakeBroker::default();
    for grant in grants {
        broker.set(grant);
    }
    broker
}

fn reason(reply: &Reply) -> String {
    reply.details()["reason"].as_str().unwrap_or("").to_owned()
}

fn start(grant_id: &str) -> Value {
    json!({ "grant_id": grant_id })
}

fn run_of(reply: &Reply) -> String {
    reply.data()["run_id"].as_str().expect("a run id").to_owned()
}

/// A key start without a grant is refused even where grants are not required; a person's
/// start without one is served as before.
#[tokio::test]
async fn a_key_run_needs_a_grant_in_every_environment() {
    for require in [false, true] {
        let edge = edge_with(FakeBroker::default(), require, no_caps()).await;
        let refused = edge.post(&key_token(), "/v1/agent/runs", json!({})).await;
        assert_eq!(refused.status, 403, "require={require}: {}", refused.body);
        assert_eq!(reason(&refused), "api_key_grant_required", "require={require}");
        let blank = edge.post(&key_token(), "/v1/agent/runs", start("  ")).await;
        assert_eq!(reason(&blank), "api_key_grant_required", "require={require}");
        assert!(edge.driver.acquired().is_empty(), "require={require}");
    }
    // Where grants are optional a person still starts with none: nothing changed for them.
    let edge = edge_with(FakeBroker::default(), false, no_caps()).await;
    assert_eq!(edge.post(&member_token(), "/v1/agent/runs", json!({})).await.status, 200);
}

/// A grant that is not this key's is refused in every direction, and no browser is acquired.
#[tokio::test]
async fn a_grant_that_is_not_this_keys_is_refused_both_ways() {
    for require in [false, true] {
        let broker = broker_of(vec![
            plain_grant("g-mine"),
            // Another key of the same owner.
            key_grant("g-key2", |f| {
                f.insert("key_id".into(), json!("key-2"));
            }),
            // A person's grant: the owner, no key.
            key_grant("g-person", |f| {
                f.remove("key_id");
            }),
            // Another owner's grant for a key of the same id (a swapped owner).
            key_grant("g-owner", |f| {
                f.insert("owner_id".into(), json!("user-other"));
            }),
        ]);
        let edge = edge_with(broker, require, no_caps()).await;
        for grant in ["g-key2", "g-person", "g-owner"] {
            let refused = edge.post(&key_token(), "/v1/agent/runs", start(grant)).await;
            assert_eq!(refused.status, 403, "{grant} require={require}: {}", refused.body);
            assert_eq!(
                reason(&refused),
                "api_key_grant_owner_mismatch",
                "{grant} require={require}: {}",
                refused.body
            );
        }
        // An unknown grant is the broker's not-found, not a pass.
        let unknown = edge.post(&key_token(), "/v1/agent/runs", start("g-none")).await;
        assert!(unknown.status >= 400, "{}", unknown.body);
        assert!(edge.driver.acquired().is_empty(), "require={require}");

        // A key grant backs only its key's runs: not another key's token (same owner),
        // not the owner's session, not another person, not a service.
        for (label, bearer) in [
            ("the other key", other_key_token()),
            ("the owner", member_token()),
            ("another person", other_member_token()),
            ("a service", service_token()),
        ] {
            let reply = edge.post(&bearer, "/v1/agent/runs", start("g-mine")).await;
            assert_eq!(reply.status, 403, "{label} require={require}: {}", reply.body);
            assert_eq!(
                reason(&reply),
                "api_key_grant_owner_mismatch",
                "{label} require={require}: {}",
                reply.body
            );
        }
        assert!(edge.driver.acquired().is_empty(), "require={require}");

        // The key's own start is served.
        let own = edge.post(&key_token(), "/v1/agent/runs", start("g-mine")).await;
        assert_eq!(own.status, 200, "require={require}: {}", own.body);
    }
}

/// Only a plain allow-list session grant with hosts and a budget backs a key.
#[tokio::test]
async fn only_an_allowlist_session_grant_with_hosts_and_a_budget_backs_a_key() {
    let broker = broker_of(vec![
        key_grant("g-human", |f| {
            f.insert("kind".into(), json!("human_browse"));
        }),
        key_grant("g-open", |f| {
            f.insert("policy_mode".into(), json!("open_web"));
            f.insert("allowed_domains".into(), json!([]));
            f.insert("denied_domains".into(), json!(["169.254.169.254"]));
        }),
        key_grant("g-run", |f| {
            f.insert("kind".into(), json!("run"));
        }),
        key_grant("g-approval", |f| {
            f.insert("kind".into(), json!("sensitive_approval"));
        }),
        key_grant("g-nobudget", |f| {
            f.remove("budget");
        }),
        key_grant("g-nohosts", |f| {
            f.insert("allowed_domains".into(), json!([]));
        }),
        key_grant("g-uncanonical", |f| {
            f.insert("allowed_domains".into(), json!(["b.example.com", "a.example.com"]));
        }),
        key_grant("g-inactive", |f| {
            f.insert("active".into(), json!(false));
        }),
        plain_grant("g-ok"),
    ]);
    let edge = edge_with(broker, false, no_caps()).await;
    let cases = [
        ("g-human", "api_key_open_web_refused"),
        ("g-open", "api_key_open_web_refused"),
        ("g-run", "api_key_grant_kind_refused"),
        ("g-approval", "api_key_grant_kind_refused"),
        ("g-nobudget", "api_key_grant_budget_required"),
        ("g-nohosts", "api_key_grant_hosts_required"),
        ("g-uncanonical", "api_key_grant_hosts_required"),
    ];
    for (grant, expected) in cases {
        let refused = edge.post(&key_token(), "/v1/agent/runs", start(grant)).await;
        assert_eq!(refused.status, 403, "{grant}: {}", refused.body);
        assert_eq!(reason(&refused), expected, "{grant}: {}", refused.body);
    }
    // An inactive grant is the ordinary 403 (no key reason: it is not the key's rule).
    let inactive = edge.post(&key_token(), "/v1/agent/runs", start("g-inactive")).await;
    assert_eq!(inactive.status, 403, "{}", inactive.body);
    assert_eq!(inactive.message(), "browser grant is inactive or expired");
    assert!(edge.driver.acquired().is_empty());
    assert_eq!(edge.post(&key_token(), "/v1/agent/runs", start("g-ok")).await.status, 200);
}

/// The grant's hosts are the run's hosts, in every environment, whatever the body says.
#[tokio::test]
async fn the_grants_hosts_replace_the_bodys_in_every_environment() {
    for require in [false, true] {
        let edge = edge_with(broker_of(vec![plain_grant("g1")]), require, no_caps()).await;
        let reply = edge
            .post(
                &key_token(),
                "/v1/agent/runs",
                json!({
                    "grant_id": "g1",
                    "constraints": { "max_steps": 0, "allowed_domains": ["evil.example", "example.com"] },
                }),
            )
            .await;
        assert_eq!(reply.status, 200, "require={require}: {}", reply.body);
        let handle = edge.live(&run_of(&reply)).expect("live");
        let entry = handle.entry.lock().await;
        assert_eq!(entry.constraints.allowed_domains, vec!["example.com".to_owned()], "require={require}");
        assert_eq!(entry.key_id.as_deref(), Some("key-1"));
        assert_eq!(entry.actor_id, "apikey:key-1");
        assert_eq!(entry.grant_id.as_deref(), Some("g1"));
        drop(entry);

        // A step that leaves the grant's hosts is refused, before any navigation.
        let run = run_of(&reply);
        let off = edge
            .step(&key_token(), &run, json!({ "type": "navigate", "url": "https://evil.example/" }), None)
            .await;
        assert_eq!(off.status, 403, "{}", off.body);
        assert_eq!(off.code(), "SECURITY_BLOCKED");
    }
}

/// A key run keeps no profile and cannot be resumed.
#[tokio::test]
async fn a_key_run_takes_no_profile_and_no_resume() {
    let edge = edge_with(broker_of(vec![plain_grant("g1")]), false, no_caps()).await;
    let bodies = [
        (json!({ "grant_id": "g1", "profile_id": "prof_01J0000000000000000000000" }), "api_key_profile_refused"),
        (json!({ "grant_id": "g1", "persist_profile": true }), "api_key_profile_refused"),
        (json!({ "grant_id": "g1", "resume_run_id": "run_01J0000000000000000000000" }), "api_key_resume_refused"),
    ];
    for (body, expected) in bodies {
        let refused = edge.post(&key_token(), "/v1/agent/runs", body).await;
        assert_eq!(refused.status, 403, "{}", refused.body);
        assert_eq!(reason(&refused), expected, "{}", refused.body);
    }
    assert!(edge.driver.acquired().is_empty());
}

/// ZDR: a key token of a ZDR org makes a ZDR run whatever the body or the grant say, and a
/// ZDR grant cannot back a run that is not ZDR.
#[tokio::test]
async fn a_zdr_key_token_makes_a_zdr_run_and_a_zdr_grant_needs_one() {
    let broker = broker_of(vec![
        plain_grant("g-plain"),
        key_grant("g-zdr", |f| {
            f.insert("zdr".into(), json!(true));
        }),
    ]);
    let edge = edge_with(broker, false, no_caps()).await;

    // The token's posture raises the run.
    let zdr_key = api_key_token("key-1", "user-golden", |c| {
        c.insert("zdr".into(), json!(true));
    });
    let reply = edge.post(&zdr_key, "/v1/agent/runs", start("g-plain")).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let handle = edge.live(&run_of(&reply)).expect("live");
    assert!(handle.entry.lock().await.zdr.is_active(), "a ZDR key token cannot start a non-ZDR run");

    // A ZDR grant needs a ZDR run: a non-ZDR key token and body is refused, with `zdr: true` it is served.
    let refused = edge.post(&key_token(), "/v1/agent/runs", start("g-zdr")).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.message(), "a ZDR browser grant cannot back a non-ZDR run");
    let raised = edge
        .post(&key_token(), "/v1/agent/runs", json!({ "grant_id": "g-zdr", "zdr": true }))
        .await;
    assert_eq!(raised.status, 200, "{}", raised.body);
    let handle = edge.live(&run_of(&raised)).expect("live");
    assert!(handle.entry.lock().await.zdr.is_active());
}

/// A key run is an agent run (the org's agent caps count it), and a person's cap is not its cap.
#[tokio::test]
async fn a_key_run_is_an_agent_run_and_counts_against_the_org_cap() {
    let broker = broker_of(vec![
        plain_grant("g1"),
        key_grant("g2", |f| {
            f.insert("key_id".into(), json!("key-2"));
        }),
    ]);
    let limits = RunLimits {
        max_runs_per_org: 1,
        max_interactive_runs_per_org: 0,
        max_concurrent_runs: 0,
        ..RunLimits::default()
    };
    let edge = edge_with(broker, false, limits).await;
    let first = edge.post(&key_token(), "/v1/agent/runs", start("g1")).await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.data()["constraints"]["class"], "agent", "{}", first.body);
    let handle = edge.live(&run_of(&first)).expect("live");
    assert_eq!(handle.live.class(), RunClass::Agent);
    // The organization is full: another key hears the organization's cap, not the key's.
    let second = edge.post(&other_key_token(), "/v1/agent/runs", start("g2")).await;
    assert_eq!(second.status, 429, "{}", second.body);
    assert_eq!(reason(&second), "org_run_cap", "{}", second.body);
}

/// The grant's budget binds a key run although `QUARRY_EDGE__GRANT_BUDGET_CLAIM` is `ignore`.
#[tokio::test]
async fn the_grants_budget_binds_a_key_run_whatever_the_claim_switch_says() {
    let broker = broker_of(vec![key_grant("g3", |f| {
        f.insert(
            "budget".into(),
            json!({ "max_steps": 3, "source": "request", "minted_at_unix": 1_760_000_000 }),
        );
    })]);
    let edge = edge_with(broker, false, no_caps()).await;
    let key = key_token();
    let started = edge
        .post(
            &key,
            "/v1/agent/runs",
            json!({ "grant_id": "g3", "constraints": { "max_steps": 50 } }),
        )
        .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert_eq!(started.data()["constraints"]["max_steps"], 3, "{}", started.body);
    let run = run_of(&started);
    for n in 1..=3 {
        let step = edge.step(&key, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "step {n}: {}", step.body);
    }
    let refused = edge.step(&key, &run, wait_action(), None).await;
    assert_eq!(refused.status, 402, "{}", refused.body);
    assert_eq!(refused.code(), "BUDGET_EXHAUSTED");
    assert_eq!(refused.details()["limit_source"], "grant", "{}", refused.body);
}

/// A step is an agent's whatever the caller sends, and its receipt names the key.
#[tokio::test]
async fn receipts_name_the_key_and_a_hint_cannot_make_it_a_person() {
    let edge = edge_with(broker_of(vec![plain_grant("g1")]), false, no_caps()).await;
    let key = key_token();
    let run = run_of(&edge.post(&key, "/v1/agent/runs", start("g1")).await);
    for hint in [None, Some("human"), Some("agent")] {
        let step = edge.step(&key, &run, wait_action(), hint).await;
        assert_eq!(step.status, 200, "hint {hint:?}: {}", step.body);
    }
    let receipts = edge.receipts(&key, &run).await;
    assert_eq!(receipts.len(), 3, "{receipts:?}");
    for receipt in &receipts {
        assert_eq!(receipt["initiated_by"], "agent", "{receipt}");
        assert_eq!(receipt["agent"]["basis"], "api_key", "{receipt}");
        assert_eq!(receipt["agent"]["principalId"], "apikey:key-1", "{receipt}");
        assert_eq!(receipt["actor_id"], "apikey:key-1", "{receipt}");
    }
    // The owner's person is not on the receipt at all.
    assert!(!serde_json::to_string(&receipts).unwrap().contains("user-golden"));
}

/// A run belongs to the key that started it: its owner's session cannot step, read or close
/// it, another key of the same owner cannot either, and the key cannot touch the owner's run.
#[tokio::test]
async fn a_key_run_is_scoped_to_the_key_in_both_directions() {
    let broker = broker_of(vec![
        plain_grant("g1"),
        key_grant("g2", |f| {
            f.insert("key_id".into(), json!("key-2"));
        }),
    ]);
    let edge = edge_with(broker, false, no_caps()).await;
    let key_run = run_of(&edge.post(&key_token(), "/v1/agent/runs", start("g1")).await);
    // The owner's own session runs a run of its own.
    let person_run = edge.start_run(&member_token(), json!({})).await;

    for (label, bearer) in [
        ("the owner's session", member_token()),
        ("another key of the owner", other_key_token()),
        ("another person", other_member_token()),
    ] {
        let step = edge.step(&bearer, &key_run, wait_action(), None).await;
        assert_eq!(step.status, 404, "{label} stepping the key's run: {}", step.body);
    }
    let read = edge.get(&member_token(), &format!("/v1/agent/runs/{key_run}/tabs")).await;
    assert_eq!(read.status, 404, "{}", read.body);
    let close = edge.delete(&member_token(), &format!("/v1/agent/runs/{key_run}")).await;
    assert_eq!(close.status, 404, "{}", close.body);
    let other_close = edge.delete(&other_key_token(), &format!("/v1/agent/runs/{key_run}")).await;
    assert_eq!(other_close.status, 404, "{}", other_close.body);

    // And the key cannot step, read or close a person's run.
    let step = edge.step(&key_token(), &person_run, wait_action(), None).await;
    assert_eq!(step.status, 404, "{}", step.body);
    let read = edge.get(&key_token(), &format!("/v1/agent/runs/{person_run}/receipts")).await;
    assert_eq!(read.status, 404, "{}", read.body);
    let close = edge.delete(&key_token(), &format!("/v1/agent/runs/{person_run}")).await;
    assert_eq!(close.status, 404, "{}", close.body);

    // The key's own run still works, and the owner's too.
    assert_eq!(edge.step(&key_token(), &key_run, wait_action(), None).await.status, 200);
    assert_eq!(edge.step(&member_token(), &person_run, wait_action(), None).await.status, 200);
}

/// Every step and every tab opens revalidate the grant: a revocation ends the run, and so does
/// a grant that now answers for another key.
#[tokio::test]
async fn a_revoked_or_swapped_grant_stops_the_next_step() {
    let broker = broker_of(vec![plain_grant("g1"), plain_grant("g2")]);
    let edge = edge_with(broker.clone(), false, no_caps()).await;
    let key = key_token();
    let revoked = run_of(&edge.post(&key, "/v1/agent/runs", start("g1")).await);
    let swapped = run_of(&edge.post(&key, "/v1/agent/runs", start("g2")).await);
    assert_eq!(edge.step(&key, &revoked, wait_action(), None).await.status, 200);

    // Revoked at the broker.
    broker.set(key_grant("g1", |f| {
        f.insert("active".into(), json!(false));
    }));
    let step = edge.step(&key, &revoked, wait_action(), None).await;
    assert_eq!(step.status, 403, "{}", step.body);
    assert_eq!(step.message(), "browser grant is inactive or expired");
    let tab = edge.new_tab(&key, &revoked, None).await;
    assert_eq!(tab.status, 403, "{}", tab.body);

    // The broker now says the grant belongs to another key.
    broker.set(key_grant("g2", |f| {
        f.insert("key_id".into(), json!("key-2"));
    }));
    let step = edge.step(&key, &swapped, wait_action(), None).await;
    assert_eq!(step.status, 403, "{}", step.body);
    assert_eq!(step.message(), "browser grant no longer belongs to this key");
    // ... or to none.
    broker.set(key_grant("g2", |f| {
        f.remove("key_id");
    }));
    let step = edge.step(&key, &swapped, wait_action(), None).await;
    assert_eq!(step.status, 403, "{}", step.body);
    assert_eq!(step.message(), "browser grant no longer belongs to this key");

    // Or its hosts widened under the run: the run's list no longer matches (even with grants not required).
    broker.set(plain_grant("g2"));
    assert_eq!(edge.step(&key, &swapped, wait_action(), None).await.status, 200);
    broker.set(key_grant("g2", |f| {
        f.insert("allowed_domains".into(), json!(["evil.example", "example.com"]));
    }));
    let step = edge.step(&key, &swapped, wait_action(), None).await;
    assert_eq!(step.status, 403, "{}", step.body);
    assert_eq!(step.message(), "browser run domain policy no longer matches its broker grant");
}

/// A key takes no sensitive action, and the broker is never asked about an approval.
#[tokio::test]
async fn a_key_run_refuses_every_sensitive_action_before_the_broker() {
    let broker = broker_of(vec![plain_grant("g1"), plain_grant("approval-1")]);
    let edge = edge_with(broker.clone(), false, no_caps()).await;
    let key = key_token();
    let run = run_of(&edge.post(&key, "/v1/agent/runs", start("g1")).await);
    let sensitive = [
        json!({ "type": "respond_dialog", "dialog_id": "d1", "accept": true, "approval_grant_id": "approval-1" }),
        json!({ "type": "upload_ref", "snapshot_id": "s1", "generation": 1, "ref_id": "r1",
                "artifact_id": "art_01J00000000000000000000000", "approval_grant_id": "approval-1" }),
        json!({ "type": "download_ref", "snapshot_id": "s1", "generation": 1, "ref_id": "r1",
                "approval_grant_id": "approval-1" }),
    ];
    for action in sensitive {
        let reply = edge.step(&key, &run, action.clone(), None).await;
        assert_eq!(reply.status, 403, "{action}: {}", reply.body);
        assert_eq!(reason(&reply), "api_key_sensitive_action_refused", "{action}: {}", reply.body);
    }
    assert!(
        !broker.asked().iter().any(|id| id == "approval-1"),
        "the broker was asked about an approval grant: {:?}",
        broker.asked()
    );
    // The run is not harmed: an ordinary step still works.
    assert_eq!(edge.step(&key, &run, wait_action(), None).await.status, 200);
}

/// At most two live runs per key; another key of the same organization is not counted; closing
/// a run frees its slot; the refusal says so and when to retry.
#[tokio::test]
async fn a_key_has_a_cap_on_its_live_runs() {
    let broker = broker_of(vec![
        plain_grant("g1"),
        key_grant("g2", |f| {
            f.insert("key_id".into(), json!("key-2"));
        }),
    ]);
    let edge = edge_with(broker, false, no_caps()).await;
    let key = key_token();
    let first = run_of(&edge.post(&key, "/v1/agent/runs", start("g1")).await);
    let _second = run_of(&edge.post(&key, "/v1/agent/runs", start("g1")).await);
    let third = edge.post(&key, "/v1/agent/runs", start("g1")).await;
    assert_eq!(third.status, 429, "{}", third.body);
    assert_eq!(third.code(), "RATE_LIMITED");
    assert_eq!(reason(&third), "per_key_run_cap", "{}", third.body);
    assert_eq!(third.details()["scope"], "key");
    assert_eq!(third.details()["limit"], 2);
    assert_eq!(third.details()["active"], 2);
    assert!(third.headers.get("retry-after").is_some(), "{:?}", third.headers);
    assert_eq!(edge.driver.acquired().len(), 2, "the refused start acquired nothing");

    // Another key of the same organization, and a person, are not counted against it.
    let other = edge.post(&other_key_token(), "/v1/agent/runs", start("g2")).await;
    assert_eq!(other.status, 200, "{}", other.body);
    assert_eq!(edge.post(&member_token(), "/v1/agent/runs", json!({})).await.status, 200);

    // Closing a run frees its slot.
    let closed = edge.delete(&key, &format!("/v1/agent/runs/{first}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(edge.post(&key, "/v1/agent/runs", start("g1")).await.status, 200);
}

/// The allowed tab and receipt routes work for the key's own run.
#[tokio::test]
async fn the_allowed_routes_work_on_the_keys_own_run() {
    let edge = edge_with(broker_of(vec![plain_grant("g1")]), false, no_caps()).await;
    let key = key_token();
    let run = run_of(&edge.post(&key, "/v1/agent/runs", start("g1")).await);
    let tab = edge.new_tab(&key, &run, None).await;
    assert_eq!(tab.status, 200, "{}", tab.body);
    let listed = edge.get(&key, &format!("/v1/agent/runs/{run}/tabs")).await;
    assert_eq!(listed.status, 200, "{}", listed.body);
    assert_eq!(edge.step(&key, &run, wait_action(), None).await.status, 200);
    assert_eq!(edge.get(&key, &format!("/v1/agent/runs/{run}/receipts")).await.status, 200);
    // The routes it may not use stay shut for the run it owns.
    for route in ["browser-session", "live-view", "devtools"] {
        let reply = edge.get(&key, &format!("/v1/agent/runs/{run}/{route}")).await;
        assert_eq!(reply.status, 403, "{route}: {}", reply.body);
        assert_eq!(reason(&reply), "api_key_route_refused", "{route}");
    }
    assert_eq!(edge.delete(&key, &format!("/v1/agent/runs/{run}")).await.status, 200);
}

/// An event stream a key opened ends when the token that opened it expires.
#[tokio::test]
async fn an_event_stream_ends_when_the_keys_token_expires() {
    let edge = edge_with(broker_of(vec![plain_grant("g1")]), false, no_caps()).await;
    let run = run_of(&edge.post(&key_token(), "/v1/agent/runs", start("g1")).await);
    let short = api_key_token("key-1", "user-golden", |c| {
        c.insert("exp".into(), json!(now_unix() + 2));
    });
    let started = Instant::now();
    let mut stream = edge
        .open_stream(&short, &format!("/v1/agent/runs/{run}/events/stream"))
        .await;
    let mut text = String::new();
    let ended = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(chunk) = stream.chunk().await.expect("a chunk") {
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
    })
    .await;
    assert!(ended.is_ok(), "the stream outlived its token: {text}");
    assert!(text.contains("token_expired"), "{text}");
    assert!(
        started.elapsed() < Duration::from_secs(12),
        "the verifier's leeway must not extend the stream: {:?}",
        started.elapsed()
    );
}
