//! The ZDR matrix's gap rows as they are today, and the refusals the matrix calls
//! enforced: behavioural evidence for `quarry_core::zdr_matrix` (WS4.G).
//!
//! The matrix says where Zero Data Retention is NOT honoured yet. For a ZDR
//! organization, the schedule create, the browser profile create and save, the
//! change check, history, snapshot and refresh, the search suggest and the
//! orchestrator's internal change record are all answered as if the organization
//! were not ZDR. One `gap_rows_currently_violate_*` test per gap row measures that
//! with real RS256 tokens, the golden Auth Core claims and the real router (the
//! method of `zdr_claim_floor.rs`), so a gap is shown, not only described. Where
//! this build cannot reach the write a gap is about (no postgres-queue, so no
//! baseline store), the test says so and pins what it can.
//!
//! Each `gap_rows_currently_violate_*` test is a snapshot of a violation and must be
//! INVERTED when WS4.H lands (gated on ADR-4). Then the ZDR organization's request is
//! refused (`details.zdr_refusal.code == "zdr_durable_mutation_forbidden"` for the
//! durable writes) and the refused request never reaches the control double. Until
//! then, a failing test here means a gap has moved: change the matrix row and this
//! test together, never one alone.
//!
//! `enforced_refusals_hold_for_durable_jobs` and
//! `a_service_principal_with_body_zdr_gets_the_same_refusals` pin refusals the matrix
//! calls enforced. They keep passing after WS4.H.
//!
//! Own binary, on the trimmed harness in `support/zdr_harness.rs`.

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use std::collections::BTreeSet;

use serde_json::{json, Value};

use quarry_edge::state::AppState;

use zdr_harness::{
    control_plane_double, golden_token, model_plane_double, user_in_standard_org,
    user_in_zdr_org, Harness,
};

/// The durable job routes: refused for a ZDR organization, open otherwise.
fn durable_jobs() -> [(&'static str, Value); 2] {
    [
        (
            "/v1/crawl",
            json!({ "url": "https://page.test/", "max_pages": 1 }),
        ),
        ("/v1/batch", json!({ "urls": ["https://page.test/a"] })),
    ]
}

/// Asserts that a ZDR organization gets the same answer as a standard organization
/// and that the answer is not a refusal naming ZDR. Used for the rows that are not
/// refused today: whatever the build answers (here 501, the postgres-queue feature
/// is off by default), a ZDR organization gets the same answer.
async fn same_answer_and_no_zdr_refusal(
    route: &str,
    for_zdr: reqwest::Response,
    for_standard: reqwest::Response,
) {
    let zdr_status = for_zdr.status().as_u16();
    let standard_status = for_standard.status().as_u16();
    let payload: Value = for_zdr.json().await.unwrap_or(Value::Null);
    assert_eq!(
        zdr_status, standard_status,
        "{route}: a ZDR organization gets the same answer as a standard one: {payload}"
    );
    let message = payload["error"]["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("ZDR"),
        "{route}: refused for ZDR today, so the gap has closed: {payload}"
    );
}

// ── gap rows: today's violations (each one INVERTS when WS4.H lands) ─────────

/// Row `schedules` (Gap). Today a ZDR organization creates a change-monitor
/// schedule: the edge forwards it to Control Plane and answers 200.
///
/// INVERT when WS4.H lands: the create is refused 403 with
/// `details.zdr_refusal.code == "zdr_durable_mutation_forbidden"`, and the control
/// double records no request.
#[tokio::test]
async fn gap_rows_currently_violate_schedules() {
    let control = control_plane_double().await;
    let control_url = control.uri();
    let harness = Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        ..state
    })
    .await;

    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/schedules",
            json!({ "preset": "hourly", "target_ref": "https://page.test/watched" }),
        )
        .await;
    assert_eq!(
        response.status().as_u16(),
        200,
        "the gap: a ZDR organization creates a schedule today"
    );

    let forwarded = control.received_requests().await.unwrap();
    assert_eq!(forwarded.len(), 1, "the schedule reached Control Plane");
    assert_eq!(forwarded[0].url.path(), "/v1/schedules");
    assert_eq!(forwarded[0].url.query(), Some("org_id=org-zdr-golden"));
    let body: Value = forwarded[0].body_json().unwrap();
    assert_eq!(body["org_id"], "org-zdr-golden");
    assert_eq!(body["target_kind"], "change_monitor");
    assert_eq!(body["target_ref"], "https://page.test/watched");
}

