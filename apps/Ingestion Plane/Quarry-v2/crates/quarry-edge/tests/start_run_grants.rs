//! What `POST /v1/agent/runs` does with a BrowserBroker grant, through the real router
//! and real token verification (the harness is in `support/agent_harness.rs`), with a
//! fake grant validator in place of the broker.
//!
//! Three rules, each in both enforcement modes where the mode matters (the edge
//! requires a grant for every run in production; elsewhere a grant is optional):
//!
//! - a user's grant is theirs: another user's grant is refused and no browser is
//!   acquired for it;
//! - a ZDR grant may only back a ZDR run;
//! - the broker's hosts, not the caller's, are the run's hosts whenever the grant
//!   carries a host policy, in every mode.
//!
//! The pure binding rules are unit-tested in `agent_routes/tests.rs`
//! (`bind_validated_grant`, `zdr_grant_may_back_run`); these tests pin that the route
//! applies them before it acquires a browser, and that what they decide reaches the run.

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
use quarry_edge::run_limits::RunLimits;
use quarry_runtime::{GrantValidation, GrantValidator};
use serde_json::{json, Value};

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

/// A broker that records which organization every validation was made for.
/// `validate` (the org-blind call) is recorded as `<plain>`, so a call site
/// that skipped `validate_for_org` shows up in the assertions.
struct RecordingBroker {
    grants: HashMap<String, GrantValidation>,
    calls: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl GrantValidator for RecordingBroker {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.calls
            .lock()
            .unwrap()
            .push(("<plain>".to_owned(), grant_id.to_owned()));
        self.grants
            .get(grant_id)
            .cloned()
            .ok_or_else(|| QuarryError::new(ErrorCode::NotFound, "unknown browser grant"))
    }

    async fn validate_for_org(
        &self,
        org_id: &str,
        grant_id: &str,
    ) -> QuarryResult<GrantValidation> {
        self.calls
            .lock()
            .unwrap()
            .push((org_id.to_owned(), grant_id.to_owned()));
        self.validate_inner(grant_id)
    }
}

impl RecordingBroker {
    fn validate_inner(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.grants
            .get(grant_id)
            .cloned()
            .ok_or_else(|| QuarryError::new(ErrorCode::NotFound, "unknown browser grant"))
    }
}

/// An edge whose broker records the organization of every validation.
async fn recording_edge(
    grants: Vec<GrantValidation>,
    require: bool,
) -> (Edge, Arc<std::sync::Mutex<Vec<(String, String)>>>) {
    let calls: Arc<std::sync::Mutex<Vec<(String, String)>>> = Arc::default();
    let broker = RecordingBroker {
        grants: grants
            .into_iter()
            .map(|grant| (grant.grant_id.clone(), grant))
            .collect(),
        calls: calls.clone(),
    };
    let edge = Edge::start_with(RunLimits::default(), move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state.require_browser_grants = require;
        state
    })
    .await;
    (edge, calls)
}

/// A usable grant. Every field not named here is the broker's default (empty).
fn grant(id: &str, tweak: impl FnOnce(&mut serde_json::Map<String, Value>)) -> GrantValidation {
    let mut fields = json!({ "grant_id": id, "active": true })
        .as_object()
        .cloned()
        .expect("an object");
    tweak(&mut fields);
    serde_json::from_value(Value::Object(fields)).expect("a grant validation")
}

/// An edge whose broker knows `grants`; `require` is the edge's
/// `require_browser_grants` (the production posture).
async fn edge_with(grants: Vec<GrantValidation>, require: bool) -> Edge {
    let broker = FakeBroker(
        grants
            .into_iter()
            .map(|grant| (grant.grant_id.clone(), grant))
            .collect(),
    );
    Edge::start_with(RunLimits::default(), move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state.require_browser_grants = require;
        state
    })
    .await
}

