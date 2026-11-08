"""Revision 5 of the S8-B spec: applies the round-4 review findings to the built spec dict.

Every replacement is asserted unique (or counted), so a drifted anchor fails loudly.
"""

LOG = []


def _strings(spec, section):
    """Yield (setter, getter) pairs for every string in a section."""
    v = spec[section]
    if isinstance(v, str):
        yield (lambda s, sec=section: spec.__setitem__(sec, s), lambda: spec[section])
    elif isinstance(v, list):
        for i, e in enumerate(v):
            if isinstance(e, str):
                yield (lambda s, i=i, v=v: v.__setitem__(i, s), lambda i=i, v=v: v[i])
            elif isinstance(e, dict):
                for k in ('path', 'change'):
                    if k in e:
                        yield (lambda s, e=e, k=k: e.__setitem__(k, s), lambda e=e, k=k: e[k])
    else:
        raise TypeError(section)


def rep(spec, section, old, new, count=1):
    hits = [(set_, get_) for set_, get_ in _strings(spec, section) if old in get_()]
    total = sum(get_().count(old) for _, get_ in hits)
    assert total == count, (section, 'expected', count, 'found', total, old[:100])
    for set_, get_ in hits:
        set_(get_().replace(old, new))
    LOG.append((section, old[:50]))


def rep_span(spec, section, start, end, new):
    hits = [(set_, get_) for set_, get_ in _strings(spec, section) if start in get_()]
    assert len(hits) == 1, (section, 'span start hits', len(hits), start[:100])
    set_, get_ = hits[0]
    text = get_()
    assert text.count(start) == 1, (section, 'span start not unique', start[:100])
    i = text.index(start)
    j = text.index(end, i)
    set_(text[:i] + new + text[j + len(end):])
    LOG.append((section, 'span ' + start[:40]))


def rep_elem(spec, section, startswith, new):
    v = spec[section]
    idx = [i for i, e in enumerate(v) if isinstance(e, str) and e.startswith(startswith)]
    assert len(idx) == 1, (section, 'elem hits', len(idx), startswith[:100])
    v[idx[0]] = new
    LOG.append((section, 'elem ' + startswith[:40]))


def insert_before(spec, section, startswith, new):
    v = spec[section]
    idx = [i for i, e in enumerate(v) if isinstance(e, str) and e.startswith(startswith)]
    assert len(idx) == 1, (section, 'insert hits', len(idx), startswith[:100])
    v.insert(idx[0], new)
    LOG.append((section, 'insert_before ' + startswith[:40]))


def insert_after(spec, section, startswith, new):
    v = spec[section]
    idx = [i for i, e in enumerate(v) if isinstance(e, str) and e.startswith(startswith)]
    assert len(idx) == 1, (section, 'insert hits', len(idx), startswith[:100])
    v.insert(idx[0] + 1, new)
    LOG.append((section, 'insert_after ' + startswith[:40]))


def rep_all(spec, old, new):
    total = 0
    for section in spec:
        for set_, get_ in list(_strings(spec, section)):
            c = get_().count(old)
            if c:
                set_(get_().replace(old, new))
                total += c
    assert total >= 1, ('rep_all found nothing', old[:100])
    LOG.append(('ALL', old[:50] + ' x%d' % total))


