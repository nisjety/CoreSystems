# F3-matrix-gap-tests progress

Unit: behavioural tests for the ZDR feature matrix (leftover of WS4.G). Worktree: vigorous-perlman-e7ae5b, branch quarryv2-next.

## State at resume (2026-10-18, second agent)
- Commits from the first agent: none (39386dc5 is the WS4.G matrix commit, not ours).
- Uncommitted, ours: tests/zdr_gap_rows.rs (new, 7 tests), tests/support/zdr_harness.rs (start_with, get, golden_token, control_plane_double, model_plane_double), crates/quarry-core/src/zdr_matrix.rs (enforced_by of 5 gap rows), docs/ZDR_MATRIX.md, crates/quarry-runtime/contracts/zdr-feature-matrix.json and .sha256 (regenerated 06:43).
- First agent's run1 (scratchpad F3-matrix-gap-tests/run1.log, 06:45): quarry-core zdr_matrix 6/6, quarry-edge zdr_claim_floor 22/22, zdr_feature_matrix 7/7, zdr_gap_rows 7/7, zdr_log_capture 6/6. Re-verify before committing.
- Gaps found at resume: change_record_internal had no test and no enforced_by.
- scripts/tests/zdr-contract-sync.test.sh is tracked at HEAD and unmodified.

## Plan
1. Add gap_rows_currently_violate_change_record_internal (token check 401, declared zdr 501 same as undeclared).
2. enforced_by: change_record_internal, durable_jobs (+2), agent_persistent_profile (+1).
3. Regenerate golden (QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix) and run the four suites.
4. Commit: tests; matrix+golden; then GOLDENS entry in the sync script (after the golden is committed).

## Commands
- qtest: bash .../scratchpad/tools/qtest.sh '<cmd>' (lock, quarry-rust-env:1, CARGO_BUILD_JOBS=2).
- commits: bash .../scratchpad/tools/qcommit.sh <msg-file> <paths...>.

## Milestones (second agent, 2026-10-18)
- Built and ran everything through qtest (run2.log, exit 0 for all): quarry-core zdr_matrix 6 passed (golden regenerated with QUARRY_UPDATE_GOLDEN=1, then rerun 6 passed); quarry-edge zdr_feature_matrix 7, zdr_gap_rows 8, zdr_claim_floor 22, zdr_log_capture 6.
- Commit 80078994 test(quarry-edge): gap-row tests and harness (zdr_gap_rows.rs new, support/zdr_harness.rs).
- Commit ff1be0f6 test(quarry-core): enforced_by citations for the six gap rows (+ durable_jobs, agent_persistent_profile); ZDR_MATRIX.md, zdr-feature-matrix.json and .sha256 regenerated.
- Commit bb951ab5 test(contracts): GOLDENS entry for the matrix golden (digest aebe9ef0...6cc). zdr-contract-sync.test.sh: 50 passed, 0 failed; --self-test ok.
- Not done here: the plan's WS4.G list also names every_row_has_a_log_canary_or_is_marked_log_unverified and every_code_and_feature_the_matrix_names_is_in_the_closed_catalogue_list in zdr_feature_matrix.rs (not in this unit's owned paths; neither exists yet). Node test scripts/tests/zdr-contract-schema.test.mjs not run.
- Next: final git status check of owned paths, then StructuredOutput.