/// Row `profiles` (Gap, WS4.H). Today a ZDR organization creates a named browser
/// profile, saves a snapshot (cookies and storage) and lists both.
///
/// INVERT when WS4.H lands: the create and the save are refused with
/// `zdr_durable_mutation_forbidden`; the list stays 200 (the wind-down rule).
#[tokio::test]
async fn gap_rows_currently_violate_profiles() {
    let harness = Harness::start().await;
    let token = user_in_zdr_org();

    let created = harness
        .post(
            &token,
            "/v1/profiles/create",
            json!({ "name": "matrix probe", "scope": "user_private" }),
        )
        .await;
    assert_eq!(
        created.status().as_u16(),
        200,
        "the gap: a ZDR organization creates a profile today"
    );
    let created: Value = created.json().await.unwrap();
    let created_id = created["data"]["profile_id"]
        .as_str()
        .expect("a profile id")
        .to_owned();

    let saved = harness
        .post(
            &token,
            "/v1/profiles",
            json!({ "snapshot": { "cookies": [], "local_storage": [], "session_storage": [] } }),
        )
        .await;
    assert_eq!(
        saved.status().as_u16(),
        200,
        "the gap: a ZDR organization saves a snapshot today"
    );
    let saved: Value = saved.json().await.unwrap();
    let saved_id = saved["data"]["profile_id"]
        .as_str()
        .expect("a profile id")
        .to_owned();

    let listed = harness.get(&token, "/v1/profiles").await;
    assert_eq!(listed.status().as_u16(), 200);
    let listed: Value = listed.json().await.unwrap();
    let listed_ids: BTreeSet<&str> = listed["data"]["profiles"]
        .as_array()
        .expect("a profiles array")
        .iter()
        .filter_map(|profile| profile["profile_id"].as_str())
        .collect();
    assert!(
        listed_ids.contains(created_id.as_str()) && listed_ids.contains(saved_id.as_str()),
        "both profiles are listed for the ZDR organization: {listed}"
    );
}

/// Row `change_history` (Gap, WS4.H). A change check and the history read answer a
/// ZDR organization as they answer any other. In this build (no postgres-queue, so
/// no baseline store) both answer 501 for everyone. What the matrix means by "keeps a
/// baseline and history" needs a postgres-queue build, which this harness cannot run.
///
/// INVERT when WS4.H lands: the ZDR organization's check is refused 403 with
/// `zdr_durable_mutation_forbidden`.
#[tokio::test]
async fn gap_rows_currently_violate_change_history() {
    let harness = Harness::start().await;
    let zdr = user_in_zdr_org();
    let standard = user_in_standard_org();

    let check = json!({ "url": "https://page.test/" });
    let for_zdr = harness.post(&zdr, "/v1/change/check", check.clone()).await;
    let for_standard = harness.post(&standard, "/v1/change/check", check).await;
    same_answer_and_no_zdr_refusal("/v1/change/check", for_zdr, for_standard).await;

    let history = "/v1/change/history?url=https%3A%2F%2Fpage.test%2F";
    let for_zdr = harness.get(&zdr, history).await;
    let for_standard = harness.get(&standard, history).await;
    same_answer_and_no_zdr_refusal(history, for_zdr, for_standard).await;
}

/// Row `change_snapshot` (Gap, WS4.H). Promoting a tracked result to a snapshot and
/// scheduling a refresh are not refused for a ZDR organization.
///
/// In this build both answer the same 501 as for a standard organization (no
/// postgres-queue, no request queue) and the message does not name ZDR. That is "not
/// refused", not "works": in a postgres-queue build with a baseline store they would
/// run for a ZDR organization, and this harness cannot show that without Postgres.
///
/// INVERT when WS4.H lands: both are refused 403 with `zdr_durable_mutation_forbidden`
/// for a ZDR organization, and a standard organization is unchanged.
#[tokio::test]
async fn gap_rows_currently_violate_change_snapshot() {
    let harness = Harness::start().await;
    let zdr = user_in_zdr_org();
    let standard = user_in_standard_org();

    let promote = "/v1/change/snapshot?url=https%3A%2F%2Fpage.test%2F";
    let for_zdr = harness.post(&zdr, promote, json!({})).await;
    let for_standard = harness.post(&standard, promote, json!({})).await;
    same_answer_and_no_zdr_refusal(promote, for_zdr, for_standard).await;

    let refresh = "/v1/change/refresh";
    let body = json!({ "url": "https://page.test/" });
    let for_zdr = harness.post(&zdr, refresh, body.clone()).await;
    let for_standard = harness.post(&standard, refresh, body).await;
    same_answer_and_no_zdr_refusal(refresh, for_zdr, for_standard).await;
}

