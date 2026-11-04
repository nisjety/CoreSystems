//! The MCP golden, model-gateway side (build plan WS05, "ZDR clause and regression guards"; register row
//! D1-007): under zero retention the gateway writes no MCP tool observation, and its MCP path is never
//! served to a zdr caller.
//!
//! `fixtures/zdr-posture-matrix.json` is a byte-identical copy of the canonical matrix, pinned beside it and
//! checked by `scripts/tests/zdr-contract-sync.test.sh`. Every `enforcement_rows` row is decided the way a
//! request reaches the gateway's MCP path: each bearer is a real `Claims` (the serde path the middleware
//! uses), the route is the plain route the MCP RPCs use (`Route::plain(false)`, as `grpc.rs` `list_mcp_tools`
//! and `proxy_mcp_tool` do), and the decision is `zdr_posture::decide`, the resolver entry every gateway
//! decision goes through. Two gates then answer, as production answers them:
//!
//! * the MCP proxy's contract (`grpc.rs` `require_explicit_zdr_contract`, the `mcp_execution` gate): a zdr
//!   caller is refused with the typed refusal, a persistent caller is served, and a refused or waived caller
//!   is refused with the resolver's own refusal;
//! * the listing's observation gate (`grpc.rs` `list_mcp_tools`, `capability_observation::should_observe`):
//!   a write is made only for a caller that is positively not ZDR, and only with the switch on. A write the
//!   gate opens is made through the real `spawn_observation`, so the count below is of real POSTs to a mock
//!   capability-core.
//!
//! `VerifiedIdentity` is built only inside `grpc_auth`, so the identity's `zdr_for` is replayed through the
//! same resolver call it makes (`mcp_contract`). The wiring from the gates to the identity and to the write
//! is pinned by reading the source (`the_mcp_gates_are_wired_to_the_plain_route_posture`).

use model_gateway::auth::Claims;
use model_gateway::mcp_jsonrpc::{McpToolAnnotations, McpToolDef};
use model_gateway::runtime_registries::capability_observation::{
    should_observe, spawn_observation, ObservationLedger, ToolObservation,
};
use model_gateway::zdr_posture::{decide, Route, TurnPosture};
use mp_contracts::zdr_refusal::{code, Refusal};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

/// The enforcement rows of the canonical matrix when this test was written, and how they split by decision.
/// A row added or removed without this file being read is a change nobody reviewed against the MCP gate.
const ENFORCEMENT_ROWS: usize = 33;
const ZDR: usize = 7;
const PERSISTENT: usize = 3;
const WAIVED: usize = 4;
const REFUSE: usize = 18;

/// The root secret the observation post is signed with in the test. Any non-empty value serves: the golden
/// counts posts and does not check the credential, which `capability_observation`'s own tests pin.
const ROOT_SECRET: &str = "mcp-observation-golden-root-secret";
const ORG: &str = "org-1";

/// The rows where the gateway's plain-route answer differs from what the matrix says the turn is. Each is
/// pinned, not accepted: a change to any of them fails the test, which makes the change a deliberate one.
fn known_limit(row_id: &str) -> Option<&'static str> {
    match row_id {
        // zdr in the matrix. The listing RPC carries no explicit-ZDR field (`ListMcpToolsRequest` has
        // request_id, org_id, user_id and unstaged only), so the gate sees the token's own posture, which is
        // persistent here, and writes. The run path never asks: execution-core passes `req.zdr` into the
        // offer (`agent.rs` `offered_tools`, `retention_bound`) and never lists tools for a retention-bound
        // run (`merged_tool_defs_governed`, the `!zdr` guard and `resolve_run_offer` in `Off` mode). Owner
        // decision: add an explicit flag to the listing RPC, or keep execution-core's guard as the control.
        "e_explicit_zdr_raises_persistent_token" => Some(
            "zdr: the listing RPC cannot carry the explicit flag, so the gate sees a persistent caller",
        ),
        // refuse in the matrix. The turn is refused at the run boundary (`grpc.rs` `enforce_agent_route`,
        // `agent_route.rs` `validate_run_route`) before any listing is asked, but the gate sees a persistent
        // caller on the plain route.
        "e_temporary_chat_refuses_subscription_org_off" => Some(
            "refuse: refused at the run boundary before any listing, but the gate sees a persistent caller",
        ),
        _ => None,
    }
}

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the vendored matrix is JSON")
}

fn rows(matrix: &Value) -> &Vec<Value> {
    matrix["enforcement_rows"]
        .as_array()
        .expect("enforcement_rows is an array")
}

fn id(row: &Value) -> &str {
    row["id"].as_str().expect("a row has an id")
}

fn expected(row: &Value) -> &str {
    row["expect"]["decision"]
        .as_str()
        .expect("a row expects a decision")
}

