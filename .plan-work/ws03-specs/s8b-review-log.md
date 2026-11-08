# S8-B design: adversarial review log

Spec: `.plan-work/ws03-specs/s8b-v1.json` (copy of this log: `.plan-work/ws03-specs/s8b-review-log.md`).
Every round used fresh reviewers who were given the spec, the repo and the five defects, not the author's conclusions. Full per-reviewer findings of round 4 and 5 are in the files named below (this folder). Rounds 1 to 3 outputs were delivered only as task notifications in the design session; what is kept here is what the later revisions recorded.

## Rounds 1 to 3 (revisions 1 to 4 of the design, before the file existed in this form)

Each round found at least one critical or high defect; all were fixed. The ones that shaped the design:
- Splitting `run_parkable` / writing the S8-A park keys on every boundary fold turned committed S8-A guards and mutation rows red. Fixed: one payload key (`crash_parkable`) and an independent `run_crash_parkable`; `run_parkable` and `park_fold` untouched.
- A zombie executor can still write a terminal receipt: the heartbeat and receipt use fresh service tokens, only fold saves and AppendMessage use the (expired) delegated bearer. Fixed: the exit funnel fences every outcome that reaches `finalize`, typed failures included.
- `SelfClosed` skipped the fence; the async-block re-indent broke committed mutation rows; the `visible_hold` closure captured the writer; S8B-2 needed symbols that landed in S8B-3. Each fixed in the text and the step order.
- The v4 thread-watermark / `loop_checkpoint_superseded` machinery was dropped for a tombstone ordered through the fold's own seq: no clock comparison, no thread lock, no migration.
- Round 3's two highs: the fail-open hole for a silent zombie (answered by the executor-local heartbeat-age rule) and a marker that did not mean park-resumable at rest (answered by `crash_reseal` shared with the executor's oracle test and the tool-names proof from the real allowlist).

## Round 4 (revision 4), three reviewers

Files: `s8b-review-r4-executor.md`, `s8b-review-r4-session-core.md`, `s8b-review-r4-rollout.md`.
Result: no critical. Three high.

| # | Reviewer | Sev | Finding | Disposition (revision 5) |
|---|---|---|---|---|
| E1 | executor | high | The fail-open decision bounds when finalize STARTS, not when its append LANDS: one partition can fail the tombstone and hang the append past a resume, giving a second answer (R3 mis-described it). | Fixed: the retire fence is fail closed. An unconfirmed tombstone (transient failure after one retry, an unconfirmed Stale, ContentRefused) stops the loop at any age. Only Denied (expired bearer: the append cannot land either) uses the age rule. NotFound is lost ownership. R3/F23/F29 rewritten. The cost (a degraded session-core at run end costs the run its answer, never a duplicate) is stated and listed for the owner. |
| R1 | rollout | high | `cargo fmt --all -- --check` is red at the base (2854 hunks, 112 files): the S8B-3 gate cannot pass, and a wholesale fmt would rewrite every guard anchor. | Fixed: per-file baseline of `Diff in` counts, never `cargo fmt --all`; owner note (R19e). |
| R2 | rollout | high | The heartbeat-age rule cannot be tested through the loop (`Instant`, refreshed every round) and nothing pins its wiring. | Fixed: `BeatMark` + `beat_age` with a `#[cfg(test)]` seam; `retire_violations` rows pin init, assignment before the heartbeat, `park::beat_age(&last_beat)` at both sites, and heartbeat-after-deadline order, with mutation rows; in-loop tests through the seam. |
| E2 | executor | med | retire's three retries vs SeqTracker's single probe: own ambiguous commits become a false `Failed(Stale)`. | Fixed: one retry, gated timeout; the outcome (stop, lost answer, never a duplicate) is documented and tested. |
| E3, S5, R3 | all three | med/low | `Instant` does not count a VM/host suspend. | Fixed: age is the larger of monotonic and wall-clock elapsed. Live-proof stage c4b (whole-stack suspend). |
| E4 | executor | low | Soundness depends on loop-top order (deadline check before heartbeat). | Fixed: order guard in `retire_violations`. |
| E5 | executor | low-med | The 900 s pin ties source trees, not deployed images; `renewed_until` is discarded. | Documented (R19f, F46) and listed as follow-up H4. |
| E6 | executor | low | `Closed` from a drift-close kills a live loop's answer; NotFound is definitive. | R5 note; NotFound moved to lost ownership. |
| E7 | executor | low | A superseded loop with a valid bearer and a transient pre_effect failure can land a late `plan.propose`. | Accepted, documented in R13. |
| E8, E9 | executor | low | `effectful_calls` reuse feeds the held-effects metrics; the `matches!` pattern does not compile. | Fixed: separate `visible_calls`; `matches!(&outcome, ...)` with an annotated async-block result. |
| E10, E11 | executor | low | Retryable error frame on a user Stop; children's heartbeats do not refresh the root's mark. | Documented (R4(f), R19g). |
| S1 | session-core | med | `every_refusal_ends_the_run_as_committed` cannot pass for requested/granted approval rows (the worker never reaches them). | Fixed: table reduced to reachable rows; separate assertion that an unconsumed approval is spared. |
| S2 | session-core | med | S8B-1 verify omits the two contract tests that scan session-core source; the spare statement change is not dormant. | Fixed: both added to the verify line; "dormant in behaviour, not in SQL" stated; lead's harness run required. |
| S3 | session-core | low | The committed rule "the spare is called once" would go red with two calls. | Fixed: the crash branch runs before the single spare call. |
| S4 | session-core | low | A sibling `crash_park_db_tests` module cannot reach `park_recovery_db_tests`' private helpers. | Fixed: tests are appended inside `park_recovery_db_tests`. |
| S6 | session-core | low | A worker-made park is indistinguishable in the audit event; the worker opens ciphertext. | Accepted, R19h. |
| R4 | rollout | med | Kill order written for compose env only; no drain query. | Fixed: new step "HOW EACH ENVIRONMENT FLIPS AND KILLS" (dev carry-over, production signed artifacts, drain query, checklist). |
| R5 | rollout | med | DB verification bypasses the harness; the harness env scrub misses the TTL variable. | Fixed: harness invocation with count floors; scrub, leak list and recorded count entry in file_changes. |
| R6 | rollout | med | The worker-level tests never run in CI; compose-only edits are not gated. | Proposed (owner-visible) CI filter and path-filter change in file_changes; also listed as an owner question. |
| R7 | rollout | med | Missing failure rows: session-core restart, key rotation, GDPR purge lock-order, gateway drain nuance. | Added F43-F45, F27 corrected. |
| R8 | rollout | med | `a_read_only_rung_refuses_every_call_the_ledger_classes_as_effectful` is a tautology. | Replaced by a golden-list test. |
| R9, R10 | rollout | low | Internal inconsistencies; diff against HEAD; unordered race test; invariant 10 overstated; retention window not listed. | All fixed or listed. |

