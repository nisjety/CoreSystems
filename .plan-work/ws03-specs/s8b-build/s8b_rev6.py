"""Revision 6 of the S8-B spec: applies the round-5 review findings on top of revision 5."""
import importlib.util


def _load(name):
    spec = importlib.util.spec_from_file_location(name, name + '.py')
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


_r5 = _load('s8b_rev5')
rep = _r5.rep
rep_span = _r5.rep_span
rep_elem = _r5.rep_elem
insert_before = _r5.insert_before
insert_after = _r5.insert_after
rep_all = _r5.rep_all
LOG = _r5.LOG


def apply(spec):
    # ============================================================ X1 (HIGH): loop-top age rule for ANY writer state
    rep(spec, 'approach',
        "At every loop top, before anything else, an armed loop stops when its writer has lost ownership, or when it is unproven (Denied) and the heartbeat age is at or past the window, so a loop whose end-of-round save was refused does not run the rest of its budget.",
        "At every loop top, before anything else (before the route-stopped, latched-refusal and deadline checks and before the heartbeat), an armed loop stops when its writer has lost ownership, or when its own last accepted heartbeat is at or past the window WHATEVER its writer's state: a loop silent that long must not heartbeat, because the worker may already have crash-parked the run, and a zombie's heartbeat would renew that park's obligation and reset its own age (round 5: a zombie whose in-flight end-of-round save returned Ok after a host suspend, with a monotonic-clock deadline that did not fire, could otherwise refresh its mark, reach the funnel with a young age and finalize over the crash park). A healthy armed loop with one round longer than 780 s therefore stops at its next loop top (with the default 300 s bearers it would have ended OutOfTime anyway), and a loop whose end-of-round save was refused does not run the rest of its budget either.")
    rep(spec, 'approach',
        "(pure: armed && (lost_ownership || (ownership_unproven && heartbeat_age >= fail_open_window()))), used by the loop top and by retire_fence;",
        "(pure: armed && (lost_ownership || heartbeat_age >= fail_open_window()), whatever the writer's state), used by the loop top only (retire_fence applies the age to a Denied answer by its own match);")
    rep(spec, 'file_changes',
        "`armed && (folds.lost_ownership() || (folds.ownership_unproven() && heartbeat_age >= mp_contracts::crash_park::fail_open_window()))` (S8B-3; used by the loop top and by retire_fence).",
        "`armed && (folds.lost_ownership() || heartbeat_age >= mp_contracts::crash_park::fail_open_window())` (S8B-3; whatever the writer's state, so a silent zombie cannot reach the heartbeat; used by the loop top only).")
    rep(spec, 'interfaces',
        "// armed && (lost_ownership || (ownership_unproven && age >= fail_open_window))",
        "// armed && (lost_ownership || age >= fail_open_window): any writer state, loop top only")
    rep(spec, 'invariants',
        "its writer has lost ownership (Stale, Closed or NotFound), or it is unproven (Denied) and park::beat_age(&last_beat) is at or past the fail-open window.",
        "its writer has lost ownership (Stale, Closed or NotFound), or park::beat_age(&last_beat) is at or past the fail-open window, whatever the writer's state (a loop silent that long must not heartbeat).")
    rep(spec, 'tests',
        "a_denied_loop_top_stops_only_past_the_window (the same seam at the loop top: an unproven writer stops the loop before any heartbeat or inference at 800 s and keeps running at 10 s)",
        "a_silent_armed_loop_stops_at_the_loop_top_whatever_its_writer_state (the same seam at the loop top: an ACTIVE writer whose last save returned Ok stops the loop at 800 s before any heartbeat or inference (the fake counts the heartbeats: none is sent) and keeps running at 10 s; the same for a Failed(Denied) writer)")
    rep(spec, 'file_changes',
        "the loop-top heartbeat comes AFTER the deadline check and after the must_stop check (an order guard: a woken dead-bearer zombie leaves through OutOfTime before it can reset its heartbeat age);",
        "the loop-top heartbeat comes AFTER the must_stop check, which is the first statement of the while and carries the age rule (an order guard: a woken zombie leaves at the loop top before it can heartbeat and reset its own age, whatever the deadline check does; the heartbeat also stays after the deadline check);")
    rep(spec, 'failure_matrix',
        "A returns Aborted continue_in_progress at its next loop top or at its exit funnel, before any append, receipt or approval;",
        "A returns Aborted continue_in_progress at its next loop top (a silent armed loop stops there by age alone, before it can heartbeat, whatever its writer's state) or at its exit funnel, before any append, receipt or approval;")
    rep(spec, 'failure_matrix',
        "its saves are Denied or Stale, its funnel returns the stop;",
        "its next loop top stops it by age before it can heartbeat; a save in flight at the pause is Denied or Stale and its funnel returns the stop;")
    rep(spec, 'summary',
        "and after a Denied one once its heartbeat age is at the window, so a superseded loop does not burn the rest of its budget.",
        "and, whatever its writer's state, once its own last accepted heartbeat is at the window (it must not heartbeat then), so a superseded loop does not burn the rest of its budget.")
    rep(spec, 'risks',
        "(the SPA's reaction to that frame is unverified).",
        "(the SPA's reaction to that frame is unverified); (g) a healthy armed loop with one round longer than the 780 s window stops at its next loop top instead of heartbeating (with the default 300 s bearers it would have ended OutOfTime anyway; it matters only for long-lived bearers).")

    # ============================================================ X2 (MEDIUM): fence only while a marked fold may be stored
    rep(spec, 'approach',
        "The decision. The loop proceeds past retire_fence only when it holds a committed tombstone or a reason why none can matter; everything else stops.",
        "The decision. (0) The fence exists to keep a MARKED fold from being crash-parked, so it applies only while one may be stored: the writer tracks that (FoldWriter::marked_fold_may_be_stored(): a save that carried the marker and returned Ok or ambiguous sets it, an Ok save that did not carry it clears it, an ambiguous one leaves it, a writer that never saved has it false). A run that answers in its first round, or whose last stored fold is unmarked (budget spent, a visible-call round's open-round pre_effect fold, an Ok tombstone), has none: for such a writer that has not lost ownership retire_fence returns Ok at once, with no RPC, no wait and no stop, because no crash park can exist for it. Otherwise the loop proceeds past retire_fence only when it holds a committed tombstone or a reason why none can matter; everything else stops.")
    rep(spec, 'file_changes',
        "then the decision of the approach (1(d)): a writer that lost ownership stops;",
        "then the decision of the approach (1(d)): a writer that lost ownership stops; a writer for which `!folds.marked_fold_may_be_stored()` returns Ok at once with no RPC and no wait (no crash park can exist for it);")
    rep(spec, 'file_changes',
        "They sit in S8B-2 because the visible-call hold uses lost_ownership and the CrashGate arms the writer.",
        "They sit in S8B-2 because the visible-call hold uses lost_ownership and the CrashGate arms the writer. A fifth pure accessor, `marked_fold_may_be_stored(&self) -> bool`, is backed by ONE private bool `marker_stored`, updated by one private helper `note_marker(carried: bool, outcome: &Result<u64, SaveError>)` called once inside each existing fold-saving wrapper after its save (save_round, save_pre_effect, save_fence, save_park, pause_fold; `carried` is false for the last two): Ok sets it to `carried`; Unavailable{ambiguous:true} sets it to `carried || marker_stored`; any other error leaves it. `carried` comes from the pure fn `carries_marker(snap: &Snapshot) -> bool` (`snap.crash_mark.is_some() && snap.round.is_none() && snap.pending.is_none()`), which build_fold itself uses for its marker rule, so the rule has one source; the tombstone save never carries it (an Ok retire clears the flag).")
    rep(spec, 'interfaces',
        "pub(crate) fn ownership_unproven(&self) -> bool;",
        "pub(crate) fn marked_fold_may_be_stored(&self) -> bool;     // S8B-2: marker_stored, set by a save that carried the marker (Ok or ambiguous), cleared by an Ok save that did not\n    pub(crate) fn ownership_unproven(&self) -> bool;")
    rep(spec, 'invariants',
        "or in one of these cases: (a)",
        "or in one of these cases: (0) no marked fold may be stored for the run (FoldWriter::marked_fold_may_be_stored() is false: a run that answers in its first round, or whose last stored fold is unmarked), so no crash park can exist and nothing needs fencing; (a)")
    rep(spec, 'risks',
        "An unconfirmed tombstone never proceeds: a healthy loop that cannot reach session-core at its end STOPS (after up to about 30 s) instead of appending,",
        "An unconfirmed tombstone never proceeds, for a run that may have a marked fold stored (a run that answers in its first round, or whose last stored fold is unmarked, makes no tombstone call at all and is not affected): a healthy loop that cannot reach session-core at its end STOPS (after up to about 30 s) instead of appending,")
    rep(spec, 'approach',
        "(i) An unconfirmed tombstone never proceeds: a healthy loop that cannot reach session-core at its end STOPS instead of risking a late append",
        "(i) An unconfirmed tombstone never proceeds, for a run that may have a marked fold stored (the others make no tombstone call): a healthy loop that cannot reach session-core at its end STOPS instead of risking a late append")
    rep(spec, 'failure_matrix',
        "for a healthy loop with a recent heartbeat: the loop STOPS",
        "for a healthy loop that may have a marked fold stored, with a recent heartbeat: the loop STOPS")
    rep(spec, 'failure_matrix',
        "a run end waits up to about 30 s (two tombstone attempts at the gated timeout plus one probe) and then the loop STOPS (F23).",
        "a run end waits up to about 30 s (two tombstone attempts at the gated timeout plus one probe) and then the loop STOPS (F23); only a run that may have a marked fold stored pays this, a first-round answer makes no tombstone call.")
    insert_after(spec, 'tests', "S8B-2 execution-core fold tests: lost_ownership_is_true_only_after",
        "S8B-2 execution-core fold tests, the marker tracker: marked_fold_may_be_stored is a pure table over save outcomes (a marked save Ok sets it; a marked save ambiguous sets it; a marked save Denied or Stale leaves it; an unmarked save Ok clears it; an unmarked save ambiguous leaves it; a tombstone Ok clears it; a writer that never saved has it false; a park fold and a pause fold never carry it), and carries_marker is the one rule build_fold uses (a Snapshot with crash_mark Some, round None and pending None carries it; every other shape does not). S8B-3 execution-core agent tests, no marked fold stored: a run that answers in its FIRST round (no fold saved), a run whose last stored fold is unmarked (budget spent; a visible-call round's open-round pre_effect fold) and a run after an Ok tombstone make NO retire RPC and cannot be stopped by an unconfirmed tombstone even when the store fake answers Unavailable and Denied to everything, and finalize runs as committed; a writer that lost ownership still stops there.")

    # ============================================================ X3 (MEDIUM): no 'unconfirmed Stale' arm
    rep(spec, 'approach',
        "Unavailable or a timeout (after the one retry of (5)), a Stale that follows an ambiguous attempt of the same retire (the loop cannot tell its own earlier commit from a foreign writer's), and a ContentRefused (nothing was sent).",
        "Unavailable or a timeout (after the one retry of (5)) and a ContentRefused (refused, not committed: a local credential-shape or size refusal, or a server InvalidArgument or OutOfRange). A Stale that follows an ambiguous attempt is NOT in this list: it is resolved inside save_bytes by the shared one-shot probe at expected+1, which matches the loop's own earlier commit and returns Ok (proceed: the tombstone is stored), or is Stale too (the stored seq is at least expected+2: a foreign writer) and is the definitive stop of (2).")
    rep(spec, 'approach',
        "a tombstone save answered Stale, Closed, run_ended or NotFound (the seq moved, the run ended, the run is gone)",
        "a tombstone save answered Stale, Closed (which includes run_ended: classify_status folds it) or NotFound (the seq moved, the run ended, the run is gone)")
    rep(spec, 'file_changes',
        "EVERYTHING ELSE (Unavailable or a timeout after the retry, a Stale after an ambiguous attempt, a ContentRefused) STOPS at any age: an unconfirmed tombstone never proceeds.",
        "EVERYTHING ELSE (Unavailable or a timeout after the retry, a ContentRefused) STOPS at any age: an unconfirmed tombstone never proceeds (a Stale after an ambiguous attempt never reaches this match: save_bytes' shared probe turns it into Ok or a definitive Stale).")
    rep(spec, 'file_changes',
        "Ok proceeds; Stale, Closed, run_ended and NotFound stop at any age;",
        "Ok proceeds; Stale, Closed (run_ended is Closed) and NotFound stop at any age;")
    rep(spec, 'invariants',
        "(Unavailable or a timeout after the one retry, an unconfirmed Stale after an ambiguous attempt, ContentRefused)",
        "(Unavailable or a timeout after the one retry, ContentRefused)")
    rep(spec, 'risks',
        "(Unavailable or a timeout, an unconfirmed Stale, a ContentRefused)",
        "(Unavailable or a timeout, a ContentRefused)")
    rep(spec, 'tests',
        "a transient failure (Unavailable or a timeout, one retry), an unconfirmed Stale after an ambiguous attempt and a ContentRefused STOP at ANY age",
        "a transient failure (Unavailable or a timeout, one retry) and a ContentRefused STOP at ANY age")
    rep(spec, 'tests',
        "an ambiguous first attempt that COMMITTED server-side followed by a Stale retry is the same stop (a fake that times out client-side but commits, then answers Stale: the loop's own tombstone is stored, nothing is appended, the worker ends the run later);",
        "an ambiguous first attempt that COMMITTED server-side followed by the retry: the retry's Stale is resolved by the shared probe, which matches the stored own tombstone and returns Ok, so the loop proceeds (a fake that times out client-side but commits); a probe that is also Stale (the stored seq at least expected+2) is a definitive stop;")

    # ============================================================ X4: NotPersistent
    rep(spec, 'approach',
        "The exception is a store that answers disabled or Unimplemented, or a missing descriptor key:",
        "The exception is a store that answers disabled or Unimplemented, a missing descriptor key, or a run the store reports as not persistent (NotPersistent):")
    rep(spec, 'file_changes',
        "A store that answers disabled or Unimplemented, or a missing key, turns the writer Off or Failed for a non-ownership reason and the loop proceeds as committed (R19b).",
        "A store that answers disabled or Unimplemented, a missing key or NotPersistent turns the writer Off or Failed for a non-ownership reason and the loop proceeds as committed (R19b).")
    rep(spec, 'tests',
        "an UNARMED loop makes no retire call and never stops, at both sites",
        "a writer that is Off or Failed for a non-ownership reason (StoreOff, NotPersistent, Unimplemented, KeyUnavailable) proceeds with no RPC; an UNARMED loop makes no retire call and never stops, at both sites")

    # ============================================================ X5-X10
    insert_before(spec, 'risks', "R19: a new user turn on the same thread while a crash park waits:",
        "R19j: finalize and the terminal-receipt RPCs carry no client timeout (the session channel sets only connect_timeout), so the 120 s margin bounds when a Denied loop that proceeds STARTS its receipt, not when it lands: a loop at age 770 s that then hangs more than 130 s in the token mint or the receipt RPC can land a failed receipt after the worker re-sealed. It needs a hang of minutes in a call that takes milliseconds; the effect is a failed receipt, never an answer (the append is refused by the same expired bearer). Optional hardening: re-check beat_age immediately before the receipt for a loop that proceeded on a Denied answer.")
    rep(spec, 'risks',
        "the one remaining skew is between the executor's and session-core's wall clocks, absorbed by the 120 s margin.",
        "the one remaining skew is between the executor's and session-core's wall clocks, absorbed by the 120 s margin. A forward wall-clock step of 780 s or more (an NTP or VM-resume correction) stops every armed loop at its next loop top and a Denied one at its funnel; acceptable, since after such a step the worker's own deadline has usually passed too.")
    rep(spec, 'failure_matrix',
        "if it could, the approvals fact refuses the run.",
        "if it could, the approvals fact refuses the run. If the tombstone is confirmed and then the approval cannot be created (continuation_descriptor fails, create_approval Unavailable), the loop returns Err with the 'paused' tombstone as the stored fold and no approval anywhere: the run is terminalized outcome_unknown at the deadline instead of being crash-parked (R14).")
    rep(spec, 'risks',
        "any other unconfirmed save stops the loop before the approval exists.",
        "any other unconfirmed save stops the loop before the approval exists. A confirmed tombstone followed by a failed approval creation leaves a non-resumable fold and no approval: the run ends outcome_unknown at the deadline instead of being crash-parked (F29).")
    rep(spec, 'tests',
        "a_denied_tombstone_at_800_s_stops_and_at_10_s_proceeds (through the beat_test_clock seam: Denied end-of-round and tombstone answers with the age forced to 800 s: Aborted continue_in_progress with no AppendMessage and no RecordTerminalOutcome in Recorder.terminal_order; forced to 10 s: the loop finalizes and writes the committed failed receipt);",
        "a_denied_tombstone_at_800_s_stops_and_at_10_s_proceeds (through the beat_test_clock seam, on a run whose FINAL round answers so that the loop reaches the funnel without another loop top: an earlier tool round stored a marked boundary fold (so a marked fold may be stored), the tombstone is answered Denied and the age is forced to 800 s: Aborted continue_in_progress with no AppendMessage and no RecordTerminalOutcome in Recorder.terminal_order; forced to 10 s: the loop proceeds to finalize, and the test injects an append failure (the Recorder's append always succeeds, it never models an expired bearer) so that the receipt is the committed failed one; the Denied injection must count the end-of-round save and the tombstone, both kind round, because LoopSaveFailure matches by kind and counts down);")
    rep(spec, 'approach',
        "run_rounds keeps `last_beat: park::BeatMark`, initialised from it;",
        "run_rounds keeps `last_beat: park::BeatMark`, initialised from it (BeatMark derives Clone and Copy; LoopContext is built in two places, the root literal and the child literal of a delegated loop, and both gain the field);")
    rep(spec, 'file_changes',
        "run_rounds keeps `last_beat: park::BeatMark`, initialised from it;",
        "run_rounds keeps `last_beat: park::BeatMark`, initialised from it (BeatMark derives Clone and Copy; LoopContext is built in two places, the root literal and the child literal of a delegated loop, and both gain the field);")
    rep(spec, 'open_questions',
        "or a poison run can kill an executor five times and every in-flight run with it.",
        "or a poison run can kill an executor five times and every in-flight run with it. It also needs a rule for how a client retry and a resume interact: the stopped loop's stream ends in a RETRYABLE agent_dispatch_unavailable frame, so a client retry starts a NEW run while the stopped one is crash-parkable, and with a resumer the turn could be answered twice.")

    # ============================================================ C1 (medium): kill order operability
    rep_span(spec, 'approach',
        "While a crash park exists NONE of the resume lane may go off first:",
        "cannot be resumed.",
        "While a crash park exists the resume lane stays ON: it is switched off only after (6) is off AND the drain query of the environment step reads 0, or after the owner accepts that the remaining parks end outcome_unknown at the crash TTL. The lane is AGENT_CONTINUE_AFTER_APPROVAL_ENABLED and EXECUTION_LOOP_RESUME_ENABLED (each is ONE host variable that both model-gateway and execution-core read in the compose file, so the two halves flip together in one flip: there is no 'gateway first' or 'neither first' to perform, and the committed S7 text that rolls the gateway back first cannot be followed while parks exist), EXECUTION_LOOP_CHECKPOINT_ENABLED, EXECUTION_EFFECT_LEDGER_ENABLED, and the session-core store and park-recovery switches. The park form (MODEL_GATEWAY_LOOP_PARKING_ENABLED and EXECUTION_LOOP_PARKING_ENABLED, plus the BFF flag) is the only pair with separate host variables, and an image below S8-7 or S8-8 or an executor image below S8-A also counts as the lane going off; otherwise those parks are held for the crash TTL and cannot be resumed.")
    rep_span(spec, 'steps',
        "While a crash park exists NONE of the resume lane may go off first: the executor's resume switches;",
        "cannot be resumed.",
        "While a crash park exists the resume lane stays ON until (6) is off AND the drain query of the environment step reads 0 (or the owner accepts losing the remaining parks at the crash TTL); the lane is the two shared host variables AGENT_CONTINUE_AFTER_APPROVAL_ENABLED and EXECUTION_LOOP_RESUME_ENABLED (one variable each for gateway and executor, flipped together), EXECUTION_LOOP_CHECKPOINT_ENABLED, EXECUTION_EFFECT_LEDGER_ENABLED and the session-core store and park-recovery switches, and the park form and the image floors of approach 6. Otherwise those parks are held for the crash TTL and cannot be resumed.")
    rep(spec, 'steps',
        "DRAIN QUERY before a session-core rollback or a descriptor key rotation:",
        "DRAIN QUERY, a GATE for every step that takes the resume lane away (the S8-A kill steps (a) to (e), a session-core rollback, a descriptor key rotation):")
    rep(spec, 'steps',
        "PRE-FLIGHT CHECKLIST before (5): the four resume switches, the park-recovery and store switches, the descriptor key and the MANAGED_DEADLINE_SECS pairing are present and equal on every replica of the three services;",
        "PRE-FLIGHT CHECKLIST before (5): the four resume switches, the park-recovery and store switches, the descriptor key, the gateway park form (MODEL_GATEWAY_LOOP_PARKING_ENABLED) and the BFF flag are present and equal on every replica of the three services; the MANAGED_DEADLINE_SECS pairing is a compile-time constant on both sides, so it is checked by comparing the deployed image revisions against the commit that holds the source pin;")

    # ============================================================ C2 (medium): CI is not a backstop
    rep(spec, 'steps',
        ", and CI runs the whole workspace, so a break would otherwise land red on this commit);",
        "; CI is no backstop, R19e: its `rust` job stops at the red formatting step, so no workspace test runs there); `bash \"apps/Model Plane/scripts/tests/session-core-db-tests-test.sh\"` (no docker: the only check of the harness edit);")
    rep(spec, 'risks',
        "a future wholesale fmt is a separate change that would need its own re-anchoring.",
        "a future wholesale fmt is a separate change that would need its own re-anchoring. CI is therefore NOT a backstop for any S8-B step: the `rust` job runs `cargo fmt --all -- --check` first and stops there, so `cargo check` and `cargo test --workspace` are never reached, and each step's Verify line is the only gate. The separate session-core-receipts job has no `needs` and still runs.")
    rep(spec, 'steps',
        "Verify: cargo test -q -p execution-core --lib runtime_loop:: and --test durable_loop_contract (all committed rows green, none edited).",
        "Verify: cargo test -q -p execution-core --lib runtime_loop:: and --test durable_loop_contract --test cross_service_loop_contract (park.rs is scanned by park_ref_violations), and `cargo test -p execution-core --tests` once (other integration tests read agent.rs text) (all committed rows green, none edited).")

    # ============================================================ C3 (medium): golden list
    rep_span(spec, 'tests',
        "the_read_only_tool_set_is_a_golden_list (it replaces a tautology",
        "fails until a person classifies it.",
        "the_read_only_tool_set_is_a_golden_list (it replaces a tautology: effect_class_of and check_autonomy_rung both derive from rung_required_for, so 'effectful implies refused' holds for ANY classifier, a wrong one included): two hard-coded lists over every offered_tool_defs name plus tickets.create, each entry with a written reason, asserted against rung_required_for(name, sample_input) at the read_only rung. (a) Names that RUN under read_only: code_interpreter (hermetic), web_fetch and web.read (outbound GETs), plan.propose and subagent.* (Read-classed visible writes), and execute_provider_action, which is classified per call by its `operation` argument, listed with each catalogued read operation explicitly. (b) Names offered but REFUSED under read_only: tickets.create (the keyword 'create' makes it DangerFullAccess, and it also needs a durable-owner approval), the shell-class names, save_memory (WorkspaceWrite), browser_agent and the like, each with the keyword or rule that refuses it. The two lists must partition the offered set and equal what rung_required_for computes, so a new tool, or a keyword-list change that flips a name, fails until a person classifies it. (The marker's tool-names proof admits tickets.create into the marked set; its calls are refused at the rung gate, so the exactness claim holds.)")

    # ============================================================ C4: approvals status
    rep(spec, 'tests',
        "(status denied, status consumed, and, with expiry_test_switch::Forced::set(true), status requested with expires_at in the past)",
        "(status denied; status granted with consumed_at set, which is how consumption is recorded (no code writes a status 'consumed'); and, with expiry_test_switch::Forced::set(true), status requested with expires_at in the past)")

    # ============================================================ C5: fmt per-file command
    rep(spec, 'approach',
        "must not grow beyond the count recorded when S8B-3 starts, and the builder indents the async block by hand as rustfmt would.",
        "must not grow beyond the count recorded at the START of S8B-2 (the per-file command is in the S8B-3 step; the literal `grep -c` returns one package total), at S8B-2, S8B-3 and S8B-4 alike, and the builder indents the async block by hand as rustfmt would.")
    rep(spec, 'steps',
        "Instead record `cargo fmt -p execution-core -- --check 2>&1 | grep -c 'Diff in'` per touched file when S8B-3 starts and require that no touched file's count grows (indent the async block by hand as rustfmt would), and run `cargo test -p execution-core --tests` once (other integration tests read agent.rs text).",
        "Instead record the per-file baseline at the START of S8B-2 with `cargo fmt -p execution-core -- --check 2>&1 | sed 's/\\x1b\\[[0-9;]*m//g' | grep '^Diff in' | sed -E 's/:[0-9]+:$//' | sort | uniq -c` (the literal `grep -c 'Diff in'` returns ONE package total, 799 at the base; the per-file counts at the base are agent.rs 322, durable_loop_contract.rs 123, park.rs 30, fold.rs 26, cross_service_loop_contract.rs 7) and require that no touched file's count grows at S8B-2, S8B-3 or S8B-4; new files (crash_park.rs) must pass `rustfmt --check <file>`; indent the async block by hand as rustfmt would (allow one iteration: indenting the loop body by 4 pushes lines at columns 97 to 100 over the limit, for example the `children_started: children_started.load(..)` line, so re-wrap them); and run `cargo test -p execution-core --tests` (other integration tests read agent.rs text).")
    rep(spec, 'verification_commands',
        "at S8B-3 compare the per-file `Diff in` counts of `cargo fmt -p execution-core -- --check` against the counts recorded when S8B-3 started (no touched file's count may grow)",
        "at S8B-2, S8B-3 and S8B-4 compare the per-file `Diff in` counts of `cargo fmt -p execution-core -- --check` (form in the S8B-3 step) against the counts recorded at the start of S8B-2 (no touched file's count may grow)")
    rep(spec, 'risks',
        "S8B-3 gates on a per-file baseline instead, and the three re-anchored mutation rows are whitespace-insensitive;",
        "S8B-2 to S8B-4 gate on a per-file baseline instead, and the three re-anchored mutation rows are whitespace-insensitive;")

    # ============================================================ C6: worker metric labels
    rep(spec, 'file_changes',
        "`mp_session_managed_parks_expired_total` gains a `reason` label.",
        "`mp_session_managed_parks_expired_total` gains a `reason` label. The label routing is two small pure functions with unit tests (park_reason to the expiry label `crash` or `typed`; a refusal reason to its metric family `skipped`, `refused` or `unreadable`), because no async-capable recorder exists in the test build (metrics::with_local_recorder takes a synchronous closure): the DB tests assert the returned reasons and the row facts, never a live counter.")
    rep(spec, 'tests',
        "terminalized once with parks_expired reason crash;",
        "terminalized once (the expiry label `crash` is asserted through the pure label function, not a live counter);")

    # ============================================================ C7: AAD assertion placement
    rep(spec, 'tests',
        "the ciphertext opens under AAD seq 4 and parses;",
        "the row's fields are asserted here (the open of the ciphertext under AAD seq 4 is asserted by loop_checkpoint::crash_park_db, a child of loop_checkpoint that can call the private scope_aad, and by the claim in crash_park_is_due_at_once_resumable_and_keeps_its_anchor, whose open uses it);")
    rep(spec, 'tests',
        "The metric labels are the returned reasons, asserted here, not through a recorder.",
        "The metric labels are the returned reasons, asserted here, not through a recorder. The kind case (an active row of another kind) cannot be produced through save, so that row is seeded directly.")

    # ============================================================ C8: harness recorded count
    rep(spec, 'file_changes',
        "and its recorded full-run count (hard-coded, 680 when this was written) is updated DELIBERATELY to the new total, never loosened.",
        "S8B-1 does NOT touch the recorded full-run count (hard-coded, 680 when this was written, in the harness comment and printf and grepped as a literal by the contract test): the new total is only known after a docker run, so the lead updates BOTH files together in a follow-up commit after the first full harness run, never loosening the check and never changing one without the other (the contract test would go red).")
    rep(spec, 'verification_commands',
        "and update the recorded full-run count in session-core-db-tests-test.sh deliberately;",
        "and the lead then updates the recorded full-run count in the harness and in session-core-db-tests-test.sh together, in a follow-up commit;")

    # ============================================================ C9: compose comments do not decide the order
    rep(spec, 'file_changes',
        "flip last and only with the owner's go-ahead; turn it off first;",
        "owner-gated, the proposed order is (5) then (6) (see the ledger entry); turn it off first;")
    rep(spec, 'file_changes',
        "flip before session-core's, turn it off after it).",
        "owner-gated, the proposed flip order is before session-core's (see the ledger entry); turn it off AFTER session-core's, see the kill order).")

    # ============================================================ C11: builder trap
    rep(spec, 'open_questions',
        "re-run park_writer_violations and its guard test after each.",
        "re-run park_writer_violations and its guard test after each. Comments: the committed park_spare_violations counts `spare_parked_run(` and `park_recovery_active()` as raw substrings of the whole worker body (comments included) and its reorder mutation anchors on the exact comment prefix `    // A run parked for a continuation`, and the new source test over crash_park_run's body rejects `messages`, `created_at` and `Utc::now`: no comment in the worker body or in crash_park_run may spell those needles, and that comment prefix must stay.")
    # ============================================================ X1b: the loop-top age half applies only while a marked fold may be stored
    rep(spec, 'approach',
        "or when its own last accepted heartbeat is at or past the window WHATEVER its writer's state:",
        "or when a marked fold may be stored and its own last accepted heartbeat is at or past the window, WHATEVER its writer's state:")
    rep(spec, 'approach',
        "A healthy armed loop with one round longer than 780 s therefore stops at its next loop top",
        "A healthy armed loop that has a marked fold stored and one round longer than 780 s therefore stops at its next loop top")
    rep(spec, 'approach',
        "(pure: armed && (lost_ownership || heartbeat_age >= fail_open_window()), whatever the writer's state)",
        "(pure: armed && (lost_ownership || (marked_fold_may_be_stored && heartbeat_age >= fail_open_window())), whatever the writer's state)")
    rep(spec, 'file_changes',
        "`armed && (folds.lost_ownership() || heartbeat_age >= mp_contracts::crash_park::fail_open_window())`",
        "`armed && (folds.lost_ownership() || (folds.marked_fold_may_be_stored() && heartbeat_age >= mp_contracts::crash_park::fail_open_window()))`")
    rep(spec, 'interfaces',
        "// armed && (lost_ownership || age >= fail_open_window): any writer state, loop top only",
        "// armed && (lost_ownership || (marked_fold_may_be_stored && age >= fail_open_window)): any writer state, loop top only")
    rep(spec, 'invariants',
        "or park::beat_age(&last_beat) is at or past the fail-open window, whatever the writer's state",
        "or a marked fold may be stored and park::beat_age(&last_beat) is at or past the fail-open window, whatever the writer's state")
    rep(spec, 'tests',
        "an ACTIVE writer whose last save returned Ok stops the loop at 800 s before any heartbeat or inference (the fake counts the heartbeats: none is sent) and keeps running at 10 s; the same for a Failed(Denied) writer)",
        "an ACTIVE writer holding a stored marked fold (an earlier tool round's marked save returned Ok) stops the loop at 800 s before any heartbeat or inference (the fake counts the heartbeats: none is sent) and keeps running at 10 s; the same for a Failed(Denied) writer; a loop with NO marked fold stored does not stop at 800 s)")
    rep(spec, 'summary',
        "and, whatever its writer's state, once its own last accepted heartbeat is at the window (it must not heartbeat then)",
        "and, whatever its writer's state, once a marked fold may be stored and its own last accepted heartbeat is at the window (it must not heartbeat then)")
    rep(spec, 'risks',
        "(g) a healthy armed loop with one round longer than the 780 s window stops",
        "(g) a healthy armed loop with a stored marked fold and one round longer than the 780 s window stops")
    return spec
