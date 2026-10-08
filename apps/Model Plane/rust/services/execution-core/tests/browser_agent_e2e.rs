//! P7 · Phase F — REAL end-to-end test for the agentic browser loop.
//!
//! No mocks: this drives the actual Model Plane loop → real `QuarryAgentClient`
//! (HTTP) → quarry-edge `/v1/agent/*` (real chromiumoxide CDP browser) → real
//! `BrowserObservation`, optionally with the real inference-core LLM planner.
//!
//! It is `#[ignore]`d because it needs a live stack. To run it:
//!
//! ```bash
//! # quarry-edge must be built WITH the chromiumoxide driver and reachable,
//! # with a real Chrome/Chromium available to it. execution-core must have a
//! # registered Auth Core service principal for the `quarry` audience.
//! QUARRY_BROWSER_AGENT_ENABLED=1 \
//! QUARRY_EDGE_URL=https://quarry-edge.internal \
//! AUTH_CORE_URL=http://auth-core:3011 \
//! EXECUTION_CORE_SERVICE_API_KEY=<credential> \
//! QUARRY_BROWSER_AGENT_LLM=1 \
//! INFERENCE_CORE_URL=http://inference-core:9092 \
//! E2E_INFERENCE_BEARER=<short-lived-aud-inference-core-user-token> \
//! E2E_ORG_ID=org_real \
//! cargo test -p execution-core --test browser_agent_e2e -- --ignored --nocapture
//! ```
//!
//! This file holds two ignored tests. Name one after `--` to run only it
//! (`browser_agent_drives_real_quarry_loop`, or the E3 Terra spike
//! `planner_on_terra_returns_valid_action_json`, which has its own run notes).

use execution_core::browser_agent::{
    run_browser_agent_loop, ActionType, BrowserAction, BrowserObservation, BrowserSnapshotTarget,
    ObservationStatus, PlanConfig, PlanStatus,
};
use execution_core::llm_planner::{
    parse_action_content, planner_infer_request, ActionParse, LlmPlanner, ACTION_SCHEMA,
    ACTION_SCHEMA_STRICT,
};
use execution_core::quarry_agent::QuarryAgentClient;
use mp_contracts::model_plane::v1::inference_core_client::InferenceCoreClient;
use mp_contracts::model_plane::v1::ListModelsRequest;
use std::time::{Duration, Instant};

#[tokio::test]
#[ignore = "requires a live quarry-edge (chromiumoxide) + inference-core stack; see file header"]
async fn browser_agent_drives_real_quarry_loop() {
    // Real Quarry agent client from env — panics if the operator forgot to set
    // QUARRY_BROWSER_AGENT_ENABLED=1 + QUARRY_EDGE_URL (the whole point of the
    // test is to exercise the real wire path).
    let client = QuarryAgentClient::from_env()
        .expect("Auth Core service-principal configuration must be valid")
        .expect("set QUARRY_BROWSER_AGENT_ENABLED=1 and QUARRY_EDGE_URL to run the E2E test");

    // Optional real LLM planner. When unset, the loop uses the deterministic
    // planner (repeated Observe) — still a real Quarry round-trip per step.
    let inference_bearer = std::env::var("E2E_INFERENCE_BEARER").ok();
    let planner = LlmPlanner::from_env(inference_bearer.as_deref());

    let config = PlanConfig {
        plan_id: "e2e_plan".to_owned(),
        grant_id: "e2e_grant".to_owned(),
        run_id: "e2e_run".to_owned(),
        org_id: std::env::var("E2E_ORG_ID").unwrap_or_else(|_| "org_e2e".to_owned()),
        system_prompt: "Open https://example.com and read the page title.".to_owned(),
        max_steps: 3,
        max_runtime_s: 60,
        allowed_domains: vec!["example.com".to_owned()],
        stop_criteria: String::new(),
        require_approval: false,
        max_cost_usd: Some(0.50),
        zdr: false,
        profile_id: None,
        // Phase 2, found live: a freshly `start_run`'d Quarry lease has no
        // page loaded, so the first action needs an explicit destination.
        start_url: Some("https://example.com".to_owned()),
        postcondition: String::new(),
    };

    let result = run_browser_agent_loop(config, Some(&client), planner.as_ref(), None, None).await;

    eprintln!(
        "E2E result: status={} observations={} summary={}",
        result.status.as_str(),
        result.observations.len(),
        result.summary
    );

    assert_ne!(
        result.status,
        PlanStatus::Failed,
        "browser-agent loop failed: {}",
        result.summary
    );
    assert!(
        !result.observations.is_empty(),
        "expected at least one observation from the real Quarry browser"
    );
}

// ---------------------------------------------------------------------------
// E3 spike: can the ChatGPT Terra subscription route return the planner's JSON?
// ---------------------------------------------------------------------------

/// The only route this probe may use: Terra on the user's `ChatGPT` subscription.
/// Never Verevon Balance or any paid provider.
const PROBE_MODEL: &str = "gpt-5.6-terra";
const PROBE_PROVIDER: &str = "openai-codex-subscription";
/// The spike is capped so a probe costs a handful of Codex calls, not a run.
const MAX_PROBE_CALLS: usize = 5;
const REPLY_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Clone, Copy)]
enum Expect {
    /// `navigate` to a URL on example.com.
    Navigate,
    /// `click` bound to this exact snapshot ref.
    ClickRef {
        ref_id: &'static str,
        snapshot_id: &'static str,
        generation: u32,
    },
    /// `type` "rust lang" bound to this exact snapshot ref.
    TypeRef {
        ref_id: &'static str,
        snapshot_id: &'static str,
        generation: u32,
    },
    /// `done`.
    Done,
}

struct ProbeCase {
    name: &'static str,
    goal: &'static str,
    observation: Option<BrowserObservation>,
    expect: Expect,
}

