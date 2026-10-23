# F5-agent-run-tests progress

Unit: tests for the leftovers of WS2.C (control authority) and WS2.D (run registry,
sweeper, Retry-After, run_not_live). Tests only; no src changes.

## DONE (committed on quarryv2-next, local only)

- 2046644e test(quarry-edge): control authority ... (WS2.C)
  tests/agent_run_control.rs, tests/support/agent_harness.rs
- f853e1ad test(quarry-edge): run caps, the sweeper ... (WS2.D)
  tests/run_sweeper_integration.rs

Results at the last run (qtest, after the last edit):
- agent_run_control: 12 passed, 1 ignored (F5-1)
- run_sweeper_integration: 14 passed, 2 ignored (F5-2, F5-3)
- --lib: 341 passed (before the final harness edit; src unchanged by me)
- rustfmt --check clean on the three files; gitleaks clean on both commits.
- --ignored: F5-1, F5-2, F5-3 fail with the expected evidence.

## Left (not in scope or not done)

- Sweeper periodic loop (spawn_run_sweeper, off switch, interval): not exercised.
- Boot refusal of a misspelt QUARRY_EDGE__TAKEOVER_ENFORCEMENT (main.rs): not run.
- Postgres round trip for the receipt columns: not in these files (fd5d2a4c covers it).
- Cap and RSS numbers: not measured here.

Full notes: side-session-notes/wf-F5-agent-run-tests.md.

## Round 2 (review findings: one HIGH, one MEDIUM)

Status 15:50 2026-10-18. Tests only; no src file touched.

- MEDIUM (in-flight guard not driven through the step path): new active test
  a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run in run_sweeper_integration.rs.
  Edit applied (uncommitted). Not yet verified by a run.
- HIGH (tab select and close take no initiator, so an agent's call on a person's token is
  classed Human and passes the takeover gate): re-verified in tabs.rs:177-182 and 233-238
  (derive_initiator(&claims, None)). Fix belongs in src (out of scope for this tests-only unit).
  Test side: harness gains select/close recording (support/agent_harness.rs, additions only);
  active test a_persons_tab_select_and_close_during_a_takeover_are_admitted; ignored finding
  test F5-4 an_agent_labelled_tab_select_or_close_during_a_takeover_is_refused; module doc of
  agent_run_control.rs corrected. Not yet verified by a run.
- Queued qtest for the A state (run-a.log in scratchpad/F5r2) waited on the docker lock held by
  another agent since 15:26. Next: B and C edits, one qtest run per commit, then the mutation check
  in a copy (scratchpad/review-F5-agent-run-tests/mut/Quarry-v2 with its volume rev-mut-target).

### Round 2 milestones (16:20 2026-10-18)

- 9d88cbd7 test(quarry-edge): a step queued on the run lock is in flight through the step path
  (MEDIUM). Verified: run_sweeper_integration 15 passed, 2 ignored; rustfmt clean.
- dee8a977 test(quarry-edge): a person's tab select and close are admitted during a take-over
  (harness select/close recording added, additions only). Verified: agent_run_control 13 passed,
  1 ignored; run_sweeper_integration 15 passed, 2 ignored.
- 7b2ad43f test(quarry-edge): an agent's tab select or close during a take-over is open finding
  F5-4 (HIGH, src fix not in this unit). Ignored test shows (200, 200) vs expected (409, 409).
  Module doc corrected. Verified: agent_run_control 13 passed, 2 ignored; --ignored shows F5-1
  and F5-4 failing; run_sweeper_integration 15 passed, 2 ignored, --ignored F5-2 and F5-3 failing;
  --lib agent_routes 80 passed, run_limits 7, retry_after 5.
- Mutation check (step guard removed from step.rs, in a git-archive copy of HEAD at
  scratchpad/F5r2/mut2, volume rev-mut-target): queued behind the docker lock as of 16:20. See notes.
- Gateway check: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:951-985 passes
  body None for select and close, so a src-side label would also need the gateway to forward one.
- Mutation result (16:20): step guard removed in the git-archive copy: a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run FAILED (wait_until timeout, run_sweeper_integration.rs:87); an_action_in_flight_is_never_expired passed; 14 passed, 1 failed, 2 ignored. Details in wf-F5-agent-run-tests.md, Round 2.
- Open: F5-4 (HIGH) needs a src change in agent_routes/tabs.rs and the gateway's select/close body forwarding. Not done (tests-only unit).
- Next: final check of git status for owned paths, then StructuredOutput.
- Final verification at HEAD 7b2ad43f (copy scratchpad/F5r2/final, touched, volume rev-mut-target):
  agent_run_control 13 passed, 2 ignored; run_sweeper_integration 15 passed, 2 ignored; --lib
  agent_routes 80 passed; quarry-edge recompiled in that run. An earlier HEAD run failed on the
  step-guard test because of a stale mutated build (git archive mtime trap, see notes). Not a code
  result.
- Mutation re-run with a fresh build (scratchpad/F5r2/mut3, touched, quarry-edge recompiled): the
  new step-queued test FAILED with "timed out waiting for the queued step to count itself in flight";
  an_action_in_flight_is_never_expired passed; 14 passed, 1 failed, 2 ignored.
- ROUND 2 COMPLETE (16:40): commits 9d88cbd7, dee8a977, 7b2ad43f; status partial (F5-4 src fix not made, tests-only unit). No uncommitted changes in quarry-edge/tests.

## Round 3 (review findings: wait-helper failure message; WS in-flight guard uncovered; trailers)

- 3219ce1e test(quarry-edge): a queued step that is not in flight fails with its cause. wait_until became wait_for (returns bool); both step-queued tests assert with the cause and the observed in_flight. Verified: run_sweeper_integration 15 passed, 2 ignored; --ignored F5-2 and F5-3 still fail (lines 769, 818). rustfmt clean.
- Step 2 in progress: new active test a_socket_action_queued_on_the_run_lock_holds_the_run_past_the_backstop (WS action queued on the run lock, held past the 14 400 s backstop by the in-flight guard only, because the open socket is a viewer). Doc line of an_action_in_flight_is_never_expired updated. Verification run launched (scratchpad/F5r3/step2-run.log). Next: commit, then mutation copies (scratchpad/F5r3/mut) removing only the execute_ws_action guard, then only the step() guard.
- Trailers: the three earlier commits (9d88cbd7, dee8a977, 7b2ad43f) say Co-Authored-By Claude Haiku 5.5; history cannot be rewritten under this unit. New commits use Haiku 5.5 as the session attribution reminder requires (the workflow text said Sonnet 5.5; the model that wrote the work is Haiku 5.5).

- ROUND 3 COMPLETE (2026-10-18 evening): 3219ce1e (step-queued failure names its cause), 1afaacda (socket action in-flight test). Verified at 1afaacda: run_sweeper_integration 16 passed, 2 ignored; agent_run_control 13 passed, 2 ignored; --lib agent_routes 80 passed. Seeded regressions: WS guard removed -> new socket test fails with its message (run_sweeper_integration.rs:599); step guard removed -> step-queued test fails with its message (:538); before 1afaacda, WS guard removal left the suite green. Trailers on 9d88cbd7, dee8a977, 7b2ad43f NOT rewritten (history rewrite forbidden); new commits carry Haiku 5.5. Details: wf-F5-agent-run-tests.md, Round 3.
- Round 3 repeat: run_sweeper_integration run three times in the worktree at 1afaacda, 16 passed and 2 ignored each time (no flake seen). Worktree status for quarry-edge: clean.
