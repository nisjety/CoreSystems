//! The Do loop's computer tools, as one contract (W4-C C1).
//!
//! The model names two computer tools, `computer.observe` and `computer.act`. Their contract sits in places no
//! unit test sees together: the names the connector serves, the capability each name is governed by, the class
//! the capability seed declares for it, the risk the name heuristics give it, the answer a zero-retention run
//! gets for every enforcement row of the vendored matrix, and the answer a run gets through the real step. This
//! file holds those places to one another.
//!
//! Most of them are private to the crate, so the checks read the source that declares them, as
//! `browser_do_contract.rs` does. The rest go through the public surface: `trusted_capability_id`,
//! `is_risky_tool`, the real resolver and `execute_step`. Source text is compared with its whitespace removed.

use execution_core::capability_policy::{
    trusted_capability_id, CapabilityDecision, CapabilityPolicy,
};
use execution_core::permission::is_risky_tool;
use execution_core::runtime_loop::execute_step;
use mp_contracts::retention::{resolve_turn, ClaimsView, Resolution, RouteView, WaiverView};
use mp_contracts::zdr_refusal::Refusal;
use regex::Regex;
use serde_json::Value;
use tonic::Status;

const CONNECTOR: &str = include_str!("../src/computer_action.rs");
const AGENT: &str = include_str!("../src/runtime_loop/agent.rs");
const RUNTIME_LOOP: &str = include_str!("../src/runtime_loop/mod.rs");
const TOOL_BRIDGE: &str = include_str!("../src/tool_bridge/mod.rs");
const CAPABILITY_SEED: &str = include_str!(
    "../../../../go/services/capability-core/migrations/0038_computer_tool_capabilities.up.sql"
);
const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

/// The enforcement row that documents the computer tools' zero-retention refusal.
const DO_COMPUTER_ROW: &str = "e_computer_tools_refused_under_zdr";

const OBSERVE_TOOL: &str = "computer.observe";
const ACT_TOOL: &str = "computer.act";
const CLICK: &str = r#"{"action":"click","target":"e1"}"#;

/// The answer to a computer call the connector cannot serve: the switch is off, as it is in every test run.
const UNAVAILABLE: &str = "computer_unavailable: The computer tools are not available right now.";

/// Source text with its line endings normalised.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Source text with every whitespace character removed.
fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The text of the item that starts with `header`, through its closing brace at column zero.
fn item<'a>(source: &'a str, header: &str) -> &'a str {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("`{header}` is not in the source"));
    let rest = &source[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |end| end + 2);
    &rest[..end]
}

/// Asserts that `source`, with whitespace removed, still carries every fragment.
fn assert_carries(source: &str, what: &str, fragments: &[&str]) {
    let code = compact(source);
    for fragment in fragments {
        assert!(
            code.contains(&compact(fragment)),
            "{what} no longer carries `{fragment}`"
        );
    }
}

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the vendored matrix is JSON")
}

fn enforcement_rows(matrix: &Value) -> &Vec<Value> {
    matrix["enforcement_rows"]
        .as_array()
        .expect("enforcement_rows is an array")
}

/// The claims of every bearer of a row, borrowing the strings of the matrix.
fn claims_of(row: &Value) -> Vec<ClaimsView<'_>> {
    row["inputs"]["bearers"]
        .as_array()
        .expect("bearers are an array")
        .iter()
        .map(|bearer| ClaimsView {
            zdr: bearer["zdr"].as_bool().expect("every bearer states zdr"),
            org_zdr: bearer["org_zdr"].as_bool(),
            waiver: (!bearer["waiver"].is_null()).then(|| WaiverView {
                version: u32::try_from(
                    bearer["waiver"]["version"]
                        .as_u64()
                        .expect("a waiver has a version"),
                )
                .expect("the version fits"),
                provider: bearer["waiver"]["provider"].as_str().expect("provider"),
                connection: bearer["waiver"]["connection"].as_str().expect("connection"),
            }),
        })
        .collect()
}