/// The claims of one bearer, as the token payload Auth Core mints carries them.
fn bearer_claims(bearer: &Value) -> Claims {
    let mut payload = json!({
        "sub": "user-1",
        "iss": "https://auth.test/model",
        "aud": "model-gateway",
        "exp": 4_102_444_800_i64,
        "nbf": 1_700_000_000_i64,
        "org_id": "org-1",
        "user_id": "user-1",
        "principal_type": "user",
        "zdr": bearer["zdr"],
    });
    if !bearer["org_zdr"].is_null() {
        payload["org_zdr"] = bearer["org_zdr"].clone();
    }
    if !bearer["waiver"].is_null() {
        payload["zdr_waiver"] = bearer["waiver"].clone();
    }
    serde_json::from_value(payload).expect("a minted token's payload is valid Claims")
}

/// Every bearer of a row, as the claims a request presents.
fn bearers_of(row: &Value) -> Vec<Claims> {
    row["inputs"]["bearers"]
        .as_array()
        .expect("bearers are an array")
        .iter()
        .map(bearer_claims)
        .collect()
}

/// `grpc.rs` `require_explicit_zdr_contract(identity, "mcp_execution")`, through the call its identity makes.
/// `VerifiedIdentity::zdr_for(&Route::plain(false))` is `posture(route).map(zdr)`, and `posture` is
/// `decide_views` over the identity's one ingress token. A request whose bearers agree has the same posture
/// on every bearer, so `decide` over all of them answers as the identity does; a request whose bearers
/// disagree is refused before any identity exists (`grpc_auth`), which `decide` also refuses.
fn mcp_contract(posture: Result<TurnPosture, Refusal>) -> Result<(), Refusal> {
    match posture {
        Ok(posture) if posture.zdr() => Err(Refusal::feature_not_supported("mcp_execution")),
        Ok(_) => Ok(()),
        Err(refusal) => Err(refusal),
    }
}

fn is_typed_mcp_refusal(contract: &Result<(), Refusal>) -> bool {
    matches!(
        contract,
        Err(refusal) if refusal.code() == code::FEATURE_NOT_SUPPORTED
            && refusal.feature() == Some("mcp_execution")
    )
}

fn is_waiver_route_mismatch(contract: &Result<(), Refusal>) -> bool {
    matches!(contract, Err(refusal) if refusal.code() == code::WAIVER_ROUTE_MISMATCH)
}

/// Source text with every whitespace character removed, so a check reads the code and not its layout.
fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

async fn posts_received(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .map_or(0, |requests| requests.len())
}

#[test]
fn the_vendored_matrix_has_the_rows_this_test_was_written_for() {
    let matrix = matrix();
    assert_eq!(matrix["contract"], "zdr.posture-matrix");
    let rows = rows(&matrix);
    assert_eq!(
        rows.len(),
        ENFORCEMENT_ROWS,
        "a changed matrix must be reviewed against the MCP gate"
    );
    let count = |wanted: &str| rows.iter().filter(|row| expected(row) == wanted).count();
    assert_eq!(count("zdr"), ZDR);
    assert_eq!(count("persistent"), PERSISTENT);
    assert_eq!(count("waived"), WAIVED);
    assert_eq!(count("refuse"), REFUSE);
}