/// The shared token the orchestrator presents to `POST /v1/internal/change/record`.
/// This binary sets it for itself; only that route reads the variable.
const INTERNAL_TOKEN: &str = "matrix-probe-internal-token";

/// Row `change_record_internal` (Gap, WS4.H). The orchestrator's internal change
/// record takes the schedule's posture in its body (`zdr`), and a declared ZDR run
/// must not persist a baseline. This build has no baseline store (no postgres-queue),
/// so the route answers 501 before it could write anything, whatever the body says:
/// the write this gap is about cannot be observed here. The test pins what it can:
///
/// * the shared token is checked (a wrong token is 401), so the 501 below comes from
///   past the token check, not from an endpoint with no token configured;
/// * a declared `zdr: true` changes nothing: the answer is the same as for a body that
///   says nothing, and it names no ZDR refusal.
///
/// INVERT when WS4.H lands, in a build that persists: a record declared `zdr: true`
/// must write no baseline for its URL. That needs the postgres-queue harness, which
/// this binary cannot run.
#[tokio::test]
async fn gap_rows_currently_violate_change_record_internal() {
    std::env::set_var("QUARRY_EDGE_INTERNAL_TOKEN", INTERNAL_TOKEN);
    let harness = Harness::start().await;
    let record = |zdr: Option<bool>| {
        let mut body = json!({
            "org_id": "org-zdr-golden",
            "url": "https://page.test/",
            "fresh_fingerprint": "matrix-probe-fingerprint",
            "run_id": "matrix-probe-run",
        });
        if let Some(zdr) = zdr {
            body["zdr"] = json!(zdr);
        }
        body
    };

    let wrong_token = harness
        .post(
            "not-the-internal-token",
            "/v1/internal/change/record",
            record(Some(true)),
        )
        .await;
    assert_eq!(
        wrong_token.status().as_u16(),
        401,
        "the internal token is checked before the body is read"
    );

    let declared = harness
        .post(
            INTERNAL_TOKEN,
            "/v1/internal/change/record",
            record(Some(true)),
        )
        .await;
    let undeclared = harness
        .post(INTERNAL_TOKEN, "/v1/internal/change/record", record(None))
        .await;
    let declared_status = declared.status().as_u16();
    let undeclared_status = undeclared.status().as_u16();
    let declared: Value = declared.json().await.unwrap();
    let undeclared: Value = undeclared.json().await.unwrap();
    assert_eq!(
        declared_status, 501,
        "the gap: no baseline store, so the route answers 501 for a declared ZDR run: {declared}"
    );
    assert_eq!(
        declared_status, undeclared_status,
        "a declared zdr changes nothing in this build: {undeclared}"
    );
    assert_eq!(
        declared["error"]["message"], undeclared["error"]["message"],
        "a declared zdr changes nothing in this build"
    );
    let message = declared["error"]["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("ZDR"),
        "refused for ZDR today, so the gap has closed: {declared}"
    );
}

