# C1-ws4h-posture progress
- COMMITTED: 2a7759f6 control (migration 023, store, wire, source monitor), 3bfc6a0b orchestrator, 06959092 edge (schedule_routes, resource_routes, tests/zdr_posture_stamp.rs), 8b3c108b matrix texts + golden + pin, efebf3e0 RUNBOOKS + plan (R-2 row + ledger row).
- TRAP: efebf3e0 also swept in ANOTHER unit's uncommitted RUNBOOKS hunks (QUARRY_BLOCK_ESCALATION default row -> honest, "Where the default stands", "Page cache" paragraph; 6 lines). They appeared between my git diff and the commit. Report to lead.
- Go mutation checks done on the copy (all killed, all restored): scratchpad/C1-ws4h-posture/mut-go.log.
- IN PROGRESS: Rust mutation run on the copy (scratchpad/C1-ws4h-posture/mut-rs.sh, log mut-rs.log, volume c1-mut-target, to remove afterwards: docker volume rm c1-mut-target under the lock).
- NEXT: read mut-rs.log; write notes file wf-C1-ws4h-posture.md; remove the volume; final answer.
- ALL DONE: mutation checks complete (see wf-C1-ws4h-posture.md); volume c1-mut-target removed; copy deleted.

## ROUND 4 (review fixes), 2026-10-21
- DONE uncommitted (mine): edge schedule_routes.rs + resource_routes.rs (body zdr raises the stamp via effective_zdr(req.zdr.unwrap_or(false)); CreateScheduleRequest.zdr is skip_serializing; CreateSourceRequest.zdr no longer skip_deserializing), tests/zdr_posture_stamp.rs (10 tests), quarry-core zdr_matrix.rs text + regenerated golden/.sha256/ZDR_MATRIX.md + pin in scripts/tests/zdr-contract-sync.test.sh, RUNBOOKS (own section corrections + history notes), quarry-control pg_test.go (QUARRY_REQUIRE_DB), .github/workflows/quarry-v2-ci.yml (new job quarry-control-postgres).
- Finding 5 (service principal): the service half is WRONG: require_service_route_scope fails /v1/schedules and /v1/sources closed for every service (403 before the handler). Pinned by a_service_principal_cannot_create_a_monitor_in_either_mode. The standard-user half is real and fixed.
- Mutation run: scratchpad/C1-ws4h-posture/mut4.sh -> mut4.log (volume c1-mut4-target, remove under lock afterwards; copy dir mut4).
- NEXT: read mut4.log, full edge+core test run, commit (edge fix; matrix/pin; runbooks; control pg_test + CI), notes file, remove volume.

## ROUND 5 (review findings, 2026-10-21)
- DONE uncommitted (mine): orchestrator activities.go (RunPageInput.Zdr, run_page body zdr when stamped, CheckChange passes in.Zdr) + zdr_posture_test.go (2 new tests) + schedules/schedules_zdr_test.go (new, 2 tests); Go mutations killed (run_page call w/o Zdr, body w/o zdr, create Args stripped, update Args stripped), all restored; go test ./... in orchestrator ok.
- DONE uncommitted: edge test a_legacy_cron_create_never_forwards_a_declared_zdr (tests/zdr_posture_stamp.rs), schedule_routes.rs doc comment; zdr_matrix.rs basis (no service principal clause; run_page note); RUNBOOKS (own section, history corrections).
- IN PROGRESS: qtest r5-gold.log (QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix) then update digest in scripts/tests/zdr-contract-sync.test.sh, then full edge tests (r5-edge.log).
- RUNBOOKS has FOREIGN hunks (WS3.I: FallbackDriver rows, Known limit) at lines ~516-530: commit only my hunks via git apply --cached of a filtered patch, then git commit -F msg (no paths) under with-lock.sh git.
- Commits planned: (1) orchestrator, (2) edge + tests, (3) matrix+golden+pin, (4) control pg_test + CI, (5) RUNBOOKS hunks.
- ROUND 5 ALL DONE. Commits: 1101b6c9 (orchestrator), 47a2c189 (control pg_test + CI), 65ccf059 (matrix+golden+pin), c4510d88 (RUNBOOKS hunk only), 96c81447 (edge). Volume c1-mut4-target removed. Remaining uncommitted in tree are other units' (RUNBOOKS WS3.I hunk, fallback_driver.rs, cache_policy.rs, go.work.sum).

## ROUND 6 (verifier note, 2026-10-21)
- The only finding is a verifier note, not a defect: it did not rerun the full sets and ran a wrong target (quarry-core --test zdr_feature_matrix; the test is crates/quarry-edge/tests/zdr_feature_matrix.rs, as the 96c81447 message and zdr_matrix.rs line 7 say). No code or message change needed, no new commit.
- Reran in Docker via qtest: cargo test -p quarry-edge --no-fail-fast + cargo test -p quarry-core --no-fail-fast: 34 result lines, 1222 passed, 0 failed (zdr_feature_matrix 12, log_field_discipline 22, zdr_gap_rows 9, zdr_posture_stamp 11, zdr_claim_floor 22, zdr_behavior 4). Log: scratchpad/C1-ws4h-posture/r6-full.log.

## ROUND 7 (review findings 2026-10-23)
- Findings 1-4 (false claims in old commits): history cannot be amended; HEAD is accurate (sync script 65 passed 0 failed 7 pending; RUNBOOKS history notes cover 1,3,4; test-count note (4) added this round). Verified, not re-fixed.
- DONE uncommitted (mine): edge change_webhook.rs (emit_change_side_effects moved here from change_routes.rs, pub, ungated in lib), change_routes.rs (call site + removed 2 gated imports), tests/zdr_posture_stamp.rs (+2 tests: fan-out suppression, run_page zdr; 13 total, qtest 13 passed; cargo check --features postgres-queue --all-targets no errors), .github/workflows/quarry-v2-ci.yml (new gating job quarry-go-zdr-stamp-chain), RUNBOOKS own hunks (r7-doc.js).
- Go: orchestrator activities/workflows/schedules ok, control internal/resources ok (native).
- IN PROGRESS: mutation run scratchpad/C1-ws4h-posture/mut7.sh -> mut7.log (volume c1-mut7-target; REMOVE under lock afterwards; copy dir mut7).
- RUNBOOKS has FOREIGN hunks (others' units): commit only mine via filtered patch (git apply --cached), see mkpatch.js.
- NEXT: read mut7.log, commit (edge+tests, CI, RUNBOOKS hunks), update notes file wf-C1-ws4h-posture.md, remove volume, final answer.
- ROUND 8 DONE: b8b7495b (poll positive controls); notes appended.
