//! Guards the connected-app read tool (`read_provider_action`, WS23-S3) against the three ways it
//! could quietly stop being a read: a write slipping into its operations, a body slipping into its
//! schema, and a path that reaches the provider without the switch, the ZDR and Space refusals,
//! or the egress hold after a flagged read.
//!
//! These are properties of where a call sits and what a name means, not of a value a test can
//! observe, so they are read as source text, the way `autonomy_ladder_contract.rs` reads its own.

use std::path::{Path, PathBuf};

use mp_contracts::provider_reads::READ_OPERATIONS;

fn source(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do not delete it.",
            path.display()
        )
    });
    // The sources are CRLF in the working copy; the markers below are written with LF.
    text.replace("\r\n", "\n")
}

/// The text of the function whose signature starts with `signature`, up to the next line that
/// opens at column zero, so a nested item is never mistaken for the end of its function.
fn function_body(text: &str, signature: &str) -> String {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found"));
    let rest = &text[start..];
    let end = rest[signature.len()..]
        .find("\n}\n")
        .map_or(rest.len(), |offset| signature.len() + offset + 3);
    rest[..end].to_owned()
}

#[test]
fn the_read_tool_is_on_only_for_the_literal_true_and_only_where_it_is_offered() {
    let tools = source("src/integration_tools.rs");
    assert!(
        tools.contains(r#"raw == Some("true")"#),
        "only the literal `true` enables the read tool"
    );
    let agent = source("src/runtime_loop/agent.rs");
    assert!(
        agent.contains("read_provider_action_offered(")
            && agent.contains("provider_read_tool_enabled()"),
        "the offer must pass through the switch and the predicate that also checks ZDR, Space and the selection"
    );
}

#[test]
fn the_read_schema_has_no_body_and_the_executor_refuses_unknown_fields() {
    let module = source("src/runtime_loop/mod.rs");
    let executor = function_body(&module, "async fn execute_read_provider_action(");
    assert!(
        executor.contains("#[serde(deny_unknown_fields)]"),
        "a body field must be refused, not ignored"
    );
    assert!(
        !executor.contains("body:"),
        "the read input carries no body field"
    );
    let agent = source("src/runtime_loop/agent.rs");
    let schema = function_body(&agent, "fn read_provider_action_def() ");
    assert!(
        !schema.contains("\"body\""),
        "the read tool's schema must not name a body"
    );
}

#[test]
fn every_contract_read_names_a_read_and_no_write_verb() {
    for read in READ_OPERATIONS {
        for verb in ["send", "create", "delete", "update", "post", "publish", "upload", "replace"] {
            assert!(
                !read.operation.contains(verb),
                "{} is listed as a read but names a {verb}",
                read.operation
            );
        }
    }
}

#[test]
fn the_dispatch_refuses_the_read_under_zdr_waived_and_in_a_space_room() {
    let module = source("src/runtime_loop/mod.rs");
    let guard = function_body(&module, "async fn execute_step_inner(");
    assert!(
        guard.contains("(zdr || waived) && crate::provenance::is_provider_tool(tool_name)"),
        "ZDR and waived routes refuse every connector tool before the capability gate"
    );
    assert!(
        guard.contains("if tool_name == READ_PROVIDER_ACTION_TOOL && sandbox.is_some()"),
        "a Space room refuses the connected-app read"
    );
}

#[test]
fn an_egress_call_is_held_before_its_effect_is_reserved() {
    let agent = source("src/runtime_loop/agent.rs");
    let hold = agent
        .find("if super::tainted_egress_held(")
        .expect("the egress hold is in the agent loop");
    let release = agent
        .find("let is_release = ctx.release.as_ref()")
        .expect("the release check is in the agent loop");
    let reserve = agent
        .find("let ledger_entry = if ctx.effect_ledger")
        .expect("the effect reservation is in the agent loop");
    assert!(
        release < hold && hold < reserve,
        "the hold must follow the release check and precede the reservation, so a held call reserves nothing"
    );
}