/// Row `search_suggest` (Gap, unassigned). Today the partial query goes to the Model
/// Plane without the ZDR posture: the invoke body is `content` and nothing else,
/// whatever the caller's organization.
///
/// INVERT when the row is assigned: the body carries a posture the Model Plane
/// honours, or the ZDR organization's suggest is refused.
#[tokio::test]
async fn gap_rows_currently_violate_search_suggest() {
    let model_plane = model_plane_double(
        r#"{"corrected_query":"rust async","related_queries":["rust async await"],"entity":null}"#,
    )
    .await;
    let model_plane_url = model_plane.uri();
    let harness = Harness::start_with(move |state| AppState {
        model_plane_url: Some(model_plane_url),
        ..state
    })
    .await;

    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/search/suggest",
            json!({ "query": "rust asnyc" }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    let payload: Value = response.json().await.unwrap();
    assert_eq!(
        payload["corrected_query"], "rust async",
        "the suggestion came back from the Model Plane"
    );

    let invokes = model_plane.received_requests().await.unwrap();
    assert_eq!(invokes.len(), 1, "one invoke reached the Model Plane");
    assert_eq!(invokes[0].url.path(), "/v1/invoke");
    let body: Value = invokes[0].body_json().unwrap();
    let keys: BTreeSet<&str> = body
        .as_object()
        .expect("the invoke body is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from(["content"]),
        "the Model Plane body carries no ZDR posture: {body}"
    );
    assert!(body["content"]
        .as_str()
        .unwrap_or_default()
        .contains("rust asnyc"));
}

// ── refusals the matrix calls enforced (these keep passing after WS4.H) ──────

/// Row `durable_jobs` (Enforced). A durable crawl or batch is refused for a ZDR
/// organization, whether the posture comes from the token or from the body, and it
/// never reaches Control Plane. A standard organization is not refused.
#[tokio::test]
async fn enforced_refusals_hold_for_durable_jobs() {
    let control = control_plane_double().await;
    let control_url = control.uri();
    let harness = Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        ..state
    })
    .await;
    let zdr = user_in_zdr_org();
    let standard = user_in_standard_org();

    for (route, body) in durable_jobs() {
        let refused = harness.post(&zdr, route, body.clone()).await;
        assert_eq!(refused.status().as_u16(), 403, "{route}");
        let payload: Value = refused.json().await.unwrap();
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("ZDR"),
            "{route}: the refusal says why: {payload}"
        );

        let mut with_flag = body.clone();
        with_flag["zdr"] = json!(true);
        let refused_by_body = harness.post(&standard, route, with_flag).await;
        assert_eq!(
            refused_by_body.status().as_u16(),
            403,
            "{route} with body zdr=true"
        );
    }
    assert!(
        control.received_requests().await.unwrap().is_empty(),
        "a refused durable job never reaches Control Plane"
    );

    for (route, body) in durable_jobs() {
        let accepted = harness.post(&standard, route, body).await;
        assert_eq!(
            accepted.status().as_u16(),
            200,
            "{route}: a standard organization is not refused"
        );
    }
    assert_eq!(control.received_requests().await.unwrap().len(), 2);
}

/// A service principal's posture is its body flag: the golden `service_model_gateway`
/// token says `zdr: true` (its registry retention), and that claim is ignored by
/// design. Sent with `zdr: true` in the body:
///
/// * a durable crawl or batch: model-gateway holds no durable-job scope, so the route
///   scope gate answers 403 before the ZDR refusal can run. That is a refusal, but
///   the gate's bare 403, not the ZDR message, and no request reaches Control Plane.
///   It changes if the scope is ever granted.
/// * an agent run with a persistent profile: execution-core (`service_execution_core`,
///   which holds `browser:execute`) gets the same ZDR refusal as a ZDR organization's
///   user, from the body flag alone.
#[tokio::test]
async fn a_service_principal_with_body_zdr_gets_the_same_refusals() {
    let control = control_plane_double().await;
    let control_url = control.uri();
    let harness = Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        ..state
    })
    .await;

    let model_gateway = golden_token("service_model_gateway");
    for (route, body) in durable_jobs() {
        let mut with_flag = body.clone();
        with_flag["zdr"] = json!(true);
        let response = harness.post(&model_gateway, route, with_flag).await;
        assert_eq!(
            response.status().as_u16(),
            403,
            "{route}: the scope gate refuses model-gateway"
        );
    }
    assert!(
        control.received_requests().await.unwrap().is_empty(),
        "a service's refused durable job never reaches Control Plane"
    );

    if cfg!(feature = "browser-agent") {
        let user = harness
            .post(
                &user_in_zdr_org(),
                "/v1/agent/runs",
                json!({ "zdr": false, "persist_profile": true }),
            )
            .await;
        let service = harness
            .post(
                &golden_token("service_execution_core"),
                "/v1/agent/runs",
                json!({ "zdr": true, "persist_profile": true }),
            )
            .await;
        let user_status = user.status().as_u16();
        let service_status = service.status().as_u16();
        let user: Value = user.json().await.unwrap();
        let service: Value = service.json().await.unwrap();
        assert_eq!(user_status, 400, "the ZDR user's persistent profile: {user}");
        assert_eq!(
            service_status, user_status,
            "a service's body zdr:true refuses as a ZDR user does: {service}"
        );
        for payload in [&user, &service] {
            let message = payload["error"]["message"].as_str().unwrap_or_default();
            assert!(
                message.contains("zdr_persistent_profile_forbidden"),
                "{payload}"
            );
        }
    }
}
