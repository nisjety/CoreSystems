# W2-U68 progress (WS8.H, execution lanes; GA gate for ATTACH)

Status: BUILT to scope for the lane split. Test suites written and compile-checked (`cargo check -p quarry-edge --tests` rc 0), NOT run (build-first rule). No flag (a run without an attach has one lane). Browser-backed DoD evidence not taken.

## Committed (branch quarryv2-next, local only)

- `16fa15f39` feat(quarry-edge): execution lanes for a run (WS8.H). 13 agent_routes files, test_support.rs, tests/agent_evaluate_denied.rs, tests/web_policy_agent.rs.
- `f7b7fbe78` docs(quarry-v2): execution lanes in the RUNBOOKS (WS8.H). Patch-based: only the new section (9 lines). See the depth note on the working copy.

## Verified (what was run)

- `cargo check -p quarry-edge --tests` through qtest (quarry-rust-env:2): rc 0, no errors, after one fix (two missing imports in `tab_ledger.rs` and `test_support.rs`).

## Not run

- All seven lane tests named in the plan (written in `agent_routes/tests.rs`, end of file; lock-level).
- The deployment DoD: the 8 s delegate wait with a person's action returning under 1 s.
- The log-field lint, the ZDR feature matrix and the I5 allow-list table.

## Design (as built)

- `RunEntry` has `human_lane: LaneRef` (`Arc<TokioMutex<LaneState{ctx, last_observation}>>`) and `summary: LaneSummary` (current URL, page hash, successful step count, last observation). `RunEntry.ctx` and `last_observation` are gone.
- `AttachState.lane: LaneRef` is the delegate's lane. `RunLive.steps_reserved` is the run-wide step key (`reserve_step`, after admission; `with_resume_step` at start).
- Lock order: a lane, then the entry, never the reverse. A step releases the entry while the browser runs and re-takes it after. `finish_run` and the drain name the lanes while the entry is held, release it, then wait (5 s total, drain uses its own timeout).

## Hand-over traps (for the next batch)

- A `MutexGuard` from `handle.live.attaches()` must not live across an `.await` (fixed in `delegate_attach_of` and the delegate step; `live_mut()` block has no await).
- `attach.rs`, `close.rs`, `lifecycle.rs`, `run_live.rs`, `tab_ledger.rs` are LF in the working tree (the helper keeps CRLF files CRLF). Git warns on each add; that is normal here.
- The working-tree `docs/RUNBOOKS.md` carries another unit's uncommitted edits that do not include the committed W2-U68 section. Whoever commits that file must work from HEAD (or re-apply the section).
- Other units' uncommitted hunks remain in `auth.rs`, `lib.rs`, `main.rs`, `profile_guard.rs`, `profile_routes.rs`, `routes.rs`, `tests/profile_roundtrip.rs`, `tests/zdr_refusal_modes.rs`, `tests/support/*`; none were committed by this unit.
- Commit trailer: the session's attribution reminder asks for Claude Haiku 5.5; the workflow text asked for Sonnet 5.5. The commits carry Haiku 5.5.

## Next

- The browser-backed DoD and the seven test runs at the wave's test phase.
- Depth register: `.plan-work/quarry-w1w2/depth/W2-U68.md`.
