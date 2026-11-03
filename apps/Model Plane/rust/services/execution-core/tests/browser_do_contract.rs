//! The Do loop's browser tools, as one contract (build plan WS21-S10).
//!
//! The model reaches a page through four names: `browser.open`, `browser.observe`, `browser.close` and
//! `browser.act`. Their contract is spread over places that no single unit test sees together: the names
//! the offer lists, the names the dispatch serves, the capability each name is governed by, the class the
//! capability seed declares for it, the answer a zero-retention run gets, and the fields the model is
//! shown. This file holds those places to one another.
//!
//! Most of them are private to the crate, so the checks read the source that declares them, as
//! `zdr_mcp_golden.rs` and `tool_class_registry_parity.rs` do. The rest go through the public surface that
//! already answers them: `trusted_capability_id`, `is_risky_tool`, the real resolver and `execute_step`.
//! Source text is compared with its whitespace removed, so formatting and CRLF checkouts do not change an
//! answer.
//!
//! The zero-retention rule is held as a golden over every enforcement row of the vendored matrix
//! (`fixtures/zdr-posture-matrix.json`, checked by `scripts/tests/zdr-contract-sync.test.sh`), not as a row
//! of its own: the matrix loaders reject unknown fields, and the rule follows from the decision
//! (`api/contracts/README.md`, "MCP goldens"). The row `e_browser_do_tools_refused_under_zdr` documents it.

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

const BROWSER_SESSION: &str = include_str!("../src/browser_session.rs");
const AGENT: &str = include_str!("../src/runtime_loop/agent.rs");
const RUNTIME_LOOP: &str = include_str!("../src/runtime_loop/mod.rs");
const TOOL_BRIDGE: &str = include_str!("../src/tool_bridge/mod.rs");
const CAPABILITY_SEED: &str = include_str!(
    "../../../../go/services/capability-core/migrations/0027_browser_tool_capabilities.up.sql"
);
const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

const DO_BROWSER_ROW: &str = "e_browser_do_tools_refused_under_zdr";

/// The call every golden row makes: the Do open of a page, which needs no session to be refused.
const DO_BROWSER_TOOL: &str = "browser.open";
const DO_BROWSER_INPUT: &str = r#"{"url":"https://example.com/"}"#;

/// The read tools the offer lists by default. `browser.act` is offered only on its own switch.
const OFFERED: [&str; 3] = ["browser.open", "browser.observe", "browser.close"];

/// The grant-backed MCP aliases share the names `browser.observe` and `browser.act`. A step with no Do
/// session reaches them before the Do refusal, and without a grant they fail closed.
const MISSING_GRANT: &str = "browser MCP requires a broker-validated grant";

/// The answer to a Do browser call from a run with no session outside zero retention.
const UNAVAILABLE: &str =
    "browser_unavailable: browser tools need a verified browser sign-in for this run";

/// Source text with its line endings normalised, the form [`item`] reads.
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

/// Runs one step of a browser tool with no Do session and returns its status and error.
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
fn the_session_module_names_the_family_once() {
    let session = lf(BROWSER_SESSION);
    assert_carries(
        &session,
        "the session module",
        &[
            r#"pub(crate) const OPEN_TOOL: &str = "browser.open";"#,
            r#"pub(crate) const OBSERVE_TOOL: &str = "browser.observe";"#,
            r#"pub(crate) const CLOSE_TOOL: &str = "browser.close";"#,
            r#"pub(crate) const ACT_TOOL: &str = "browser.act";"#,
            "pub(crate) const FAMILY_CLASSIFIED: &[&str] = &[OPEN_TOOL, OBSERVE_TOOL, CLOSE_TOOL, ACT_TOOL];",
            "matches!(name, OPEN_TOOL | OBSERVE_TOOL | CLOSE_TOOL)",
            "matches!(name, OPEN_TOOL | CLOSE_TOOL)",
        ],
    );
}

