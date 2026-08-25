//! Guards that compaction's recovery route stays connected end to end.
//!
//! Compaction edits the PROMPT; the durable thread keeps every message. That
//! makes "I cannot see that" a statement about one request, not about history —
//! and `reattach_context` is the only thing that turns it back into a question
//! the model can answer. Three links carry that, and each fails silently:
//!
//! 1. The dropped/summarised notices must NAME the tool. A renamed tool leaves
//!    the model reading an instruction to call something that does not exist.
//!    (`tool_loop`'s unit test pins this half — the tool is advertised.)
//! 2. The tool must be handed the CURRENT prompt, not an empty slice. `&[]`
//!    compiles perfectly and degrades the feature to "returns things the model
//!    can already see", including turning a genuine miss into a false hit.
//! 3. The snapshot must come from the round's live message list, not from the
//!    turn's opening prompt, or a tool result appended last round reads as
//!    missing history this round.
//!
//! Read as source text because all three are call-shape properties inside one
//! private function: nothing a caller can observe, and nothing the type checker
//! objects to. A compile cannot tell `&[]` from the real thing.

use std::path::{Path, PathBuf};

fn source(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do \
             not delete it — it is what keeps context recovery from silently \
             becoming a no-op.",
            path.display()
        )
    })
}

/// Extract one function body by brace balance, starting at its signature.
fn function_body(text: &str, signature: &str) -> String {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is gone; re-point this test"));
    let open = text[start..]
        .find('{')
        .expect("a function signature is followed by a body")
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

#[test]
fn the_recovery_tool_is_handed_the_real_prompt() {
    let text = source("src/tool_loop.rs");
    let arm = function_body(&text, "pub async fn dispatch_tool(");
    let recovery = arm
        .split(r#""reattach_context" => {"#)
        .nth(1)
        .expect("the reattach_context dispatch arm is gone; re-point this test");
    let call = recovery
        .split("select_reattachment(")
        .nth(1)
        .expect("the arm no longer selects a reattachment")
        .split(')')
        .next()
        .expect("a call has a closing paren");
    assert!(
        call.contains("prompt_contents"),
        "the recovery arm must exclude what the model can already see; it passes: {call}"
    );
    assert!(
        !call.contains("&[]"),
        "an empty prompt makes every visible message look like recovered history: {call}"
    );
}

#[test]
fn the_prompt_snapshot_follows_the_round_not_the_turn() {
    let text = source("src/tool_loop.rs");
    let rounds = function_body(&text, "pub async fn run_tool_rounds(");
    let snapshot = rounds
        .split("let prompt_contents: Vec<String> =")
        .nth(1)
        .expect("run_tool_rounds no longer snapshots the prompt for recovery")
        .split(';')
        .next()
        .expect("a let binding ends in a semicolon");
    assert!(
        snapshot.contains("messages"),
        "the snapshot must come from the live message list: {snapshot}"
    );
    assert!(
        !snapshot.contains("base_messages"),
        "base_messages is the turn's OPENING prompt — using it makes last round's \
         tool result read as dropped history: {snapshot}"
    );
}

/// The notices are what make the model reach for recovery at all. Both
/// compaction outcomes need it: dropping is the summariser-outage fallback, and
/// summarising is the common path — a summary that loses a detail is the far
/// more likely reason the model needs the original text back.
#[test]
fn both_compaction_outcomes_offer_recovery() {
    let text = source("src/compaction.rs");
    for constant in ["DROPPED_HISTORY_NOTICE", "SUMMARY_PREFIX"] {
        let declaration = text
            .split(&format!("pub const {constant}: &str ="))
            .nth(1)
            .unwrap_or_else(|| panic!("`{constant}` is gone; re-point this test"))
            .split(";\n")
            .next()
            .expect("a const ends in a semicolon");
        assert!(
            declaration.contains("reattach_context"),
            "{constant} stopped telling the model recovery is possible: {declaration}"
        );
    }
}