/// Every enforcement row, through the plain-route decision, the MCP contract and the observation gate. A zdr
/// row writes no observation and is refused as `mcp_execution`; a persistent row writes one and is served.
/// The writes are counted at the mock capability-core.
#[tokio::test]
async fn mcp_tool_observation_under_zdr() {
    let matrix = matrix();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"results": [{"outcome": "recorded"}]})),
        )
        .mount(&server)
        .await;
    let tool = McpToolDef {
        name: "echo_read".to_owned(),
        description: "Reads one item.".to_owned(),
        input_schema_json: r#"{"type":"object"}"#.to_owned(),
        annotations: McpToolAnnotations {
            read_only_hint: Some(true),
            ..McpToolAnnotations::default()
        },
    };
    let observed = ToolObservation::from_tool("mcp_srv", &tool);
    let client = reqwest::Client::new();
    let (mut zdr_rows, mut persistent_rows, mut writes) = (0_usize, 0_usize, 0_usize);
    let mut failures = Vec::new();
    for row in rows(&matrix) {
        let row_id = id(row);
        let claims = bearers_of(row);
        let refs: Vec<&Claims> = claims.iter().collect();
        let posture = decide(&refs, &Route::plain(false));
        let caller_zdr = posture.as_ref().ok().map(TurnPosture::zdr);
        let observe = should_observe(true, caller_zdr);
        if should_observe(false, caller_zdr) {
            failures.push(format!(
                "{row_id}: the switch is off, yet the listing would observe"
            ));
        }
        let contract = mcp_contract(posture);
        let pinned = known_limit(row_id);
        match expected(row) {
            "zdr" => {
                zdr_rows += 1;
                match pinned {
                    Some(why) => {
                        if !(observe && contract.is_ok()) {
                            failures.push(format!(
                                "{row_id}: the pinned known limit ({why}) changed: observe={observe}, contract={contract:?}"
                            ));
                        }
                    }
                    None => {
                        if observe {
                            failures.push(format!("{row_id}: a zdr row writes an observation"));
                        }
                        if !is_typed_mcp_refusal(&contract) {
                            failures.push(format!(
                                "{row_id}: a zdr row is not refused as mcp_execution: {contract:?}"
                            ));
                        }
                    }
                }
            }
            "persistent" => {
                persistent_rows += 1;
                if !observe {
                    failures.push(format!("{row_id}: a persistent row writes no observation"));
                }
                if let Err(refusal) = contract {
                    failures.push(format!(
                        "{row_id}: a persistent row is refused: {refusal:?}"
                    ));
                }
            }
            "waived" => match pinned {
                Some(why) => failures.push(format!("{row_id}: a waived row is pinned ({why})")),
                None => {
                    if observe {
                        failures.push(format!("{row_id}: a waived row writes an observation"));
                    }
                    if !is_waiver_route_mismatch(&contract) {
                        failures.push(format!(
                            "{row_id}: a waived row is not refused on the plain route as a route mismatch: {contract:?}"
                        ));
                    }
                }
            },
            "refuse" => match pinned {
                Some(why) => {
                    if !(observe && contract.is_ok()) {
                        failures.push(format!(
                            "{row_id}: the pinned known limit ({why}) changed: observe={observe}, contract={contract:?}"
                        ));
                    }
                }
                None => {
                    if observe {
                        failures.push(format!("{row_id}: a refused row writes an observation"));
                    }
                    if contract.is_ok() {
                        failures.push(format!(
                            "{row_id}: a refused row is served on the plain route"
                        ));
                    }
                }
            },
            other => failures.push(format!("{row_id}: unknown decision {other}")),
        }
        if observe {
            writes += 1;
            spawn_observation(
                &ObservationLedger::default(),
                &client,
                &server.uri(),
                ROOT_SECRET,
                ORG,
                std::slice::from_ref(&observed),
            );
        }
    }
    // The posts are made by spawned tasks: wait for the ones the gate opened, then give any extra post time
    // to arrive, so a write the gate did not open is counted too.
    let deadline = Instant::now() + Duration::from_secs(5);
    while posts_received(&server).await < writes && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let received = posts_received(&server).await;
    if received != writes {
        failures.push(format!(
            "{received} observation post(s) reached capability-core; the gate opened for {writes}"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        (zdr_rows, persistent_rows),
        (ZDR, PERSISTENT),
        "every zdr and persistent row was driven through the gates"
    );
    eprintln!(
        "MCP observation gate driven: {zdr_rows} zdr rows wrote nothing and were refused as \
         mcp_execution; {persistent_rows} persistent rows wrote {writes} observation(s) in total"
    );
}

/// The gateway's wiring, read from source: the MCP proxy refuses a zdr caller before its service check; the
/// listing's observation flag is the switch and the caller's plain-route posture; the write happens only on
/// that flag; and the inline chat listing does not observe. Source reading proves the wiring, not the
/// behaviour, which `mcp_tool_observation_under_zdr` drives.
#[test]
fn the_mcp_gates_are_wired_to_the_plain_route_posture() {
    let grpc = compact(&include_str!("../src/grpc.rs").replace("\r\n", "\n"));
    assert!(
        grpc.contains("identity.zdr_for(&crate::zdr_posture::Route::plain(false))?"),
        "require_explicit_zdr_contract no longer asks the identity's plain-route posture"
    );
    assert!(
        grpc.contains("mp_contracts::zdr_refusal::Refusal::feature_not_supported(feature)"),
        "require_explicit_zdr_contract no longer refuses with the typed feature refusal"
    );
    let proxy = &grpc[grpc
        .find("asyncfnproxy_mcp_tool(")
        .expect("proxy_mcp_tool is defined")..];
    let contract = proxy
        .find("require_explicit_zdr_contract(&identity,\"mcp_execution\")?;")
        .expect("proxy_mcp_tool refuses a zdr caller as mcp_execution");
    let service = proxy
        .find("if!identity.is_service()")
        .expect("proxy_mcp_tool checks the service identity");
    assert!(
        contract < service,
        "the mcp_execution contract must come before the service check in proxy_mcp_tool"
    );
    let listing = &grpc[grpc
        .find("asyncfnlist_mcp_tools(")
        .expect("list_mcp_tools is defined")..];
    assert!(
        listing.contains(
            "should_observe(runtime_registries::capability_observation::observation_enabled(),identity.zdr_for(&crate::zdr_posture::Route::plain(false)).ok(),"
        ),
        "list_mcp_tools no longer derives its observation flag from the switch and the plain-route posture"
    );
    let registries = compact(&include_str!("../src/runtime_registries.rs").replace("\r\n", "\n"));
    assert!(
        registries.contains("ifobserve{capability_observation::spawn_observation("),
        "the listing no longer writes an observation only on its observe flag"
    );
    assert!(
        registries.contains("McpListingOptions::default()"),
        "the inline chat listing (mcp_tool_defs) must keep observing nothing"
    );
}