/// The one place this probe builds a `PlanConfig`; the fields the planner reads
/// are the goal (`system_prompt`) and the org.
fn probe_config(org_id: &str, goal: &str) -> PlanConfig {
    PlanConfig {
        plan_id: "e3_probe_plan".to_owned(),
        grant_id: "e3_probe_grant".to_owned(),
        run_id: "e3_probe_run".to_owned(),
        org_id: org_id.to_owned(),
        system_prompt: goal.to_owned(),
        max_steps: 8,
        max_runtime_s: 60,
        allowed_domains: vec!["example.com".to_owned()],
        stop_criteria: String::new(),
        require_approval: false,
        max_cost_usd: None,
        zdr: false,
        profile_id: None,
        start_url: None,
        postcondition: String::new(),
    }
}

fn snapshot_target(ref_id: &str, role: &str, name: &str) -> BrowserSnapshotTarget {
    BrowserSnapshotTarget {
        ref_id: ref_id.to_owned(),
        role: Some(role.to_owned()),
        name: Some(name.to_owned()),
        text: None,
        frame_id: None,
    }
}

/// A fictional observation shaped like what Quarry hands the planner.
fn observation(
    url: &str,
    title: &str,
    text: &str,
    snapshot_id: &str,
    generation: u32,
    targets: Vec<BrowserSnapshotTarget>,
) -> BrowserObservation {
    BrowserObservation {
        observation_id: "e3_probe_obs".to_owned(),
        action_id: "e3_probe_act".to_owned(),
        grant_id: "e3_probe_grant".to_owned(),
        status: ObservationStatus::Success,
        page_url: url.to_owned(),
        page_title: title.to_owned(),
        extracted_text: text.to_owned(),
        screenshot_ref: String::new(),
        dom_snapshot_ref: snapshot_id.to_owned(),
        snapshot_generation: Some(generation),
        snapshot_targets: targets,
        error_message: String::new(),
    }
}

/// The probe cases, in the order they are sent (fictional fixtures only).
fn probe_cases() -> Vec<ProbeCase> {
    vec![
        ProbeCase {
            name: "navigate",
            goal: "Open https://example.com and read the page title.",
            observation: None,
            expect: Expect::Navigate,
        },
        ProbeCase {
            name: "click_by_ref",
            goal: "Open the 'More information' link on this page.",
            observation: Some(observation(
                "https://example.com/",
                "Example Domain",
                "Example Domain. This domain is for use in illustrative examples. More information...",
                "snap-7f3a",
                4,
                vec![
                    snapshot_target("@e1", "heading", "Example Domain"),
                    snapshot_target("@e2", "link", "More information..."),
                ],
            )),
            expect: Expect::ClickRef {
                ref_id: "@e2",
                snapshot_id: "snap-7f3a",
                generation: 4,
            },
        },
        ProbeCase {
            name: "type_by_ref",
            goal: "Search this site for 'rust lang'.",
            observation: Some(observation(
                "https://example.com/search",
                "Search",
                "Search. Type a query, then press the Search button.",
                "snap-b21c",
                1,
                vec![
                    snapshot_target("@e1", "searchbox", "Search"),
                    snapshot_target("@e2", "button", "Search"),
                ],
            )),
            expect: Expect::TypeRef {
                ref_id: "@e1",
                snapshot_id: "snap-b21c",
                generation: 1,
            },
        },
        ProbeCase {
            name: "done",
            goal: "Report the title of the page that is already open.",
            observation: Some(observation(
                "https://example.com/",
                "Example Domain",
                "Example Domain. This domain is for use in illustrative examples.",
                "snap-c9d0",
                2,
                vec![snapshot_target("@e1", "heading", "Example Domain")],
            )),
            expect: Expect::Done,
        },
    ]
}

/// `Ok` when the parsed action is the one the case expects.
fn judge(expect: Expect, action: Option<&BrowserAction>) -> Result<(), String> {
    let bound_to = |action: &BrowserAction, ref_id: &str, snapshot_id: &str, generation: u32| {
        match &action.target {
            Some(t) if t.ref_id == ref_id && t.snapshot_id == snapshot_id && t.generation == generation => Ok(()),
            Some(t) => Err(format!(
                "bound to ref={} snapshot={} generation={}, expected ref={ref_id} snapshot={snapshot_id} generation={generation}",
                t.ref_id, t.snapshot_id, t.generation
            )),
            None => Err(format!(
                "no complete snapshot binding (selector={:?}); expected ref={ref_id} snapshot={snapshot_id} generation={generation}",
                action.selector
            )),
        }
    };
    match (expect, action) {
        (Expect::Done, None) => Ok(()),
        (Expect::Done, Some(a)) => Err(format!("expected done, got {}", a.action_type.as_str())),
        (_, None) => Err("expected an action, got done".to_owned()),
        (Expect::Navigate, Some(a)) if a.action_type == ActionType::Goto && a.url.contains("example.com") => Ok(()),
        (Expect::Navigate, Some(a)) => Err(format!(
            "expected goto example.com, got {} url={:?}",
            a.action_type.as_str(),
            a.url
        )),
        (Expect::ClickRef { ref_id, snapshot_id, generation }, Some(a)) => {
            if a.action_type != ActionType::Click {
                return Err(format!("expected click, got {}", a.action_type.as_str()));
            }
            bound_to(a, ref_id, snapshot_id, generation)
        }
        (Expect::TypeRef { ref_id, snapshot_id, generation }, Some(a)) => {
            if a.action_type != ActionType::Type {
                return Err(format!("expected type, got {}", a.action_type.as_str()));
            }
            if !a.value.to_lowercase().contains("rust lang") {
                return Err(format!("typed {:?}, expected \"rust lang\"", a.value));
            }
            bound_to(a, ref_id, snapshot_id, generation)
        }
    }
}

