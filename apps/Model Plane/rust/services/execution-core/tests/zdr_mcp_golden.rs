//! The MCP golden, execution-core side (build plan WS05, "ZDR clause and regression guards"; register
//! row D1-007): under zero retention an `mcp__` tool is never offered and never executed.
//!
//! `fixtures/zdr-posture-matrix.json` is a byte-identical copy of the canonical matrix, pinned beside it
//! and checked by `scripts/tests/zdr-contract-sync.test.sh`. Every `enforcement_rows` row is decided here
//! by the real resolver, `mp_contracts::retention::resolve_turn` (the call the run boundary and the durable
//! loop make; see `loop_fold_zdr_matrix.rs`). The decided `zdr` and `waived` flags then go to the plane's
//! real step entry, `runtime_loop::execute_step`, which holds the MCP gate: the `mcp__` check at the top of
//! `execute_step_inner` (`runtime_loop/mod.rs`, refusing `feature_not_supported("mcp_execution")`). Nothing
//! here reimplements that gate.
//!
//! # What this file drives, and what it does not
//!
//! * Execution, for every row. A zdr row is refused with the typed `mcp_execution` refusal before the
//!   capability policy is asked. A persistent row and a waived row are not refused by that gate and are
//!   asked of the policy once. The compatibility entry point has no waiver input: `execute_step` passes
//!   `waived = false` into the gate (`runtime_loop/mod.rs`, the `false` after its compatibility comment),
//!   so a waived row is driven as a non-waived step. That is what the entry point does today. The waived
//!   refusal (`waived = true` at the same gate) is reached only through crate-private entry points, and
//!   the crate's own test `an_mcp_tool_is_refused_under_zdr_or_a_waived_route_before_the_gate_is_asked`
//!   pins it there.
//! * The offer, by wiring only. The tool set a run is offered is built by a private function
//!   (`runtime_loop/agent.rs`, `merged_tool_defs_governed`), which no integration test can reach. The last
//!   test in this file reads the source of the offer path, as `durable_loop_contract.rs` does for the
//!   durable gates. It proves the wiring, not the behaviour.

use execution_core::capability_policy::{CapabilityDecision, CapabilityPolicy};
use execution_core::runtime_loop::execute_step;
use mp_contracts::retention::{resolve_turn, ClaimsView, Resolution, RouteView, WaiverView};
use mp_contracts::zdr_refusal::Refusal;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use tonic::Status;

const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

/// The enforcement rows of the canonical matrix when this test was written, and how they split by
/// decision (the counts `loop_fold_zdr_matrix.rs` pins). A row added or removed without this file being
/// read is a change nobody reviewed against the MCP gate.
const ENFORCEMENT_ROWS: usize = 28;
const ZDR: usize = 5;
const PERSISTENT: usize = 3;
const WAIVED: usize = 2;
const REFUSE: usize = 18;

/// The tool every row is driven with: a registry tool, which the gate refuses by its `mcp__` prefix.
const MCP_TOOL: &str = "mcp__srv__echo";

/// A capability policy that counts how often the gate asks it and denies every call. A call that gets
/// past the MCP gate therefore ends at the policy without running anything, and the count proves the
/// gate let it reach the policy.
#[derive(Default)]
struct CountingPolicy {
    asked: AtomicUsize,
}

#[tonic::async_trait]
impl CapabilityPolicy for CountingPolicy {
    async fn evaluate(
        &self,
        _tool_name: &str,
        _run_id: &str,
        _org_id: &str,
    ) -> Result<CapabilityDecision, Status> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(CapabilityDecision::Deny)
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

/// What the real resolver says about a row. No decision is built by hand.
fn resolved(row: &Value) -> Resolution<'_> {
    resolve_turn(&claims_of(row), &route_of(row))
}

