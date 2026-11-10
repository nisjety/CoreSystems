//! The connected-app read in the Ask loop, held to the Do loop's contract (WS23-S5).
//!
//! The name, the description and the schema come from one table, `mp_contracts::provider_reads`, so the two
//! loops cannot describe different tools. These tests pin that, and pin where the gate holds in the Ask
//! loop: the read is offered through `connector_reads::offered`, and its call is refused before any network
//! call unless the turn admits it. The unit tests in `connector_reads` and `tool_loop` check the behaviour;
//! these check the contract the two loops share.

use model_gateway::tool_loop::connector_reads;
use mp_contracts::provider_reads::{
    read_operation_names, read_tool_parameters_json, READ_OPERATIONS, READ_TOOL_DESCRIPTION,
    READ_TOOL_NAME,
};
use serde_json::{json, Value};

/// The text of the function whose signature starts with `signature`, up to the closing brace at column
/// zero, so a nested block is never mistaken for the end of its function.
fn function_body<'a>(text: &'a str, signature: &str) -> &'a str {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found; re-point this test"));
    let rest = &text[start..];
    let end = rest[signature.len()..]
        .find("\n}\n")
        .map_or(rest.len(), |offset| signature.len() + offset + 3);
    &rest[..end]
}

#[test]
fn the_ask_loop_offers_the_contract_read_by_name_schema_and_enum() {
    let def = connector_reads::tool_definition();
    assert_eq!(def.name, READ_TOOL_NAME);
    assert_eq!(def.description, READ_TOOL_DESCRIPTION);
    assert_eq!(def.parameters_json, read_tool_parameters_json());

    let schema: Value = serde_json::from_str(&def.parameters_json).expect("the schema is JSON");
    assert!(
        schema["properties"].get("body").is_none(),
        "a read carries no body"
    );
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["required"], json!(["connection_id", "operation"]));
    let names: Vec<&str> = schema["properties"]["operation"]["enum"]
        .as_array()
        .expect("a closed operation enum")
        .iter()
        .map(|op| op.as_str().expect("a string"))
        .collect();
    assert_eq!(names, read_operation_names());
}

#[test]
fn every_contract_read_names_a_read_and_no_write_verb() {
    for read in READ_OPERATIONS {
        for verb in [
            "send", "create", "delete", "update", "post", "publish", "upload", "replace",
        ] {
            assert!(
                !read.operation.contains(verb),
                "{} is listed as a read but names a {verb}",
                read.operation
            );
        }
    }
}

#[test]
fn the_ask_loop_reaches_the_read_only_through_the_gate() {
    let text = include_str!("../src/tool_loop.rs").replace("\r\n", "\n");

    let rounds = function_body(
        &text,
        "async fn run_tool_rounds_for_model_with_attachments_inner(",
    );
    assert!(
        rounds.contains("connector_reads::offered(")
            && rounds.contains("connector_reads::enabled()"),
        "the offer must pass through the switch and the gate that also checks ZDR, Space and the selection"
    );
    assert!(
        rounds.contains("tools_for_connector_scope(tools, connector_offered)"),
        "the offered set is settled once, before any round"
    );
    assert!(
        rounds.contains("connector_reads::settle("),
        "the sources and the parking are settled after the result is shown"
    );

    let arm = function_body(&text, "async fn dispatch_tool_with_cost(");
    assert!(
        arm.contains("connector_reads::read_outcome("),
        "the dispatch arm goes through the read's own refusals"
    );
}