#[test]
fn the_offer_lists_the_three_read_tools_and_act_only_on_its_own_switch() {
    let agent = lf(AGENT);
    let defs = item(
        &agent,
        "fn browser_tool_defs() -> Vec<pb::ToolDefinition> {",
    );
    let names = Regex::new(r#"name: "(browser\.[a-z]+)"\.to_owned\(\)"#).expect("the name pattern");
    let mut offered: Vec<&str> = names
        .captures_iter(defs)
        .map(|capture| capture.get(1).expect("the name group").as_str())
        .collect();
    offered.sort_unstable();
    let mut expected = OFFERED.to_vec();
    // `browser.console` (WS9.E, merged from mp-w2w3) rides the same offer; the grant-bound MCP
    // path serves it, not the Do session, so it stays out of OFFERED.
    expected.push("browser.console");
    expected.sort_unstable();
    assert_eq!(
        offered, expected,
        "the default offer must be the three read tools and the console"
    );

    let act = item(&agent, "fn browser_act_tool_def() -> pb::ToolDefinition {");
    assert!(compact(act).contains(r#"name:"browser.act".to_owned()"#));
    assert_carries(
        &agent,
        "the offer path",
        &[
            "fn browser_tools_offered(browser_bearer: bool, zdr: bool, switched_on: bool) -> bool {\n    browser_bearer && !zdr && switched_on\n}",
            "if browser_tools_offered(browser_bearer && !space_scoped, zdr, browser_do) {",
            "tools.extend(browser_tool_defs());",
            "if browser_act {\n            tools.push(browser_act_tool_def());",
            "browser_act: do_browser_act_enabled(),",
            "fn parse_do_browser_act_enabled(raw: Option<&str>) -> bool {\n    raw == Some(\"true\")\n}",
            "fn parse_do_browser_enabled(raw: Option<&str>) -> bool {\n    raw == Some(\"1\")\n}",
        ],
    );
}

#[test]
fn the_dispatch_serves_the_offered_names_and_refuses_the_rest() {
    let bridge = lf(TOOL_BRIDGE);
    let dispatch = item(&bridge, "pub(crate) async fn execute_browser_do_tool(");
    assert_carries(
        dispatch,
        "the Do dispatch",
        &[
            "browser_session::OPEN_TOOL =>",
            "browser_session::OBSERVE_TOOL =>",
            "browser_session::CLOSE_TOOL =>",
            "browser_session::ACT_TOOL =>",
            "_ => Err(browser_session::UNAVAILABLE),",
        ],
    );

    let step_source = lf(RUNTIME_LOOP);
    assert_carries(
        &step_source,
        "the step dispatch",
        &[
            "browser.filter(|_| !zdr && (browser_session::is_do_tool(tool_name) || browser_act))",
            "tool_bridge::execute_browser_do_tool(tool_name, tool_input, dispatch)",
            "} else if browser_session::is_do_tool(tool_name) {",
            "tool_bridge::browser_do_unavailable(zdr)",
            "let browser_act = !zdr",
            "&& tool_name == browser_session::ACT_TOOL",
            "&& agent::do_browser_act_enabled();",
        ],
    );
}

#[test]
fn each_browser_name_is_governed_by_the_capability_the_contract_names() {
    for (tool, capability) in [
        ("browser.open", "cap.browser.observe"),
        ("browser.observe", "cap.browser.observe"),
        ("browser.close", "cap.browser.observe"),
        ("browser.act", "cap.browser.act"),
        ("browser_agent", "cap.browser.open"),
    ] {
        assert_eq!(
            trusted_capability_id(tool).as_deref(),
            Some(capability),
            "{tool} is governed by the wrong capability"
        );
    }
}

#[test]
fn the_capability_seed_declares_each_browser_name_under_its_capability() {
    let seed = compact(CAPABILITY_SEED);
    let row = Regex::new(
        r"\('global','(cap\.browser\.[a-z]+)','(browser\.[a-z]+)','([a-z_]+)','none',NULL,'seed'\)",
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
        ("cap.browser.observe", "browser.open", "read"),
        ("cap.browser.observe", "browser.observe", "read"),
        ("cap.browser.observe", "browser.close", "read"),
        ("cap.browser.act", "browser.act", "external_write"),
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
    assert_eq!(declared, expected, "the seeded browser class rows changed");

    for (capability, tool, _) in &declared {
        assert_eq!(
            trusted_capability_id(tool).as_deref(),
            Some(capability.as_str()),
            "the seed binds {tool} to {capability}, but the gate does not"
        );
    }
}

#[test]
fn only_act_is_risky_among_the_browser_names() {
    assert!(is_risky_tool("browser.act"));
    for tool in OFFERED {
        assert!(
            !is_risky_tool(tool),
            "{tool} is a read and must not be risky"
        );
    }
}

#[test]
fn the_model_is_shown_no_grant_run_or_lease_identifier() {
    let session = lf(BROWSER_SESSION);
    let forbidden = [
        "grant_id",
        "quarry_run_id",
        "lease_id",
        "session_key",
        "bearer",
        "run_id",
        "org_id",
        "user_id",
    ];
    for header in ["struct PageView<'a> {", "struct RefView {"] {
        let shape = compact(item(&session, header));
        for field in forbidden {
            assert!(
                !shape.contains(&format!("{field}:")),
                "`{header}` carries `{field}`, which the model would read"
            );
        }
    }
    assert_carries(
        &session,
        "the page view",
        &[
            "let wrapped = wrap_untrusted(&json);",
            "#[serde(deny_unknown_fields)]\npub(crate) struct OpenInput {",
            "#[serde(deny_unknown_fields)]\npub(crate) struct NoInput {}",
        ],
    );
    let bridge = lf(TOOL_BRIDGE);
    assert_carries(
        &bridge,
        "the act input",
        &["#[serde(deny_unknown_fields)]\nstruct BrowserDoActInput {"],
    );
}

/// Under zero retention a Do browser call cannot succeed. `browser.open` and `browser.close` are refused
/// by the feature they are refused for. `browser.observe` and `browser.act` share their names with the
/// grant-backed MCP aliases, which the step routes first, so they fail on the missing grant, which zero
/// retention never mints. The answers are pinned so that a change to the order is seen.
#[tokio::test]
async fn a_zero_retention_run_cannot_use_a_do_browser_tool() {
    let feature = Refusal::feature_not_supported("browser_tool");
    let typed = format!("{}: {}", feature.code(), feature.message());
    for (tool, input) in [(DO_BROWSER_TOOL, DO_BROWSER_INPUT), ("browser.close", "{}")] {
        let (status, error) = step(tool, input, true).await;
        assert_eq!(
            status, "failed",
            "{tool} must not succeed under zero retention"
        );
        assert_eq!(
            error, typed,
            "{tool} must be refused with the browser_tool feature"
        );
    }
    for (tool, input) in [
        ("browser.observe", "{}"),
        (
            "browser.act",
            r#"{"action":{"type":"click_ref","snapshot_id":"s","generation":0,"ref_id":"r"}}"#,
        ),
    ] {
        let (status, error) = step(tool, input, true).await;
        assert_eq!(
            status, "failed",
            "{tool} must not succeed under zero retention"
        );
        assert_eq!(
            error, MISSING_GRANT,
            "{tool} fails closed without a grant; the grant alias answers before the Do refusal"
        );
    }
}

#[tokio::test]
async fn a_run_with_no_session_is_refused_as_unavailable_outside_zero_retention() {
    let (status, error) = step(DO_BROWSER_TOOL, DO_BROWSER_INPUT, false).await;
    assert_eq!(status, "failed");
    assert_eq!(error, UNAVAILABLE);
}

/// Every enforcement row, through the real resolver and then the real step, with `browser.open` as the call
/// that stands for the Do browser tools (the other three are held by the tests above). A row the matrix
/// decides as zdr refuses that call by its feature. Every other row that starts a run reaches the step with
/// no session and is refused as unavailable, so the feature is not given to a row that is not zdr. A row
/// that refuses the turn starts no run and has no step to drive.
#[tokio::test]
async fn the_do_browser_open_is_refused_exactly_where_the_matrix_decides_zdr() {
    let matrix = matrix();
    let feature = Refusal::feature_not_supported("browser_tool");
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
        let (status, error) = step(DO_BROWSER_TOOL, DO_BROWSER_INPUT, zdr).await;
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
        zdr_rows.iter().any(|id| id == DO_BROWSER_ROW),
        "the Do browser row must be decided as zdr"
    );
    assert!(
        other_rows > 0,
        "the golden must also drive rows that are not zdr"
    );
}

#[test]
fn the_do_browser_row_names_its_feature_and_its_switch() {
    let matrix = matrix();
    let row = enforcement_rows(&matrix)
        .iter()
        .find(|row| row["id"] == DO_BROWSER_ROW)
        .expect("the Do browser row is in the matrix");
    assert_eq!(row["expect"]["decision"], "zdr");
    assert_eq!(row["expect"]["code"], "explicit_zdr");

    let note = row["note"].as_str().expect("the row says what it covers");
    for fragment in [
        "browser.open",
        "browser.observe",
        "browser.close",
        "EXECUTION_DO_BROWSER_ENABLED=1",
        "browser_tool",
    ] {
        assert!(
            note.contains(fragment),
            "the row's note no longer says `{fragment}`"
        );
    }
}