/// One probed call: what came back and whether it was usable.
struct Row {
    name: &'static str,
    /// The call itself failed (rejected by the route, or no reply in time).
    call_failed: bool,
    mode: Option<ActionParse>,
    problem: Option<String>,
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn required_env(name: &str) -> String {
    optional_env(name)
        .unwrap_or_else(|| panic!("set {name} to run the Terra spike; see the test's doc comment"))
}

/// Who the probe calls inference-core as: a user-bound `aud=inference-core`
/// bearer, the org it is for, and the user's own subscription connection.
/// Never printed.
struct ProbeAuth {
    bearer: String,
    connection_id: String,
    org_id: String,
}

/// The `Cookie` header for the localhost cookies of a Playwright storage state,
/// `None` when it holds none.
fn localhost_cookie_header(state: &serde_json::Value) -> Option<String> {
    let header = state["cookies"]
        .as_array()?
        .iter()
        .filter(|cookie| matches!(cookie["domain"].as_str(), Some("localhost" | ".localhost")))
        .filter_map(|cookie| Some(format!("{}={}", cookie["name"].as_str()?, cookie["value"].as_str()?)))
        .collect::<Vec<_>>()
        .join("; ");
    (!header.is_empty()).then_some(header)
}

/// The claims of a JWT, read for selection only: inference-core verifies the
/// token itself, so decoding here grants nothing.
fn jwt_claims(token: &str) -> Result<serde_json::Value, String> {
    use base64::Engine as _;
    let payload = token.split('.').nth(1).ok_or("the token is not a JWT")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "the token payload is not base64url")?;
    serde_json::from_slice(&bytes).map_err(|_| "the token payload is not JSON".to_owned())
}

/// The id of the first active, not-deleted `ChatGPT` subscription connection in
/// the BFF's connection list (`{data: [...]}`, `{data: {connections: [...]}}`
/// or a bare array).
fn active_subscription_connection(connections: &serde_json::Value) -> Option<String> {
    let data = connections.get("data").unwrap_or(connections);
    let rows = data.as_array().or_else(|| data["connections"].as_array())?;
    let blank = |row: &serde_json::Value, key: &str| row[key].as_str().is_none_or(str::is_empty);
    rows.iter()
        .find(|row| {
            row.get("providerKey")
                .or_else(|| row.get("provider_key"))
                .or_else(|| row.get("providerId"))
                .and_then(serde_json::Value::as_str)
                == Some(PROBE_PROVIDER)
                && row["status"].as_str().is_some_and(|status| status.eq_ignore_ascii_case("active"))
                && blank(row, "deletedAt")
                && blank(row, "deleted_at")
        })
        .and_then(|row| row["id"].as_str())
        .map(str::to_owned)
}

/// The probe's credentials from a local Playwright session (the same way the
/// labelled review eval gets them): the session mints an `aud=inference-core`
/// token at Auth Core, and the BFF lists the session user's connections. The
/// errors carry a status code at most, never a token, cookie or id.
async fn auth_from_session_state(state_path: &str) -> Result<ProbeAuth, String> {
    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(state_path).map_err(|_| "E2E_SESSION_STATE is not readable".to_owned())?,
    )
    .map_err(|_| "E2E_SESSION_STATE is not JSON".to_owned())?;
    let cookies = localhost_cookie_header(&state)
        .ok_or("the session state holds no localhost cookies: sign in again")?;
    let auth_url = optional_env("E2E_AUTH_URL").unwrap_or_else(|| "http://localhost:3011".to_owned());
    let bff_url = optional_env("E2E_BFF_URL").unwrap_or_else(|| "http://localhost:5173".to_owned());
    let http = reqwest::Client::new();

    let token_reply = http
        .get(format!("{}/api/inference-core/token", auth_url.trim_end_matches('/')))
        .header("Cookie", &cookies)
        .send()
        .await
        .map_err(|_| "Auth Core is not reachable at E2E_AUTH_URL".to_owned())?;
    if !token_reply.status().is_success() {
        return Err(format!(
            "the session could not mint an inference token (HTTP {}): sign in again",
            token_reply.status().as_u16()
        ));
    }
    let token_reply: serde_json::Value = token_reply
        .json()
        .await
        .map_err(|_| "the token reply is not JSON".to_owned())?;
    let bearer = token_reply["token"]
        .as_str()
        .ok_or("the token reply has no token")?
        .to_owned();
    let claims = jwt_claims(&bearer)?;
    if claims["zdr"].as_bool() == Some(true) {
        return Err("the account policy is zero data retention: the subscription route is refused".to_owned());
    }
    let org_id = claims["org_id"]
        .as_str()
        .filter(|org| !org.is_empty())
        .ok_or("the token has no active org")?
        .to_owned();

    let connections = http
        .get(format!("{}/api/v1/integrations/connections", bff_url.trim_end_matches('/')))
        .header("Cookie", &cookies)
        .header("x-verevon-org-id", &org_id)
        .send()
        .await
        .map_err(|_| "the BFF is not reachable at E2E_BFF_URL".to_owned())?;
    if !connections.status().is_success() {
        return Err(format!(
            "could not read the subscription connections (HTTP {})",
            connections.status().as_u16()
        ));
    }
    let connections: serde_json::Value = connections
        .json()
        .await
        .map_err(|_| "the connections reply is not JSON".to_owned())?;
    let connection_id = active_subscription_connection(&connections)
        .ok_or("no active ChatGPT connection: connect ChatGPT in Verevon Integrations; no fallback is allowed")?;
    Ok(ProbeAuth { bearer, connection_id, org_id })
}

/// Whether `claims` are the ones inference-core turns into a user principal for
/// `org_id` with provider-invoke authority: a `user` principal whose `user_id`
/// is its `sub`, with no service marker, in that org, with `inference:invoke`.
/// A service token fails inference-core's subscription route, so this says so
/// before a call is made. The caller still has to prove the signature: only
/// inference-core verifies the token.
fn check_user_bound(claims: &serde_json::Value, org_id: &str) -> Result<(), String> {
    let user_id = claims["user_id"].as_str().unwrap_or_default();
    let user_principal = matches!(claims["principal_type"].as_str(), None | Some("user"))
        && !user_id.is_empty()
        && user_id.trim() == user_id
        && claims["sub"].as_str() == Some(user_id)
        && claims["service_id"].is_null()
        && claims["reason"].is_null();
    if !user_principal {
        return Err("the token is not a user token: a subscription connection needs the signed-in user's own token, not a service token".to_owned());
    }
    if claims["org_id"].as_str() != Some(org_id) {
        return Err("the token's org is not the org the probe will send".to_owned());
    }
    let invokes = claims["scopes"]
        .as_array()
        .is_some_and(|scopes| scopes.iter().any(|scope| scope.as_str() == Some("inference:invoke")));
    if !invokes {
        return Err("the token lacks the inference:invoke scope".to_owned());
    }
    Ok(())
}

