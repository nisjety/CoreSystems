//! The declared tool classes in capability-core's seed must say what the name
//! heuristics say today (WS05-S1).
//!
//! Three things have to stay true together, and none of them is visible to the
//! compiler because the registry is Go and the seed is SQL:
//!
//! 1. every tool `capability_policy::trusted_capability_id` binds has a seeded
//!    `(capability_id, tool_name)` row, bound to the capability the gate binds
//!    it to, so a new tool is not offered until it has a class;
//! 2. every tool the legacy classification golden covers (the S0 oracle in
//!    `permission/mod.rs`) is seeded;
//! 3. the class and floor seeded for a tool reproduce, for a read and for a write
//!    form of its arguments, every answer the name heuristics give: risky,
//!    restricted context write, required rung, the two never-auto predicates and
//!    the posture gate under `ask` and `auto` - except for the explicit,
//!    documented exception table (`DECLARED_STRICTER_EXCEPTIONS`), which names
//!    each tool whose declared class is deliberately stricter than the heuristics
//!    and exactly which answers differ. The test fails if the exception set
//!    changes or if any other tool differs.
//!
//! The seed is read as text from migration 0016 and the sources are read as text
//! for the same reason `autonomy_ladder_contract.rs` reads them: these are
//! properties of how two services line up, not of a value a unit test can
//! observe. If a file moved, re-point the test; do not delete it, because it is
//! what stops the class table and the gate from drifting apart in silence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use execution_core::capability_policy::trusted_capability_id;
use execution_core::permission::{
    evaluate_call, is_restricted_context_write, is_risky_call, requires_consent_to_disclose,
    requires_durable_owner_approval, rung_required_for, PermissionDecision, PermissionMode,
};
use mp_contracts::autonomy;
use mp_contracts::tool_class::{
    effective_floor, ApprovalFloor, SideEffectClass, CALL_REFINEMENT_PROVIDER_OPERATION,
};
use regex::Regex;

/// The two argument forms the S0 golden evaluates: a catalogued read operation
/// and a catalogued write operation of `execute_provider_action`.
const READ_CALL: &str = r#"{"connection_id":"c1","operation":"pages.list"}"#;
const WRITE_CALL: &str = r#"{"connection_id":"c1","operation":"pages.post"}"#;

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_text(relative: &str) -> String {
    let path = manifest_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read {} ({error}). If the file moved, re-point this test; do not delete it.",
                path.display()
            )
        })
        .replace("\r\n", "\n")
}

#[derive(Debug)]
struct Seeded {
    capability_id: String,
    tool: String,
    class: SideEffectClass,
    floor: ApprovalFloor,
    refinement: Option<String>,
}

fn class_from_seed(raw: &str) -> SideEffectClass {
    let class = SideEffectClass::from_wire(raw);
    assert_eq!(
        class.as_str(),
        raw,
        "the seed holds `{raw}`, which is not in the class vocabulary"
    );
    class
}

fn floor_from_seed(raw: &str) -> ApprovalFloor {
    let floor = ApprovalFloor::from_wire(raw);
    assert_eq!(
        floor.as_str(),
        raw,
        "the seed holds `{raw}`, which is not an approval floor"
    );
    floor
}

fn seeded_rows() -> Vec<Seeded> {
    // 0016 seeds the classes; a later migration that registers a tool adds its own seed row.
    let mut sql = read_text(
        "../../../go/services/capability-core/migrations/0016_capability_tool_classes.up.sql",
    );
    for later in [
        "0018_workspace_read_capability.up.sql",
        "0019_workspace_write_capability.up.sql",
        "0020_result_query_capability.up.sql",
        "0024_provider_read_tool.up.sql",
        "0027_browser_tool_capabilities.up.sql",
        "0026_knowledge_list_documents_tool.up.sql",
    ] {
        sql.push_str(&read_text(&format!(
            "../../../go/services/capability-core/migrations/{later}"
        )));
    }
    let row = Regex::new(
        r"\('global', '([^']+)', '([^']+)', '([a-z_]+)', '([a-z_]+)', (NULL|'[a-z_]+'), 'seed'\)",
    )
    .expect("seed row pattern");
    // 0025 moves browser.observe and browser.act off cap.browser.open; its DELETE is the record of the
    // move, so the effective seed no longer carries those two rows under cap.browser.open.
    assert!(
        read_text(
            "../../../go/services/capability-core/migrations/0027_browser_tool_capabilities.up.sql",
        )
        .contains("tool_name IN ('browser.observe', 'browser.act')"),
        "0025 no longer moves browser.observe and browser.act; update the moved set"
    );
    let rows: Vec<Seeded> = row
        .captures_iter(&sql)
        .filter(|c| {
            !(&c[1] == "cap.browser.open" && matches!(&c[2], "browser.observe" | "browser.act"))
        })
        .map(|c| Seeded {
            capability_id: c[1].to_owned(),
            tool: c[2].to_owned(),
            class: class_from_seed(&c[3]),
            floor: floor_from_seed(&c[4]),
            refinement: match &c[5] {
                "NULL" => None,
                quoted => Some(quoted.trim_matches('\'').to_owned()),
            },
        })
        .collect();
    assert!(
        rows.len() >= 30,
        "found only {} seed rows in migration 0016; the row format changed, re-point the parser",
        rows.len()
    );
    rows
}

