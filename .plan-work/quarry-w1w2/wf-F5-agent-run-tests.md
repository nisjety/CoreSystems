# F5-agent-run-tests: hand-over notes (2026-10-18)

Unit: integration tests for the leftovers of WS2.C (control authority) and WS2.D (run
registry, sweeper, Retry-After, run_not_live hints). Tests only; no src file changed.
Branch quarryv2-next, local commits only, nothing pushed.

## Committed

- 2046644e test(quarry-edge): control authority over a live agent run, through the real
  router (WS2.C). Files: tests/agent_run_control.rs (new),
  tests/support/agent_harness.rs (new; the real router on loopback, a recording driver
  double, golden-claim token helpers, a minimal RFC 6455 client).
- f853e1ad test(quarry-edge): run caps, the sweeper and the released-run hints, through
  the real router (WS2.D). File: tests/run_sweeper_integration.rs (new).

Not touched: support/mod.rs (unchanged, the harness is declared by #[path] from the two
test files), every src file, every other agent's uncommitted file.

## Verified (qtest.sh, shared Docker lock)

- agent_run_control: 12 passed, 1 ignored (F5-1).
- run_sweeper_integration: 14 passed, 2 ignored (F5-2, F5-3).
- Repeated four times, plus once after the last harness edit: all green, both binaries.
- cargo test -p quarry-edge --lib: 341 passed, 0 failed (includes the agent_routes,
  run_limits and retry_after unit tests).
- --ignored: all three findings fail with the evidence below.
- rustfmt --check clean on the three owned files (host rustfmt 1.9.0; support/mod.rs was
  checked and was already clean, so rustfmt changed nothing outside the owned files).
- gitleaks pre-commit gate: no leaks in both commits.

## Findings

Each is an #[ignore] test with a "finding F5-n" reason. Un-ignore once the code agrees.

- F5-1 (agent_run_control: control_timeline_items_carry_initiated_by_under_the_documented_name).
  A control timeline item serialises `initiated_by`, not `initiatedBy`. Cause:
  agent_routes/types.rs, BrowserTimelineItemDetail::Control (about line 275-280) has no
  serde rename on initiated_by; the Tab and Action variants do. The plan (WS2.C) says
  timeline items carry initiatedBy. Evidence (--ignored): item JSON has
  "initiated_by":"human" and no initiatedBy (left Null, right "human").

- F5-2 (run_sweeper_integration: a_resumed_agent_run_keeps_the_agent_lifetime). A resumed
  agent run is swept with the interactive class. Cause: lifecycle.rs:248 classifies the
  run from the request alone (classify(claims.is_service(), body.initiated_by)) and
  lifecycle.rs:386 builds RunLive from that class. The budget does keep the stricter of
  stored and derived (budget.rs:676-678, and the module doc at the top of budget.rs),
  so the budget and the sweeper disagree. Same function, lifecycle.rs:386, takes the
  explicit runtime from body.constraints.max_runtime_s, although lifecycle.rs:99-102 says
  the resumed constraints are authoritative. Evidence (--ignored): a resumed run idle for
  120 s with a 60 s agent limit is not released (left 0, right 1). The plan's tighten-only
  rule (R-15) is what the test expects.

- F5-3 (run_sweeper_integration: a_step_queued_behind_an_expiry_gets_the_resume_hint).
  A step (and, by the same code, a tab call) that is waiting for the run's lock when the
  sweeper closes the run gets 404 run_not_live with details {reason} only. Cause:
  admission.rs:350-353 (admit_action, "1. closed") and admission.rs:431-434
  (admit_tab_operation) do not add resumable/resume_run_id; the released-run path
  (types.rs run_gone_err) does. The plan (WS2.D) asks for both on a step or tab call on a
  dead run. Evidence (--ignored): body {"error":{"code":"NOT_FOUND","details":{"reason":
  "run_not_live"},...}}, no resumable.

## Observations (not tests; for the reviewer)