/// `E2E_INFERENCE_BEARER` + `E2E_SUBSCRIPTION_CONNECTION_ID` + `E2E_ORG_ID` when
/// the bearer is set, otherwise the session in `E2E_SESSION_STATE`. Either way
/// the token must be a user token for the org (see [`check_user_bound`]).
async fn probe_auth() -> ProbeAuth {
    let auth = if optional_env("E2E_INFERENCE_BEARER").is_some() {
        ProbeAuth {
            bearer: required_env("E2E_INFERENCE_BEARER"),
            connection_id: required_env("E2E_SUBSCRIPTION_CONNECTION_ID"),
            org_id: required_env("E2E_ORG_ID"),
        }
    } else {
        let state = optional_env("E2E_SESSION_STATE").unwrap_or_else(|| {
            panic!("set E2E_SESSION_STATE (a Playwright storage state), or E2E_INFERENCE_BEARER with E2E_SUBSCRIPTION_CONNECTION_ID and E2E_ORG_ID; see the test's doc comment")
        });
        auth_from_session_state(&state)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
    };
    jwt_claims(&auth.bearer)
        .and_then(|claims| check_user_bound(&claims, &auth.org_id))
        .unwrap_or_else(|error| panic!("{error}"));
    auth
}

/// The request the probe sends for one case: exactly what the planner builds,
/// on the one route the probe may use. Panics if the route is anything else, so
/// a change here cannot send a case to another model or provider.
fn probe_request(
    org_id: &str,
    case: &ProbeCase,
    schema: &str,
    connection_id: &str,
) -> mp_contracts::model_plane::v1::InferRequest {
    let config = probe_config(org_id, case.goal);
    let mut request = planner_infer_request(&config, case.observation.as_ref(), PROBE_MODEL, schema);
    PROBE_PROVIDER.clone_into(&mut request.provider_hint);
    connection_id.clone_into(&mut request.subscription_connection_id);
    assert_eq!(request.model, PROBE_MODEL);
    assert_eq!(request.provider_hint, PROBE_PROVIDER);
    assert!(!request.subscription_connection_id.is_empty());
    assert_eq!(request.org_id, org_id);
    assert_eq!(request.structured_output_schema, schema);
    request
}

/// The probe's decision line for a finished run. `schema_name` is `current` or
/// `strict`; the other three say how the rows came out. Rows that failed at the
/// call are not a schema decision until the cause is a schema rejection, which
/// the broker log tells apart from a usage limit or a sign-in problem.
fn verdict(schema_name: &str, all_ok: bool, tolerant_needed: bool, call_failed: bool) -> &'static str {
    match (schema_name, all_ok, tolerant_needed, call_failed) {
        ("current", true, false, _) => "KEEP ACTION_SCHEMA: the route accepted it and every reply was one JSON object mapping to the expected action. ACTION_SCHEMA_STRICT is not needed.",
        ("current", true, true, _) => "ACTION_SCHEMA was accepted, but a reply needed the tolerant extractor: read planner replies with parse_action_content.",
        ("strict", true, false, _) => "ADOPT ACTION_SCHEMA_STRICT on the subscription route: accepted, and every reply was one JSON object mapping to the expected action.",
        ("strict", true, true, _) => "ADOPT ACTION_SCHEMA_STRICT and parse_action_content on the subscription route: accepted, but a reply needed the tolerant extractor.",
        ("current", false, _, true) => "ACTION_SCHEMA FAILED at the call. Read the row, then the broker log (docker logs integration-api | grep codex_subscription_upstream): error_code=bad_request with reason subscription_broker_failed (app-server transport), or reason subscription_bad_request (direct transport), is a schema rejection: rerun with E2E_PLANNER_SCHEMA=strict. subscription_usage_limit, subscription_reauth_required or an upstream-unavailable reason is NOT a schema result: fix that and rerun.",
        ("strict", false, _, true) => "ACTION_SCHEMA_STRICT FAILED at the call. Not a schema decision unless the broker log (docker logs integration-api | grep codex_subscription_upstream) shows error_code=bad_request: a usage limit, a sign-in problem or an upstream outage is a reason to rerun later, not to change the schema.",
        (_, false, _, false) => "The route took the schema but a reply did not map to the expected action: read the replies above before deciding.",
        _ => unreachable!("schema name was validated before the run"),
    }
}