/// The matrix's own word for a resolution.
fn decided_as(resolution: &Resolution<'_>) -> &'static str {
    match resolution {
        Resolution::Zdr { .. } => "zdr",
        Resolution::Persistent { .. } => "persistent",
        Resolution::Waived { .. } => "waived",
        Resolution::Refuse(_) => "refuse",
    }
}

/// Source text with every whitespace character removed, so a check reads the code and not its layout
/// (line breaks, indentation and CRLF line endings all change with rustfmt).
fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
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

/// Every enforcement row, through the real resolver and then the real step gate. A zdr row is refused
/// before the policy, as `mcp_execution`. A row the gate lets through is asked of the policy once, so the
/// refusal cannot be passing by having nothing to ask.
#[tokio::test]
async fn mcp_tools_under_zdr_not_offered_not_executed() {
    let matrix = matrix();
    let refusal = Refusal::feature_not_supported("mcp_execution").message();
    let mut failures = Vec::new();
    let (mut zdr_driven, mut persistent_driven, mut waived_driven) = (0_usize, 0_usize, 0_usize);
    for row in rows(&matrix) {
        let resolution = resolved(row);
        if decided_as(&resolution) != expected(row) {
            failures.push(format!(
                "{}: the resolver answers {}, the matrix says {}",
                id(row),
                decided_as(&resolution),
                expected(row)
            ));
            continue;
        }
        // The flags a run carries into its step: a zdr turn is persistence-free; a waived turn is not zdr
        // and carries its waiver; a refused turn never starts a run, so it has no step to drive.
        let (zdr, waived) = match resolution {
            Resolution::Zdr { .. } => (true, false),
            Resolution::Persistent { .. } => (false, false),
            Resolution::Waived { .. } => (false, true),
            Resolution::Refuse(_) => continue,
        };
        let policy = CountingPolicy::default();
        let out = execute_step(
            MCP_TOOL, "{}", "auto", "", "org-1", "user-1", "run-1", "step-1", "", None, None, None,
            zdr, 0, None, None, None, &policy, None,
        )
        .await;
        let asked = policy.asked.load(Ordering::SeqCst);
        if zdr {
            zdr_driven += 1;
            if out.status != "failed" || out.error != refusal || asked != 0 {
                failures.push(format!(
                    "{}: zdr, the step answered {} with {:?} and the policy was asked {asked} time(s); \
                     it must be refused as mcp_execution before the policy is asked",
                    id(row),
                    out.status,
                    out.error
                ));
            }
            continue;
        }
        if waived {
            waived_driven += 1;
        } else {
            persistent_driven += 1;
        }
        if out.status != "permission_denied" || asked != 1 {
            failures.push(format!(
                "{}: {}, the step answered {} with {:?} and the policy was asked {asked} time(s); \
                 the gate must let it reach the policy exactly once",
                id(row),
                if waived { "waived" } else { "persistent" },
                out.status,
                out.error
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        (zdr_driven, persistent_driven, waived_driven),
        (ZDR, PERSISTENT, WAIVED),
        "every non-refused row was driven through the step gate"
    );
    eprintln!(
        "MCP step gate driven: {zdr_driven} zdr rows refused as mcp_execution, \
         {persistent_driven} persistent and {waived_driven} waived rows asked of the policy"
    );
}

/// The offer of a retention-bound run, read from its source. `offered_tools` makes the run's
/// retention-bound flag (`req.zdr || waived`) the `zdr` input of the governed offer, the governed offer
/// runs in `Off` mode for such a run, and the ungoverned listing is asked only of a run that is not
/// retention-bound. Source reading proves that wiring, not the behaviour (see the header).
#[test]
fn the_offer_of_a_retention_bound_run_asks_for_no_mcp_tool() {
    let source = compact(&include_str!("../src/runtime_loop/agent.rs").replace("\r\n", "\n"));
    for wiring in [
        "letretention_bound=req.zdr||waived;",
        "merged_tool_defs_governed(&req.run_id,",
        "space_scoped,retention_bound,browser_bearer,connector_scope_of(req),governance",
        "ifmode==McpDoTools::Off&&!zdr{",
        "}elseifzdr{count_withheld(&[(withheld::ZDR,1)].into());McpDoTools::Off}",
        "resolve_run_offer(source,governed_mode,",
    ] {
        assert!(
            source.contains(wiring),
            "the offer path of a retention-bound run no longer carries `{wiring}`; the zdr offer may have changed"
        );
    }
}