- O1. A service principal cannot step or open a tab on a person's run: caller_owns_browser_run
  (control.rs:29-37) matches the actor that started the run, and a service's actor is its
  own. The brief's scenario ("the agent, a service token, steps while the person has
  control") is therefore not reachable on this code. The agent path is the person's own
  token labelled initiated_by/actor "agent" (what the gateway sends). Pinned by
  a_service_principal_cannot_drive_a_persons_run (404, no receipt).
- O2. The ZDR timeline read answers 409 with error.code BAD_REQUEST: projection.rs:280-286
  uses status_err, which always carries ErrorCode::BadRequest. The status and the code
  disagree. The first draft of the ZDR test asserted 200 and failed on this; the test now
  asserts the refusal.
- O3. sweep_once counts a run as expired when finish_run returns Ok, and finish_run returns
  Ok without releasing when the run is already closed (lifecycle.rs:494-496;
  run_sweeper.rs counts Ok as expired and logs "run expired"). A DELETE that races the
  sweeper therefore overstates the sweep report. Releases stay exactly once
  (two_overlapping_sweeps_release_an_idle_run_once).
- O4. WebSocket error frames carry the QuarryError Display, e.g. "UserTookOver: the person
  using the browser has taken control...", not the wire code USER_TOOK_OVER used on REST.
  Every WS error frame does this (ws.rs sends err.to_string()). Pinned loosely by the
  socket test (starts_with "UserTookOver:").
- O5. Tabs do not hold an in-flight guard (tabs.rs new_tab takes the entry lock without
  in_flight_guard). A tab queued behind an expiry hits the same F5-3 gap; not tested.

## Not done

- The sweeper's periodic loop (spawn_run_sweeper, the QUARRY_EDGE__RUN_SWEEPER=0 switch,
  the interval) is not exercised; the tests call sweep_once with an injected clock.
- The boot-time refusal of a misspelt QUARRY_EDGE__TAKEOVER_ENFORCEMENT (main.rs) is not
  run; the parser is tested (the_takeover_switch_is_off_unless_asked_and_a_typo_is_named).
- The Postgres round trip for the new receipt columns is not in these files (fd5d2a4c
  covers it, DATABASE_URL-gated).