/// E3 spike. Sends up to 4 real `Infer` calls through inference-core on the
/// user's `ChatGPT` Terra subscription (never Balance) with exactly the request
/// the planner builds (`planner_infer_request`), and reports whether the route
/// accepts the schema and whether the replies parse into the expected actions:
/// navigate, click-by-ref, type-by-ref and done. It stops at the first call that
/// is rejected or unparseable, so a schema rejection costs one call.
///
/// Run it once with the plain schema (the default). If it is rejected, or a
/// reply is fenced prose, run it again with `E2E_PLANNER_SCHEMA=strict`. The
/// verdict line says which schema variant, and whether the tolerant extractor
/// (`parse_action_content`), the planner should use on the subscription route.
/// A plain-schema rejection costs one call (the first), so the whole spike is at
/// most 1 + 4 Terra calls.
///
/// Credentials come from a local Playwright session (one variable), the way the
/// labelled review eval gets them: the session mints an `aud=inference-core`
/// token at Auth Core and the BFF lists the user's connections.
///
/// ```bash
/// E2E_SESSION_STATE="<repo>/apps/Frontend Plane/verevonv3/tests/e2e/.auth/local-state.json" \
/// INFERENCE_CORE_URL=http://127.0.0.1:9092 \
/// E2E_PLANNER_DRY_RUN=1 \
/// cargo test -p execution-core --test browser_agent_e2e planner_on_terra -- --ignored --nocapture
/// ```
///
/// That is the free dry run: it resolves the credentials, asks inference-core's
/// catalog (no model call) whether the bearer is accepted and Terra is offered
/// on the subscription provider, builds every request and checks its route, and
/// stops before any `Infer`. Drop `E2E_PLANNER_DRY_RUN` to spend the calls, and
/// add `E2E_PLANNER_SCHEMA=strict` for the second pass.
///
/// Or pass the credentials yourself, which wins over the session:
/// `E2E_INFERENCE_BEARER` (user-bound, `aud=inference-core`, scope
/// `inference:invoke`), `E2E_SUBSCRIPTION_CONNECTION_ID` (owned by that user)
/// and `E2E_ORG_ID` (the bearer's active org).
///
/// `E2E_PLANNER_SCHEMA` is `current` (default, `ACTION_SCHEMA`) or `strict`
/// (`ACTION_SCHEMA_STRICT`). `E2E_PLANNER_CASES` optionally limits the run to a
/// comma-separated list of case names. `E2E_AUTH_URL` and `E2E_BFF_URL` move the
/// session lookups off `localhost:3011` and `localhost:5173`. The test never
/// prints the bearer, the cookies or the connection id.
///
/// Every call carries `cache-control: no-store`, because inference-core caches
/// identical prompts for five minutes and a cached reply would read as a live
/// one; the test fails a row whose reply does not echo the header back.
///
/// Pass criteria, per schema: all four cases come back as one JSON object each
/// (`exact`) that maps to the expected action, with the click and the type bound
/// to the observation's own `snapshot_id`, `generation` and `ref_id`. `tolerant`
/// rows mean the planner needs `parse_action_content`; a failed call is only a
/// schema answer when the broker log says so (see the verdict text).
#[tokio::test]
#[ignore = "spends ChatGPT Terra subscription calls; needs a live inference-core + broker; see doc comment"]
#[allow(clippy::too_many_lines)]
async fn planner_on_terra_returns_valid_action_json() {
    let url = std::env::var("INFERENCE_CORE_URL")
        .or_else(|_| std::env::var("INFERENCE_CORE_ADDR"))
        .unwrap_or_else(|_| "http://127.0.0.1:9092".to_owned());
    let schema_name = std::env::var("E2E_PLANNER_SCHEMA").unwrap_or_else(|_| "current".to_owned());
    let schema = match schema_name.as_str() {
        "current" => ACTION_SCHEMA,
        "strict" => ACTION_SCHEMA_STRICT,
        other => panic!("E2E_PLANNER_SCHEMA must be `current` or `strict`, got `{other}`"),
    };
    let dry_run = optional_env("E2E_PLANNER_DRY_RUN").is_some_and(|value| matches!(value.as_str(), "1" | "true"));
    let only: Option<Vec<String>> = std::env::var("E2E_PLANNER_CASES").ok().map(|raw| {
        raw.split(',')
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect()
    });
    let cases: Vec<ProbeCase> = probe_cases()
        .into_iter()
        .filter(|case| only.as_ref().is_none_or(|names| names.iter().any(|n| n == case.name)))
        .collect();
    assert!(!cases.is_empty(), "E2E_PLANNER_CASES matched no case");
    assert!(cases.len() <= MAX_PROBE_CALLS, "the spike is capped at {MAX_PROBE_CALLS} calls");

    let auth = probe_auth().await;
    let mut client = InferenceCoreClient::connect(url)
        .await
        .expect("inference-core is reachable at INFERENCE_CORE_URL");
    let authorization: tonic::metadata::MetadataValue<_> = format!("Bearer {}", auth.bearer)
        .parse()
        .expect("the bearer is a valid header value");

    // Free check before any model call: a catalog read needs the same verified
    // bearer, and shows whether Terra is offered on the subscription provider.
    let mut catalog_request = tonic::Request::new(ListModelsRequest {
        modality: "chat".to_owned(),
        provider: String::new(),
    });
    catalog_request.metadata_mut().insert("authorization", authorization.clone());
    let catalog = client.list_models(catalog_request).await;
    let bearer_accepted = catalog.is_ok();
    let terra_offered = catalog.as_ref().is_ok_and(|reply| {
        reply
            .get_ref()
            .models
            .iter()
            .any(|model| model.id == PROBE_MODEL && model.provider == PROBE_PROVIDER)
    });
    eprintln!(
        "catalog check (no model call): bearer accepted={bearer_accepted} terra offered on {PROBE_PROVIDER}={terra_offered}{}",
        catalog.as_ref().err().map_or_else(String::new, |status| format!(" (code={:?})", status.code()))
    );

    eprintln!(
        "planner_on_terra_returns_valid_action_json: schema={schema_name} model={PROBE_MODEL} provider={PROBE_PROVIDER} calls<={} dry_run={dry_run}",
        cases.len()
    );
    if dry_run {
        for case in &cases {
            let request = probe_request(&auth.org_id, case, schema, &auth.connection_id);
            eprintln!(
                "  would send {:<13} prompt_chars={}",
                case.name,
                request.messages.iter().map(|message| message.content.len()).sum::<usize>()
            );
        }
        assert!(bearer_accepted, "the bearer was not accepted by inference-core's catalog read");
        assert!(terra_offered, "{PROBE_MODEL} is not offered on {PROBE_PROVIDER}");
        eprintln!("DRY RUN OK: credentials resolved, bearer accepted, Terra offered, {} request(s) built on the subscription route, no Infer sent.", cases.len());
        return;
    }

    let mut rows: Vec<Row> = Vec::new();
    for case in &cases {
        let request = probe_request(&auth.org_id, case, schema, &auth.connection_id);
        let mut grpc_request = tonic::Request::new(request);
        grpc_request.metadata_mut().insert("authorization", authorization.clone());
        // Never read this probe's reply from the response cache.
        grpc_request.metadata_mut().insert("cache-control", "no-store".parse().expect("static metadata"));
        let started = Instant::now();
        let reply = tokio::time::timeout(REPLY_TIMEOUT, client.infer(grpc_request)).await;
        let elapsed_ms = started.elapsed().as_millis();
        let (response, no_store_echoed) = match reply {
            Err(_) => {
                rows.push(Row {
                    name: case.name,
                    call_failed: true,
                    mode: None,
                    problem: Some(format!("no reply within {} s", REPLY_TIMEOUT.as_secs())),
                });
                break;
            }
            Ok(Err(status)) => {
                let message: String = status.message().chars().take(300).collect();
                rows.push(Row {
                    name: case.name,
                    call_failed: true,
                    mode: None,
                    problem: Some(format!("call rejected: code={:?} message={message}", status.code())),
                });
                break;
            }
            Ok(Ok(response)) => {
                let echoed = response
                    .metadata()
                    .get("cache-control")
                    .is_some_and(|value| value == "no-store");
                (response.into_inner(), echoed)
            }
        };
        assert_eq!(
            response.provider_used, PROBE_PROVIDER,
            "the probe left the subscription route"
        );
        assert_eq!(response.model_used, PROBE_MODEL, "the probe left Terra");
        eprintln!(
            "--- {} ({elapsed_ms} ms, in={} out={} stop={})\n{}",
            case.name,
            response.input_tokens,
            response.output_tokens,
            response.stop_reason,
            response.content
        );
        let cache_problem = (!no_store_echoed).then(|| {
            "inference-core did not echo cache-control: no-store, so this reply may come from its cache".to_owned()
        });
        match parse_action_content(&response.content) {
            Err(error) => {
                rows.push(Row {
                    name: case.name,
                    call_failed: false,
                    mode: None,
                    problem: Some(format!("unparseable reply: {error}")),
                });
                break;
            }
            Ok((action, mode)) => rows.push(Row {
                name: case.name,
                call_failed: false,
                mode: Some(mode),
                problem: judge(case.expect, action.as_ref()).err().or(cache_problem),
            }),
        }
    }

    eprintln!("--- results (schema={schema_name})");
    for row in &rows {
        eprintln!(
            "  {:<13} {:<9} {}",
            row.name,
            match (row.call_failed, row.mode) {
                (true, _) => "NO-REPLY",
                (false, None) => "NO-PARSE",
                (false, Some(ActionParse::Exact)) => "exact",
                (false, Some(ActionParse::Tolerant)) => "tolerant",
            },
            row.problem.as_deref().unwrap_or("ok")
        );
    }
    let all_ok = rows.len() == cases.len() && rows.iter().all(|row| row.problem.is_none());
    let tolerant_needed = rows.iter().any(|row| row.mode == Some(ActionParse::Tolerant));
    let call_failed = rows.iter().any(|row| row.call_failed);
    let decision = verdict(&schema_name, all_ok, tolerant_needed, call_failed);
    eprintln!("VERDICT: {decision}");
    assert!(all_ok, "{decision}");
}