## Round 5 (revision 5), two reviewers

Files: `s8b-review-r5-executor.md`, `s8b-review-r5-session-core.md`. Result: no critical. One high (narrow), several medium.

| # | Reviewer | Sev | Finding | Disposition (revision 6) |
|---|---|---|---|---|
| E1 | executor | high | A zombie that wakes after a host suspend (monotonic deadline does not fire) can heartbeat, refresh its own age, reach the funnel young and finalize over the crash park. | Fixed: the loop-top rule is the FIRST statement of the loop and stops an armed loop that may have a stored marked fold when its own last accepted heartbeat is at or past the 780 s window, whatever its writer's state, before the heartbeat. |
| E2 | executor | med | Fail-closed applies to runs that have no marked fold stored (a first-round answer is the dominant shape), where the stop protects nothing. | Fixed: `FoldWriter::marked_fold_may_be_stored` tracker; with none stored, `retire_fence` makes no RPC and cannot stop the loop (positive evidence still stops). |
| E3 | executor | med | The "Stale after an ambiguous attempt stops" row is unreachable: the shared SeqTracker probe resolves it. | Fixed: row and test variant replaced by the probe behaviour (Ok proceeds, a Stale probe is definitive). |
| E4-E10 | executor | low | NotPersistent unmapped; receipt landing is unbounded; NTP forward step; confirmed tombstone then failed approval creation; test-mock caveats; fmt baseline practicality, second LoopContext literal; retryable frame. | NotPersistent proceeds; R19j, R16, F29/R14, test text, fmt step, LoopContext note, R4(f) and open question. |
| S1 | session-core | med | Kill order names an order compose and the production artifact path cannot express (both halves of the resume lane are one host variable); drain query not attached to the S8-A kill steps. | Fixed: kill order stated as a condition (lane stays on until (6) is off and the drain query reads 0); drain query is a gate for every step that takes the lane away. |
| S2 | session-core | med | CI is not a backstop: the `rust` job stops at the red formatting step. | Fixed: backstop claims removed, R19e rewritten; Verify lines extended (S8B-1 harness contract test, S8B-2 cross-service contract and `--tests`). |
| S3 | session-core | med | The golden-list test contradicts itself for `tickets.create` and argument-dependent tools. | Fixed: two lists (runs / refused), derived from the real offered set. |
| S4-S11 | session-core | low | "status consumed" is not an approvals status; fmt command returns one total; metric labels not assertable; AAD assertion location; recorded count circular; compose comments pre-decide the order; checklist gaps; comment needles in the park-spare guard. | All fixed or documented in revision 6/7. |

## Round 6 (revision 6), two reviewers

Files: `s8b-review-r6-executor.md`, `s8b-review-r6-consistency.md`. Result: no critical. Four high, all spec-versus-code buildability defects, no design hole.