- Cap numbers and RSS figures are not measured here (WS2.D's own deployment evidence).

## Traps found

- The shared Docker lock can queue a qtest for a long time behind other agents (it waited
  about ten minutes once). Check the lock holder (scratchpad/locks/docker.d/holder) before
  assuming a hang. I cancelled my own queued run (identified by its start time and its
  process chain, while another agent held the lock) so that I could edit first.
- `cargo test` stops at the first failing test binary. Use --no-fail-fast; the previous
  agent's run never reached run_sweeper_integration because of this.
- Inside qtest.sh the mounted root is the Quarry-v2 workspace (/src), so paths are
  crates/quarry-edge/... there.
- The pre-commit gitleaks gate flags a literal base64 Sec-WebSocket-Key as
  generic-api-key. The socket client now encodes its nonce in code. To see the rule,
  pipe `git diff --cached` into the same gitleaks image with --verbose --redact through
  with-lock.sh.
- AgentConstraints.max_steps has no serde default, so a body with only max_runtime_s
  fails to deserialise; send "max_steps": 0 too.
- The AppState (and so the registry) is per test; the takeover switch is a process-wide
  env var, so every #[tokio::test] in agent_run_control.rs is #[serial] (checked).
- live_frame and full_visual_fidelity are needed for the frames WebSocket route; the
  harness double sets both (it is the only way to reach the socket without a real
  browser).

## Decisions

- Findings are #[ignore] tests, so a plain `cargo test` stays green and the failure is one
  `--ignored` away. They are not deleted and not bent to match the code.
- Commit trailer: Co-Authored-By: Claude Haiku 5.5, per the session's attribution
  reminder. The workflow text asked for Sonnet 5.5; the reminder is the one this session
  was given, so it was followed. Easy to change if the orchestrator disagrees.
- No plan doc or RUNBOOKS edits: the unit is tests only.

## Round 2: review findings (2026-10-18, afternoon)

Two findings came back from review of 2046644e and f853e1ad. Commits, all local, none pushed:

- 9d88cbd7 test(quarry-edge): a step queued on the run lock is in flight through the step path
  (MEDIUM). New active test a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run in
  run_sweeper_integration.rs. It holds the run's lock, sends a step that queues behind it, waits
  until handle.live.in_flight() == 1, checks the verdict and a sweep both leave the run alone,
  releases the lock, and checks the next sweep expires the run. The old direct-guard test stays.
- dee8a977 test(quarry-edge): a person's tab select and close are admitted during a take-over
  (test gap from the HIGH finding). The harness RecordingDriver now implements select_tab and
  close_tab and records the tab ids (additions only). New active test
  a_persons_tab_select_and_close_during_a_takeover_are_admitted.
- 7b2ad43f test(quarry-edge): an agent's tab select or close during a take-over is open finding
  F5-4. Ignored test an_agent_labelled_tab_select_or_close_during_a_takeover_is_refused. The module
  doc of agent_run_control.rs is corrected. Its commit message carries the correction to 2046644e.

### F5-4 (HIGH, OPEN, needs a src change outside this unit)

Tab select and close are not refused for an agent on a person's token during a take-over.
- Code: agent_routes/tabs.rs select_tab and close_tab (lines ~177-182 and ~233-238) call
  derive_initiator(&claims, None). Neither route takes a body or an initiator. A person's token is
  classed Human, so admit_tab_operation passes. new_tab takes initiatedBy in its body; select and
  close do not.
- Gateway: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:951-985 (select_tab and
  close_tab) call tab_mutation with body None. A src fix that reads a label also needs the gateway
  to forward one.
- Evidence (the ignored test, run with --ignored): left (200, 200), right (409, 409). Both calls are
  admitted and reach the driver.
- The admission comment (admission.rs, tab gate, "an agent cannot do them during a takeover") states
  the intent the code does not meet.
- Suggested fix (not done here, the unit is tests-only): take initiatedBy on select and close the way
  new_tab does (or a query parameter), have the gateway forward it, then make the ignored test active
  and add the same check for a service token.

### Correction to 2046644e

Its message says every agent action during a take-over gets 409 USER_TOOK_OVER. That is false for
tab select and close (F5-4). The message cannot be amended under this unit's rules, so 7b2ad43f's
message carries the correction.

### Mutation check for the MEDIUM finding

Seeded in a git-archive copy of HEAD (scratchpad/F5r2/mut2/apps/Ingestion Plane/Quarry-v2; the
worktree was not touched). The line `let _in_flight = handle.live.in_flight_guard();` was removed
from step() only (execute_ws_action keeps its guard). Target volume rev-mut-target (the reviewers'
volume for their copy).
Run: cargo test -p quarry-edge --features browser-agent --test run_sweeper_integration --no-fail-fast.
Result: a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run FAILED (panicked at
run_sweeper_integration.rs:87, the wait_until timeout, "timed out waiting for the queued step to
count itself in flight"). an_action_in_flight_is_never_expired still passed, as expected: it calls the
guard directly. Totals 14 passed, 1 failed, 2 ignored. On the real tree the same binary is 15 passed,
2 ignored.

### Verified (qtest, quarry-rust-env:1, default features)

- run_sweeper_integration: 15 passed, 0 failed, 2 ignored (F5-2, F5-3) at 9d88cbd7 and again at 7b2ad43f.
- agent_run_control: 13 passed, 0 failed, 2 ignored (F5-1, F5-4) at 7b2ad43f.
- --ignored: F5-1 left Null right "human"; F5-4 left (200, 200) right (409, 409); F5-2 left 0 right 1;
  F5-3 resumable absent from the 404.
- --lib agent_routes: 80 passed; --lib run_limits: 7 passed; --lib retry_after: 5 passed.
- rustfmt --check (host rustfmt 1.9.0) on tests/run_sweeper_integration.rs and tests/agent_run_control.rs
  (the latter covers its child modules, including the harness): clean.
- No src file changed.

### Traps found in round 2

- Do not put a trailing & inside the command string given to run_in_background. The wrapper returns
  at once and the run is detached, which is harder to track. Use run_in_background with no &.
- qwait.sh blocks up to its argument (max 570 s). Give the Bash call timeout 600000, or the tool moves
  the wait to the background and returns early.
- The docker lock was held by other units' runs for about 25 minutes, then about 15 minutes. Queue the
  verification run as early as possible.
- git archive HEAD gives a clean copy for a mutation. Do not cp the worktree: it carries other agents'
  uncommitted edits, and its target/ dir is not part of the source.
- Reusing a target volume across source trees is only safe if the copy's files are newer than the
  build outputs. git archive writes the COMMIT time as each file's mtime, so a fresh copy can look older
  than an earlier mutated build in the same volume: cargo then reuses the mutated quarry-edge. The first
  HEAD run failed the same way (step guard test, wait_until timeout), which was an artifact of this.
  Fix: find . -type f -exec touch {} + in the copy, then run. cp and tar keep the mtimes they copy.
- node edit scripts make the Edit and Write tools warn "modified ... Call Read before editing" on the
  next use. This is expected. Nothing is lost.
- The attribution trailer: the workflow text asks for Co-Authored-By Claude Sonnet 5.5, but the
  session's attribution reminder says Haiku 5.5. Round 2 uses Haiku 5.5, the same as round 1.

### Addendum (end of round 2)

- The plan supports F5-4. docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS2.C detail",
  Contracts paragraph: optional initiated_by on /step, /interact, tab bodies (initiatedBy) and the
  WebSocket message. Select and close have no body, so they cannot meet that contract as written.
- Mutation re-run with a fresh build: scratchpad/F5r2/mut3 (quarry-edge recompiled in that run). The
  new step-queued test FAILED with "timed out waiting for the queued step to count itself in flight"
  (run_sweeper_integration.rs:87). an_action_in_flight_is_never_expired passed. 14 passed, 1 failed,
  2 ignored. This is the confirmed result; the mut2 run gave the same failure.
- Final HEAD verification (7b2ad43f, copy scratchpad/F5r2/final, touched): agent_run_control 13 passed,
  2 ignored; run_sweeper_integration 15 passed, 2 ignored; --lib agent_routes 80 passed.

## Round 3: review findings (2026-10-18, evening)

Commits, all local, none pushed. Tests only: the only file changed is
tests/run_sweeper_integration.rs (agent_run_control.rs and support/ are untouched in this round).

- 3219ce1e test(quarry-edge): a queued step that is not in flight fails with its cause (WS2.D review).
  wait_until became wait_for, which returns whether the condition came true. Both step-queued tests
  assert the result with the cause and the observed in_flight. Before, a regression failed with
  "timed out waiting for the queued step to count itself in flight" at the helper (line 87).
- 1afaacda test(quarry-edge): a socket action queued on the run lock holds the run past the backstop
  (WS2.D review). New active test a_socket_action_queued_on_the_run_lock_holds_the_run_past_the_backstop.
  It uses the existing TestSocket (no harness change). The open socket is a viewer: a viewer blocks the
  idle limit but not the 14 400 s backstop, so the test sweeps at later(14_500_000) and only the
  in-flight guard keeps the run. Doc line of an_action_in_flight_is_never_expired points at it.

### Findings

1. Failure message of the step-queued test (medium, real): fixed in 3219ce1e. A regression now fails with
   "a step queued on the run lock must count itself in flight before it waits for the lock (step() takes
   the in-flight guard first); in_flight is 0" (mutated run: run_sweeper_integration.rs:538).
2. execute_ws_action's in-flight guard not covered (medium, real): fixed in 1afaacda. Removing only that
   line, on the tree before 1afaacda, left the suite green (checked after the commit, see below). Now the
   new test fails with its own message (mutated run: run_sweeper_integration.rs:599).
3. Trailers (medium): NOT changed. 9d88cbd7, dee8a977 and 7b2ad43f carry "Co-Authored-By: Claude Haiku 5.5".
   Changing them needs a history rewrite (rebase or amend), which this unit forbids. The workflow text asked
   for Sonnet 5.5, but the session's attribution reminder asks for Haiku 5.5, and the work was done by
   Haiku 5.5. The two new commits (3219ce1e, 1afaacda) carry Haiku 5.5. Whether to rewrite is the
   orchestrator's call.
4. "No functional regression found" (informational): no action.

### Verified (qtest, quarry-rust-env:1, default features)

- 3219ce1e: run_sweeper_integration 15 passed, 2 ignored. --ignored: F5-2 fails at :769 (left 0, right 1),
  F5-3 fails at :818 (left Null, right true). rustfmt --check clean.
- 1afaacda (tests at HEAD): run_sweeper_integration 16 passed, 2 ignored; agent_run_control 13 passed,
  2 ignored; --lib agent_routes 80 passed. --ignored: F5-2 at :836 (left 0, right 1), F5-3 at :885
  (left Null, right true). rustfmt --check clean. gitleaks pre-commit: no leaks for both commits.
- Seeded regressions, each in a git-archive copy of HEAD (scratchpad/F5r3/mut, script mut.sh, one guard
  removed per run, target volume rev-mut-target, copy touched first):
  - M1, execute_ws_action guard removed (step.rs:155): run_sweeper_integration 15 passed, 1 failed:
    a_socket_action_queued_on_the_run_lock_holds_the_run_past_the_backstop, at :599, with the socket
    message above. The step test still passed.
  - M2, step() guard removed (step.rs:20): 15 passed, 1 failed: a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run,
    at :538, with the step message. The socket test still passed.
  - Before the socket test (copy of 3219ce1e, scratchpad/F5r3/mut-prev, execute_ws_action guard removed):
    agent_run_control 13 passed, 2 ignored; run_sweeper_integration 15 passed, 2 ignored; --lib agent_routes
    80 passed. This confirms the gap that 1afaacda closes.

Exact commands: `bash <scratchpad>/tools/qtest.sh 'cargo test -p quarry-edge --test run_sweeper_integration --no-fail-fast'`
(same with --test agent_run_control, `--lib agent_routes`, and `-- --ignored`); mutation runs with
`QROOT=<copy>/apps/Ingestion Plane/Quarry-v2 QTARGET=rev-mut-target bash qtest.sh 'bash mut.sh'`.

### Traps found in round 3

- Within a run_in_background command, a trailing `&` or a nohup'd child returns at once; the work
  continues, and its log stays empty until the first cargo pipeline ends. Check the docker lock holder
  (scratchpad/locks/docker.d/holder) to see that the run is yours.
- A mutation copy must be `git archive <commit>` plus `find . -type f -exec touch {} +`. Without the touch,
  cargo can reuse artifacts of an earlier mutated build that shares the target volume.
- The per-unit progress file changed on disk during this round (another writer). Append to it; do not
  rewrite it.
- A commit message is a claim. "Removing the one line left the suite green" in 1afaacda was not checked
  before the commit; it was checked afterwards on the pre-socket-test copy (above). Check claims before
  committing.

### Still open (not done in this unit)

- F5-4 (round 2, HIGH): tab select and close take no initiator, so an agent's call on a person's token is
  classed Human during a take-over. Needs a src change in agent_routes/tabs.rs and the gateway's
  select/close body forwarding. The ignored test an_agent_labelled_tab_select_or_close_during_a_takeover_is_refused stays ignored.
- O5 (round 1): tabs have no in-flight guard (tabs.rs new_tab, select and close take the entry lock without
  one). A tab call queued behind an expiry is not covered by any test. Needs a src change.
- Trailers on 9d88cbd7, dee8a977 and 7b2ad43f (see finding 3).
- Unchanged from round 1: the sweeper's periodic loop, the boot refusal of a misspelt takeover switch
  (main.rs), the Postgres round trip for the receipt columns, cap and RSS numbers.
