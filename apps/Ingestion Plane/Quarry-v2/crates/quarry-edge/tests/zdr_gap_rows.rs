//! The ZDR matrix's gap rows as they are today, and the refusals the matrix calls
//! enforced: behavioural evidence for `quarry_core::zdr_matrix` (WS4.G).
//!
//! The matrix says where Zero Data Retention is NOT honoured yet. For a ZDR
//! organization, the schedule create, the browser profile create and save, the
//! change check, history, snapshot and refresh and the
//! orchestrator's internal change record are all answered as if the organization
//! were not ZDR. One `gap_rows_currently_violate_*` test per gap row measures that
//! with real RS256 tokens, the golden Auth Core claims and the real router (the
//! method of `zdr_claim_floor.rs`), so a gap is shown, not only described. Where
//! this harness cannot reach the write a gap is about (it wires no baseline store
//! under any feature set), the test says so and pins what it can.
//!
//! Each `gap_rows_currently_violate_*` test except the change_record_internal one is a
//! snapshot of a violation and must be INVERTED when WS4.H lands (gated on ADR-4).
//! Then the ZDR organization's request is refused (`details.zdr_refusal.code ==
//! "zdr_durable_mutation_forbidden"` for the durable writes) and the refused request
//! never reaches the control double. Until then, a failing test here means a gap has
//! moved: change the matrix row and this test together, never one alone. The
//! change_record_internal test cannot observe its write in this harness, so it is not
//! an inversion target (see its doc).
//!
//! `enforced_refusals_hold_for_durable_jobs` and
//! `a_service_principal_with_body_zdr_gets_the_same_persistent_profile_refusal` pin
//! refusals the matrix calls enforced. They keep passing after WS4.H.
//! `a_zdr_caller_gets_no_suggestions_and_the_model_plane_sees_no_request` pins the
//! `search_suggest` row, closed by WS4.G (an empty 200 under ZDR, no Model Plane call).
//! `a_service_principal_is_refused_durable_jobs_by_the_scope_gate` pins the route scope
//! gate's refusal of service principals on the durable routes. That is not the ZDR
//! refusal, so no matrix row cites it.
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
    user_in_zdr_org, user_in_zdr_org_with_profile_import, Harness,
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

/// Asserts the answer a change route gives in this harness: 501 for a ZDR organization,
/// the same status a standard organization gets, and not a refusal naming ZDR. The
/// harness wires no baseline store and no request queue under any feature set
/// (`support::base_state` leaves both `None`), so every change route that needs one
/// answers 501 before it could store anything. A ZDR organization is not refused, and
/// it is not served either.
async fn same_501_and_no_zdr_refusal(
    route: &str,
    for_zdr: reqwest::Response,
    for_standard: reqwest::Response,
) {
    let zdr_status = for_zdr.status().as_u16();
    let standard_status = for_standard.status().as_u16();
    let payload: Value = for_zdr.json().await.unwrap_or(Value::Null);
    assert_eq!(
        zdr_status, 501,
        "{route}: no store in this harness, so 501 for a ZDR organization: {payload}"
    );
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

    // The save needs the profile import scope (WS17.P0); no golden case carries it.
    let saved = harness
        .post(
            &user_in_zdr_org_with_profile_import(),
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
/// ZDR organization as they answer any other: in this harness both answer 501 (no
/// baseline store, so nothing is read or kept). What the matrix means by "keeps a
/// baseline and history" needs a wired baseline store, which this harness cannot run.
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
    same_501_and_no_zdr_refusal("/v1/change/check", for_zdr, for_standard).await;

    let history = "/v1/change/history?url=https%3A%2F%2Fpage.test%2F";
    let for_zdr = harness.get(&zdr, history).await;
    let for_standard = harness.get(&standard, history).await;
    same_501_and_no_zdr_refusal(history, for_zdr, for_standard).await;
}

/// Row `change_snapshot` (Gap, WS4.H). Promoting a tracked result to a snapshot and
/// scheduling a refresh are not refused for a ZDR organization.
///
/// In this harness both answer 501, the same as for a standard organization (no
/// baseline store, no request queue), and the message does not name ZDR. That is "not
/// refused", not "works": with a baseline store and a queue they would run for a ZDR
/// organization, and this harness cannot show that without Postgres.
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
    same_501_and_no_zdr_refusal(promote, for_zdr, for_standard).await;

    let refresh = "/v1/change/refresh";
    let body = json!({ "url": "https://page.test/" });
    let for_zdr = harness.post(&zdr, refresh, body.clone()).await;
    let for_standard = harness.post(&standard, refresh, body).await;
    same_501_and_no_zdr_refusal(refresh, for_zdr, for_standard).await;
}

/// The shared token the orchestrator presents to `POST /v1/internal/change/record`.
/// This binary sets it for itself; only that route reads the variable.
const INTERNAL_TOKEN: &str = "matrix-probe-internal-token";