/// A start body that names `grant_id` and, when given, the client's own domains.
fn start_body(grant_id: &str, client_domains: Option<&[&str]>) -> Value {
    let mut body = json!({ "grant_id": grant_id });
    if let Some(domains) = client_domains {
        body["constraints"] = json!({ "max_steps": 0, "allowed_domains": domains });
    }
    body
}

/// The hosts the live run is confined to.
async fn run_domains(edge: &Edge, run: &str) -> Vec<String> {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry.constraints.allowed_domains.clone()
}

/// A person's grant is theirs. Another user of the same organization who presents it
/// is refused with 403 and no browser is acquired for them; the owner is served and
/// reads their own grant id back on the run's projection. Both modes.
#[tokio::test]
async fn user_cannot_use_another_users_grant() {
    for require in [false, true] {
        let edge = edge_with(
            vec![grant("grant-of-other", |fields| {
                fields.insert("owner_id".into(), json!("user-other"));
                fields.insert("allowed_domains".into(), json!(["example.com"]));
            })],
            require,
        )
        .await;

        let stolen = edge
            .post(
                &member_token(),
                "/v1/agent/runs",
                start_body("grant-of-other", None),
            )
            .await;
        assert_eq!(stolen.status, 403, "require={require}: {}", stolen.body);
        assert_eq!(
            stolen.message(),
            "browser grant belongs to another user",
            "require={require}"
        );
        assert!(
            edge.driver.acquired().is_empty(),
            "require={require}: no browser is acquired for a refused grant"
        );

        let own = edge
            .post(
                &other_member_token(),
                "/v1/agent/runs",
                start_body("grant-of-other", None),
            )
            .await;
        assert_eq!(own.status, 200, "require={require}: {}", own.body);
        let run = own.data()["run_id"].as_str().expect("a run id").to_owned();
        let projection = edge.projection(&other_member_token(), &run).await;
        assert_eq!(
            projection.data()["grantId"],
            "grant-of-other",
            "require={require}: the owner is served the grant id"
        );
    }
}

/// A grant with no owner (a broker that predates owners) is admitted when grants are
/// optional and refused when they are required; either way it is not another user's
/// grant to take on trust in production.
#[tokio::test]
async fn an_ownerless_grant_is_refused_to_a_user_only_when_grants_are_required() {
    let ownerless = || {
        grant("grant-ownerless", |fields| {
            fields.insert("allowed_domains".into(), json!(["example.com"]));
        })
    };

    let optional = edge_with(vec![ownerless()], false).await;
    let admitted = optional
        .post(
            &member_token(),
            "/v1/agent/runs",
            start_body("grant-ownerless", None),
        )
        .await;
    assert_eq!(admitted.status, 200, "{}", admitted.body);

    let required = edge_with(vec![ownerless()], true).await;
    let refused = required
        .post(
            &member_token(),
            "/v1/agent/runs",
            start_body("grant-ownerless", None),
        )
        .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.message(), "browser grant belongs to another user");
    assert!(required.driver.acquired().is_empty());
}

