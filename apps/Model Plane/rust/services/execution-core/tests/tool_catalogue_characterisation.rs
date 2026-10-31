//! Characterisation golden for the Do-mode tool catalogue (WS24-S0).
//!
//! For every tool in the three literal offer lists of `runtime_loop::agent` (33
//! names), the golden pins three answers the gates give for its name: the capability it is bound to
//! (`trusted_capability_id`), the rung the name heuristics require
//! (`rung_required_for`, read for empty arguments) and whether the name is risky
//! (`is_risky_tool`). A new offered tool, or a changed answer, fails here until
//! the fixture is regenerated and the diff is reviewed.
//!
//! The offer lists are private to `runtime_loop::agent`, and their names are
//! literals in that file, so the names are read from its source text the same
//! way `tool_class_registry_parity.rs` reads the capability arms. A name that
//! comes from a constant must be resolved in `resolve_constant`; an unknown one
//! fails the test rather than being skipped.
//!
//! Not covered: `tickets.create`, which is offered through the server-resolved
//! path (`append_server_resolved_tool_defs`), and the MCP and client tools, which
//! are dynamic per run.
//!
//! Regenerate after a reviewed change with:
//! `DO_TOOL_CATALOGUE_UPDATE=1 cargo test -q -p execution-core --test tool_catalogue_characterisation`

use std::path::{Path, PathBuf};

use execution_core::capability_policy::trusted_capability_id;
use execution_core::permission::{is_risky_tool, rung_required_for};
use execution_core::plan::PROPOSE_TOOL;
use execution_core::subagent::research_fan_out::RESEARCH_TOOL_NAME;
use mp_contracts::autonomy;
use serde_json::{json, Value};

const FIXTURE: &str = "tests/fixtures/do_tool_catalogue.json";
const AGENT_SOURCE: &str = "src/runtime_loop/agent.rs";
const SCHEMA: &str = "do-tool-catalogue-v1";

/// The three offer lists whose union is every tool a Do run can be offered.
const OFFER_FUNCTIONS: &[&str] = &[
    "fn offered_tool_defs()",
    "fn browser_tool_defs()",
    "fn process_tool_defs()",
];

/// The rung is read for empty arguments. Names decide the rung here; the
/// arguments decide it per call (see `autonomy_ladder_contract.rs`).
const RUNG_INPUT: &str = "{}";

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

/// The text of one top-level function: from its signature to the first `}` at
/// column 0.
fn function_body<'a>(text: &'a str, signature: &str) -> &'a str {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found in {AGENT_SOURCE}; re-point this test"));
    let rest = &text[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`{signature}` has no closing brace at column 0"));
    &rest[..end]
}

fn resolve_constant(expression: &str) -> &'static str {
    match expression {
        "crate::subagent::research_fan_out::RESEARCH_TOOL_NAME" => RESEARCH_TOOL_NAME,
        "crate::plan::PROPOSE_TOOL" => PROPOSE_TOOL,
        other => panic!(
            "tool name `{other}` comes from a constant this golden does not resolve; \
             add it to resolve_constant"
        ),
    }
}

/// Every tool name the offer lists declare, in source order.
fn offered_names() -> Vec<String> {
    let text = read_text(AGENT_SOURCE);
    let mut names = Vec::new();
    for signature in OFFER_FUNCTIONS {
        for line in function_body(&text, signature).lines() {
            let Some(field) = line.trim_start().strip_prefix("name: ") else {
                continue;
            };
            let value = field.strip_suffix(".to_owned(),").unwrap_or_else(|| {
                panic!("`{field}` is not `<value>.to_owned(),`; re-point this test")
            });
            let name = match value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
                Some(literal) => literal,
                None => resolve_constant(value),
            };
            names.push(name.to_owned());
        }
    }
    names
}

fn catalogue() -> Vec<Value> {
    offered_names()
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "trusted_capability_id": trusted_capability_id(&name),
                "rung_required_for": autonomy::label(rung_required_for(&name, RUNG_INPUT)),
                "is_risky_tool": is_risky_tool(&name),
            })
        })
        .collect()
}

#[test]
fn do_catalogue_matches_characterisation_golden() {
    let tools = catalogue();
    assert!(
        !tools.is_empty(),
        "parsed no offered tool names - this test broke, not the invariant"
    );
    let golden = json!({
        "schema": SCHEMA,
        "rung_input": RUNG_INPUT,
        "tools": tools,
    });
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(&golden).expect("the golden serialises")
    );

    if std::env::var_os("DO_TOOL_CATALOGUE_UPDATE").is_some() {
        std::fs::write(manifest_path(FIXTURE), &rendered).expect("write the golden");
        return;
    }
    assert_eq!(
        rendered,
        read_text(FIXTURE),
        "the Do tool catalogue changed. Review the diff; if it is intended, regenerate with \
         DO_TOOL_CATALOGUE_UPDATE=1 cargo test -q -p execution-core --test tool_catalogue_characterisation"
    );
}