/// The route of a row, borrowing the strings of the matrix.
fn route_of(row: &Value) -> RouteView<'_> {
    let route = &row["inputs"]["route"];
    RouteView {
        provider: route["provider"].as_str().expect("a route has a provider"),
        connection: route["connection"].as_str(),
        explicit_zdr: route["explicit_zdr"]
            .as_bool()
            .expect("a route states explicit_zdr"),
    }
}

/// A policy that allows every call, so a call reaches the gates behind it.
struct AllowPolicy;

#[tonic::async_trait]
impl CapabilityPolicy for AllowPolicy {
    async fn evaluate(
        &self,
        _tool_name: &str,
        _run_id: &str,
        _org_id: &str,
    ) -> Result<CapabilityDecision, Status> {
        Ok(CapabilityDecision::Allow)
    }
}

/// Runs one step of a computer tool in `auto` mode and returns its status and error.
async fn step(tool: &str, input: &str, zdr: bool) -> (String, String) {
    let out = execute_step(
        tool,
        input,
        "auto",
        "",
        "org-1",
        "user-1",
        "run-1",
        "step-1",
        "",
        None,
        None,
        None,
        zdr,
        0,
        None,
        None,
        None,
        &AllowPolicy,
        None,
    )
    .await;
    (out.status, out.error)
}

#[test]
fn each_computer_name_is_governed_by_its_own_capability() {
    for (tool, capability) in [
        (OBSERVE_TOOL, "cap.computer.observe"),
        (ACT_TOOL, "cap.computer.act"),
    ] {
        assert_eq!(
            trusted_capability_id(tool).as_deref(),
            Some(capability),
            "{tool} is governed by the wrong capability"
        );
    }
}

#[test]
fn the_capability_seed_declares_each_computer_name_under_its_capability() {
    let seed = compact(CAPABILITY_SEED);
    let row = Regex::new(
        r"\('global','(cap\.computer\.[a-z]+)','(computer\.[a-z]+)','([a-z_]+)','none',NULL,'seed'\)",
    )
    .expect("the seed row pattern");
    let mut declared: Vec<(String, String, String)> = row
        .captures_iter(&seed)
        .map(|capture| {
            (
                capture[1].to_owned(),
                capture[2].to_owned(),
                capture[3].to_owned(),
            )
        })
        .collect();
    declared.sort();
    let mut expected: Vec<(String, String, String)> = [
        ("cap.computer.act", "computer.act", "external_write"),
        ("cap.computer.observe", "computer.observe", "read"),
    ]
    .iter()
    .map(|(capability, tool, class)| {
        (
            (*capability).to_owned(),
            (*tool).to_owned(),
            (*class).to_owned(),
        )
    })
    .collect();
    expected.sort();
    assert_eq!(declared, expected, "the seeded computer class rows changed");

    for (capability, tool, _) in &declared {
        assert_eq!(
            trusted_capability_id(tool).as_deref(),
            Some(capability.as_str()),
            "the seed binds {tool} to {capability}, but the gate does not"
        );
    }
}

#[test]
fn only_act_is_risky_among_the_computer_names() {
    assert!(is_risky_tool(ACT_TOOL));
    assert!(
        !is_risky_tool(OBSERVE_TOOL),
        "a look at the screen is a read and must not be risky"
    );
}

#[test]
fn the_connector_names_exactly_the_two_tools_and_no_position() {
    let connector = lf(CONNECTOR);
    assert_carries(
        &connector,
        "the connector",
        &[
            r#"pub(crate) const OBSERVE_TOOL: &str = "computer.observe";"#,
            r#"pub(crate) const ACT_TOOL: &str = "computer.act";"#,
            "matches!(name, OBSERVE_TOOL | ACT_TOOL)",
            r#"#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ComputerAct {"#,
        ],
    );
    let act = item(&connector, "pub(crate) enum ComputerAct {");
    let position = Regex::new(r"(?m)\b(x|y|coordinates|position|pixel|pixels|offset|left|top)\s*:")
        .expect("the position pattern");
    assert!(
        !position.is_match(act),
        "ComputerAct names a position field: {act}"
    );
}

#[test]
fn no_computer_tool_is_offered_in_this_slice() {
    assert!(
        !lf(AGENT).contains("\"computer."),
        "a computer tool is offered; the offer waits for the executor"
    );
}