// ---------------------------------------------------------------------------
// Offline checks of the spike harness itself: no network, no model call. They
// keep a typo in a fixture or a pass criterion from costing Terra quota.
// ---------------------------------------------------------------------------

fn probe_case(name: &str) -> ProbeCase {
    probe_cases()
        .into_iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("no probe case named {name}"))
}

/// Judges a literal planner reply against the named case, reading it with the
/// same function the live probe uses.
fn judged(case_name: &str, reply: &str) -> Result<(), String> {
    let (action, _) = parse_action_content(reply).expect("the reply is an action object");
    judge(probe_case(case_name).expect, action.as_ref())
}

#[test]
fn probe_fixtures_agree_with_what_each_case_expects() {
    let cases = probe_cases();
    assert_eq!(cases.len(), 4, "navigate, click_by_ref, type_by_ref and done");
    assert!(cases.len() <= MAX_PROBE_CALLS);
    let names: std::collections::BTreeSet<&str> = cases.iter().map(|case| case.name).collect();
    assert_eq!(names.len(), cases.len(), "case names must be unique");
    for case in &cases {
        match (case.expect, &case.observation) {
            (Expect::Navigate, None) | (Expect::Done, Some(_)) => {}
            (
                Expect::ClickRef { ref_id, snapshot_id, generation }
                | Expect::TypeRef { ref_id, snapshot_id, generation },
                Some(observed),
            ) => {
                assert_eq!(observed.dom_snapshot_ref, snapshot_id, "{}", case.name);
                assert_eq!(observed.snapshot_generation, Some(generation), "{}", case.name);
                let role = observed
                    .snapshot_targets
                    .iter()
                    .find(|target| target.ref_id == ref_id)
                    .unwrap_or_else(|| panic!("{}: the expected ref is not in the observation", case.name))
                    .role
                    .clone();
                let wanted = if matches!(case.expect, Expect::ClickRef { .. }) { "link" } else { "searchbox" };
                assert_eq!(role.as_deref(), Some(wanted), "{}", case.name);
            }
            _ => panic!("{}: the expectation and the observation do not fit", case.name),
        }
    }
}

#[test]
fn every_probe_request_is_on_the_terra_subscription_route() {
    for schema in [ACTION_SCHEMA, ACTION_SCHEMA_STRICT] {
        for case in &probe_cases() {
            let request = probe_request("org_x", case, schema, "conn_x");
            assert_eq!(request.model, "gpt-5.6-terra", "{}", case.name);
            assert_eq!(request.provider_hint, "openai-codex-subscription", "{}", case.name);
            assert_eq!(request.subscription_connection_id, "conn_x", "{}", case.name);
            assert_eq!(request.org_id, "org_x", "{}", case.name);
            assert_eq!(request.structured_output_schema, schema, "{}", case.name);
            // The subscription provider refuses tools together with a schema.
            assert!(request.tools.is_empty(), "{}", case.name);
            assert!(!request.zdr, "{}", case.name);
        }
    }
}

