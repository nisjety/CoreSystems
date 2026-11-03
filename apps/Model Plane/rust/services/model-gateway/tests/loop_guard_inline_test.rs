//! The inline chat loop's stop rules, driven the way `tool_loop.rs` drives them (WS25-S1).
//!
//! The loop keeps one `LoopGuard` per turn, feeds it each model round's calls as `CallRecord`s
//! keyed by `call_signature`, and acts on a verdict only under `LOOP_GUARD_MODE=enforce`. A whole
//! chat turn needs a live inference and tool stack, so these tests drive the same sequence through
//! the public guard and pin the wiring in `tool_loop.rs` and `sse.rs` by reading their source, as
//! `tool_retry_contract.rs` does for its own loop.

use std::path::Path;

use mp_contracts::loop_guard::{
    call_signature, CallOutcome, CallRecord, LoopGuard, StopReason, Verdict, REMIND_FIRST,
    REMIND_SECOND,
};

/// One model round in which the model asked for a single call.
fn one_call(tool: &str, arguments: &str, outcome: CallOutcome) -> Vec<CallRecord> {
    vec![CallRecord {
        tool: tool.to_owned(),
        signature: call_signature(tool, arguments),
        outcome,
    }]
}

#[test]
fn eight_failing_fetch_url_calls_stop_the_inline_loop_at_eight() {
    let arguments = r#"{"url":"https://unreachable.example/page"}"#;
    let mut guard = LoopGuard::new();
    let verdicts: Vec<Verdict> = (0..8)
        .map(|_| guard.observe_round(&one_call("fetch_url", arguments, CallOutcome::Err)))
        .collect();

    assert!(verdicts[..2].iter().all(|verdict| *verdict == Verdict::Continue));
    assert_eq!(
        verdicts[2],
        Verdict::Remind {
            level: 1,
            text: REMIND_FIRST
        }
    );
    assert_eq!(
        verdicts[4],
        Verdict::Remind {
            level: 2,
            text: REMIND_SECOND
        }
    );
    let Verdict::Stop(stop) = &verdicts[7] else {
        panic!("the eighth failing call must stop the loop, got {:?}", verdicts[7]);
    };
    assert_eq!(stop.reason, StopReason::Repeat);
    assert_eq!(stop.tool, "fetch_url");
    assert_eq!(stop.count, 8);
    assert_eq!(stop.signature_hash, call_signature("fetch_url", arguments));
    assert!(verdicts[..7].iter().all(|verdict| !matches!(verdict, Verdict::Stop(_))));
}

#[test]
fn a_failing_side_effecting_call_is_only_observed_and_its_stop_carries_no_arguments() {
    let arguments = r#"{"title":"Board memo","content":"Confidential draft text"}"#;
    let mut guard = LoopGuard::new();
    let verdicts: Vec<Verdict> = (0..8)
        .map(|_| guard.observe_round(&one_call("create_artifact", arguments, CallOutcome::Err)))
        .collect();

    // The guard returns a verdict and nothing more. It holds no dispatcher, so a stop cannot run
    // the write again; the loop that owns the write decides what happens next.
    let Verdict::Stop(stop) = &verdicts[7] else {
        panic!("eight failing writes must stop the loop, got {:?}", verdicts[7]);
    };
    assert_eq!(stop.reason, StopReason::Repeat);
    assert_eq!(stop.tool, "create_artifact");
    let rendered = format!("{stop:?}");
    assert!(!rendered.contains("Confidential"), "{rendered}");
    assert!(!rendered.contains("Board memo"), "{rendered}");
}

#[test]
fn four_rounds_of_failing_different_calls_stop_the_inline_loop_as_no_progress() {
    let mut guard = LoopGuard::new();
    let rounds = [
        one_call("fetch_url", r#"{"url":"https://a.example"}"#, CallOutcome::Err),
        one_call("web_search", r#"{"query":"coresystem"}"#, CallOutcome::Err),
        one_call("knowledge_search", r#"{"query":"coresystem"}"#, CallOutcome::Suppressed),
        one_call("fetch_url", r#"{"url":"https://d.example"}"#, CallOutcome::Err),
    ];
    let verdicts: Vec<Verdict> = rounds.iter().map(|round| guard.observe_round(round)).collect();

    assert!(verdicts[..3].iter().all(|verdict| *verdict == Verdict::Continue));
    let Verdict::Stop(stop) = &verdicts[3] else {
        panic!("four stalled rounds must stop the loop, got {:?}", verdicts[3]);
    };
    assert_eq!(stop.reason, StopReason::NoProgress);
    assert_eq!(stop.count, 4);
}

/// A file of this crate's `src` as the compiler reads it. CRLF line endings are folded, so the
/// pins compare line breaks the same way on every checkout.
fn source_of(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read {} ({error}); re-point this test if the file moved",
                path.display()
            )
        })
        .replace("\r\n", "\n")
}

/// `tool_loop.rs`, the inline loop the pins above read.
fn tool_loop_source() -> String {
    source_of("tool_loop.rs")
}

#[test]
fn the_inline_loop_reads_the_mode_once_and_acts_on_a_verdict_only_when_it_enforces() {
    let source = tool_loop_source();
    assert_eq!(source.matches("fn loop_guard_mode() -> LoopGuardMode {").count(), 1);
    assert_eq!(source.matches("let guard_mode = loop_guard_mode();").count(), 1);
    assert_eq!(
        source.matches("if guard_mode.enforces() {").count(),
        2,
        "a reminder and a stop each change what the model sees only under enforce"
    );
    let stop = "loop_stop = Some(stop);";
    let at = source.find(stop).expect("an enforced stop is recorded");
    let next = source[at + stop.len()..]
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .expect("a line follows the recorded stop");
    assert_eq!(next, "break;", "a stop ends the tool phase before another round is sent");
}

#[test]
fn the_guard_observes_model_rounds_and_never_the_gateway_prefetch() {
    let source = tool_loop_source();
    assert!(
        source.contains("loop_guard.as_mut().filter(|_| round.is_some())"),
        "the gateway's own prefetch is no model round and must not reach the guard"
    );
}

#[test]
fn the_repeat_check_and_the_guard_share_one_query_normaliser() {
    let source = tool_loop_source();
    assert_eq!(
        source
            .matches("loop_guard::normalise_query(&arg_str(&call.arguments_json, \"query\"))")
            .count(),
        2
    );
    assert!(!source.contains("fn normalize_tool_query"));
}

#[test]
fn an_enforced_stop_leaves_the_tool_phase_and_reaches_the_answers_metadata() {
    let source = source_of("sse.rs");
    assert_eq!(
        source.matches("loop_stop = rounds.loop_stop;").count(),
        1,
        "the stop leaves the tool phase with the rounds"
    );
    assert_eq!(
        source.matches("loop_stop.as_ref(),").count(),
        1,
        "and the answer's metadata is the one place it is written"
    );
}
