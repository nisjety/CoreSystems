//! Closing the calls a pause left dangling (build plan WS03 S5).
//!
//! When a run resumes, the round that was in flight may hold calls that never
//! produced a result: the loop died, the process restarted, or the human paused it
//! mid-round. The resumed loop must not guess what happened to them. This module is
//! the whole decision, as a pure function of what the ledger recorded:
//!
//! * a call whose result is already recorded is kept as it is (for every tool);
//! * a call the ledger says was closed without running, or a read that may have run,
//!   runs again;
//! * a call that may have run and could repeat an effect is closed with a sentence
//!   that says so, and is never retried. A call the ledger has no row for is such a
//!   call: absence is unknown, never "not run".
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
    /// The ledger row's state, or `None` when no row exists. A missing row is UNKNOWN
    /// (the call may never have started, or its reserve may have been lost, or the read
    /// may hide the row): it never proves the call did not run.
    pub ledger: Option<LedgerStatus>,
    /// Whether the call's result is recorded in the fold. A recorded result wins over
    /// every ledger state, for every tool.
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

/// Decide what to do with one dangling call. The rules, in order:
///
/// 1. A recorded output wins: `Keep`, for every tool (a recorded `browser_agent` result
///    included) and whatever the ledger says. Keeping re-runs nothing.
/// 2. `browser_agent` with no recorded output is `NotResumableTool`.
/// 3. A workspace tool whose workspace is gone is `WorkspaceStateLost`.
/// 4. A finished row (`completed`, `failed`) whose result is not in the fold is `ResultLost`.
/// 5. `not_run`: nothing happened, so it runs now.
/// 6. `reserved` or `unknown_outcome`: it may have run. An effect is `UnknownOutcome`, a
///    read runs again.
/// 7. No row: unknown, as in 6. An effect is `UnknownOutcome`, a read runs again.
///
/// This function sees no arguments, so the rule that a call whose arguments were withheld
/// is never redispatched is enforced by the caller (`resume::restore`) before it gets here.
/// The ORIGINAL paused step is never fed to it either: the release runs under a new step id
/// and the paused step is not a dangling call.
pub(crate) fn decide(call: &DanglingCall<'_>) -> CloseDecision {
    if call.has_output {
        return CloseDecision::Keep;
    }
    if call.tool_name == NOT_RESUMABLE_TOOL {
        return CloseDecision::Close(CloserKind::NotResumableTool);
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
        // Reserved and never finished, or no row at all: it may have run (absence is
        // unknown, never "not run"). Repeating a read is harmless; repeating an effect is
        // not, so an effect is never retried.
        Some(LedgerStatus::Reserved | LedgerStatus::Unknown) | None => {
            if call.class.is_effectful() {
                CloseDecision::Close(CloserKind::UnknownOutcome)
            } else {
                CloseDecision::Redispatch
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
            has_output: false,
            workspace_available: true,
        }
    }

    /// Every state a ledger row can be in, and no row at all.
    const EVERY_LEDGER_STATE: [Option<LedgerStatus>; 6] = [
        None,
        Some(LedgerStatus::Reserved),
        Some(LedgerStatus::Completed),
        Some(LedgerStatus::Failed),
        Some(LedgerStatus::NotRun),
        Some(LedgerStatus::Unknown),
    ];

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

    /// Absence from the ledger is UNKNOWN, never "not run": a row can be missing because the
    /// call never started, because the reserve was lost, or because the read hides it. An
    /// effect that may have run is closed, and only a read is safe to run again.
    #[test]
    fn an_absent_row_is_unknown_so_an_effect_is_closed_and_a_read_runs_again() {
        for class in EFFECTS {
            assert_eq!(
                decide(&call("send_email", class, None)),
                CloseDecision::Close(CloserKind::UnknownOutcome),
                "an effect with no row may have run unrecorded: {class:?}"
            );
        }
        for class in SAFE {
            assert_eq!(
                decide(&call("knowledge_search", class, None)),
                CloseDecision::Redispatch,
                "repeating a read is harmless: {class:?}"
            );
        }
    }

    /// A result the fold recorded is authoritative: whatever the ledger says, even a row
    /// that never closed (a failed best-effort finalize leaves `reserved`), and even with
    /// no row at all, the call is kept as it is. Nothing is re-run and nothing is closed.
    #[test]
    fn fold_output_is_authoritative_over_an_open_reservation() {
        for ledger in EVERY_LEDGER_STATE {
            for class in EFFECTS.into_iter().chain(SAFE) {
                let mut recorded = call("send_email", class, ledger);
                recorded.has_output = true;
                assert_eq!(
                    decide(&recorded),
                    CloseDecision::Keep,
                    "{ledger:?} {class:?}: a recorded output wins"
                );
            }
        }
        // A tool that keeps workspace state, its workspace gone: the result is still recorded.
        let mut workspace_lost = call("code_interpreter", EffectClass::Write, Some(LedgerStatus::Reserved));
        workspace_lost.has_output = true;
        workspace_lost.workspace_available = false;
        assert_eq!(decide(&workspace_lost), CloseDecision::Keep);
    }

    /// A read whose output is in the fold is not run a second time, whatever its row says.
    #[test]
    fn a_recorded_read_is_not_rerun() {
        for ledger in EVERY_LEDGER_STATE {
            for class in SAFE {
                let mut recorded = call("knowledge_search", class, ledger);
                recorded.has_output = true;
                assert_ne!(
                    decide(&recorded),
                    CloseDecision::Redispatch,
                    "{ledger:?} {class:?}: a read with a recorded output is never run again"
                );
                assert_eq!(decide(&recorded), CloseDecision::Keep, "{ledger:?} {class:?}");
            }
        }
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

    /// The browser rule is for a browser run with NO recorded output. A recorded output wins
    /// for every tool: keeping it re-runs nothing, and closing it would throw away a result
    /// the run already has. (The tool is still never dispatched again; `Keep` is not a run.)
    #[test]
    fn a_recorded_browser_output_is_kept() {
        for ledger in EVERY_LEDGER_STATE {
            let mut recorded = call("browser_agent", EffectClass::External, ledger);
            recorded.has_output = true;
            assert_eq!(decide(&recorded), CloseDecision::Keep, "{ledger:?}");
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
