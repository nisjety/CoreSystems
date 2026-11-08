"""Revision 8 of the S8-B spec: mechanical fixes from the round-7 verification (no critical or high finding stood)."""
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
LOG = _r5.LOG


def apply(spec):
    rep(spec, 'slice', "DESIGN ONLY, revision 7,", "DESIGN ONLY, revision 8,")
    # 1 (medium): the retire timeout test belongs in the agent tests, with real time
    rep(spec, 'tests',
        "is sent with the gated timeout (a Recorder fake that answers after 5 s under paused tokio time: retire succeeds while an end-of-round save times out),",
        "is sent with the gated timeout (pinned by the source guard that retire passes GATED_SAVE_TIMEOUT; the behaviour test lives in the AGENT tests, because the Recorder is private to them: its save handler gets a test-only `delay` field and the test uses REAL time with a 3 s delay, between the 2 s round timeout and the 10 s gated one, so retire succeeds while an end-of-round save times out; no paused tokio time, nothing under runtime_loop/ uses it and its interplay with a real socket is unverified),")

    # 2: verifier leeway
    rep(spec, 'file_changes',
        "`run_budget::bearer_expiry(bearer).is_some_and(|exp| exp <= run_budget::now_epoch_secs())` (the two pure helpers RunDeadline already uses; an unreadable exp is NOT expired)",
        "`run_budget::bearer_expiry(bearer).is_some_and(|exp| exp + VERIFIER_LEEWAY_SECS <= run_budget::now_epoch_secs())` with `VERIFIER_LEEWAY_SECS: u64 = 30` mirroring session-core's default verifier leeway (auth.rs DEFAULT_LEEWAY_SECS; a true-expiry Denied is only answered after exp plus the leeway, so a Denied inside the band is not caused by expiry; a deployment with a larger AUTH_CORE_JWT_LEEWAY_SECS needs the constant raised) (the two pure helpers RunDeadline already uses; an unreadable exp is NOT expired)")
    rep(spec, 'interfaces',
        "// run_budget::bearer_expiry(bearer) <= now; unreadable exp = not expired; the Denied rule's second condition",
        "// run_budget::bearer_expiry(bearer) + 30 s verifier leeway <= now; unreadable exp = not expired; the Denied rule's second condition")
    rep(spec, 'approach',
        "run_budget::bearer_expiry of the session bearer at or before the current epoch second,",
        "run_budget::bearer_expiry of the session bearer plus the verifier's 30 s leeway at or before the current epoch second,")

    # 3: compile slip
    rep(spec, 'file_changes',
        "park::session_bearer_expired(&ctx.session_bearer))",
        "ctx.session_bearer.is_some_and(park::session_bearer_expired)) (ctx.session_bearer is an Option<&str>; None means the writer is Off(NoBearer) and the argument is unused)")

    # 4: CrashGate placement
    rep(spec, 'file_changes',
        "Built ONCE before the `while` loop from",
        "Built ONCE after park_gate and BEFORE the park-resume fence block (whose Snapshot passes crash_gate.mark_for; folds.arm_fence() runs before it), so before the `while` loop, from")
    rep(spec, 'approach',
        "is built ONCE before the loop from",
        "is built ONCE after park_gate and before the park-resume fence block (so before the loop) from")

    # 5: soften the cast rationale, layout and NULL notes
    rep_span(spec, 'file_changes',
        "The casts are REQUIRED:",
        "while park recovery is on.",
        "The casts keep the statement valid however it is prepared (sqlx-postgres 0.8 sends declared parameter types taken from the bound values, so $2 and $4 are float8 and $5 text even without them: belt and braces; the statement still runs for EVERY recovered run while park recovery is on, so the harness run is its gate).")
    rep(spec, 'file_changes',
        "and $5 as `ParkReason::CrashInterrupted.as_str()`.",
        "and $5 as `ParkReason::CrashInterrupted.as_str()`. Keep the line layout of the statement: the line break and 8-space indent before `AND updated_at` and the 6-space `FOR UPDATE` tail (the committed mutation row 'spares a paused fold too' survives only if they are kept). PARK_HELD_REASON_SQL's `park_reason` is NULL for a park stored while park recovery was off, so decode it as Option<Option<String>> (or COALESCE): never read 'held but no reason' as 'not held'.")

    # 6: marker_after / MarkerOutcome
    rep(spec, 'file_changes',
        "record_save is placed after the match on the save result and spells none of `save_tombstone` or `probe` (committed guards anchor on park_with's and save_bytes's text: the `let pause_ref = match` block and the `park_rpc(` count).",
        "record_save is a thin wrapper over the pure fn `marker_after(prev: bool, carried: bool, outcome: MarkerOutcome) -> bool` with `enum MarkerOutcome { Stored, Ambiguous, Rejected, NotAccepted }` (each of the three sites maps its own result shape to it: save_bytes and fence_with return Result<u64, SaveError>, park_with's closure returns Result<(u64, bool), SaveError>), so the table test needs no I/O; in park_with write `let outcome = match ..; self.record_save(..); outcome` (the match is currently its tail expression); record_save spells none of `save_tombstone`, `save_bytes`, `save_fold` or `probe` (the committed guard forbids all four inside save_park and park_with; it anchors on park_with's `let pause_ref = match` block and the `park_rpc(` count).")

    # 7a: Denied exception wording
    rep(spec, 'approach',
        "(iii) A Denied tombstone with a recent heartbeat proceeds:",
        "(iii) A Denied tombstone from a verifiably expired bearer with a recent heartbeat proceeds:")
    rep(spec, 'approach',
        "except for a Denied answer with a recent heartbeat, where the approval is attempted",
        "except for a Denied answer from a verifiably expired bearer with a recent heartbeat, where the approval is attempted")
    rep(spec, 'failure_matrix',
        "except for a Denied answer with a recent heartbeat, where crash_park_run's approvals fact still refuses the run.",
        "except for a Denied answer from a verifiably expired bearer with a recent heartbeat, where crash_park_run's approvals fact still refuses the run.")
    rep(spec, 'failure_matrix',
        "A Denied tombstone with a recent heartbeat attempts the approval",
        "A Denied tombstone from a verifiably expired bearer with a recent heartbeat attempts the approval")
    rep(spec, 'risks',
        "If that save is Denied with a recent heartbeat the approval is attempted anyway",
        "If that save is Denied from a verifiably expired bearer with a recent heartbeat the approval is attempted anyway")

    # 7b: spare_parked_run's crash TTL is the fourth parameter
    rep(spec, 'amendments_to_prior_specs',
        "PARK_SPARE_HORIZON_SQL gains a fourth bind and the CASE on park_reason for the wait in both places; spare_parked_run gains a third argument;",
        "PARK_SPARE_HORIZON_SQL gains two binds ($4 the crash TTL, $5 the reason token) and the CASE on park_reason for the wait in both places; spare_parked_run gains a fourth parameter (the crash TTL, after tx, run_id and the park TTL);")
    rep(spec, 'file_changes',
        "spare_parked_run takes the crash TTL as a third argument",
        "spare_parked_run takes the crash TTL as a fourth parameter (after tx, run_id and the park TTL)")
    rep(spec, 'steps',
        "spare_parked_run's third argument",
        "spare_parked_run's fourth parameter")

    # 7c: flip (5) effects, approach 6
    rep(spec, 'approach',
        "What (5) changes for a MARKED run:",
        "What (5) changes for a MARKED run that may have a marked fold stored (a direct first-round answer behaves as committed):")
    rep(spec, 'approach',
        "(up to the 10 s gated timeout plus one probe when session-core is degraded).",
        "(up to the 10 s gated timeout plus one probe when session-core is degraded). A healthy armed loop with a stored marked fold and a round longer than 780 s stops at its next loop top, and a user's Stop ends the stopped loop's stream in a retryable agent_dispatch_unavailable frame (R4(f), R4(g)).")

    # 7d: F36 order and R19g
    fm = spec['failure_matrix']
    i36 = [i for i, e in enumerate(fm) if e.startswith('F36 A new user turn is appended')]
    i43 = [i for i, e in enumerate(fm) if e.startswith('F43 ')]
    assert len(i36) == 1 and len(i43) == 1
    row = fm.pop(i36[0])
    i43 = [i for i, e in enumerate(fm) if e.startswith('F43 ')][0]
    fm.insert(i43, row)
    rep_elem(spec, 'risks', "R19g: a delegated child's own heartbeats",
        "R19g: a delegated child's own heartbeats extend the run's obligation but not the root's BeatMark, so a root loop with a stored marked fold stops at its next loop top once 780 s have passed since the root's own last accepted heartbeat (a long delegation round), although the run is healthy; and a depth-1 child's loop-top heartbeat renews the ROOT run's obligation and can abandon the worker's crash-park pass if it lands between the claim and the recovery transaction (bounded by the child's budget, harmless to safety). Conservative; a shared atomic mark that children publish to would remove the first effect.")

    # 7e, 7f, 7g: test wording
    rep(spec, 'tests',
        "each stop returns Aborted continue_in_progress with no AppendMessage, no RecordTerminalOutcome and no CreateApproval (the typed-failure exits included);",
        "each stop returns Aborted continue_in_progress with no AppendMessage, no RecordTerminalOutcome and no CreateApproval (the typed-failure exits included) and reports its closed site word (loop_top, exit or pause; `unproven` for a stop decided by a Denied answer or an unconfirmed tombstone);")
    rep(spec, 'tests',
        "and a content refusal cannot happen (a tombstone holds no content)",
        "and a LOCAL content refusal cannot happen (a tombstone holds no content; a server InvalidArgument or OutOfRange is the ContentRefused case that can)")
    rep(spec, 'tests',
        "the Denied injection must count the end-of-round save and the tombstone, both kind round, because LoopSaveFailure matches by kind and counts down);",
        "the failure injection is a LIST of LoopSaveFailure entries consumed in order (the handler takes the first entry with times > 0 that matches the kind, and both saves are kind round): an entry 'ambiguous, commits' for the end-of-round save, then an entry 'Denied' for the tombstone; a single entry with times: 2 would fail the end-of-round save, clear the flag and bypass the rule);")
    rep(spec, 'tests',
        "(the tests mint the bearer with a past or a future exp, as the run_budget tests do);",
        "(the tests mint the bearer with a past or a future exp, as the run_budget tests do); the PROCEED cases inject an append failure (the Recorder's append always succeeds), so that the receipt is the committed failed one;")

    # 8: step lists, ineligible reason, module placement, zdr/waived fns
    rep(spec, 'steps',
        "the new crash_park.rs with CrashMark, CrashRefusal and crash_reseal;",
        "the new crash_park.rs with CrashMark, crash_mark_in, CrashRefusal, crash_reseal and the window constants (MANAGED_DEADLINE_SECS, AGE_MARGIN_SECS, age_window());")
    rep(spec, 'steps',
        "fold.rs (OffReason::SelfClosed, note_self_closed, retire);",
        "fold.rs (OffReason::SelfClosed, note_self_closed, retire, and the `timeout` parameter of save_bytes and save_tombstone_as, whose other callers save_tombstone and PauseFold::save pass their kind's timeout);")
    rep(spec, 'steps',
        "park.rs (must_stop, BeatMark, beat_age and the beat_test_clock seam;",
        "park.rs (must_stop, BeatMark, beat_age, session_bearer_expired and the beat_test_clock seam;")
    rep(spec, 'file_changes',
        "the first failing fact as a closed word, used ONLY for the one per-run info! line",
        "the first failing fact as a closed word (its first check is crash_marking_active(), so a switch that is on with the resume half off reads that word and never 'armed'), used ONLY for the one per-run info! line")
    rep(spec, 'tests',
        "harness filters `terminalization::park_recovery_db_tests::crash_park_` and `loop_checkpoint::crash_park_db::`)",
        "harness filters `terminalization::park_recovery_db_tests::crash_park_` and `loop_checkpoint::crash_park_db::`; module placement: every test that drives a worker pass (worker_pass_on, isolate, make_due, started_run: the eligible park, every_refusal_ends_the_run_as_committed, crash_park_off_is_the_committed_worker, the ordering variants, the claim, TTL and cap tests, the live-loop heartbeat test) lives INSIDE park_recovery_db_tests, and the direct crash_park_run tests (crash_park_run_refuses_every_ineligible_fold_with_its_own_reason, crash_park_never_runs_for_a_zero_retention_or_waived_run, the AAD open) live in loop_checkpoint::crash_park_db)")
    rep(spec, 'tests',
        "a_fresh_read_only_pinned_built_in_run_marks_its_boundary_folds; each exclusion writes folds with no marker through run_agent:",
        "a_fresh_read_only_pinned_built_in_run_marks_its_boundary_folds; the ZDR and waived cases are two separate fns (a_zdr_run_never_marks and a_waived_route_never_marks, each with #[tokio::test] directly before the fn, because the store_proofs Park entry names exact (file, fn) pairs); each exclusion writes folds with no marker through run_agent:")

    # 9: one early Denied blip costs the answer
    insert_before(spec, 'risks', "R20: a new user turn on the same thread while a crash park waits:",
        "R19k: one early Denied blip (a verifier or authorization hiccup with a valid bearer) in a tool-using run turns the writer Failed(Denied) for the rest of the run (classify_status maps every Unauthenticated or PermissionDenied to Refused(Denied) and apply makes it terminal); lost_ownership() is false, so the loop does its remaining rounds with no further saves, and at its end retire_fence takes the Denied rule with a valid bearer and STOPS: the work is done and the answer is discarded. A permanent denial from the first save is harmless (the flag is never set). Optional mitigation, not built: allow ONE plain tombstone attempt for a Failed(Denied) writer with a valid bearer (an Ok proves safety); otherwise the cost belongs in the owner's list of flip (5)'s effects.")
    return spec
