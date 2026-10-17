//! Closing the calls a pause left dangling (build plan WS03 S5).
//!
//! When a run resumes, the round that was in flight may hold calls that never
//! produced a result: the loop died, the process restarted, or the human paused it
//! mid-round. The resumed loop must not guess what happened to them. This module is
//! the whole decision, as a pure function of what the ledger recorded:
//!
//! * a call whose result is already recorded is kept as it is;
//! * a call that provably never started, or is safe to repeat, runs again;
//! * a call that may have run and could repeat an effect is closed with a sentence
//!   that says so, and is never retried.
//!
//! The sentence comes from `mp_contracts::loop_fold::closer_text`. It names the
//! tool and the kind of closure, never the call's arguments or output.

// Nothing calls this until the in-place resume (build plan WS03 S7) does. The decision
// table is pinned by the tests below now, so the resume cannot invent its own.
#![allow(dead_code)]

use mp_contracts::effect_ledger::EffectClass;
use mp_contracts::loop_fold::{CloserKind, LedgerStatus};

/// What the resumed loop does with one dangling call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseDecision {
    /// The result is already recorded; nothing to close.
    Keep,
    /// Run the call again: it never started, or repeating it is harmless.
    Redispatch,
    /// Do not run it. Tell the model why, with this closer.
    Close(CloserKind),
}

/// What is known about one dangling call.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DanglingCall<'a> {
    /// The tool the model called.
    pub tool_name: &'a str,
    /// What the call may change.
    pub class: EffectClass,
    /// The ledger row's state, or `None` when no row exists.
    pub ledger: Option<LedgerStatus>,
    /// Whether the ledger was on when the call was made. With it off, a missing
    /// row proves nothing: the call may have run unrecorded.
    pub ledger_was_on: bool,
    /// Whether the call's result is recorded in the fold.
    pub has_output: bool,
    /// Whether the workspace the tool needs still exists.
    pub workspace_available: bool,
}

/// The tool that is never resumed: a browser run is stateful outside the platform
/// and its grant does not outlive the process that held it.
const NOT_RESUMABLE_TOOL: &str = "browser_agent";

/// Whether a tool keeps state in the run's workspace, so a resume that lost the
/// workspace cannot honestly run it again.
fn needs_workspace(tool_name: &str) -> bool {
    tool_name == "code_interpreter" || tool_name.starts_with("process_")
}