## Review round 2 (third agent, 2026-10-18): fixing HIGH 1-2, MEDIUM 3-5
Earlier commits 80078994, ff1be0f6, bb951ab5 are NOT amended (rules). Their messages overstate:
- ff1be0f6 says durable_jobs AND agent_persistent_profile both cite the two enforced-refusal tests. Wrong for agent_persistent_profile (it never cited enforced_refusals_hold_for_durable_jobs, correctly). Corrected in the commit below; the old message stays wrong.
- bb951ab5 says a matrix-table change that is not regenerated fails the repo-level check. Wrong: the sync script hashes the committed golden only; the table is guarded by quarry-core test the_golden_and_markdown_are_what_the_table_generates (quarry-v2-ci.yml runs cargo test --workspace). Corrected by the commit that adds the comment and the workflow trigger.
Plan and status:
1. DONE e1889655 test(quarry-edge): change routes pinned to 501 (both orgs); change_record_internal doc says it cannot observe the write. Verified qtest: zdr_gap_rows 8/0, zdr_feature_matrix 7/0.
2. DONE 264832dd test(quarry-core): service test split (a_service_principal_is_refused_durable_jobs_by_the_scope_gate; a_service_principal_with_body_zdr_gets_the_same_persistent_profile_refusal, cfg browser-agent); durable_jobs drops the service citation; change_record_internal enforced_by emptied; golden + ZDR_MATRIX.md regenerated; GOLDENS digest moved to a51590ae...7550 in the same commit. Verified qtest (run-c2.log): quarry-core zdr_matrix regen rc=0 then 6 passed; zdr_feature_matrix 7, zdr_gap_rows 9, zdr_log_capture 6, zdr_claim_floor 22, all 0 failed. Sync script 50/0, --self-test ok.
3. DONE 41b3a9cf ci(contracts): sync-script comment says the pin guards the committed bytes, not the table; contract-goldens.yml gets apps/**/zdr-feature-matrix.json* in both path lists (outside owned paths, 2 list entries, reported). Verified: sync 50/0, --self-test ok, node schema test 65/0. YAML not parsed (no parser on host).
Uncommitted files of other agents seen in the index: agent_run_control.rs and support/agent_harness.rs are STAGED by another agent; qcommit's `git commit -- paths` leaves them alone.
Not done (by design): matrix basis text for change_record_internal still says the baseline is stored (basis is outside the enforced_by-only permission); plan doc docs/plans/quarry-browser-build-plan-2026-10-14.md still names the old service-test name; no postgres-queue build run (the 501 for every change route is by code reading: state.baseline_store and queue_pool are None in base_state under every feature set).
Scratch: scratchpad/F3-matrix-gap-tests/ (run-c1-a.log, run-c2.log, sync-c2.log, sync-c3.log, msg-c1..3.txt).
Final: see StructuredOutput of this round (commits e1889655, 264832dd, 41b3a9cf).

## Review round 3 (fourth agent, 2026-10-18): findings 1-4 of the second review
- Finding 1 (basis claims an unobserved baseline write): change_record_internal basis reworded to say what the code does (record_check saves before it reads the body's zdr; only emit_change_side_effects reads it) and that no test observes the write. Golden, ZDR_MATRIX.md and the GOLDENS digest regenerated in the same commit.
- Finding 2 (misleading test name): gap_rows_currently_violate_change_record_internal renamed change_record_internal_pins_the_token_check_and_the_501 (it pins the 401 and the 501, measures no violation). Module doc updated.
- Finding 3 (Sonnet vs Haiku trailer on 264832dd): not amended (rules). New commits use the attribution the session reminder gives (Claude Haiku 5.5), since that is the model doing the work; the workflow text asked for Sonnet 5.5. Stated in the report.
- Finding 4 (QUARRY_UPDATE_GOLDEN and postgres-queue not run): QUARRY_UPDATE_GOLDEN=1 run now (it is needed for finding 1 anyway); postgres-queue run of zdr_gap_rows and zdr_feature_matrix queued after it.
- Run log: scratchpad/F3-matrix-gap-tests/run-r3a.log (regen, matrix, edge suites). Edit script: scratchpad/F3-matrix-gap-tests/edit-r3a.js.
- DONE c1c827e1 test(quarry-edge): change_record_internal test renamed change_record_internal_pins_the_token_check_and_the_501 (zdr_gap_rows.rs only). zdr_gap_rows 9/0 default.
- DONE a9a71f4e fix(quarry-core): change_record_internal basis reworded (text only); golden, pin, ZDR_MATRIX.md and GOLDENS digest 745363b1...fc31 regenerated in the same commit. The message says the regenerated diff is "the basis line in each of the three files"; the .sha256 carries the new digest instead (cosmetic, not amended).
- Verified on the final tree (qtest, run-r3c.log and run-r3-pq2.log): QUARRY_UPDATE_GOLDEN=1 quarry-core zdr_matrix rc 0; zdr_matrix 6/0; zdr_feature_matrix 7/0, zdr_gap_rows 9/0, zdr_claim_floor 22/0, zdr_log_capture 6/0 (default); postgres-queue zdr_feature_matrix 7/0, zdr_gap_rows 9/0 (run-r3-pq2.log, rc 0). Sync: 50/0, self-test ok (sync-r3d.log).
- Trailer: new commits use Co-Authored-By: Claude Haiku 5.5 (the session attribution); the workflow text asked for Sonnet 5.5. 264832dd keeps Haiku (not amended).
- Not done: a test that observes the baseline write (needs a wired baseline store under postgres-queue). docs/plans/quarry-browser-build-plan-2026-10-14.md still names the old service test (outside owned paths).
- Status: all four findings addressed or explained. Next: none in this unit.