| # | Reviewer | Sev | Finding | Disposition (revision 7) |
|---|---|---|---|---|
| C1 | consistency | high | `make_interval(secs => CASE WHEN park_reason = $5 THEN $4 ELSE $2 END)` does not type-check (the parameters lose their typed use, Postgres resolves the CASE as text). Would stop every terminalization while park recovery is on. | Fixed: explicit `::text` / `::float8` casts in the statement, the guard re-pin text and the test; the harness run of park_recovery_db_tests is the first gate of S8B-1. |
| C2 / E1 | consistency + executor | high | The tracker cannot be built as specified: `Snapshot` has no round/pending, the wrappers take a `LoopFoldV1`, the SeqTracker is advanced in three places, `pause_fold` does not save, `park_with` has a NotAccepted outcome. | Fixed: `record_save` at the three tracker sites (save_bytes, fence_with, park_with), `carried` read from the fold via `crash_mark_in`, NotAccepted clears, open_round_fold passes `crash_mark: None`, steps and tests updated. |
| E2 | executor | high | F35 and the visible-call test say Denied holds a visible call; `lost_ownership` excludes Denied. | Fixed: F35 and test say Stale/Closed/NotFound; Denied dispatches (R13). |
| E3 | executor | med | "Denied means expired bearer" is an assumption (R19g admits an earlier blip); a valid-bearer Denied could proceed and duplicate. | Fixed: Denied proceeds only for a verifiably expired bearer (`session_bearer_expired`) with a young age. |
| E4 | executor | med | retire's gated 10 s timeout is not what `save_tombstone_as` does (`kind.timeout()` is 2 s). | Fixed: `save_bytes` gains a `timeout` parameter; guard and test added. |
| C3-C13, E5-E10, C14-C25 | both | med/low | Stale paragraphs (a silent loop "heartbeats"), flip-effect lists, OpenRound vs NotMarked, metric families, amendments count, guard placement, golden list names, harness contract test passes vacuously, SPA comment edit, wording leaks, labels, builder details. | All fixed in revision 7; the 780 s constant renamed `age_window` / `AGE_MARGIN_SECS` so no name says "fail open". |

## Round 7 (revision 7), one targeted verification reviewer

File: `s8b-review-r7.md`. Scope: verify each round-6 fix against the code (SQL typing, the tracker's three save sites, the Denied rule's helpers, the timeout plumbing) and sweep for contradictions.
Result: **no critical, no high.** One medium, ten low. All five fixes work against the code: the three tracker sites cover every fold save with the fold unconsumed at each; the SQL is valid with a consistent bind order and exactly the needles and mutation row the spec names break; `bearer_expiry` and `now_epoch_secs` exist with the stated semantics; the bearer passed is the one `finalize` appends with.

| # | Sev | Finding | Disposition (revision 8) |
|---|---|---|---|
| 1 | med | The retire-timeout test sits in fold.rs tests where there is no seam, Recorder or precedent for paused time. | Moved to the agent tests: the Recorder's save handler gets a test-only `delay`, REAL time with a 3 s delay (between the 2 s and 10 s timeouts); source guard kept. |
| 2 | low | The verifier accepts a bearer for a 30 s leeway after `exp`. | `session_bearer_expired` uses `exp + VERIFIER_LEEWAY_SECS (30)`; a larger configured leeway needs the constant raised. |
| 3, 4 | low | `&ctx.session_bearer` does not type-check (it is `Option<&str>`); the CrashGate must exist before the park-resume fence. | `ctx.session_bearer.is_some_and(park::session_bearer_expired)`; gate built after `park_gate`, before the fence block. |
| 5 | low | The "casts REQUIRED" rationale is false under sqlx 0.8 (declared parameter types); a mutation row depends on line layout; `park_reason` can be NULL. | Rationale softened to belt-and-braces; layout and NULL-decode notes added; the harness run stays the gate. |
| 6 | low | `record_save`'s outcome type unspecified; `park_with` needs a restructure; guard-word list was half. | Pure `marker_after(prev, carried, MarkerOutcome)`; `let outcome = match ..; record_save; outcome`; all four forbidden words named. |
| 7a-g, 8 | low | Wording residue (Denied exception without the bearer condition, "third argument", flip (5) qualifier, F36 order, R19g double sentence, stop-table site word, ContentRefused wording, append-failure injection), step lists under-listing, unnamed test modules, ZDR/waived fns. | All fixed. |
| 9 | low | One early Denied blip in a tool-using run costs the whole run's answer. | R19k; the optional single-tombstone mitigation is recorded, not built. |
| 10 | low | Failure injection needs a list of entries, not `times: 2`. | Stated in the test text. |

Revision 8's edits are mechanical corrections of those findings and were not put through a further review round.

## Final state

- Spec: `.plan-work/ws03-specs/s8b-v1.json`, revision 8, valid JSON, 13 sections. Reproducible from `.plan-work/ws03-specs/s8b-build/` (`build_spec.py <out>` applies the part files and revisions 5 to 8).
- 7 review rounds (2 to 3 reviewers each in rounds 4 to 6), about 200 anchored edits. Last two rounds: no critical, no high.
- Repo: only the untracked `.plan-work/`; no tracked file edited; nothing committed; docker never run.
