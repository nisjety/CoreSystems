//! F-f10-21: the MCP-app side-call lane of `ExecuteStep`.
//!
//! An app drawn under a chat answer asks its host to call a tool on its own server. The step is
//! marked `side_call`, and three things must hold however the handler is edited: it creates no
//! approval, it writes nothing about the run (no checkpoint, no step row, no snapshot), and it
//! takes the tool's class and digest from its own registry binding, not from the in-process run
//! views. The behaviour is unit-tested next to the code; this file pins the order of the handler,
//! which a unit test cannot reach without a signed caller.

use std::path::{Path, PathBuf};

fn source(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}); re-point this test",
            path.display()
        )
    })
}

fn function_body(text: &str, signature: &str) -> String {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is gone; re-point this test"));
    let open = text[start..]
        .find('{')
        .expect("a signature is followed by a body")
        + start;
    let mut depth = 0usize;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return text[open..=open + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after `{signature}`");
}

fn at(body: &str, needle: &str) -> usize {
    body.find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is no longer in the handler: {body}"))
}

/// The handler answers a side call before it creates an approval, saves a checkpoint, completes a
/// step or advances the run snapshot, and it never reads or creates the snapshot for one.
#[test]
fn a_side_call_returns_before_anything_is_written() {
    let grpc = source("src/grpc.rs");
    let body = function_body(&grpc, "async fn execute_step(");
    let early = at(&body, "return side_call_response(");
    for write in [
        "create_durable_approval(",
        ".save_checkpoint(",
        ".complete_step(",
        "self.state.update(",
    ] {
        assert!(
            early < at(&body, write),
            "the side-call return must come before `{write}`: {body}"
        );
    }
    assert!(
        body.contains("(!side_call).then(|| self.state.get_or_create(&run_id))"),
        "a side call must not read or create the run's snapshot: {body}"
    );
    assert!(
        body.contains("(!side_call).then_some(&self.state)"),
        "a side call must be handed no run state: {body}"
    );
    // The caller and the run owner are authorized before the shape check, and the check is the
    // only gate on the flag: the request is never trusted for more than a refusal-only lane.
    let authorized = at(&body, "self.authorize_run(");
    let shape = at(&body, "validate_side_call(side_call,");
    assert!(authorized < shape, "the owner check comes first: {body}");
    assert!(
        shape < at(&body, "runtime_loop::execute_step_with_browser_grant("),
        "the shape is judged before the step runs: {body}"
    );
}

/// The response helper, which is all the handler returns for a side call, touches neither the
/// session channel nor the run state.
#[test]
fn the_side_call_answer_touches_no_service_and_no_state() {
    let grpc = source("src/grpc.rs");
    let body = function_body(&grpc, "fn side_call_response(");
    for forbidden in [
        "session_channel",
        "session_client",
        "create_durable_approval",
        "StateStore",
        "state.update",
        "save_checkpoint",
        "complete_step",
    ] {
        assert!(
            !body.contains(forbidden),
            "`{forbidden}` in the side-call answer: {body}"
        );
    }
    assert!(body.contains("app_call_requires_chat") || body.contains("APP_CALL_REQUIRES_CHAT"));
}

/// A governed call's view comes from the binding the call's own decision carried when it is a side
/// call, and from the run views otherwise; the signed-class comparison and the mode check stay.
#[test]
fn a_side_call_takes_its_view_from_its_binding_and_keeps_every_other_check() {
    let text = source("src/runtime_loop/mod.rs");
    let body = function_body(&text, "async fn execute_governed_mcp(");
    let binding = at(&body, "signed.binding_view(tool_name)");
    let views = at(&body, "permission::run_view(org_id, run_id, tool_name)");
    let compared = at(&body, "signed.differs_from(");
    let mode = at(&body, "mode.allows(");
    assert!(
        binding < compared && views < compared && compared < mode,
        "{body}"
    );

    // The permission gate of a side call reads the binding, never the run views.
    let step = function_body(&text, "async fn execute_step_inner(");
    assert!(
        step.contains("side_call_permission(mode, tool_name, tool_input, &signed_class)"),
        "the permission gate of a side call must read the call's own binding: {step}"
    );
    // A side call is never offered a standing grant: the pause comes back before one is read.
    let pause = at(
        &step,
        "if let (true, Some(cause)) = (side_call, permission_pause)",
    );
    let grant = at(&step, "standing.released()");
    assert!(
        pause < grant,
        "a side call must not reach a standing grant: {step}"
    );
}

/// The gateway sets the flag itself, on the app-call route only, and never copies it from the
/// app's request body.
#[test]
fn only_the_app_call_route_sets_the_flag() {
    let gateway = source("../model-gateway/src/mcp_app_calls.rs");
    let body = function_body(&gateway, "async fn execute_step(");
    assert!(body.contains("side_call: true"), "{body}");
    for other in [
        "../model-gateway/src/tools.rs",
        "../model-gateway/src/browser_run.rs",
    ] {
        let text = source(other);
        assert!(
            !text.contains("side_call: true"),
            "{other} must not mark a step as a side call"
        );
    }
    let request = function_body(&gateway, "pub struct AppCallBody");
    assert!(
        !request.contains("side_call"),
        "the app's body must not carry the flag: {request}"
    );
}