#[test]
fn the_pass_criteria_accept_the_right_action_and_reject_each_wrong_one() {
    // navigate
    assert!(judged("navigate", r#"{"action":"navigate","url":"https://example.com/"}"#).is_ok());
    assert!(judged("navigate", r#"{"action":"navigate","url":"https://other.test/"}"#).is_err());
    assert!(judged("navigate", r#"{"action":"scroll"}"#).is_err());
    assert!(judged("navigate", r#"{"action":"done"}"#).is_err());

    // click by ref: the exact snapshot binding, not a selector
    let click = |ref_id: &str, generation: i64| {
        format!(r#"{{"action":"click","snapshot_id":"snap-7f3a","generation":{generation},"ref_id":"{ref_id}"}}"#)
    };
    assert!(judged("click_by_ref", &click("@e2", 4)).is_ok());
    assert!(judged("click_by_ref", &click("@e1", 4)).is_err(), "the heading is not the link");
    assert!(judged("click_by_ref", &click("@e2", 3)).is_err(), "a stale generation");
    assert!(judged("click_by_ref", r#"{"action":"click","selector":"a"}"#).is_err(), "a selector is no binding");
    assert!(judged("click_by_ref", r#"{"action":"click","snapshot_id":"snap-7f3a","ref_id":"@e2"}"#).is_err(), "no generation");
    assert!(judged("click_by_ref", r#"{"action":"scroll"}"#).is_err());

    // type by ref
    let typed = |value: &str, ref_id: &str| {
        format!(r#"{{"action":"type","snapshot_id":"snap-b21c","generation":1,"ref_id":"{ref_id}","value":"{value}"}}"#)
    };
    assert!(judged("type_by_ref", &typed("rust lang", "@e1")).is_ok());
    assert!(judged("type_by_ref", &typed("Rust Lang", "@e1")).is_ok(), "case does not matter");
    assert!(judged("type_by_ref", &typed("python", "@e1")).is_err(), "the wrong text");
    assert!(judged("type_by_ref", &typed("rust lang", "@e2")).is_err(), "the button is not the box");
    assert!(judged("type_by_ref", &click("@e1", 1)).is_err(), "a click is not a type");

    // done is the only terminal answer, and only where it is expected
    assert!(judged("done", r#"{"action":"done"}"#).is_ok());
    assert!(judged("done", r#"{"action":"navigate","url":"https://example.com/"}"#).is_err());
    let (not_done, _) = parse_action_content(&click("@e2", 4)).expect("parses");
    assert!(judge(probe_case("navigate").expect, None).is_err(), "done where an action was expected");
    assert!(judge(probe_case("done").expect, not_done.as_ref()).is_err());
}

#[test]
fn a_strict_shaped_reply_passes_the_same_criteria() {
    // The reply a strict-mode model has to produce: every key present, the
    // unused ones as sentinels.
    let strict_click = r#"{"action":"click","selector":"","snapshot_id":"snap-7f3a","generation":4,"ref_id":"@e2","frame_id":"","value":"","url":"","reason":"open the link","risk_category":"none"}"#;
    let strict_navigate = r#"{"action":"navigate","selector":"","snapshot_id":"","generation":-1,"ref_id":"","frame_id":"","value":"","url":"https://example.com","reason":"","risk_category":"none"}"#;
    let strict_done = r#"{"action":"done","selector":"","snapshot_id":"","generation":-1,"ref_id":"","frame_id":"","value":"","url":"","reason":"the title is read","risk_category":"none"}"#;
    assert!(judged("click_by_ref", strict_click).is_ok());
    assert!(judged("navigate", strict_navigate).is_ok());
    assert!(judged("done", strict_done).is_ok());
    // And it is read as one object, not through the tolerant extractor.
    for reply in [strict_click, strict_navigate, strict_done] {
        assert_eq!(parse_action_content(reply).expect("parses").1, ActionParse::Exact);
    }
}

#[test]
fn the_verdict_names_each_outcome() {
    for schema in ["current", "strict"] {
        let accepted = verdict(schema, true, false, false);
        let tolerant = verdict(schema, true, true, false);
        let call = verdict(schema, false, false, true);
        let reply = verdict(schema, false, false, false);
        assert!(accepted.starts_with(if schema == "current" { "KEEP" } else { "ADOPT" }), "{accepted}");
        assert!(tolerant.contains("tolerant extractor"), "{tolerant}");
        assert!(call.contains("FAILED at the call"), "{call}");
        assert!(call.contains("codex_subscription_upstream"), "{call}");
        assert!(reply.contains("did not map"), "{reply}");
        // A failed call is never reported as a map failure or an acceptance.
        assert_ne!(call, reply);
        assert_ne!(call, accepted);
        // Rows that failed at the call whatever else is true are still a call failure.
        assert_eq!(verdict(schema, false, true, true), call);
    }
    // Only the plain schema's failure tells the operator to try the strict one.
    assert!(verdict("current", false, false, true).contains("E2E_PLANNER_SCHEMA=strict"));
    assert!(!verdict("strict", false, false, true).contains("rerun with E2E_PLANNER_SCHEMA=strict"));
}

#[test]
#[should_panic(expected = "schema name was validated before the run")]
fn an_unknown_schema_name_has_no_verdict() {
    let _ = verdict("lenient", true, false, false);
}

#[test]
fn only_localhost_cookies_are_sent_to_the_local_stack() {
    let state = serde_json::json!({"cookies": [
        {"name": "a", "value": "1", "domain": "localhost"},
        {"name": "b", "value": "2", "domain": ".localhost"},
        {"name": "c", "value": "3", "domain": "example.com"},
        {"name": "d", "value": "4", "domain": "localhost.evil.test"},
        {"name": "e", "domain": "localhost"}
    ]});
    assert_eq!(localhost_cookie_header(&state).as_deref(), Some("a=1; b=2"));
    assert_eq!(localhost_cookie_header(&serde_json::json!({"cookies": []})), None);
    assert_eq!(localhost_cookie_header(&serde_json::json!({})), None);
    assert_eq!(
        localhost_cookie_header(&serde_json::json!({"cookies": [{"name": "c", "value": "3", "domain": "example.com"}]})),
        None
    );
}

#[test]
fn a_token_payload_is_read_for_selection_and_a_non_token_is_refused() {
    use base64::Engine as _;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"org_id":"org_1","zdr":false,"sub":"user_1"}"#);
    let claims = jwt_claims(&format!("header.{payload}.signature")).expect("reads");
    assert_eq!(claims["org_id"], "org_1");
    assert_eq!(claims["zdr"], false);
    assert!(jwt_claims("not-a-jwt").is_err());
    assert!(jwt_claims("a.%%%.c").is_err());
    let not_json = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"plain text");
    assert!(jwt_claims(&format!("a.{not_json}.c")).is_err());
}

#[test]
fn only_a_user_token_for_the_org_with_the_invoke_scope_is_accepted() {
    let user = serde_json::json!({
        "sub": "user_1", "user_id": "user_1", "org_id": "org_1",
        "principal_type": "user", "scopes": ["inference:invoke", "other"]
    });
    assert_eq!(check_user_bound(&user, "org_1"), Ok(()));
    // `principal_type` defaults to user, as it does in inference-core.
    let mut default_type = user.clone();
    default_type.as_object_mut().expect("object").remove("principal_type");
    assert_eq!(check_user_bound(&default_type, "org_1"), Ok(()));

    assert!(check_user_bound(&user, "org_2").is_err(), "another org");
    let with = |key: &str, value: serde_json::Value| {
        let mut claims = user.clone();
        claims[key] = value;
        claims
    };
    assert!(check_user_bound(&with("sub", serde_json::json!("user_2")), "org_1").is_err(), "sub is not the user");
    assert!(check_user_bound(&with("user_id", serde_json::json!("")), "org_1").is_err(), "no user");
    assert!(check_user_bound(&with("scopes", serde_json::json!(["inference:read"])), "org_1").is_err(), "no invoke");
    assert!(check_user_bound(&with("service_id", serde_json::json!("service:x")), "org_1").is_err(), "a service marker");
    let service = serde_json::json!({
        "sub": "service:execution-core", "service_id": "service:execution-core", "org_id": "org_1",
        "principal_type": "service", "reason": "scheduled run", "scopes": ["inference:invoke"]
    });
    assert!(check_user_bound(&service, "org_1").is_err(), "a service token has no user to own a connection");
}

#[test]
fn the_connection_is_the_active_chatgpt_one_in_any_known_response_shape() {
    let rows = serde_json::json!([
        {"id": "conn_other", "providerKey": "microsoft-365", "status": "active"},
        {"id": "conn_revoked", "providerKey": "openai-codex-subscription", "status": "revoked"},
        {"id": "conn_deleted", "providerKey": "openai-codex-subscription", "status": "active", "deletedAt": "2026-09-01T00:00:00Z"},
        {"id": "conn_deleted_snake", "provider_key": "openai-codex-subscription", "status": "active", "deleted_at": "2026-09-01T00:00:00Z"},
        {"id": "conn_good", "providerKey": "openai-codex-subscription", "status": "ACTIVE", "deletedAt": ""},
        {"id": "conn_later", "providerKey": "openai-codex-subscription", "status": "active"}
    ]);
    let want = Some("conn_good".to_owned());
    assert_eq!(active_subscription_connection(&rows), want, "a bare array");
    assert_eq!(active_subscription_connection(&serde_json::json!({"data": rows})), want, "data array");
    assert_eq!(
        active_subscription_connection(&serde_json::json!({"data": {"connections": rows}})),
        want,
        "data.connections"
    );
    let snake = serde_json::json!([{"id": "conn_snake", "provider_key": "openai-codex-subscription", "status": "active"}]);
    assert_eq!(active_subscription_connection(&snake), Some("conn_snake".to_owned()));
    let none = serde_json::json!([{"id": "x", "providerKey": "openai-codex-subscription", "status": "revoked"}]);
    assert_eq!(active_subscription_connection(&none), None);
    assert_eq!(active_subscription_connection(&serde_json::json!({"unexpected": true})), None);
}

#[tokio::test]
async fn a_session_failure_never_carries_a_cookie_a_path_or_an_id() {
    let secret = "super-secret-cookie-value";
    let path = std::env::temp_dir().join(format!("e3-probe-state-{}.json", std::process::id()));
    std::fs::write(
        &path,
        serde_json::json!({"cookies": [{"name": "session", "value": secret, "domain": "localhost"}]}).to_string(),
    )
    .expect("writes the fake state");
    // Nothing listens on port 1, so the token request fails at connect.
    std::env::set_var("E2E_AUTH_URL", "http://127.0.0.1:1");
    let error = auth_from_session_state(path.to_str().expect("utf-8 path"))
        .await
        .err()
        .expect("Auth Core is unreachable");
    std::env::remove_var("E2E_AUTH_URL");
    let _ = std::fs::remove_file(&path);
    assert!(!error.contains(secret), "{error}");
    assert!(!error.contains("e3-probe-state"), "{error}");

    let missing = auth_from_session_state("Z:/does/not/exist.json").await.err().expect("no such file");
    assert!(!missing.contains("does/not/exist"), "{missing}");
    let empty = std::env::temp_dir().join(format!("e3-probe-empty-{}.json", std::process::id()));
    std::fs::write(&empty, r#"{"cookies": []}"#).expect("writes the empty state");
    let signed_out = auth_from_session_state(empty.to_str().expect("utf-8 path")).await.err().expect("no cookies");
    let _ = std::fs::remove_file(&empty);
    assert!(signed_out.contains("sign in again"), "{signed_out}");
}
