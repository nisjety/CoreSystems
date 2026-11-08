"""Revision 7 of the S8-B spec: applies the round-6 review findings on top of revision 6."""
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
    # ======================================================================= H1: SQL typing of the spare statement
    rep(spec, 'file_changes',
        "both places that read the wait become `make_interval(secs => CASE WHEN park_reason = $5 THEN $4 ELSE $2 END)` ($1 run, $2 park TTL, $3 the deadline period, $4 crash TTL, $5 token).",
        "both places that read the wait become `make_interval(secs => CASE WHEN park_reason = $5::text THEN $4::float8 ELSE $2::float8 END)` and the handback keeps `now() + make_interval(secs => $3::float8)` ($1 run, $2 park TTL, $3 the deadline period, $4 crash TTL, $5 token). The casts are REQUIRED: with the CASE, $2 and $4 no longer have a direct typed use, and Postgres resolves a CASE whose branches are all unknown-typed parameters as text, which make_interval(secs => ..) rejects (and sqlx's f64 binds, sent as float8, would then clash with a text parameter): the statement would fail to prepare for EVERY recovered run while park recovery is on. $4 is bound as `crash_ttl_secs as f64` (the same `#[allow(clippy::cast_precision_loss)]` as the committed binds) and $5 as `ParkReason::CrashInterrupted.as_str()`.")
    rep(spec, 'file_changes',
        "each is re-pinned on `CASE WHEN park_reason = $5 THEN $4 ELSE $2 END` in both places with a target that still exists.",
        "each is re-pinned on `CASE WHEN park_reason = $5::text THEN $4::float8 ELSE $2::float8 END` in both places of the one horizon statement (PARK_SPARE_SQL has no wait) with a target that still exists.")
    rep(spec, 'tests',
        "the horizon SQL carries the CASE on `$5` in both statements with `$4`,",
        "the horizon SQL carries the CASE on `$5::text` in both places of the one horizon statement (PARK_SPARE_SQL has no wait) with `$4::float8` and `$2::float8`,")
    rep(spec, 'steps',
        "merging requires the lead's harness run of the committed park_recovery_db_tests against the new statement.",
        "merging requires the lead's harness run of the committed park_recovery_db_tests against the new statement, which is the FIRST gate of this step (run before S8B-1 is called done, because a type slip in that statement stops all terminalization while park recovery is on).")

    # ======================================================================= H2: marked-fold tracker wiring
    rep_span(spec, 'file_changes',
        "A fifth pure accessor, `marked_fold_may_be_stored(&self) -> bool`",
        "(an Ok retire clears the flag).",
        "A fifth pure accessor, `marked_fold_may_be_stored(&self) -> bool`, is backed by ONE private bool `marker_stored`, advanced by one private helper `record_save(&mut self, carried: bool, outcome)` placed immediately after each of the THREE places where the SeqTracker is advanced: `save_bytes` (round and pre_effect folds, tombstones, and the fallback tombstone that save_round and save_pre_effect write after a ContentRefused), `fence_with` (the park-resume fence) and `park_with` (parks). The wrappers (save_round, save_pre_effect, save_fence, save_park) keep their signatures: committed guards pin their call shapes (park_fence_violations pins fence_parked to `folds.save_fence(fold::build_fold(snapshot, None, None))`, one `save_fence(` in park.rs). `carried` is read from the fold being saved BEFORE it is consumed: `mp_contracts::crash_park::crash_mark_in(&fold).is_some()` (the only reader of the key, so crash_mark_violations is not tripped); `save_bytes` gains a private `carried: bool` parameter (save_fold passes the fold's value; tombstones and retire pass false). Ok sets the flag to `carried`; Unavailable{ambiguous:true} sets it to `carried || marker_stored`; any other error leaves it; park_with's NotAccepted outcome (`Ok((seq, false))`: the store stored the loop's own UNMARKED park row over the marked fold) clears it. FoldWriter::pause_fold saves nothing (PauseFold::save is never reached by an armed, pinned loop: its pause is cold). The rule has one mechanical source, the fold that was saved, so nothing depends on build_fold's own guard; open_round_fold passes `crash_mark: None` (it has an open round and can never be marked). record_save is placed after the match on the save result and spells none of `save_tombstone` or `probe` (committed guards anchor on park_with's and save_bytes's text: the `let pause_ref = match` block and the `park_rpc(` count).")
    rep(spec, 'file_changes',
        "the park-resume fence snapshot, `open_round_fold` and the end-of-round fold pass `crash_gate.mark_for(rounds_used)`;",
        "the park-resume fence snapshot and the end-of-round fold pass `crash_gate.mark_for(rounds_used)`; `open_round_fold` passes None (it has an open round and can never be marked);")
    rep(spec, 'approach',
        "the park-resume fence snapshot, the open-round and the end-of-round snapshots pass crash_gate.mark_for(rounds_used);",
        "the park-resume fence snapshot and the end-of-round snapshot pass crash_gate.mark_for(rounds_used); the open-round snapshot passes None (it can never be marked);")
    rep(spec, 'file_changes',
        "park_at_boundary passes crash_mark None;",
        "park_at_boundary and open_round_fold pass crash_mark None;")
    rep(spec, 'approach',
        "and ownership_unproven() (State::Failed(Refused(Denied)))",
        "and ownership_unproven() (State::Failed(Refused(Denied))), and marked_fold_may_be_stored(), backed by a private marker_stored flag advanced by record_save at the three places the SeqTracker is advanced (save_bytes, fence_with, park_with) with `carried` read from the fold being saved (crash_mark_in(&fold).is_some(), before it is consumed)")
    rep(spec, 'approach',
        "an Ok save that did not carry it clears it, an ambiguous one leaves it, a writer that never saved has it false",
        "an Ok save that did not carry it clears it (a NotAccepted park, which stores the loop's own unmarked park row, too), an ambiguous one leaves it, a writer that never saved has it false")
    rep(spec, 'steps',
        "fold.rs (Snapshot's one field, build_fold's marker rule, and the four pure accessors arm_fence, is_fenced, lost_ownership and ownership_unproven, which the CrashGate and the visible-call hold use);",
        "fold.rs (Snapshot's one field, build_fold's marker rule, the pure accessors arm_fence, is_fenced, lost_ownership, ownership_unproven (used only by retire_fence in S8B-3: a dead_code warning until then, not an error) and marked_fold_may_be_stored, whose private marker_stored flag is advanced by record_save at the three places the SeqTracker is advanced (save_bytes, fence_with, park_with); the CrashGate, the visible-call hold and S8B-3's fence read the accessors);")
    rep(spec, 'tests',
        "and carries_marker is the one rule build_fold uses (a Snapshot with crash_mark Some, round None and pending None carries it; every other shape does not).",
        "and `carried` is read from the fold handed to the save (crash_mark_in(&fold).is_some()): a marked boundary fold sets the flag; the open-round pre_effect fold of a visible-call round does not carry it and clears it on Ok (so after a visible-call fence returns Ok the flag is false and the no-RPC fast path fires); a marked park-resume fence Ok sets it (the fence path, fence_with); a park fold never carries it and a NotAccepted park clears it (park_with); a ContentRefused fallback tombstone Ok clears it (the second save inside save_round and save_pre_effect goes through save_bytes).")

    # ======================================================================= E3: Denied needs a verifiably expired bearer
    rep(spec, 'approach',
        "is the one UNPROVEN case, resolved with the executor's OWN CLOCK:",
        "is the one UNPROVEN case, and it is resolved only when the session bearer is VERIFIABLY EXPIRED (run_budget::bearer_expiry of the session bearer at or before the current epoch second, the two pure helpers RunDeadline already uses; an unreadable exp counts as not expired): a Denied answer from a bearer that is not expired is not explained by expiry (a verifier or authorization hiccup, the earlier blip of R19g), so it is treated as unconfirmed and STOPS. For an expired bearer it is resolved with the executor's OWN CLOCK:")
    rep(spec, 'summary',
        "the loop proceeds only while the last heartbeat the store accepted was sent less than 780 s ago (the larger of its monotonic and wall-clock age)",
        "the loop proceeds only while its bearer is verifiably expired and the last heartbeat the store accepted was sent less than 780 s ago (the larger of its monotonic and wall-clock age)")
    rep(spec, 'invariants',
        "(b) the only answer was Denied (an expired bearer), or the writer is already Failed(Denied), AND the loop's own last accepted heartbeat was SENT",
        "(b) the only answer was Denied, or the writer is already Failed(Denied), AND the session bearer is verifiably expired, AND the loop's own last accepted heartbeat was SENT")
    rep(spec, 'invariants',
        "and a Denied at or past the window:",
        "and a Denied at or past the window or with a bearer that is not verifiably expired:")
    rep(spec, 'file_changes',
        "Denied (Unauthenticated, PermissionDenied) stops when `heartbeat_age >= fail_open_window()` and proceeds otherwise;",
        "Denied (Unauthenticated, PermissionDenied) proceeds only when the session bearer is verifiably expired (`session_bearer_expired`) AND `heartbeat_age < fail_open_window()`, and stops otherwise;")
    rep(spec, 'file_changes',
        "a writer that is unproven (Failed(Refused(Denied))) makes no RPC and takes the age rule;",
        "a writer that is unproven (Failed(Refused(Denied))) makes no RPC and takes the same Denied rule (bearer verifiably expired and age under the window, else stop);")
    rep(spec, 'file_changes',
        "through which loop tests force an age (S8B-3).",
        "through which loop tests force an age (S8B-3).\n- `pub(crate) fn session_bearer_expired(bearer: &str) -> bool` = `run_budget::bearer_expiry(bearer).is_some_and(|exp| exp <= run_budget::now_epoch_secs())` (the two pure helpers RunDeadline already uses; an unreadable exp is NOT expired): the second condition of the Denied rule (S8B-3).")
    rep(spec, 'file_changes',
        "\"answering\", \"exit\", park::beat_age(&last_beat))",
        "\"answering\", \"exit\", park::beat_age(&last_beat), park::session_bearer_expired(&ctx.session_bearer))")
    rep(spec, 'interfaces',
        "reason: &'static str, site: &'static str, heartbeat_age: Duration) -> Result<(), tonic::Status>;",
        "reason: &'static str, site: &'static str, heartbeat_age: Duration, session_bearer_expired: bool) -> Result<(), tonic::Status>;")
    rep(spec, 'interfaces',
        "pub(crate) struct BeatMark { at: Instant, wall: SystemTime }",
        "pub(crate) fn session_bearer_expired(bearer: &str) -> bool;       // run_budget::bearer_expiry(bearer) <= now; unreadable exp = not expired; the Denied rule's second condition\npub(crate) struct BeatMark { at: Instant, wall: SystemTime }")
    rep(spec, 'approach',
        "retire_fence(folds, crash_gate, counters, authority: Option<&Authority>, reason, site, heartbeat_age)",
        "retire_fence(folds, crash_gate, counters, authority: Option<&Authority>, reason, site, heartbeat_age, session_bearer_expired)")
    rep(spec, 'risks',
        "(ii) a Denied tombstone with a recent heartbeat proceeds, which cannot duplicate (no crash park can exist yet and the same expired bearer cannot append).",
        "(ii) a Denied tombstone proceeds only for a verifiably EXPIRED bearer with a recent heartbeat, which cannot duplicate (no crash park can exist yet and the same expired bearer cannot append); a Denied answer from a bearer that is not expired stops.")
    rep(spec, 'failure_matrix',
        "answered Unauthenticated or PermissionDenied it stops only at or past the fail-open window (a healthy loop whose bearers just expired proceeds and writes the committed immediate failed receipt).",
        "answered Unauthenticated or PermissionDenied it proceeds only when the session bearer is verifiably expired and the heartbeat age is under the fail-open window (a healthy loop whose bearers just expired writes the committed immediate failed receipt) and stops otherwise (an old age, or a bearer that is not expired: a verifier or authorization hiccup).")
    rep(spec, 'tests',
        "Unauthenticated and PermissionDenied (the expired-bearer zombie) STOP when the age is at or past the window and PROCEED, writing the committed immediate failed receipt, when it is under it;",
        "Unauthenticated and PermissionDenied with a verifiably EXPIRED bearer (the zombie) STOP when the age is at or past the window and PROCEED, writing the committed immediate failed receipt, when it is under it; with a bearer that is NOT expired they STOP at any age (the tests mint the bearer with a past or a future exp, as the run_budget tests do);")

    # ======================================================================= E4: retire's gated timeout needs a save_bytes parameter
    rep(spec, 'file_changes',
        "(one RPC with the GATED timeout, GATED_SAVE_TIMEOUT, instead of the 2 s round-save timeout because it is a must-confirm save, the stored kind staying round; the one stale-after-ambiguous probe; apply() on failure)",
        "(one RPC with the GATED timeout: save_bytes, private with two callers, gains a `timeout: Duration` parameter (and the `carried` parameter above); save_fold passes `kind.timeout()`, save_tombstone_as passes the caller's choice, retire passes GATED_SAVE_TIMEOUT, and both the first attempt and the stale probe use it; the stored kind stays round; apply() on failure)")
    rep(spec, 'approach',
        "(the builder adds the timeout choice to save_tombstone_as)",
        "(save_bytes gains a `timeout: Duration` parameter, see file_changes)")
    rep(spec, 'tests',
        "is sent with the gated timeout, shares the stale-after-ambiguous probe,",
        "is sent with the gated timeout (a Recorder fake that answers after 5 s under paused tokio time: retire succeeds while an end-of-round save times out), shares the stale-after-ambiguous probe,")
    rep(spec, 'file_changes',
        "retire_fence's decision is an exhaustive match over the typed outcomes with no wildcard arm that proceeds;",
        "retire_fence's decision is an exhaustive match over the typed outcomes with no wildcard arm that proceeds; retire passes the gated timeout constant (GATED_SAVE_TIMEOUT), never kind.timeout();")
    rep(spec, 'file_changes',
        "`Header { run_id: self.run_id, segment_no: self.segment_no, counters, authority }`",
        "`Header { run_id: self.run_id.to_owned(), segment_no: self.segment_no, counters, authority }`")
    rep(spec, 'approach',
        "so a degraded store costs a run end up to about 30 s before the stop.",
        "so a degraded store costs a run end up to about 30 s before the stop (typically about 20 s for two Unavailable answers, 2 x 10 s plus 100 ms; the probe adds up to 10 s only after a slow Stale).")

    # ======================================================================= E5, E7, E8, E10 and the stale executor sentences
    rep(spec, 'risks',
        "(vanishingly rare, conservative)",
        "(conservative: a lost answer, never a duplicate; NOT vanishingly rare on a loaded session-core, because a few consecutive saves slower than the 2 s round timeout on a seal of up to 3 MiB make the shared one-shot probe miss its own earlier commit and fail the writer Failed(Stale); an optional info! line when an armed writer becomes Failed(Stale) with an ambiguous save in its history is the evidence for the owner's flip decision)")
    rep(spec, 'file_changes',
        "in the AMBIGUOUS arm (`Unavailable{ambiguous:true}`), `if !folds.is_fenced() { close_stray_park(..) }`",
        "in the AMBIGUOUS arm (`Unavailable{ambiguous:true}`), `if !(folds.is_fenced() && folds.marked_fold_may_be_stored()) { close_stray_park(..) }` (close exactly when the funnel's tombstone will not settle the seq: a loop with no marked fold stored makes no tombstone call)")
    rep(spec, 'approach',
        "`if !folds.is_fenced() { close_stray_park(..) }` in its ambiguous arm",
        "`if !(folds.is_fenced() && folds.marked_fold_may_be_stored()) { close_stray_park(..) }` in its ambiguous arm")
    rep(spec, 'file_changes',
        "its ambiguous arm closes only under `!folds.is_fenced()`;",
        "its ambiguous arm closes only under `!(folds.is_fenced() && folds.marked_fold_may_be_stored())`;")
    rep(spec, 'invariants',
        "A marked loop NEVER closes a row it did not write: after an ambiguous park save it does not close expected+1",
        "A marked loop with a stored marked fold NEVER closes a row it did not write: after an ambiguous park save it does not close expected+1")
    rep(spec, 'failure_matrix',
        "An UNMARKED loop closes expected+1 as S8-A does.",
        "An UNMARKED loop, or an armed loop with no marked fold stored (no tombstone will settle the seq), closes expected+1 as S8-A does.")
    rep(spec, 'risks',
        "acceptable, since after such a step the worker's own deadline has usually passed too.",
        "acceptable, since after such a step the worker's own deadline has usually passed too. The wall-clock half is exact only once the wall clock is corrected after a host or VM resume (in the window before the step both clocks read young, a zombie could still heartbeat and a Denied loop could pass the age rule); live stage c4b records the executor's wall clock right after the wake and compares it with the database's now().")
    rep(spec, 'tests',
        "docker pause keeps CLOCK_MONOTONIC running, so it cannot exercise the wall-clock half of the age);",
        "docker pause keeps CLOCK_MONOTONIC running, so it cannot exercise the wall-clock half of the age; record the executor's wall clock right after the wake and compare it with the database's now());")
    rep(spec, 'tests',
        "(the fake counts the heartbeats: none is sent)",
        "(the fake counts the heartbeats: none is sent after the previous loop top's)")
    rep(spec, 'risks',
        "a shared atomic mark that children publish to would remove it.",
        "a shared atomic mark that children publish to would remove it. (The wording now: a root loop with a stored marked fold stops at its next loop top once 780 s have passed since the root's own last accepted heartbeat, e.g. after a long delegation round, although the run is healthy; and a depth-1 child's loop-top heartbeat renews the ROOT run's obligation and can abandon the worker's crash-park pass if it lands between the claim and the recovery transaction, bounded by the child's budget and harmless to safety.)")
    rep(spec, 'risks',
        "(from a resume that dies between its heartbeat and its fence, or from a superseded loop whose ambiguous save left its writer Active)",
        "(from a resume that dies between its heartbeat and its fence, or from a depth-1 child of a superseded loop, whose heartbeat renews the ROOT run's obligation: a superseded armed loop with a stored marked fold is stopped at its own loop top before it can heartbeat)")
    rep_span(spec, 'approach',
        "it stops at its next loop top, and it can neither end the run",
        "the spare handles it.",
        "it stops at its next loop top, before its heartbeat (its own last accepted heartbeat is older than the window and a marked fold may be stored), so it neither renews the park's obligation nor resets its own age; it can neither end the run nor raise an approval (the tombstone is refused, Stale or Denied). It may waste read-only work. A depth-1 child of such a loop can still heartbeat the root run's obligation, which is harmless because the fold is a park and the spare handles it (R19d).")
    rep(spec, 'approach',
        "and it covers the Denied answer only: a transport failure of the tombstone stops the loop by itself (an unconfirmed tombstone never proceeds, at any age).",
        "at the exit funnel it covers the Denied answer only (a transport failure of the tombstone stops the loop by itself: an unconfirmed tombstone never proceeds, at any age), and at the loop top it stops a silent armed loop that may have a stored marked fold before it can heartbeat.")
    rep(spec, 'failure_matrix',
        "(a silent armed loop stops there by age alone, before it can heartbeat, whatever its writer's state)",
        "(a silent armed loop with a stored marked fold stops there by age alone, before it can heartbeat, whatever its writer's state)")
    rep(spec, 'risks',
        "now stops with no late answer and no receipt, where committed code appends the answer after the cancel;",
        "now stops with no late answer and no receipt, where committed code appends the answer after the cancel (only for a run that may have a marked fold stored: a direct first-round answer, or a run whose last stored fold is unmarked, makes no store call at its end and behaves as committed);")
    rep(spec, 'summary',
        "where committed code appended it;",
        "where committed code appended it (for a run that may have a marked fold stored; a direct first-round answer behaves as committed);")
    rep(spec, 'failure_matrix',
        "Cancel of a LIVE marked run: the loop's next save or its funnel gets run_ended",
        "Cancel of a LIVE marked run (one that may have a marked fold stored; a direct first-round answer appends as committed): the loop's next save or its funnel gets run_ended")
    rep(spec, 'slice',
        "DESIGN ONLY, revision 5,",
        "DESIGN ONLY, revision 7,")
    rep(spec, 'slice',
        "plus the compose file and the docs.",
        "plus the compose file, the docs, the DB-test harness script and its contract test, the execution-core contract tests, and an owner-visible CI workflow change.")

    # ======================================================================= consistency review: mediums
    rep(spec, 'steps',
        "a loop that meets a run that ended under it (a cancel, a worker terminalization, a duplicate dispatch) or that the store refuses (Denied) stops without a late answer or receipt; a marked loop that overran its credentials (its last round outlived the 60 s margin, ordinary for a delegation round) ends through recovery instead of an immediate failed receipt;",
        "a loop that meets a run that ended under it (a cancel, a worker terminalization, a duplicate dispatch) stops without a late answer or receipt (only when a marked fold may be stored: a direct first-round answer behaves as committed); a Denied answer stops the loop only when its bearer is not verifiably expired or its own last accepted heartbeat is 780 s old or older (a marked loop whose bearers expired a moment ago still writes the committed immediate failed receipt); a healthy armed loop with a stored marked fold and a round longer than 780 s stops at its next loop top; a user's Stop makes the stopped loop's stream end in a retryable agent_dispatch_unavailable frame (R4(f), R4(g));")
    rep(spec, 'approach',
        "or that the store refuses (Denied) now stops without a late answer or receipt",
        "now stops without a late answer or receipt (a Denied answer stops it only when its bearer is not verifiably expired or its own last heartbeat is 780 s old or older)")
    rep(spec, 'open_questions',
        "recovery instead of an immediate failed receipt for a marked loop that overran its credentials, and no answer",
        "recovery instead of an immediate failed receipt only for a marked loop that overran its credentials AND whose own last heartbeat is 780 s old or older (a recent one keeps the committed failed receipt), a stop at the next loop top for a healthy armed loop with a stored marked fold and a round longer than 780 s, a retryable agent_dispatch_unavailable frame on a user's Stop, and no answer")
    rep(spec, 'tests',
        "a pre_effect save answered Stale, Closed or Denied (lost ownership) holds the visible call (it is not dispatched, the model is told it was held back);",
        "a pre_effect save answered Stale, Closed or NotFound (lost ownership) holds the visible call (it is not dispatched, the model is told it was held back); a save answered Denied dispatches it (the zombie's own bearer is dead; R13);")
    rep(spec, 'failure_matrix',
        "A save answered Stale, Closed or Denied (ownership lost) holds the call;",
        "A save answered Stale, Closed or NotFound (ownership lost) holds the call; a save answered Denied dispatches it (the zombie's own bearer is dead, R13);")
    rep(spec, 'tests',
        "the stored fold is then unmarked with an open round (crash_reseal refuses OpenRound)",
        "the stored fold is then unmarked with an open round (crash_reseal refuses NotMarked: an open-round fold is never marked; OpenRound is reachable only for a fold built WITH a marker, see S8B-0's refusal tests)")
    rep(spec, 'failure_matrix',
        "a crash inside the round leaves an unmarked fold (OpenRound),",
        "a crash inside the round leaves an unmarked fold (crash_reseal refuses NotMarked),")
    rep(spec, 'file_changes',
        "The returned Refused(reason) is the label of mp_session_loop_crash_park_skipped_total (not_persistent, no_fold, not_active, kind, not_resumable, not_marked) or mp_session_loop_crash_park_refused_total (every other reason).",
        "The returned Refused(reason) is routed to one of three metric families by a pure function (skipped: not_persistent, no_fold, not_active, kind, not_resumable, not_marked; unreadable: key_unavailable, did_not_authenticate, unreadable, authority_digest; refused: everything else, run_mismatch included).")
    rep_span(spec, 'file_changes',
        "`mp_session_loop_crash_park_refused_total{reason}` for a candidate that carried a marker and was refused (every other store-level reason and CrashRefusal label:",
        "Both label sets are closed.",
        "`mp_session_loop_crash_park_unreadable_total{reason}` for the operational failures that hide what the fold was (key_unavailable, did_not_authenticate, unreadable, authority_digest); `mp_session_loop_crash_park_refused_total{reason}` for a candidate that carried a marker and was refused (everything else: ledger_not_empty, approval_exists, not_serializable, seal_failed, raced, and the marker-bearing CrashRefusal labels, run_mismatch included). All three label sets are closed.")
    rep(spec, 'amendments_to_prior_specs',
        "loses exactly three row ANCHORS to the async block's re-indent (the Site B ParkPoint row and two rows that passed by substring luck)",
        "loses two row anchors (Site B ParkPoint and Site A max_rounds) and the_park_resume_guards_go_red_when_the_code_they_guard_regresses loses one (the `while` row), three in all, to the async block's re-indent")
    rep(spec, 'tests',
        "the new guards and their mutation rows (crash_mark_violations, visible_call_violations, retire_violations, CRASH_RETIRE_BUILT_EXPECTED), the store_proofs Park entry",
        "the S8B-2 and S8B-3 guards and their mutation rows (crash_mark_violations, visible_call_violations, retire_violations, CRASH_RETIRE_BUILT_EXPECTED) are built in those steps and only re-run here; the store_proofs Park entry")
    rep(spec, 'tests',
        "(the store_proofs Park entry names it): refused with the committed non-persistent refusal",
        "(it lives in loop_checkpoint.rs's crash_park_db module, harness path loop_checkpoint::crash_park_db::crash_park_never_runs_..., with #[tokio::test] directly before the fn as store_proof_violations' attribute regex requires; the store_proofs Park entry names exact (file, fn) pairs): refused with the committed non-persistent refusal")
    rep(spec, 'tests',
        "the executor's zdr/waived no-marker tests",
        "the executor's zdr and waived no-marker tests (two separate fns in agent.rs's tests, a_zdr_run_never_marks and a_waived_route_never_marks, split out of the exclusion table so that each has an exact (file, fn) pair)")
    rep(spec, 'tests',
        "two hard-coded lists over every offered_tool_defs name plus tickets.create,",
        "two hard-coded lists over every name of the real offered set (base_tool_defs(false) plus tickets.create; the names below are examples, the builder derives the full lists from the tree),")
    rep(spec, 'tests',
        "the shell-class names, save_memory (WorkspaceWrite), browser_agent and the like, each with the keyword or rule that refuses it.",
        "book_shipment, publish_social_post, browser.act, save_memory (WorkspaceWrite) and the like (browser_agent and shell are in the delegated-leaf blocklist, not in the offered set), each with the keyword or rule that refuses it; read_subagent_result is Read-class but asks for consent on every posture and is listed with that reason.")
    rep(spec, 'file_changes',
        "the harness contract test's list of leak names gains the variable,",
        "the harness contract test gains SESSION_CORE_CRASH_PARK_TTL_SECONDS=60 and SESSION_CORE_CRASH_PARK_ENABLED=true in its `env -i` block (the variables the scrub is fed) as well as in its `leaked` list (adding a name only to the list passes whether or not the scrub regex was extended; the second is already matched by `SESSION_CORE_.*_ENABLED`, so it also proves that regex still covers it),")
    rep(spec, 'steps',
        "; optionally the run-park.ts comment that calls crash_interrupted reserved (a comment-only change).",
        "; no SPA file is touched (the run-park.ts comment that calls crash_interrupted reserved stays stale and is a follow-up for whoever changes the SPA).")
    rep(spec, 'amendments_to_prior_specs',
        "(optional comment-only change in S8B-5).",
        "(left as is: S8-B touches no SPA file).")
    rep(spec, 'file_changes',
        "each step lists the S8-A guard text it amends in the SAME commit:",
        "each step re-verifies the following S8-A guards and amends in the SAME commit only the places named in its own entry (the two terminalization park-spare guards in S8B-1, three mutation rows in S8B-3, store_proofs and switch_table in S8B-4):")

    # ======================================================================= consistency review: lows
    rep(spec, 'approach',
        "a payload park count at the cap, a missing prompt digest.",
        "a payload park count at the cap, a missing prompt digest, another run's fold (run_mismatch).")
    rep(spec, 'file_changes',
        "and the row on `if park_recovery_active() {` (the `spare_parked_run(` count rule stays 'exactly one' because the crash branch runs before the single spare call);",
        "(the `spare_parked_run(` count rule stays 'exactly one' and the `if park_recovery_active() {` row needs no change, because the crash branch runs before the single spare call, inside the same block);")
    rep(spec, 'amendments_to_prior_specs',
        "and its assert_ne!, and the `if park_recovery_active() {` row; the committed rule 'the spare is called once' stays true",
        "and its assert_ne!; the `if park_recovery_active() {` row needs no change and the committed rule 'the spare is called once' stays true")
    rep(spec, 'file_changes',
        "(closed site word: loop_top, exit, pause, unproven)",
        "(closed site word: loop_top, exit, pause, unproven; a stop decided by a Denied answer or by an unconfirmed tombstone reports `unproven` in place of its call site's word, and the stop-table test asserts it)")
    rep(spec, 'approach',
        "one info! line per run says whether marking is armed and, when not, the first failing fact",
        "one info! line per run (only when the switch is on, root loops only) says whether marking is armed and, when not, the first failing fact")
    rep(spec, 'open_questions',
        "Flip (6) last, owner-gated.",
        "(6) is last in the proposal, owner-gated; the owner may choose the other order.")
    rep(spec, 'summary',
        "No thread notice on a park (owner decision, listed).",
        "No thread notice is built in v1 (the question is the owner's, listed).")
    rep(spec, 'risks',
        "R19: a new user turn on the same thread while a crash park waits:",
        "R20: a new user turn on the same thread while a crash park waits:")
    rep(spec, 'verification_commands',
        "set N, M and K to the counts at the S8B-1 commit",
        "set N, M and K to the counts at the S8B-1 commit (the harness runs --include-ignored and counts every matching test, whereas CI's --ignored steps count only the DB tests, so the two sets of floors differ)")
    rep(spec, 'file_changes',
        "The new crash_park_db test module is placed AFTER `mod tests` (the cross-service pins scan everything before it, test_support included, and the module must not spell the string park:).",
        "The new crash_park_db test module is placed AFTER `mod tests`, so the cross-service pins, which scan everything before the first `mod tests {`, never see its text; the code that IS scanned and must not spell the string `park:` is crash_park_run and seed_marked_fold (in test_support, before `mod tests`).")
    rep(spec, 'steps',
        "and `cargo test -p execution-core --tests` once (other integration tests read agent.rs text) (all committed rows green, none edited).",
        "and `cargo test -p execution-core --tests` once, against a pass/fail baseline recorded at the base first (some integration targets may not build on Windows; other integration tests read agent.rs text) (all committed rows green, none edited).")
    rep(spec, 'steps',
        "- the compose rows (switch and TTL);",
        "- the compose rows (switch and TTL; the session-core switch row is held by compose_violations only once S8B-4's switch_table row exists, because SESSION_CORE_CRASH_PARK_ENABLED is not an is_loop_switch key, and nothing goes red earlier);")

    # ======================================================================= rename the 780 s window: it is an age window, not a fail-open decision
    rep_all(spec, "FAIL_OPEN_MARGIN_SECS", "AGE_MARGIN_SECS")
    rep_all(spec, "fail_open_window", "age_window")
    rep_all(spec, "fail-open window", "age window")
    return spec