def apply(spec):
    # ======================================================================== SLICE
    rep(spec, 'slice',
        "DESIGN ONLY, revision 4, made against branch model-plane at HEAD dfd1a8d5 (S8-A is built: commits c6a95ae0 .. 6ddfae42, ledger entry dfd1a8d5).",
        "DESIGN ONLY, revision 5, made against branch model-plane (S8-A is built: commits c6a95ae0 .. 6ddfae42, ledger entry dfd1a8d5; HEAD was dfd1a8d5 when the design started and is 72c51bc3 now, the two later commits being documentation and evidence only, so the build base is whatever HEAD is when S8B-0 starts, recorded by the lead).")

    # ======================================================================== SUMMARY
    rep(spec, 'summary',
        "runs one new branch in recover_claimed_terminalization, after the S8-A park spare and before the terminal write:",
        "runs one new branch in recover_claimed_terminalization, after the approval check and before the S8-A park spare (which stays the single spare call and hands the obligation back):")
    rep(spec, 'summary',
        "it makes a Denied save mean 'cannot prove ownership: stop'.",
        "it treats a Denied save as 'cannot prove ownership' and stops once the loop's own heartbeat age says the worker may have acted.")
    rep(spec, 'summary',
        "or the store refuses this loop (Denied, which is what a loop silent past its 300 s session bearer gets), the loop stops and does neither.",
        "or the save cannot be confirmed (a transient failure: a late append cannot be recalled), the loop stops and does neither; so does a Denied answer (what a loop silent past its 300 s session bearer gets) once the loop's own heartbeat age is at the window.")
    rep(spec, 'summary',
        "What the loop cannot prove (a Denied or transient answer) it resolves with its own monotonic clock: it proceeds only while the last heartbeat the store accepted was sent less than 780 s ago, because the worker cannot act before 900 s have passed.",
        "The one thing it resolves with its own clock is a Denied answer (the expired-bearer zombie, whose append, and receipt with the same bearer, cannot land either): the loop proceeds only while the last heartbeat the store accepted was sent less than 780 s ago (the larger of its monotonic and wall-clock age), because the worker cannot act before 900 s have passed. A transient failure of the tombstone is NOT resolved by the window: the window bounds when finalize starts, not when its append lands.")
    rep(spec, 'summary',
        "A marked loop also stops at its next loop top after any definitive ownership failure (Stale, Closed, Denied, NotFound), so a superseded loop does not burn the rest of its budget.",
        "A marked loop also stops at its next loop top after a definitive ownership failure (Stale, Closed, NotFound), and after a Denied one once its heartbeat age is at the window, so a superseded loop does not burn the rest of its budget.")
    rep(spec, 'summary',
        "keeps the committed immediate failed receipt. Owner-gated like the session-core switch.",
        "keeps the committed immediate failed receipt; a marked loop that cannot confirm its tombstone at run end (a degraded session-core) stops instead of answering and leaves the run to the worker. Owner-gated like the session-core switch.")
    rep(spec, 'summary',
        "Flip order: the S8-A flips (1) to (3) with the S8-A park resume live-proven, the executor switch (owner-gated, its visible effects listed), then session-core's, last and owner-gated. Kill order is the reverse.",
        "Proposed flip order (the owner's call, open_questions): the S8-A flips (1) to (3) with the S8-A park resume live-proven, the executor switch (owner-gated, its visible effects listed), then session-core's, last and owner-gated. Kill order is the reverse.")

    # ======================================================================== APPROACH
    rep(spec, 'approach',
        "after the S8-A spare and before PARK_HELD_SQL, when crash_park_active():",
        "after the approval check and BEFORE the S8-A spare (which stays the one and only spare call: for a fold that is already a park the crash branch refuses at its state check before it opens any ciphertext, and for a re-sealed fold the same spare statement then hands the obligation back), when crash_park_active():")

    rep_span(spec, 'approach',
        "The decision. A writer that has failed with Stale or Closed",
        "so a loop whose end-of-round save was refused does not run the rest of its budget.",
        """The decision. The loop proceeds past retire_fence only when it holds a committed tombstone or a reason why none can matter; everything else stops. (1) Ok (the tombstone committed): proceed. From that commit on no crash park can ever be created for the run, whatever the transport does later. (2) A writer that has failed with Stale, Closed or NotFound (FoldWriter::lost_ownership), or a tombstone save answered Stale, Closed, run_ended or NotFound (the seq moved, the run ended, the run is gone), is POSITIVE evidence that another writer or the run's end got there first: STOP at any age. The stop is Err(Refusal::ContinueInProgress.status()) (Aborted "continue_in_progress", an existing closed token, never a confirmed rejection), returned before finalize and before the approval exists: no append, no receipt, no approval. (3) Anything the save could not CONFIRM stops too, at ANY age: Unavailable or a timeout (after the one retry of (5)), a Stale that follows an ambiguous attempt of the same retire (the loop cannot tell its own earlier commit from a foreign writer's), and a ContentRefused (nothing was sent). The reason is that a delayed request cannot be recalled: the window of (4) bounds when finalize STARTS, not when its append LANDS, so one partition that fails the tombstone can also hang the append until after a crash park was resumed, and a late answer would then be a second one. The price is stated, not hidden: a session-core outage at the exact moment a marked loop ends costs that run its answer (committed code would also have lost it unless the store came back inside the append's own failure), and the run is left to the worker: crash-parked and resumable if the tombstone did not land, outcome_unknown at the deadline if it did. A stop is never a duplicate. The exception is a store that answers disabled or Unimplemented, or a missing descriptor key: the writer goes Off or Failed for a non-ownership reason, nothing was written, and this replica cannot write; the loop proceeds as committed (R19b). (4) Denied (Unauthenticated or PermissionDenied; for a loop silent past its 300 s session bearer that is the ordinary answer, because the bearer is checked before the seq), or a writer already Failed with it (FoldWriter::ownership_unproven), is the one UNPROVEN case, resolved with the executor's OWN CLOCK: the loop remembers when it last SENT a heartbeat that the store accepted (the run-level heartbeat before run_rounds, then each loop-top heartbeat; a heartbeat error already ends the loop) as a BeatMark that holds BOTH an Instant and a SystemTime, and its age is the LARGER of the two elapsed times (so a host or VM suspend, which stops the monotonic clock but not the wall clock, can only lengthen it). The worker cannot crash-park a run before its obligation's deadline_at, which is the heartbeat's receipt time plus 900 s, and the receipt is never earlier than the send, so while that age is under the fail-open window (900 s minus a 120 s skew margin, constants in mp-contracts pinned to session-core's MANAGED_DEADLINE_SECS) no crash park can exist and no resume can be running, and the append that follows is refused by the same expired bearer, so nothing can land late: the loop proceeds and writes the committed immediate failed receipt. At or past the window it stops. (5) retire's transient retry is ONE retry (two attempts, 100 ms apart, only for Unavailable and only while is_active() still holds) with the gated 10 s timeout, not the 2 s round timeout (a must-confirm save is not a best-effort one), so a degraded store costs a run end up to about 30 s before the stop. So a superseded loop silent past the deadline always stops, whatever the transport does, and a healthy loop whose bearers expired a moment ago still writes its immediate failed receipt as committed code does. At every loop top, before anything else, an armed loop stops when its writer has lost ownership, or when it is unproven (Denied) and the heartbeat age is at or past the window, so a loop whose end-of-round save was refused does not run the rest of its budget.""")

    rep(spec, 'approach',
        "A writer that is not Active and is not unproven (off,",
        "A writer that is not Active, has not lost ownership and is not unproven (off,")
    rep(spec, 'approach',
        "if that save is not confirmed, crash_park_run's approvals fact still refuses the run.",
        "if that save is not confirmed the loop stops before it raises the approval (no approval exists, the marked fold is parked at the deadline and a resume asks again), except for a Denied answer with a recent heartbeat, where the approval is attempted with the same expired bearer and crash_park_run's approvals fact still refuses the run.")
    rep(spec, 'approach',
        "so it holds even when every RPC of a woken zombie fails at the transport. The design needs all three and relies on none alone.",
        "and it covers the Denied answer only: a transport failure of the tombstone stops the loop by itself (an unconfirmed tombstone never proceeds, at any age). The design needs all of them and relies on none alone.")
    rep_span(spec, 'approach',
        "The residual is stated, not hidden, and it is the HEALTHY loop's",
        "rejected for v1 with that cost, listed for the owner.",
        """The residual is stated, not hidden, and it is narrow. (i) An unconfirmed tombstone never proceeds: a healthy loop that cannot reach session-core at its end STOPS instead of risking a late append (round-4 high finding: the earlier fail-open design let one partition fail the tombstone and hang the append past a resume). That is a cost, not a duplicate: the answer in the loop's memory is lost and the run is left to the worker. (ii) A writer that cannot write at all (Off, or Failed for a non-ownership reason such as KeyUnavailable) cannot tombstone: the loop finalizes as committed, so on a replica set whose configuration disagrees a marked fold stored earlier can be parked by another replica (R19b); with a delayed append that is a duplicate path, closed only by the drain-before-rolling rule. (iii) A Denied tombstone with a recent heartbeat proceeds: no crash park can exist yet, and the append with the same expired bearer cannot land. The exact belt for (ii) and for any unforeseen late append is a durable fact ordered with the append: an idempotent final answer per run (a unique partial index on the final assistant message per thread and run, with ON CONFLICT DO NOTHING in append_message_in_tx) or the v4 thread watermark; both need a migration, neither is built, both are listed for the owner.""")

    # ---- section 4
    rep(spec, 'approach',
        "used by the loop top and by retire_fence;",
        "used by the loop top and by retire_fence; `BeatMark { at: Instant, wall: SystemTime }` with BeatMark::now() (the instant a heartbeat is SENT) and `beat_age(&BeatMark) -> Duration` = the larger of at.elapsed() and the wall-clock time since `wall` (a wall clock that went backwards counts as zero), its pure core beat_age_of(mark, now_instant, now_wall) for a unit test with the wall ahead of the monotonic clock, and a #[cfg(test)] thread-local override beat_test_clock::Forced (the TEST_PARKING pattern: the loop tests run on the test's thread) through which the loop tests force an age, because std Instant is not moved by tokio's paused time and the loop-top heartbeat refreshes the mark every round;")
    rep(spec, 'approach',
        "lost_ownership() (State::Failed(Stale | Closed)) and ownership_unproven() (State::Failed(Refused(Denied | NotFound)))",
        "lost_ownership() (State::Failed(Stale | Closed | Refused(NotFound)): the run row is gone) and ownership_unproven() (State::Failed(Refused(Denied)))")
    rep(spec, 'approach',
        "so it shares the save_bytes path and its stale probe;",
        "so it shares the save_bytes path and its stale probe; it is sent with the gated timeout (GATED_SAVE_TIMEOUT, the one pre_effect and fence saves use) instead of the 2 s round timeout, the stored kind staying round (the builder adds the timeout choice to save_tombstone_as);")
    rep(spec, 'approach',
        "agent.rs: LoopContext gains `heartbeat_floor: Instant`, taken immediately BEFORE the run-level heartbeat in run_agent_with_route_parkable; run_rounds keeps `last_beat`, initialised from it and set to Instant::now() taken immediately BEFORE each loop-top heartbeat that returns Ok.",
        "agent.rs: LoopContext gains `heartbeat_floor: park::BeatMark`, taken (BeatMark::now()) immediately BEFORE the run-level heartbeat in run_agent_with_route_parkable; run_rounds keeps `last_beat: park::BeatMark`, initialised from it; at each loop top `let sent = park::BeatMark::now();` is taken immediately BEFORE the heartbeat call and `last_beat = sent;` is assigned only when the call returns Ok. Every age passed to must_stop and retire_fence is `park::beat_age(&last_beat)`, never a literal.")
    rep_all(spec, "last_beat.elapsed()", "park::beat_age(&last_beat)")
    rep(spec, 'approach',
        "the count of plan.* and subagent.* calls of a round is added to effectful_calls for an armed loop whose writer is Active, so the existing save_pre_effect decision runs for them;",
        "the plan.* and subagent.* calls of a round are counted in their OWN local, visible_calls, which is added only to the fence decision (the save_pre_effect condition becomes `effectful_calls + visible_calls > 0` for an armed loop whose writer is Active), so the existing save_pre_effect decision runs for them; note_effects_held, the 'held back' warn and held_for_good keep reading effectful_calls alone;")
    rep(spec, 'approach',
        "The funnel recognises Paused and Parked with `matches!(result.outcome, RoundsOutcome::Paused(_) | RoundsOutcome::Parked(_))` and never with an `outcome:` field pattern",
        "The funnel recognises Paused and Parked with `matches!(&outcome, Ok(r) if matches!(r.outcome, RoundsOutcome::Paused(_) | RoundsOutcome::Parked(_)))` (by reference, so the funnel can still return `outcome`; the async block's result is annotated Result<RoundsResult, tonic::Status>) and never with an `outcome:` field pattern")
    rep(spec, 'approach',
        "S8B-3 re-anchors them whitespace-insensitively in the same commit (see file_changes).",
        "S8B-3 re-anchors them whitespace-insensitively in the same commit (see file_changes). NOTE (round 4): `cargo fmt --all -- --check` is already RED on this branch (about 2850 hunks in 112 files at 72c51bc3, the files S8-B touches included), so the CI formatting step cannot be this change's gate, and `cargo fmt --all` must NEVER be run to make it green: it would rewrite the text every guard anchors on. S8B-3's format check is a per-file baseline: the number of `Diff in` hunks of `cargo fmt -p execution-core -- --check` for each touched file must not grow beyond the count recorded when S8B-3 starts, and the builder indents the async block by hand as rustfmt would.")

    # ---- section 5
    rep(spec, 'approach',
        "kind and a tombstone or unmarked fold: NotResumable, NotMarked)",
        "kind and a tombstone or unmarked fold: NotResumable, NotMarked; not_active also counts every S8-A park pass, because the crash branch now runs before the spare for every recovered run, so an alert belongs on the refused and unreadable families, never on skipped)")

    # ---- section 6
    rep(spec, 'approach',
        "Prerequisites for flipping anything of S8-B:",
        "Proposed prerequisites (the owner's call) for flipping anything of S8-B:")
    rep(spec, 'approach',
        "(up to about 6 s of retries when session-core is degraded)",
        "(a degraded session-core makes the run wait up to about 30 s and then STOP without its answer)")
    rep(spec, 'approach',
        "Deploy order: session-core first (dormant), then execution-core (one image holding both executor steps).",
        "Deploy order: session-core first (dormant), then execution-core (one image holding both executor steps). HOW EACH ENVIRONMENT FLIPS AND KILLS (the dev carry-over script, production signed artifacts, the drain query, the pre-flip checklist) is the last step of the steps list.")

    # ======================================================================== FILE CHANGES
    rep(spec, 'file_changes',
        "The lead records dfd1a8d5 as the S8-B base in the build ledger.",
        "The lead records the actual HEAD at the start of S8B-0 as the S8-B base in the build ledger (dfd1a8d5 when this was written; 72c51bc3 after two documentation commits).")
    rep(spec, 'file_changes',
        "and the module must not spell the string park:).",
        "and the module must not spell the string park:). CI's session-core-receipts job filters `loop_checkpoint`, which matches `loop_checkpoint::crash_park_db::`, so those tests run in CI against a migrated Postgres and must pass unattended: the seed helpers fill every NOT NULL column of the approvals and ledger rows.")
    rep_span(spec, 'file_changes',
        "- recover_claimed_terminalization: inside `if park_recovery_active() {`, after the S8-A",
        "A crash park writes no terminal state and no receipt.",
        "- recover_claimed_terminalization: inside `if park_recovery_active() {`, BEFORE the S8-A `if spare_parked_run(..)` (which stays the one and only call: the committed rule 'the spare is called once' holds), a block `let crash_parked = if crash_park_active() { match crash_park_run(&mut transaction, worker_cipher().as_ref(), &claim.row.run_id).await? { Parked { .. } => { crash_parks_total metric; true } Refused(reason) => { metric with the reason label; false } } } else { false };`. For a fold that is already a park (an executor's typed park or an earlier crash park) crash_park_run refuses at its state check before it opens any ciphertext. For a re-sealed fold the unchanged spare then hands the obligation back at the crash horizon (the fold is a park with updated_at = now(), so it must spare; if it returns false after crash_parked, the pass returns an internal error and rolls back: a crash park never falls through to the terminal write). Refused falls through to the committed spare and terminal write unchanged. A crash park writes no terminal state and no receipt.")
    rep(spec, 'file_changes',
        "and the rows on `spare_parked_run(` and `if park_recovery_active() {`; each is re-pinned on",
        "and the row on `if park_recovery_active() {` (the `spare_parked_run(` count rule stays 'exactly one' because the crash branch runs before the single spare call); each is re-pinned on")
    rep(spec, 'file_changes',
        "a `crash_park_db_tests` module of ignored DB tests (see tests).",
        "new ignored DB tests appended INSIDE the committed `park_recovery_db_tests` module (its helpers scratch_pool, isolate, make_due, started_run, parked, worker_pass_on, obligation_timing, fold_state_of and Seeded are private to it, so a sibling module could not call them; nothing committed is edited), reached as `terminalization::park_recovery_db_tests::crash_park_...` (see tests).")
    rep(spec, 'file_changes',
        "(not_persistent, no_fold, not_active, kind, and the CrashRefusal labels not_resumable and not_marked)",
        "(not_persistent, no_fold, not_active (which also counts every S8-A park pass), kind, and the CrashRefusal labels not_resumable and not_marked)")
    rep(spec, 'file_changes',
        "(S8B-3; used by the loop top and by retire_fence).",
        "(S8B-3; used by the loop top and by retire_fence).\n- `pub(crate) struct BeatMark { at: std::time::Instant, wall: std::time::SystemTime }` with `BeatMark::now()` and `pub(crate) fn beat_age(mark: &BeatMark) -> Duration` = the larger of `mark.at.elapsed()` and the wall-clock time since `mark.wall` (a wall clock that went backwards counts as zero: a VM or host suspend stops the monotonic clock but not the wall clock, so the age can only be lengthened), with its pure core `beat_age_of(mark, now_instant, now_wall)` for a unit test, and a `#[cfg(test)]` thread-local override module `beat_test_clock` with a Forced guard (the TEST_PARKING pattern) through which loop tests force an age (S8B-3).")
    rep(spec, 'file_changes',
        "`lost_ownership(&self) -> bool` = matches!(self.state, State::Failed(SaveError::Stale | SaveError::Closed)) and `ownership_unproven(&self) -> bool` = matches!(self.state, State::Failed(SaveError::Refused(RefusedBy::Denied | RefusedBy::NotFound)))",
        "`lost_ownership(&self) -> bool` = matches!(self.state, State::Failed(SaveError::Stale | SaveError::Closed | SaveError::Refused(RefusedBy::NotFound))) (NotFound: the run row is gone, which is definitive) and `ownership_unproven(&self) -> bool` = matches!(self.state, State::Failed(SaveError::Refused(RefusedBy::Denied)))")
    rep(spec, 'file_changes',
        "(one RPC with the round-save timeout, the one stale-after-ambiguous probe, apply() on failure)",
        "(one RPC with the GATED timeout, GATED_SAVE_TIMEOUT, instead of the 2 s round-save timeout because it is a must-confirm save, the stored kind staying round; the one stale-after-ambiguous probe; apply() on failure)")
    rep(spec, 'file_changes',
        "`effectful_calls` also counts the ToDispatch calls for which park::is_visible_call holds, so the existing `save_pre_effect(open_round_fold)` decision runs before such a round is dispatched.",
        "a SEPARATE local `visible_calls` counts the ToDispatch calls for which park::is_visible_call holds, and the save_pre_effect condition becomes `effectful_calls + visible_calls > 0` (for an armed loop whose writer is Active), so the existing `save_pre_effect(open_round_fold)` decision runs before such a round is dispatched; note_effects_held, the 'held back' warn and held_for_good keep reading effectful_calls alone.")
    rep(spec, 'file_changes',
        "`LoopContext` gains `heartbeat_floor: std::time::Instant`, taken immediately BEFORE the run-level heartbeat in run_agent_with_route_parkable; run_rounds keeps `last_beat`, initialised from it and set to Instant::now() taken immediately BEFORE each loop-top heartbeat that returns Ok.",
        "`LoopContext` gains `heartbeat_floor: park::BeatMark`, taken (BeatMark::now()) immediately BEFORE the run-level heartbeat in run_agent_with_route_parkable; run_rounds keeps `last_beat: park::BeatMark`, initialised from it; at each loop top `let sent = park::BeatMark::now();` is taken immediately BEFORE the heartbeat call and `last_beat = sent;` is assigned only after the call returned Ok. Every age passed to must_stop and retire_fence is `park::beat_age(&last_beat)`, never a literal.")
    rep(spec, 'file_changes',
        "is wrapped in `let outcome = async { .. }.await;`",
        "is wrapped in `let outcome: Result<RoundsResult, tonic::Status> = async { .. }.await;`")
    rep(spec, 'file_changes',
        "recognised with `matches!(outcome, Ok(r) if matches!(r.outcome,",
        "recognised, by reference so the funnel can still return `outcome`, with `matches!(&outcome, Ok(r) if matches!(r.outcome,")
    rep_span(spec, 'file_changes',
        "then the decision of the approach: a writer that lost ownership stops;",
        "Stale and Closed stop always; Ok proceeds.",
        "then the decision of the approach (1(d)): a writer that lost ownership stops; a writer that is not Active, has not lost ownership and is not unproven proceeds at once (no RPC, no sleep); a writer that is unproven (Failed(Refused(Denied))) makes no RPC and takes the age rule; otherwise it sends the tombstone (two attempts at most, 100 ms apart, the retry only for Unavailable and only while is_active() still holds): Ok proceeds; Stale, Closed, run_ended and NotFound stop at any age; Denied (Unauthenticated, PermissionDenied) stops when `heartbeat_age >= fail_open_window()` and proceeds otherwise; EVERYTHING ELSE (Unavailable or a timeout after the retry, a Stale after an ambiguous attempt, a ContentRefused) STOPS at any age: an unconfirmed tombstone never proceeds. A store that answers disabled or Unimplemented, or a missing key, turns the writer Off or Failed for a non-ownership reason and the loop proceeds as committed (R19b).")
    rep(spec, 'file_changes',
        "lost_ownership names exactly Stale and Closed; ownership_unproven names exactly Denied and NotFound; must_stop names fail_open_window.",
        "lost_ownership names exactly Stale, Closed and NotFound; ownership_unproven names exactly Denied; must_stop names fail_open_window; the loop-top heartbeat comes AFTER the deadline check and after the must_stop check (an order guard: a woken dead-bearer zombie leaves through OutOfTime before it can reset its heartbeat age); `last_beat` is initialised from ctx.heartbeat_floor, `let sent = park::BeatMark::now();` is taken immediately before the heartbeat call and `last_beat = sent;` follows only the Ok path; every age passed to must_stop and retire_fence is `park::beat_age(&last_beat)` (never a literal, never Duration::ZERO); retire_fence's decision is an exhaustive match over the typed outcomes with no wildcard arm that proceeds; mutation rows: replace `park::beat_age(&last_beat)` by `Duration::ZERO` at the exit funnel, drop the `last_beat = sent;` assignment, move the heartbeat above the deadline check, and make the unconfirmed arm proceed.")
    rep(spec, 'file_changes',
        "because the async block re-indents the loop (rustfmt is a CI check):",
        "because the async block re-indents the loop (the builder indents by hand as rustfmt would; CI's format step is already red at the base and is not the gate, see the fmt note in the approach and the steps):")

    # new file_changes entries (before the docs entry)
    fc = spec['file_changes']
    docs_idx = [i for i, e in enumerate(fc) if e['path'].startswith('apps/Model Plane/docs/decisions/ledger.md')]
    assert len(docs_idx) == 1
    fc.insert(docs_idx[0], {
        "path": "apps/Model Plane/scripts/tests/session-core-db-tests.sh and session-core-db-tests-test.sh",
        "change": "S8B-1. The project's only sanctioned route for DB-backed session-core tests (it adds --include-ignored, a minimum pass count per filter, and fails on 'skipping: DATABASE_URL unset' or any panic, so a renamed module cannot pass vacuously). Its environment scrub (the regex of variables unset before the run) gains SESSION_CORE_CRASH_PARK_TTL_SECONDS (SESSION_CORE_CRASH_PARK_ENABLED is already covered by `SESSION_CORE_.*_ENABLED`), because the new DB tests assume the default crash TTL of 3600 s and an operator's shell value would change them silently; the harness contract test's list of leak names gains the variable, and its recorded full-run count (hard-coded, 680 when this was written) is updated DELIBERATELY to the new total, never loosened.",
    })
    fc.insert(docs_idx[0] + 1, {
        "path": ".github/workflows/model-plane-rust-ci.yml",
        "change": "S8B-1 (OWNER-VISIBLE: a CI change; the owner may decline it and rely on the lead's harness run, which then stays the only gate for the worker). The session-core-receipts job (it already has a migrated Postgres) runs only the filters zdr_receipt and loop_checkpoint with a floor, so the worker-level crash-park tests would never run in CI: add the filters `crash_park` and `park_recovery` with count floors (the existing `-lt 18` pattern), and add `apps/Model Plane/deploy/**` to the workflow's path filter (durable_loop_contract reads deploy/docker-compose.yml, so a compose-only edit that drops a default-off row is otherwise not gated). The new loop_checkpoint::crash_park_db tests already run under the existing loop_checkpoint filter and must pass unattended.",
    })

    # ======================================================================== INTERFACES
    rep(spec, 'interfaces',
        "(ownership_unproven && age >= fail_open_window))",
        "(ownership_unproven && age >= fail_open_window))\npub(crate) struct BeatMark { at: Instant, wall: SystemTime }     // BeatMark::now(): the instant a heartbeat is SENT\npub(crate) fn beat_age(mark: &BeatMark) -> Duration;              // the LARGER of monotonic and wall elapsed; #[cfg(test)] beat_test_clock::Forced; pure core beat_age_of(mark, now_instant, now_wall)")
    rep(spec, 'interfaces',
        "// S8B-2: Failed(Stale | Closed)",
        "// S8B-2: Failed(Stale | Closed | Refused(NotFound))")
    rep(spec, 'interfaces',
        "// S8B-2: Failed(Refused(Denied | NotFound))",
        "// S8B-2: Failed(Refused(Denied))")
    rep(spec, 'interfaces',
        "// LoopContext gains heartbeat_floor: Instant (taken before the run-level heartbeat); run_rounds keeps last_beat",
        "// LoopContext gains heartbeat_floor: BeatMark (taken before the run-level heartbeat); run_rounds keeps last_beat: BeatMark")
    rep(spec, 'interfaces',
        "// exactly 2 call sites; Ok at once when !gate.armed",
        "// exactly 2 call sites; Ok at once when !gate.armed; an unconfirmed tombstone STOPS, Denied takes the age rule")
    rep(spec, 'interfaces',
        "// \"answering\" | \"paused\"; only retire_fence calls it",
        "// \"answering\" | \"paused\"; gated timeout; only retire_fence calls it")

    # ======================================================================== TESTS
    rep(spec, 'tests',
        "harness filters `terminalization::crash_park_db_tests::` and `loop_checkpoint::crash_park_db::`",
        "harness filters `terminalization::park_recovery_db_tests::crash_park_` and `loop_checkpoint::crash_park_db::`")
    rep(spec, 'tests',
        "the branch sits inside `if park_recovery_active() {`, after the spare, before the held-reason read and before apply_managed_terminal_outcome, behind `crash_park_active()`, calls crash_park_run exactly once, and spares through spare_parked_run (a mutation row for each: gate dropped, order swapped, a second call site);",
        "the branch sits inside `if park_recovery_active() {`, after has_pending_approval and BEFORE the single `spare_parked_run(` call (which stays the one and only call, and which a Parked outcome must reach: a crash park never falls through to the terminal write), behind `crash_park_active()`, calls crash_park_run exactly once (a mutation row for each: gate dropped, order swapped so the crash block follows the spare, a second call site of either function);")
    rep_elem(spec, 'tests', "S8B-1 DB terminalization::crash_park_db_tests::every_refusal_ends_the_run_as_committed",
        "S8B-1 DB terminalization::park_recovery_db_tests::crash_park_every_refusal_ends_the_run_as_committed (inside the committed module, using its isolate, started_run, make_due and worker_pass_on helpers): through one worker pass each, every refusal the worker can REACH ends the run outcome_unknown through the unchanged receipt path and the fold is purged in the same transaction: no fold, a closed fold, a paused fold, an active tombstone, a pre_effect fold with an open round, no marker, a malformed marker, a Space fold, an unpinned fold, plan_mode false, a wider rung, payload parks at the cap, a ledger row of any status, and an approvals row that no longer holds the run (status denied, status consumed, and, with expiry_test_switch::Forced::set(true), status requested with expires_at in the past). A `requested` or `granted` approval that is not yet consumed is NOT reached by the crash branch at all: claim_due_recovery excludes the run and has_pending_approval spares it, so one separate assertion pins that such a run is spared (the committed behaviour) and never crash-parked; the all-status approvals table stays in the direct crash_park_run test (loop_checkpoint::crash_park_db), where it is reachable. And crash_park_off_is_the_committed_worker: crash switch off, park recovery off, the store off (each with an eligible fold): terminalized at the deadline exactly as S8-A; with all three on the same fold is parked (the control).")
    rep(spec, 'tests',
        "two tasks race one run (A saves a tombstone at the old seq; the worker re-seals): exactly one wins in every interleaving the harness reaches. Tombstone first:",
        "the two ORDERED variants (no tokio::join!, which cannot force an order on one thread; the committed pattern is a held transaction, see a_cancel_that_commits_first_leaves_no_answer_on_the_run): a transaction holding the runs row stands in for the order, and A's tombstone save and the worker's re-seal run one after the other in each order. Tombstone first:")
    rep(spec, 'tests',
        "a_read_only_rung_refuses_every_call_the_ledger_classes_as_effectful (for every offered_tool_defs name with representative arguments: effect_class_of(...).is_effectful() implies check_autonomy_rung(ReadOnly, ...) is Err; ties the exactness argument to the code).",
        "the_read_only_tool_set_is_a_golden_list (it replaces a tautology: effect_class_of and check_autonomy_rung both derive from rung_required_for, so 'effectful implies refused' holds for ANY classifier, a wrong one included): a hard-coded list of the tool names allowed to run under read_only (every offered_tool_defs name plus tickets.create, each with a written reason it is a read: code_interpreter is hermetic, web_fetch and web.read are outbound GETs) must EQUAL the set of offered names whose rung_required_for is ReadOnly, so a new tool, or a keyword-list change that flips a name, fails until a person classifies it.")
    rep(spec, 'tests',
        "lost_ownership_is_true_only_after_a_stale_or_closed_save and ownership_unproven_is_true_only_after_a_denied_or_not_found_save",
        "lost_ownership_is_true_only_after_a_stale_closed_or_not_found_save and ownership_unproven_is_true_only_after_a_denied_save")
    rep(spec, 'tests',
        "the given counters), shares the stale-after-ambiguous probe,",
        "the given counters), is sent with the gated timeout, shares the stale-after-ambiguous probe,")
    rep(spec, 'tests',
        "(retire_fence takes the heartbeat age as an argument, so these tests need no clock):",
        "(the heartbeat age comes from park::beat_age, which these tests FORCE through the beat_test_clock::Forced seam: std Instant is not moved by tokio's paused time and the loop-top heartbeat refreshes the mark every round, so an unforced test could never reach 780 s):")
    rep(spec, 'tests',
        "Unauthenticated, PermissionDenied (the expired-bearer zombie) and NotFound, and an exhausted run of three transient failures, STOP when the age is at or past the window and PROCEED, writing the committed immediate failed receipt, when it is under it;",
        "Unauthenticated and PermissionDenied (the expired-bearer zombie) STOP when the age is at or past the window and PROCEED, writing the committed immediate failed receipt, when it is under it; NotFound stops at any age; a transient failure (Unavailable or a timeout, one retry), an unconfirmed Stale after an ambiguous attempt and a ContentRefused STOP at ANY age with no AppendMessage and no receipt (the case that pins it forces the age to 10 s);")
    rep_span(spec, 'tests',
        "a_transient_retire_failure_is_retried_twice_then_decided_by_the_age",
        "a Failed or Off writer makes no RPC and no sleep)",
        "a_transient_retire_failure_is_retried_once_then_stops (Unavailable twice: two RPCs at the gated timeout, then the loop stops with Aborted continue_in_progress, no AppendMessage and no receipt, even with a recent heartbeat: the fail-closed rule pinned so it cannot silently change back; an ambiguous first attempt that COMMITTED server-side followed by a Stale retry is the same stop (a fake that times out client-side but commits, then answers Stale: the loop's own tombstone is stored, nothing is appended, the worker ends the run later); a Failed or Off writer makes no RPC and no sleep)")
    insert_after(spec, 'tests', "S8B-3 execution-core agent tests, the funnel",
        "S8B-3 execution-core agent tests, the age clock: the_beat_age_is_the_larger_of_the_two_clocks (pure beat_age_of: the wall clock ahead of the monotonic one gives the wall age, behind it gives the monotonic age, a mark in the future is zero); a_denied_tombstone_at_800_s_stops_and_at_10_s_proceeds (through the beat_test_clock seam: Denied end-of-round and tombstone answers with the age forced to 800 s: Aborted continue_in_progress with no AppendMessage and no RecordTerminalOutcome in Recorder.terminal_order; forced to 10 s: the loop finalizes and writes the committed failed receipt); a_denied_loop_top_stops_only_past_the_window (the same seam at the loop top: an unproven writer stops the loop before any heartbeat or inference at 800 s and keeps running at 10 s). retire_violations pins the wiring these tests rely on (the mark's initialisation, its assignment immediately before the heartbeat, `park::beat_age(&last_beat)` at both call sites, the heartbeat after the deadline check) with mutation rows, because no test can observe a literal Duration::ZERO that the seam overrides.")
    rep(spec, 'tests',
        "(assert RunDetail and the receipt count, not only 'no append');",
        "(assert RunDetail and the receipt count, not only 'no append'); c4b the WHOLE STACK suspended past the deadline (a host sleep or VM suspend, not only docker pause: docker pause keeps CLOCK_MONOTONIC running, so it cannot exercise the wall-clock half of the age);")

    # ======================================================================== STEPS
    rep(spec, 'steps',
        "The lead records dfd1a8d5 as the S8-B base.",
        "The lead records the actual HEAD when S8B-0 starts as the S8-B base (dfd1a8d5 when this was written; 72c51bc3 after two documentation commits).")
    rep(spec, 'steps',
        "a build session checks that with `git diff --stat` at every step.",
        "a build session checks that with `git diff --stat <base>..HEAD` at every step (against the recorded base, not against HEAD, which prints nothing once a step is committed).")
    rep(spec, 'steps',
        "- the DB tests.",
        "- the DB tests (appended inside park_recovery_db_tests), the harness script's scrub and its contract test, and the CI filter step (owner-visible).")
    rep(spec, 'steps',
        "- terminalization.rs: the switch and crash_park_active, worker_cipher, the CASE and $4 in the horizon statement and spare_parked_run's third argument, PARK_HELD_REASON_SQL, the worker branch, the metrics, the amended park_spare guard and the new crash_park_violations;",
        "- terminalization.rs: the switch and crash_park_active, worker_cipher, the CASE and $4 in the horizon statement and spare_parked_run's third argument, PARK_HELD_REASON_SQL, the worker branch (before the single spare call), the metrics, the amended park_spare guard and the new crash_park_violations;")
    rep(spec, 'steps',
        "It is dormant: with SESSION_CORE_CRASH_PARK_ENABLED off the worker is S8-A's (the byte-identity of heartbeat_managed_run_inner and claim_due_recovery is pinned).",
        "It is dormant in behaviour: with SESSION_CORE_CRASH_PARK_ENABLED off the crash branch never runs (the byte-identity of heartbeat_managed_run_inner and claim_due_recovery is pinned). It is NOT dormant in SQL: the spare statement gained a CASE and two binds and runs for EVERY recovered run whenever park recovery is on, so a type slip would stop every terminalization while park recovery is on, and only a DB run catches it: merging requires the lead's harness run of the committed park_recovery_db_tests against the new statement.")
    rep(spec, 'steps',
        "Verify: cargo test -q -p session-core --bin session-core (session-core is a binary-only crate: there is no --lib target); the DB tests by the lead against a scratch Postgres with --test-threads=1 (park_recovery_db_tests::isolate pushes every other obligation a day out, so the DB tests must not run in parallel).",
        "Verify: cargo test -q -p session-core --bin session-core (session-core is a binary-only crate: there is no --lib target); cargo test -q -p execution-core --test durable_loop_contract --test cross_service_loop_contract (they scan session-core's loop_checkpoint.rs and terminalization.rs, and CI runs the whole workspace, so a break would otherwise land red on this commit); the DB tests by the lead through the harness scripts/tests/session-core-db-tests.sh with count floors (verification_commands), never the raw cargo command (the harness runs --test-threads=1: park_recovery_db_tests::isolate pushes every other obligation a day out).")
    rep(spec, 'steps',
        "park.rs (must_stop; try_park: note_self_closed in the NotAccepted arm only,",
        "park.rs (must_stop, BeatMark, beat_age and the beat_test_clock seam; try_park: note_self_closed in the NotAccepted arm only,")
    rep(spec, 'steps',
        "cargo fmt --all -- --check (CI runs it, and the async block re-indents run_rounds).",
        "NO cargo fmt --all: `cargo fmt --all -- --check` is already red on this branch (112 files, about 2850 hunks, the touched files included), so CI's format step is not this change's gate and a wholesale fmt would rewrite the text every guard anchors on. Instead record `cargo fmt -p execution-core -- --check 2>&1 | grep -c 'Diff in'` per touched file when S8B-3 starts and require that no touched file's count grows (indent the async block by hand as rustfmt would), and run `cargo test -p execution-core --tests` once (other integration tests read agent.rs text).")
    rep(spec, 'steps',
        "one extra tombstone RPC per run end and a pre_effect save before a plan or delegation round.",
        "one extra tombstone RPC per run end (a degraded session-core makes the run wait up to about 30 s and then STOP without its answer: the run is left to the worker) and a pre_effect save before a plan or delegation round.")
    rep(spec, 'steps',
        "(5) executor crash park off (no new marks, no tombstone, no ownership stop; loops already running keep the behaviour they started with);",
        "(5) executor crash park off (no new marks, no tombstone, no ownership stop; flipping it recreates the container, so no in-flight loop survives it);")
    steps = spec['steps']
    ri = [i for i, e in enumerate(steps) if e.startswith('ROLLBACK OF IMAGES')]
    assert len(ri) == 1
    steps.insert(ri[0] + 1,
        "HOW EACH ENVIRONMENT FLIPS AND KILLS (the flip, kill and rollback text above is written for compose-env flips). DEV: a switch is container environment and a `compose up` recreate drops variables the carry-over script does not re-supply; the lead's flip script gets rows for EXECUTION_LOOP_CRASH_PARK_ENABLED, SESSION_CORE_CRASH_PARK_ENABLED and SESSION_CORE_CRASH_PARK_TTL_SECONDS and must re-supply the four resume switches too: a recreate that loses one silently disables marking (crash_marking_active needs park_resume_active) and, with crash parks present, breaks the rule that none of the resume lane goes off first. PRODUCTION: scripts/compose.sh in artifact mode accepts only `config --quiet` and `up -d --wait --no-build`; non-secret switch values live in the signed runtime-public-policy.env (the new keys are classified public by config_partition_for_key) and a rollback is a previous signed artifact, so the kill order is executed as ARTIFACTS: the artifact that turns (6) off must be applied no later than, and best in the same artifact as, any image change that restarts execution-core, model-gateway or session-core while (5) and (6) are on (a session-core recreate also aborts every loop whose loop-top heartbeat falls in the downtime: F43). DRAIN QUERY before a session-core rollback or a descriptor key rotation: SELECT count(*) FROM run_loop_checkpoints WHERE park_reason = 'crash_interrupted' AND state IN ('parked','resuming') must be 0 (or wait the crash TTL after (6) is off, or cancel the runs). PRE-FLIGHT CHECKLIST before (5): the four resume switches, the park-recovery and store switches, the descriptor key and the MANAGED_DEADLINE_SECS pairing are present and equal on every replica of the three services; the per-run armed info! line is the only executor-side evidence, because execution-core records no metrics.")

    # ======================================================================== RISKS
    rep_elem(spec, 'risks', "R3: the residual duplicate answer, of a HEALTHY loop.",
        "R3: the residual duplicate answer, now narrow (round 4 closed the healthy-loop transient case by failing closed). An unconfirmed tombstone never proceeds: a healthy loop that cannot reach session-core at its end STOPS (after up to about 30 s) instead of appending, because the fail-open window bounds when finalize starts and not when its append lands (a partition that failed the tombstone could also have hung the append until after a resume). The cost is a lost answer, never a duplicate: the run is left to the worker (crash-parked and resumable if the tombstone did not land, outcome_unknown at the deadline if it did), and it is the same user-visible outcome as committed code when session-core is down at that moment. Two narrow paths remain: (i) a writer that cannot write at all (Off, or Failed for a non-ownership reason such as KeyUnavailable) cannot tombstone and finalizes as committed, so on replicas whose configuration disagrees a marked fold stored earlier can be crash-parked by another replica (R19b); with a hanging append that is a duplicate path, closed only by the drain-before-rolling rule; (ii) a Denied tombstone with a recent heartbeat proceeds, which cannot duplicate (no crash park can exist yet and the same expired bearer cannot append). The exact belt for (i) and for any unforeseen late append needs a durable fact ordered with the append: an idempotent final answer per run (a unique partial index on the final assistant message per thread and run, ON CONFLICT DO NOTHING in append_message_in_tx) or the v4 thread watermark; both need a migration and are listed for the owner.")
    rep(spec, 'risks',
        "(e) cost when session-core is degraded: one extra tombstone RPC per run end (up to about 6.4 s of retries before the committed finalize failure) and a pre_effect save before a plan or delegation round (up to the 10 s gated timeout plus one probe before dispatch).",
        "(e) cost when session-core is degraded: one extra tombstone RPC per run end (up to about 30 s, two attempts at the gated 10 s timeout plus one probe, before the stop of (f)) and a pre_effect save before a plan or delegation round (up to the 10 s gated timeout plus one probe before dispatch); (f) a marked loop that cannot confirm its tombstone (Unavailable or a timeout, an unconfirmed Stale, a ContentRefused) STOPS instead of answering, so a degraded session-core at run end costs the run its answer (R3), and a user's Stop makes the loop return Aborted continue_in_progress, which a fresh run's stream turns into a retryable agent_dispatch_unavailable frame plus RUN_DISPATCH_DEGRADED on a run the user just stopped (the SPA's reaction to that frame is unverified).")
    rep(spec, 'risks',
        "version skew between images shows up as the same park_not_restorable.",
        "version skew between images shows up as the same park_not_restorable. A resume's drift-close (close_unrestorable) also answers Closed to a still-live long-bearer loop's tombstone, which then stops without its answer: the run ends outcome_unknown at the renewed deadline although that loop was about to answer.")
    rep(spec, 'risks',
        "the test a_read_only_rung_refuses_every_call_the_ledger_classes_as_effectful pins it over the offered tool set.",
        "the golden-list test the_read_only_tool_set_is_a_golden_list pins the set of tool names allowed under read_only, so a new tool fails until a person classifies it (an earlier 'effectful implies refused' form was a tautology: both sides derive from rung_required_for).")
    rep(spec, 'risks',
        "(a second plan proposal, newest wins; a second delegation record).",
        "(a second plan proposal, newest wins; a second delegation record). For a SUPERSEDED loop with a still-valid bearer (long-lived bearers) whose fence save failed transiently the call is dispatched too, and its late plan.propose, being the newer write, can displace the plan the resumed loop proposed: accepted (low); a dead-bearer zombie is protected because its ProposePlan is Unauthenticated.")
    rep(spec, 'risks',
        "If that save is not confirmed, the approvals fact in crash_park_run still refuses the run, so a stale marked fold can never be parked after an approval.",
        "If that save is Denied with a recent heartbeat the approval is attempted anyway (with the same expired bearer) and the approvals fact in crash_park_run still refuses the run, so a stale marked fold can never be parked after an approval; any other unconfirmed save stops the loop before the approval exists.")
    rep(spec, 'risks',
        "as it moves today's terminalization.",
        "as it moves today's terminalization. The executor's age rule uses wall time as well as the monotonic clock (BeatMark), so a host or VM suspend lengthens the age; the one remaining skew is between the executor's and session-core's wall clocks, absorbed by the 120 s margin.")
    insert_before(spec, 'risks', "R19: a new user turn on the same thread while a crash park waits:",
        "R19e: CI formatting is already red on model-plane (cargo fmt --all -- --check: about 2850 hunks in 112 files at 72c51bc3, the files S8-B touches included), so a red or green format step says nothing about S8-B and `cargo fmt --all` must not be run on this branch (it would rewrite the text every guard anchors on). S8B-3 gates on a per-file baseline instead, and the three re-anchored mutation rows are whitespace-insensitive; a future wholesale fmt is a separate change that would need its own re-anchoring.")
    insert_before(spec, 'risks', "R19: a new user turn on the same thread while a crash park waits:",
        "R19f: the executor's window (900 minus 120 s) is a compile-time constant of the executor image, pinned to session-core's MANAGED_DEADLINE_SECS by a source test in each tree, not between DEPLOYED images: a session-core deploy that lowers the deadline must be preceded by an executor image with the lower window, or an older executor keeps proceeding on a Denied answer for up to 780 s while a worker may act earlier. Optional hardening (not built): record `renewed_until` from each accepted heartbeat response (the executor discards it today) and derive the window from it, and honour already_terminal as an immediate stop for an armed loop.")
    insert_before(spec, 'risks', "R19: a new user turn on the same thread while a crash park waits:",
        "R19g: a delegated child's own heartbeats extend the run's obligation but not the root's BeatMark, so a root writer that is Failed(Denied) (an earlier auth blip) stops at its next loop top once 780 s have passed since the root's own last heartbeat, although the run is healthy. Conservative; a shared atomic mark that children publish to would remove it.")
    insert_before(spec, 'risks', "R19: a new user turn on the same thread while a crash park waits:",
        "R19h: the audit trail does not distinguish a worker-made park from an executor-made one (crash_park_run writes the same content-free CHECKPOINT_SAVED {seq, kind: park} event; park_reason lives only in the row), and the worker now opens fold ciphertext for every recovered run with an active fold when the switch is on, including the runs it then refuses. Not a safety issue; a closed-word reason in the event payload is an optional follow-up.")
    insert_before(spec, 'risks', "R19: a new user turn on the same thread while a crash park waits:",
        "R19i: the S8B-1 spare statement change (a CASE and two binds) runs for every recovered run whenever park recovery is on, whatever the crash switch says, so it is on the S8-A hot path from the day S8B-1 deploys. The only gate that exercises it is the lead's harness run of park_recovery_db_tests (and CI, if the owner accepts the workflow change of S8B-1).")

    # ======================================================================== INVARIANTS
    rep_elem(spec, 'invariants', "A marked (armed) loop ends the run (finalize:",
        "A marked (armed) loop ends the run (finalize: the answer append and the terminal receipt, for ANY outcome including typed failures) or raises an approval only after retire_fence returned Ok, or in one of these cases: (a) its writer is not Active, has not lost ownership and is not unproven (a NotAccepted park the loop itself closed, a store that is off or answers Unimplemented, a missing descriptor key, a writer failed for another non-ownership reason: R19b); or (b) the only answer was Denied (an expired bearer), or the writer is already Failed(Denied), AND the loop's own last accepted heartbeat was SENT less than the fail-open window ago (900 s minus 120 s), by the larger of its monotonic and wall-clock age. A tombstone that is not confirmed (Unavailable or a timeout after the one retry, an unconfirmed Stale after an ambiguous attempt, ContentRefused) stops the loop at ANY age, as do Stale, Closed, run_ended and NotFound, a writer that lost ownership, and a Denied at or past the window: Aborted continue_in_progress, before finalize or the approval: no append, no receipt, no approval. An unarmed loop is untouched. retire_fence has exactly two call sites.")
    rep(spec, 'invariants',
        "its writer has lost ownership (Stale or Closed), or it is unproven (Denied or NotFound) and the heartbeat age is at or past the fail-open window.",
        "its writer has lost ownership (Stale, Closed or NotFound), or it is unproven (Denied) and park::beat_age(&last_beat) is at or past the fail-open window.")
    rep(spec, 'invariants',
        "The executor never writes crash_interrupted and the save RPC still refuses it (park_reason_reserved).",
        "The executor never writes crash_interrupted and the save RPC still refuses it (park_reason_reserved) while park recovery is active (validate_park_save runs only under park_recovery_active(); with park recovery off a crash_interrupted park row would be stored as an ordinary parked row, which is S8-A's own exposure, restated here).")

    # ======================================================================== FAILURE MATRIX
    rep(spec, 'failure_matrix',
        "a Denied answer, an unproven writer or any transport failure of the tombstone STOPS the loop; a Stale or Closed answer stops it at any age.",
        "a Denied answer or an unproven writer STOPS the loop (the age is past the window), and so does any transport failure of the tombstone (an unconfirmed tombstone never proceeds, at any age); a Stale, Closed or NotFound answer stops it at any age.")
    rep(spec, 'failure_matrix',
        "If the 'paused' tombstone could not be confirmed, crash_park_run's approvals fact still refuses the run.",
        "If the 'paused' tombstone could not be confirmed the loop stops before raising the approval (F29), except for a Denied answer with a recent heartbeat, where crash_park_run's approvals fact still refuses the run.")
    rep_elem(spec, 'failure_matrix', "F23 The retire save fails Unavailable or times out three times",
        "F23 The retire save fails Unavailable or times out (the one retry too) for a healthy loop with a recent heartbeat: the loop STOPS (Aborted continue_in_progress): no append, no receipt, no approval. The tombstone may or may not have committed: if it did, the worker later terminalizes the run outcome_unknown (the answer in the loop's memory is lost); if it did not, the marked fold is crash-parked at the deadline and a resume answers once. Cost: a degraded session-core at run end costs the run its answer, after up to about 30 s of waiting. No append was sent, so no late-delivered append can duplicate.")
    rep(spec, 'failure_matrix',
        "Answered Unauthenticated, PermissionDenied or NotFound it stops only at or past the fail-open window (a healthy loop whose bearers just expired proceeds and writes the committed immediate failed receipt).",
        "Answered NotFound (the run row is gone) it stops at any age; answered Unauthenticated or PermissionDenied it stops only at or past the fail-open window (a healthy loop whose bearers just expired proceeds and writes the committed immediate failed receipt).")
    rep(spec, 'failure_matrix',
        "a gateway restart drops the RunAgent call and the executor's handler future with it (no funnel, no receipt).",
        "the gateway drains in-flight turns for MODEL_GATEWAY_DRAIN_SECS (120 s) before it exits and its RunAgent task is spawned, so only a hard kill or a turn longer than the drain drops the call and the executor's handler future with it; an executor recreate has no drain (SIGTERM ends main and the handler is awaited inline), so every in-flight loop dies with no funnel and no receipt.")
    rep_elem(spec, 'failure_matrix', "F29 A pinned marked run pauses cold",
        "F29 A pinned marked run reaches a disclosure approval (cold: no pause fold exists for a pinned run) and the 'paused' tombstone is not confirmed (Unavailable after the retry): the loop STOPS before the approval exists, so no approvals row exists; the stored fold is the marked boundary fold; the run is crash-parked at the deadline and a resume re-infers and asks again. If the tombstone is confirmed the approval is raised and crash_park_run refuses the run for as long as any approvals row exists (F4). A Denied tombstone with a recent heartbeat attempts the approval with the same expired bearer, which a dead bearer cannot create; if it could, the approvals fact refuses the run.")
    rep(spec, 'failure_matrix',
        "a run end waits up to about 6.4 s for three tombstone attempts; the committed finalize failure follows.",
        "a run end waits up to about 30 s (two tombstone attempts at the gated timeout plus one probe) and then the loop STOPS (F23).")
    insert_before(spec, 'failure_matrix', "F36 A new user turn is appended",
        "F43 Session-core is restarted, deployed or recreated (flipping (6) itself is one) while (5) and (6) are on: the loop-top heartbeat maps every error to Unavailable and ends run_rounds with Err (no retry), so every loop whose loop top falls in the downtime is aborted. Each such run is a marked fold and is crash-parked at its deadline instead of ending at about 15 minutes (held up to the deadline plus the crash TTL with no resumer). Acceptable, but it belongs to the owner's flip cost list and to the choice of when to turn (6) off.")
    insert_before(spec, 'failure_matrix', "F36 A new user turn is appended",
        "F44 The descriptor key is rotated while crash parks exist (DescriptorCipher has one key and no key id): a crash park sealed under the old key fails every claim (the executor answers resume_checkpoint_unavailable and the park is held to the crash TTL) and the worker counts did_not_authenticate for marked folds under the new key (the unreadable family). The drain query of the environment step comes first.")
    insert_before(spec, 'failure_matrix', "F36 A new user turn is appended",
        "F45 A GDPR organization purge runs while a crash park is being re-sealed: purge_organization_data deletes run_loop_checkpoints rows before the runs rows, while the worker locks runs, obligation, fold. That lock-order inversion already exists for save and for S8-A's spare; S8-B lengthens the hold (decrypt and encrypt up to 4 MiB under three locks). The outcome is a Postgres deadlock victim, recovered by the purge's redelivery or the worker's lease; UNVERIFIED which side Postgres aborts. Not a row of data loss.")
    insert_before(spec, 'failure_matrix', "F36 A new user turn is appended",
        "F46 Session-core lowers MANAGED_DEADLINE_SECS (a faster recovery) while executors of the older image run: the executor's window is a compile-time constant, so an older executor keeps proceeding on a Denied answer for up to 780 s although a worker may act earlier. The source pin is per tree, not across deployed images: lower the executor's window first (R19f).")

    # ======================================================================== AMENDMENTS
    rep(spec, 'amendments_to_prior_specs',
        "S8B-1 amends them and their mutation rows in the SAME commit (the two `$2` horizon needles, the exact-string row that removes `AND updated_at + make_interval(secs => $2) > now()` and its assert_ne!, the `spare_parked_run(` and `if park_recovery_active() {` rows), and adds the crash branch to the order they pin (after the spare, before the held-reason read and the terminal write).",
        "S8B-1 amends them and their mutation rows in the SAME commit (the two `$2` horizon needles, the exact-string row that removes `AND updated_at + make_interval(secs => $2) > now()` and its assert_ne!, and the `if park_recovery_active() {` row; the committed rule 'the spare is called once' stays true because the crash branch runs BEFORE the single spare call), and adds the crash branch to the order they pin (after has_pending_approval, before the spare).")
    rep(spec, 'amendments_to_prior_specs',
        "the executor stops on a definitive Stale, Closed, Denied or NotFound, which the store already says,",
        "the executor stops on a definitive Stale, Closed or NotFound (and on Denied once its own heartbeat age is at the window, and on any unconfirmed tombstone), which the store already says,")
    rep(spec, 'amendments_to_prior_specs',
        "S8-B treats Denied as lost ownership and fences the receipt;",
        "S8-B treats Denied as unproven (stopped once the loop's own heartbeat age is at the window) and fences the receipt;")

    # ======================================================================== OPEN QUESTIONS
    rep(spec, 'open_questions',
        "or a few minutes if a resumer will act at once.",
        "or a few minutes if a resumer will act at once. RETENTION is part of this decision: before S8-B a crashed run's sealed fold was purged about 15 minutes after the last heartbeat; with (6) on it lives to the deadline plus the crash TTL (clamp up to 30 days), so the TTL is also how long a crashed run's conversation ciphertext is kept. Not a ZDR matter (ZDR and waived runs never get one), but the owner's call.")
    rep_elem(spec, 'open_questions', "OWNER (open): ACCEPT THE RESIDUAL DUPLICATE ANSWER (R3)",
        "OWNER (open): THE EXACT BELT (R3), optional. Round 4 closed the healthy-loop transient case by failing closed, so the duplicate-answer paths left are the configuration-skew ones (R19b: a writer that is Off or Failed for a non-ownership reason cannot tombstone) and anything unforeseen that delivers a late append. The exact belt would be an idempotent final answer per run (a unique partial index on the final assistant message per thread and run with ON CONFLICT DO NOTHING in append_message_in_tx; a migration) or the v4 thread watermark (migration 0049 plus a claim that takes the thread lock). Neither is built. The owner decides whether the drain-before-rolling rule suffices, and whether the cost of failing closed (a degraded session-core at run end costs the run its answer) is acceptable against it.")
    rep(spec, 'open_questions',
        "recovery instead of an immediate failed receipt for a marked loop that overran its credentials?",
        "recovery instead of an immediate failed receipt for a marked loop that overran its credentials, and no answer from a marked loop whose tombstone cannot be confirmed (a degraded session-core at run end: R3, F23)?")
    insert_before(spec, 'open_questions', "Integrator: nothing to record in plan 4.3",
        "OWNER/LEAD (open): CI IS ALREADY RED on model-plane for formatting (cargo fmt --all -- --check: about 2850 hunks in 112 files at 72c51bc3), so a red or green format step says nothing about S8-B; fixing it is a separate wholesale change that would break every whitespace-anchored guard row (a one-off re-anchoring). Separately, the worker-level crash-park tests do not run in CI unless the owner accepts the S8B-1 workflow change (the filters crash_park and park_recovery with count floors, and the deploy directory in the path filter).")
    rep(spec, 'open_questions',
        "(H3) reject a heartbeat while the fold is a crash park in state parked,",
        "(H4) record `renewed_until` from each accepted heartbeat response and derive the executor's fail-open window from it, and honour already_terminal as an immediate stop for an armed loop (removes the cross-image pin of R19f); (H3) reject a heartbeat while the fold is a crash park in state parked,")

    # ======================================================================== VERIFICATION
    rep(spec, 'verification_commands',
        "diff --stat HEAD -- '*.proto'",
        "diff --stat <base>..HEAD -- '*.proto'")
    rep(spec, 'verification_commands',
        "(must print nothing at every step: S8-B changes none of them)",
        "(must print nothing at every step: S8-B changes none of them; <base> is the HEAD the lead recorded when S8B-0 started, because a diff against HEAD prints nothing once a step is committed)")
    rep_elem(spec, 'verification_commands', "cd \"apps/Model Plane/rust\" && DATABASE_URL=<scratch Postgres",
        "bash \"apps/Model Plane/scripts/tests/session-core-db-tests.sh\" crash_park=<N> park_recovery=<M> loop_checkpoint=<K>   (S8B-1; LEAD ONLY, the owner starts the scratch Postgres whose database name contains 'test'; the harness adds --include-ignored and --test-threads=1 (isolate() pushes every other obligation a day out), enforces a minimum pass count per filter and fails on 'skipping: DATABASE_URL unset' or a panic, so a renamed module cannot pass vacuously, which the raw `cargo test -- --ignored` route would; set N, M and K to the counts at the S8B-1 commit and update the recorded full-run count in session-core-db-tests-test.sh deliberately; the build session never runs docker)")
    rep(spec, 'verification_commands',
        "(S8B-2, S8B-3, S8B-4, including every_guard_goes_red_when_the_code_it_guards_regresses",
        "(S8B-1 too, because it scans session-core's source; S8B-2, S8B-3, S8B-4, including every_guard_goes_red_when_the_code_it_guards_regresses")
    rep(spec, 'verification_commands',
        "--test cross_service_loop_contract   (S8B-4)",
        "--test cross_service_loop_contract   (S8B-1 and S8B-4)")
    rep(spec, 'verification_commands',
        "and cargo fmt --all -- --check   (after S8B-3: the async block re-indents run_rounds; CI checks formatting)",
        "and NOT cargo fmt --all (red at the base: about 2850 hunks in 112 files): at S8B-3 compare the per-file `Diff in` counts of `cargo fmt -p execution-core -- --check` against the counts recorded when S8B-3 started (no touched file's count may grow), and run `cargo test -p execution-core --tests` once at S8B-3 (other integration tests read agent.rs text)")
    return spec