/// A ZDR grant may only back a ZDR run: a run that is not ZDR would persist what the
/// grant's owner asked never to keep. Refused with 403 before any browser is acquired;
/// the same grant backs a run that asks for ZDR and a run whose organization is under
/// ZDR, and a non-ZDR grant backs a ZDR run (the run's posture can only raise ZDR).
#[tokio::test]
async fn zdr_grant_cannot_back_a_non_zdr_run() {
    for require in [false, true] {
        let edge = edge_with(
            vec![
                grant("grant-zdr", |fields| {
                    fields.insert("owner_id".into(), json!("user-golden"));
                    fields.insert("zdr".into(), json!(true));
                    fields.insert("allowed_domains".into(), json!(["example.com"]));
                }),
                grant("grant-plain", |fields| {
                    fields.insert("owner_id".into(), json!("user-golden"));
                    fields.insert("allowed_domains".into(), json!(["example.com"]));
                }),
            ],
            require,
        )
        .await;

        let refused = edge
            .post(
                &member_token(),
                "/v1/agent/runs",
                start_body("grant-zdr", None),
            )
            .await;
        assert_eq!(refused.status, 403, "require={require}: {}", refused.body);
        assert_eq!(
            refused.message(),
            "a ZDR browser grant cannot back a non-ZDR run",
            "require={require}"
        );
        assert!(
            edge.driver.acquired().is_empty(),
            "require={require}: no browser is acquired for a refused grant"
        );

        // The run that asks for ZDR is backed.
        let mut zdr_body = start_body("grant-zdr", None);
        zdr_body["zdr"] = json!(true);
        let asked = edge.post(&member_token(), "/v1/agent/runs", zdr_body).await;
        assert_eq!(asked.status, 200, "require={require}: {}", asked.body);

        // So is a run in an organization under ZDR (the token's floor), with no flag.
        let floored = edge
            .post(
                &zdr_member_token(),
                "/v1/agent/runs",
                start_body("grant-zdr", None),
            )
            .await;
        assert_eq!(floored.status, 200, "require={require}: {}", floored.body);

        // A non-ZDR grant may back a ZDR run.
        let mut raised = start_body("grant-plain", None);
        raised["zdr"] = json!(true);
        let raised = edge.post(&member_token(), "/v1/agent/runs", raised).await;
        assert_eq!(raised.status, 200, "require={require}: {}", raised.body);
    }
}

/// When the grant carries a host policy it is the run's hosts, whatever the caller
/// asked for and whether or not grants are required: the caller's wider list is
/// dropped, and a navigation to a host outside the broker's list is refused before it
/// reaches the browser. A grant with no host policy leaves the caller's constraints
/// alone when grants are optional (the contrast that shows the override is the
/// grant's, not a blanket rewrite).
#[tokio::test]
async fn validated_grant_overrides_client_domains_in_every_mode() {
    for require in [false, true] {
        let edge = edge_with(
            vec![grant("grant-hosts", |fields| {
                fields.insert("owner_id".into(), json!("user-golden"));
                fields.insert("allowed_domains".into(), json!(["example.com"]));
            })],
            require,
        )
        .await;
        let person = member_token();

        // The caller names a wider list, and one that does not include the broker's.
        for client in [
            Some(&["example.com", "evil.test"][..]),
            Some(&["evil.test"][..]),
            None,
        ] {
            let run = edge
                .start_run(&person, start_body("grant-hosts", client))
                .await;
            assert_eq!(
                run_domains(&edge, &run).await,
                vec!["example.com".to_owned()],
                "require={require}, client {client:?}: the broker's hosts are the run's"
            );

            let blocked = edge
                .step(
                    &person,
                    &run,
                    json!({ "type": "navigate", "url": "https://evil.test/" }),
                    None,
                )
                .await;
            assert_eq!(
                blocked.status, 403,
                "require={require}, client {client:?}: {}",
                blocked.body
            );
            assert!(
                blocked
                    .message()
                    .contains("outside the run's allowed domains"),
                "require={require}, client {client:?}: {}",
                blocked.body
            );
        }
    }

    // No host policy on the grant, grants optional: the caller's own list stands.
    let edge = edge_with(
        vec![grant("grant-no-hosts", |fields| {
            fields.insert("owner_id".into(), json!("user-golden"));
        })],
        false,
    )
    .await;
    let run = edge
        .start_run(
            &member_token(),
            start_body("grant-no-hosts", Some(&["client.example"])),
        )
        .await;
    assert_eq!(
        run_domains(&edge, &run).await,
        vec!["client.example".to_owned()],
        "a grant without hosts does not rewrite the caller's constraints"
    );
}