/// Decide what to do with one dangling call.
pub(crate) fn decide(call: &DanglingCall<'_>) -> CloseDecision {
    if call.tool_name == NOT_RESUMABLE_TOOL {
        return CloseDecision::Close(CloserKind::NotResumableTool);
    }
    if call.has_output && matches!(call.ledger, Some(LedgerStatus::Completed | LedgerStatus::Failed)) {
        return CloseDecision::Keep;
    }
    if needs_workspace(call.tool_name) && !call.workspace_available {
        return CloseDecision::Close(CloserKind::WorkspaceStateLost);
    }
    match call.ledger {
        // The call finished but its result never reached the fold.
        Some(LedgerStatus::Completed | LedgerStatus::Failed) => {
            CloseDecision::Close(CloserKind::ResultLost)
        }
        // Closed without running: nothing happened, so it may run now.
        Some(LedgerStatus::NotRun) => CloseDecision::Redispatch,
        // Reserved and never finished: it may have run. Repeating a read is
        // harmless; repeating an effect is not, so an effect is never retried.
        Some(LedgerStatus::Reserved | LedgerStatus::Unknown) => {
            if call.class.is_effectful() {
                CloseDecision::Close(CloserKind::UnknownOutcome)
            } else {
                CloseDecision::Redispatch
            }
        }
        // No row. With the ledger on, the loop reserves before it dispatches, so a
        // missing row means the call never started. With it off, a missing row proves
        // nothing, and an effect that may have run is not repeated.
        None => {
            if call.ledger_was_on || !call.class.is_effectful() {
                CloseDecision::Redispatch
            } else {
                CloseDecision::Close(CloserKind::UnknownOutcome)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::loop_fold::closer_text;

    fn call<'a>(tool: &'a str, class: EffectClass, ledger: Option<LedgerStatus>) -> DanglingCall<'a> {
        DanglingCall {
            tool_name: tool,
            class,
            ledger,
            ledger_was_on: true,
            has_output: false,
            workspace_available: true,
        }
    }

    const EFFECTS: [EffectClass; 2] = [EffectClass::Write, EffectClass::External];
    const SAFE: [EffectClass; 2] = [EffectClass::NoEffect, EffectClass::Read];

    #[test]
    fn a_reserved_or_unknown_effect_is_closed_and_never_retried() {
        for ledger in [LedgerStatus::Reserved, LedgerStatus::Unknown] {
            for class in EFFECTS {
                assert_eq!(
                    decide(&call("send_email", class, Some(ledger))),
                    CloseDecision::Close(CloserKind::UnknownOutcome),
                    "{ledger:?} {class:?}"
                );
            }
        }
    }

    #[test]
    fn a_reserved_read_is_safe_to_run_again() {
        for ledger in [LedgerStatus::Reserved, LedgerStatus::Unknown] {
            for class in SAFE {
                assert_eq!(
                    decide(&call("knowledge_search", class, Some(ledger))),
                    CloseDecision::Redispatch,
                    "{ledger:?} {class:?}"
                );
            }
        }
    }

    #[test]
    fn a_call_that_was_closed_without_running_runs_now_whatever_its_class() {
        for class in EFFECTS.into_iter().chain(SAFE) {
            assert_eq!(
                decide(&call("send_email", class, Some(LedgerStatus::NotRun))),
                CloseDecision::Redispatch,
                "{class:?}"
            );
        }
    }

    #[test]
    fn a_finished_call_keeps_its_result_or_reports_it_lost() {
        for status in [LedgerStatus::Completed, LedgerStatus::Failed] {
            let mut with_output = call("send_email", EffectClass::External, Some(status));
            with_output.has_output = true;
            assert_eq!(decide(&with_output), CloseDecision::Keep, "{status:?}");
            assert_eq!(
                decide(&call("send_email", EffectClass::External, Some(status))),
                CloseDecision::Close(CloserKind::ResultLost),
                "{status:?}"
            );
        }
    }

    #[test]
    fn a_missing_row_proves_the_call_never_started_only_while_the_ledger_was_on() {
        for class in EFFECTS.into_iter().chain(SAFE) {
            assert_eq!(
                decide(&call("send_email", class, None)),
                CloseDecision::Redispatch,
                "ledger on: {class:?}"
            );
        }
        let mut ledger_off = call("send_email", EffectClass::External, None);
        ledger_off.ledger_was_on = false;
        assert_eq!(
            decide(&ledger_off),
            CloseDecision::Close(CloserKind::UnknownOutcome),
            "an effect with no row and no ledger may have run unrecorded"
        );
        let mut read_off = call("knowledge_search", EffectClass::Read, None);
        read_off.ledger_was_on = false;
        assert_eq!(decide(&read_off), CloseDecision::Redispatch);
    }

    #[test]
    fn a_browser_run_is_never_resumed() {
        for ledger in [None, Some(LedgerStatus::Reserved), Some(LedgerStatus::NotRun)] {
            assert_eq!(
                decide(&call("browser_agent", EffectClass::External, ledger)),
                CloseDecision::Close(CloserKind::NotResumableTool),
                "{ledger:?}"
            );
        }
    }

    #[test]
    fn a_workspace_tool_without_its_workspace_is_closed_as_state_lost() {
        for tool in ["code_interpreter", "process_start", "process_status"] {
            let mut lost = call(tool, EffectClass::Write, Some(LedgerStatus::NotRun));
            lost.workspace_available = false;
            assert_eq!(
                decide(&lost),
                CloseDecision::Close(CloserKind::WorkspaceStateLost),
                "{tool}"
            );
        }
        // With the workspace present it follows the ledger like any other call.
        assert_eq!(
            decide(&call("code_interpreter", EffectClass::Write, Some(LedgerStatus::NotRun))),
            CloseDecision::Redispatch
        );
        // A tool that keeps no workspace state is unaffected by a lost workspace.
        let mut other = call("knowledge_search", EffectClass::Read, None);
        other.workspace_available = false;
        assert_eq!(decide(&other), CloseDecision::Redispatch);
    }

    #[test]
    fn every_closure_has_a_sentence_that_names_the_tool_and_never_the_arguments() {
        for kind in [
            CloserKind::UnknownOutcome,
            CloserKind::ResultLost,
            CloserKind::NotResumableTool,
            CloserKind::WorkspaceStateLost,
        ] {
            let text = closer_text(kind, "send_email");
            assert!(text.contains("send_email"), "{kind:?}");
            assert!(!text.contains("a@example.test"), "{kind:?}");
        }
    }
}