/// Row `change_record_internal` (Gap, WS4.H). The orchestrator's internal change
/// record takes the schedule's posture in its body (`zdr`), and a declared ZDR run
/// must not persist a baseline. This harness wires no baseline store under any feature
/// set, so the route answers 501 before it could write anything, whatever the body says.
/// The write this gap is about is not reachable here, so this test cannot observe it
/// and cannot fail when WS4.H lands. It pins what this harness can reach:
///
/// * the shared token is checked (a wrong token is 401), so the 501 below comes from
///   past the token check, not from an endpoint with no token configured;
/// * a declared `zdr: true` changes nothing: the answer is the same as for a body that
///   says nothing, and it names no ZDR refusal.
///
/// Not an inversion target: no test in this binary fails if the row's warning (the
/// baseline is still stored) stops being true. The WS4.H test must wire a baseline
/// store under postgres-queue, which this binary cannot do. The test measures no
/// violation, so its name does not use the `gap_rows_currently_violate_*` form.
#[tokio::test]
async fn change_record_internal_pins_the_token_check_and_the_501() {
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

/// Row `search_suggest` (Enforced, WS4.G). The Model Plane hop carries no ZDR posture,
/// so a ZDR organization's suggest degrades to an empty 200 and its partial query never
/// leaves Quarry: the Model Plane double receives no request. A standard organization
/// still gets the double's suggestion through one invoke.
#[tokio::test]
async fn a_zdr_caller_gets_no_suggestions_and_the_model_plane_sees_no_request() {
    let model_plane = model_plane_double(
        r#"{"corrected_query":"rust async","related_queries":["rust async await"],"entity":null}"#,
    )
    .await;
    let model_plane_url = model_plane.uri();
    let harness = Harness::start_with(move |state| AppState {
        model_plane_url: Some(model_plane_url),
        // QB-T17: /v1/search/suggest is off unless its switch is on; the ZDR guard applies
        // whenever the route is on.
        model_plane: quarry_edge::model_plane_gate::ModelPlaneFeatures {
            search_suggest: true,
            ..Default::default()
        },
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
    assert_eq!(
        response.status().as_u16(),
        200,
        "the UI degrades, it is not refused"
    );
    let payload: Value = response.json().await.unwrap();
    assert_eq!(
        payload,
        json!({ "related_queries": [] }),
        "a ZDR caller gets empty suggestions: {payload}"
    );
    assert!(
        model_plane.received_requests().await.unwrap().is_empty(),
        "a ZDR caller's partial query never reaches the Model Plane"
    );

    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/search/suggest",
            json!({ "query": "rust asnyc" }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    let payload: Value = response.json().await.unwrap();
    assert_eq!(
        payload["corrected_query"], "rust async",
        "a standard organization still gets the Model Plane's suggestion: {payload}"
    );
    let invokes = model_plane.received_requests().await.unwrap();
    assert_eq!(
        invokes.len(),
        1,
        "one invoke, from the standard organization"
    );
    assert_eq!(invokes[0].url.path(), "/v1/invoke");
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

/// A service principal's durable crawl or batch, sent with `zdr: true` in the body, is
/// refused with a 403 and nothing reaches Control Plane. Since E3.7.10 a principal that holds
/// `crawl:write` passes the route scope gate (`auth::require_service_route_scope`) and is
/// refused by `routes::refuse_durable_job_under_zdr`; one without the scope is refused by the
/// gate with a bare 403 before the body is read. Both are a 403. This does not evidence row
/// `durable_jobs`, whose basis is the ZDR refusal for users.
#[tokio::test]
async fn a_service_principal_is_refused_durable_jobs_by_the_scope_gate() {
    let control = control_plane_double().await;
    let control_url = control.uri();
    let harness = Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        ..state
    })
    .await;

    for token in [
        golden_token("service_model_gateway"),
        golden_token("service_execution_core"),
    ] {
        for (route, body) in durable_jobs() {
            let mut with_flag = body.clone();
            with_flag["zdr"] = json!(true);
            let response = harness.post(&token, route, with_flag).await;
            assert_eq!(
                response.status().as_u16(),
                403,
                "{route}: the route scope gate refuses a service principal"
            );
        }
    }
    assert!(
        control.received_requests().await.unwrap().is_empty(),
        "a service's refused durable job never reaches Control Plane"
    );
}

/// A service's persistent-profile refusal, row `agent_persistent_profile`. The body flag
/// is what counts: `service_execution_core` holds `browser:execute`, so the scope gate
/// lets it reach `/v1/agent/runs`, and with `zdr: true` in the body it gets the same
/// `zdr_persistent_profile_forbidden` refusal that a ZDR organization's user gets. Its
/// own token's `zdr` claim is ignored by design.
#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn a_service_principal_with_body_zdr_gets_the_same_persistent_profile_refusal() {
    let harness = Harness::start().await;
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