#[test]
fn the_step_dispatch_sends_the_computer_names_to_the_connector() {
    assert_carries(
        &lf(RUNTIME_LOOP),
        "the step dispatch",
        &[
            "} else if computer_action::is_computer_tool(tool_name) {",
            "tool_bridge::computer_tool(tool_name, tool_input, zdr)",
        ],
    );
    assert_carries(
        &lf(TOOL_BRIDGE),
        "the tool bridge",
        &[
            "pub(crate) fn computer_tool(tool_name: &str, tool_input: &str, zdr: bool) -> ToolExecution {",
            "computer_action::refusal_text(",
            "computer_action::computer_enabled(),",
        ],
    );
}

#[test]
fn the_zero_retention_row_names_its_feature_and_its_tools() {
    let matrix = matrix();
    let row = enforcement_rows(&matrix)
        .iter()
        .find(|row| row["id"] == DO_COMPUTER_ROW)
        .expect("the computer row is in the matrix");
    assert_eq!(row["expect"]["decision"], "zdr");
    assert_eq!(row["expect"]["code"], "explicit_zdr");

    let note = row["note"].as_str().expect("the row says what it covers");
    for fragment in [
        "computer.observe",
        "computer.act",
        "tool_execution",
        "EXECUTION_DO_COMPUTER_ENABLED",
    ] {
        assert!(
            note.contains(fragment),
            "the row's note no longer says `{fragment}`"
        );
    }
}

/// Every enforcement row, through the real resolver and then the real step, with `computer.act` as the call
/// that stands for the computer tools. A row the matrix decides as zdr refuses that call by its feature. Every
/// other row that reaches the step is refused as unavailable, so the feature is not given to a row that is not
/// zdr. A row that refuses the turn starts no run and has no step to drive.
#[tokio::test]
async fn the_computer_act_is_refused_exactly_where_the_matrix_decides_zdr() {
    let matrix = matrix();
    let feature = Refusal::feature_not_supported("tool_execution");
    let typed = format!("{}: {}", feature.code(), feature.message());
    let mut zdr_rows: Vec<String> = Vec::new();
    let mut other_rows = 0_usize;
    let mut failures = Vec::new();
    for row in enforcement_rows(&matrix) {
        let id = row["id"].as_str().expect("a row has an id");
        let zdr = match resolve_turn(&claims_of(row), &route_of(row)) {
            Resolution::Zdr { .. } => true,
            Resolution::Persistent { .. } | Resolution::Waived { .. } => false,
            Resolution::Refuse(_) => continue,
        };
        let (status, error) = step(ACT_TOOL, CLICK, zdr).await;
        let expected = if zdr { typed.as_str() } else { UNAVAILABLE };
        if status != "failed" || error != expected {
            failures.push(format!(
                "{id}: zdr {zdr}, the step answered {status} with {error:?}, expected {expected:?}"
            ));
        }
        if zdr {
            zdr_rows.push(id.to_owned());
        } else {
            other_rows += 1;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        zdr_rows.iter().any(|id| id == DO_COMPUTER_ROW),
        "the computer row must be decided as zdr"
    );
    assert!(
        other_rows > 0,
        "the golden must also drive rows that are not zdr"
    );
}

#[tokio::test]
async fn a_computer_call_outside_zero_retention_is_refused_as_unavailable() {
    for (tool, input) in [(ACT_TOOL, CLICK), (OBSERVE_TOOL, "{}")] {
        let (status, error) = step(tool, input, false).await;
        assert_eq!(status, "failed", "{tool} must not succeed with no backend");
        assert_eq!(error, UNAVAILABLE, "{tool}");
    }
}

#[tokio::test]
async fn a_zero_retention_run_is_refused_the_computer_feature() {
    let feature = Refusal::feature_not_supported("tool_execution");
    let typed = format!("{}: {}", feature.code(), feature.message());
    for (tool, input) in [(ACT_TOOL, CLICK), (OBSERVE_TOOL, "{}")] {
        let (status, error) = step(tool, input, true).await;
        assert_eq!(
            status, "failed",
            "{tool} must not succeed under zero retention"
        );
        assert_eq!(error, typed, "{tool}");
    }
}