fn seeded_by_tool() -> BTreeMap<String, Seeded> {
    seeded_rows()
        .into_iter()
        .map(|row| (row.tool.clone(), row))
        .collect()
}

/// Every name on the left of an arm of the `match tool_name` in
/// `trusted_capability_id`, comments removed.
fn trusted_capability_arm_names() -> BTreeSet<String> {
    let source = read_text("src/capability_policy.rs");
    let start = source
        .find("let capability = match tool_name {")
        .expect("`trusted_capability_id` no longer has the `let capability = match tool_name` block; re-point this test");
    let end = source[start..]
        .find("_ => return None")
        .expect("`trusted_capability_id` lost its fail-closed `_ => return None` arm")
        + start;
    let block: String = source[start..end]
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let arm = Regex::new(r#"((?:"[^"]+"\s*\|?\s*)+)=>"#).expect("arm pattern");
    let name = Regex::new(r#""([^"]+)""#).expect("name pattern");
    let names: BTreeSet<String> = arm
        .captures_iter(&block)
        .flat_map(|c| {
            name.captures_iter(&c[1])
                .map(|n| n[1].to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        names.len() >= 25,
        "parsed only {} arm names from `trusted_capability_id`; the arm layout changed, re-point the parser",
        names.len()
    );
    names
}

/// Tool names the S0 golden covers, minus the MCP spellings (an MCP tool has no
/// seeded row by design: its binding comes from observation).
fn legacy_golden_names() -> BTreeSet<String> {
    let source = read_text("src/permission/mod.rs");
    let marker = "const LEGACY_CLASSIFICATION_GOLDEN: &str = r\"";
    let start = source
        .find(marker)
        .expect("the S0 legacy classification golden moved; re-point this test")
        + marker.len();
    let end = source[start..]
        .find("\";")
        .expect("the S0 golden is unterminated")
        + start;
    let names: BTreeSet<String> = source[start..end]
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| !name.to_ascii_lowercase().starts_with("mcp__"))
        .map(str::to_owned)
        .collect();
    assert!(
        names.len() >= 30,
        "parsed only {} names from the S0 golden",
        names.len()
    );
    names
}

#[test]
fn every_trusted_capability_arm_has_a_seeded_tool_on_the_same_capability() {
    let seeded = seeded_by_tool();
    for name in trusted_capability_arm_names() {
        let row = seeded.get(&name).unwrap_or_else(|| {
            panic!(
                "`{name}` is bound by `trusted_capability_id` but has no capability_tools seed row: \
                 a new tool is not offered until it has a class"
            )
        });
        assert_eq!(
            trusted_capability_id(&name).as_deref(),
            Some(row.capability_id.as_str()),
            "`{name}` is seeded under a different capability than the gate binds it to"
        );
    }
}

#[test]
fn every_seeded_tool_resolves_to_its_seeded_capability() {
    // Covers the prefix families (`subagent.*`, `plan.*`) that no literal arm
    // names, and catches a seeded tool the gate would refuse as unbound.
    for row in seeded_rows() {
        assert_eq!(
            trusted_capability_id(&row.tool).as_deref(),
            Some(row.capability_id.as_str()),
            "seeded tool `{}` is not bound to `{}` by `trusted_capability_id`",
            row.tool,
            row.capability_id
        );
    }
}

#[test]
fn every_tool_in_the_legacy_golden_is_seeded() {
    let seeded = seeded_by_tool();
    for name in legacy_golden_names() {
        assert!(
            seeded.contains_key(&name),
            "`{name}` is in the S0 classification golden but has no capability_tools seed row"
        );
    }
}

#[test]
fn no_seeded_tool_is_left_unclassified_and_owner_effects_carry_their_floor() {
    for row in seeded_rows() {
        assert_ne!(
            row.class,
            SideEffectClass::Unclassified,
            "`{}` is seeded as unclassified; classify it on purpose",
            row.tool
        );
        assert_eq!(
            effective_floor(row.class, row.floor),
            row.floor,
            "`{}` is seeded with a floor below what its class implies",
            row.tool
        );
        if row.refinement.is_some() {
            assert_eq!(row.tool, "execute_provider_action");
            assert_eq!(
                row.refinement.as_deref(),
                Some(CALL_REFINEMENT_PROVIDER_OPERATION)
            );
        }
    }
}

/// What the gate would decide for `mode` given only the declared class and floor.
fn declared_posture(
    mode: PermissionMode,
    class: SideEffectClass,
    floor: ApprovalFloor,
) -> PermissionDecision {
    if floor != ApprovalFloor::None {
        PermissionDecision::AwaitApproval
    } else if mode == PermissionMode::Ask && class.is_risky() {
        PermissionDecision::AwaitApproval
    } else {
        PermissionDecision::Allow
    }
}

/// The one place a declared class is allowed to disagree with the name
/// heuristics, and the only one. Each entry names the tool, the class the seed
/// must declare for it, the comparison fields that differ and why. The exception
/// set is pinned twice: `the_documented_exceptions_are_exactly_these` fails if
/// the set changes, and the comparison below fails if any other tool differs or
/// an excepted tool stops differing in exactly the documented fields.
///
/// `plan.propose` is declared `context_write` and the heuristics call it a read:
/// it writes a plan durably onto the person's approval surface, so a consumer
/// that trusts the declared class (an unattended allowlist with a read-only
/// ceiling) must not see a durable write as a read. The permission engine switch
/// in WS05-S4 must keep `plan.propose` callable in plan mode through an explicit
/// rule, because the declared class alone would refuse it there.
struct DeclaredStricterException {
    tool: &'static str,
    declared_class: SideEffectClass,
    differing_fields: &'static [&'static str],
    reason: &'static str,
}

const DECLARED_STRICTER_EXCEPTIONS: &[DeclaredStricterException] = &[DeclaredStricterException {
    tool: "plan.propose",
    declared_class: SideEffectClass::ContextWrite,
    differing_fields: &["restricted_context_write", "rung"],
    reason: "durable approval-surface write: declared context_write, legacy read_only",
}];

#[test]
fn the_documented_exceptions_are_exactly_these() {
    let names: Vec<&str> = DECLARED_STRICTER_EXCEPTIONS
        .iter()
        .map(|exception| exception.tool)
        .collect();
    assert_eq!(
        names,
        ["plan.propose"],
        "the set of tools whose declared class may disagree with the name heuristics changed; \
         a new exception needs a ledger entry and a reviewer, not a quiet edit here"
    );
    for exception in DECLARED_STRICTER_EXCEPTIONS {
        assert!(
            !exception.reason.is_empty() && !exception.differing_fields.is_empty(),
            "`{}` is excepted without a reason or without naming what differs",
            exception.tool
        );
        assert!(
            exception.declared_class > SideEffectClass::Read,
            "an exception is only for a declared class STRICTER than the heuristics' read"
        );
    }
}

/// Every named comparison between what the name heuristics answer for a call and
/// what the declared class and floor would answer, returning the names of the
/// fields that differ.
fn differing_fields(row: &Seeded, args: &str) -> BTreeSet<&'static str> {
    // The one argument-dependent tool: a catalogued read operation lowers the
    // write class to read, and nothing else may lower.
    let class = if row.refinement.is_some() && args == READ_CALL {
        SideEffectClass::Read
    } else {
        row.class
    };
    let floor = effective_floor(class, row.floor);
    let mut differing = BTreeSet::new();

    if is_risky_call(&row.tool, args) != class.is_risky() {
        differing.insert("risky");
    }
    if is_restricted_context_write(&row.tool) != class.is_restricted_context_write() {
        differing.insert("restricted_context_write");
    }
    if autonomy::label(rung_required_for(&row.tool, args)) != autonomy::label(class.required_rung())
    {
        differing.insert("rung");
    }
    if requires_durable_owner_approval(&row.tool) != (floor == ApprovalFloor::DurableOwner) {
        differing.insert("durable_owner_approval");
    }
    if requires_consent_to_disclose(&row.tool)
        != (row.class == SideEffectClass::Read && floor == ApprovalFloor::Ask)
    {
        differing.insert("consent_to_disclose");
    }
    if evaluate_call(PermissionMode::Ask, &row.tool, args)
        != declared_posture(PermissionMode::Ask, class, floor)
    {
        differing.insert("posture_ask");
    }
    if evaluate_call(PermissionMode::Auto, &row.tool, args)
        != declared_posture(PermissionMode::Auto, class, floor)
    {
        differing.insert("posture_auto");
    }
    differing
}

#[test]
fn seeded_classes_reproduce_the_legacy_name_classification_except_the_documented_ones() {
    let mut excepted_tools_seen = BTreeSet::new();
    for row in seeded_rows() {
        let exception = DECLARED_STRICTER_EXCEPTIONS
            .iter()
            .find(|exception| exception.tool == row.tool);
        let expected: BTreeSet<&str> = exception
            .map(|exception| exception.differing_fields.iter().copied().collect())
            .unwrap_or_default();
        if let Some(exception) = exception {
            assert_eq!(
                row.class, exception.declared_class,
                "`{}` is excepted as {:?} but the seed declares {:?}",
                row.tool, exception.declared_class, row.class
            );
            excepted_tools_seen.insert(row.tool.clone());
        }
        for args in [READ_CALL, WRITE_CALL] {
            let actual = differing_fields(&row, args);
            assert_eq!(
                actual,
                expected,
                "`{}` with {args}: the declared class {:?}/{:?} differs from the name heuristics in {actual:?}, \
                 and the documented exception allows exactly {expected:?}",
                row.tool,
                row.class,
                row.floor
            );
        }
    }
    let documented: BTreeSet<String> = DECLARED_STRICTER_EXCEPTIONS
        .iter()
        .map(|exception| exception.tool.to_owned())
        .collect();
    assert_eq!(
        excepted_tools_seen, documented,
        "an excepted tool is not seeded at all"
    );
}