/// A service principal acts for the run's owner, so the user-owner rule does not
/// apply to it: it starts a run under a grant issued to a user, and the run is its own.
///
/// KNOWN GAP, pinned here so it is visible rather than assumed away: Quarry cannot
/// check the grant's organization, because the broker's `ValidateGrantResponse` carries
/// no `org_id` (only the broker's own caller-org scoping, keyed to Quarry's static
/// bearer, applies). Closing it needs an `org_id` on the broker's response and on
/// `GrantValidation` (Model Plane proto and the Go broker), then a check against the
/// caller's org for every principal; when that lands this test must flip to a 403 for a
/// grant of another organization.
#[tokio::test]
async fn known_gap_a_service_principal_is_not_bound_to_the_grants_owner() {
    for require in [false, true] {
        let edge = edge_with(
            vec![grant("grant-of-user", |fields| {
                fields.insert("owner_id".into(), json!("user-other"));
                fields.insert("allowed_domains".into(), json!(["example.com"]));
            })],
            require,
        )
        .await;
        let started = edge
            .post(
                &service_token(),
                "/v1/agent/runs",
                start_body("grant-of-user", None),
            )
            .await;
        assert_eq!(started.status, 200, "require={require}: {}", started.body);
        assert_eq!(edge.driver.acquired().len(), 1, "require={require}");
    }
}

/// The organization reaches the validator on every check of a run's grant: at
/// start (the caller's org), on a step, and when a tab is opened (the run's org,
/// never a body's). The org-blind `validate` is never called on these routes.
/// Both modes. (WS1.A2: the broker binds the call to this org once a service
/// token is minted for it.)
#[tokio::test]
async fn the_runs_grant_is_validated_for_the_runs_org_at_start_step_and_new_tab() {
    for require in [false, true] {
        let (edge, calls) = recording_edge(
            vec![grant("grant-org", |fields| {
                fields.insert("owner_id".into(), json!("user-golden"));
                fields.insert("allowed_domains".into(), json!(["example.com"]));
            })],
            require,
        )
        .await;
        let person = member_token();

        let run = edge
            .start_run(&person, start_body("grant-org", None))
            .await;
        let after_start = calls.lock().unwrap().len();
        assert_eq!(after_start, 1, "require={require}: one validation at start");

        // The recording driver's page is on no allowed host, so the step may end
        // in the post-action host check; the grant check is before it and is
        // what this test is about, so the replies' statuses are not asserted.
        let _ = edge.step(&person, &run, wait_action(), None).await;
        let after_step = calls.lock().unwrap().len();
        assert_eq!(after_step, 2, "require={require}: the step validates once");
        let _ = edge.new_tab(&person, &run, None).await;

        let seen = calls.lock().unwrap().clone();
        assert!(
            seen.len() == 3,
            "require={require}: start, step and new tab each validate once: {seen:?}"
        );
        for (org, grant_id) in &seen {
            assert_eq!(
                (org.as_str(), grant_id.as_str()),
                ("org-standard-golden", "grant-org"),
                "require={require}: every validation names the run's org: {seen:?}"
            );
        }
    }
}

/// A caller of another organization is refused before the broker is asked about
/// the run's grant on a step, so a foreign org never reaches the validator with
/// the run's grant id.
#[tokio::test]
async fn another_orgs_caller_never_reaches_the_validator_for_a_run_it_does_not_own() {
    let (edge, calls) = recording_edge(
        vec![grant("grant-org", |fields| {
            fields.insert("owner_id".into(), json!("user-golden"));
            fields.insert("allowed_domains".into(), json!(["example.com"]));
        })],
        true,
    )
    .await;
    let run = edge
        .start_run(&member_token(), start_body("grant-org", None))
        .await;
    let before = calls.lock().unwrap().len();
    let refused = edge
        .step(&stranger_token(), &run, wait_action(), None)
        .await;
    assert!(
        refused.status == 403 || refused.status == 404,
        "{}: {}",
        refused.status,
        refused.body
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        before,
        "the validator was not asked on behalf of a stranger"
    );
}
