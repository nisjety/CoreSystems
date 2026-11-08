# session-core DB harness, 2026-10-26 (full run, throwaway Postgres)

842 passed, 2 failed (exit 101). Last recorded full run before this: 719 passed (2026-10-22).
Every migration up to 0064 applied on a real Postgres in this run.

Failures (to fix, then re-run with the harness filters):
1. grpc::tests::record_scheduled_run_result_refuses_zdr_run (grpc.rs ~10908): the test marks a run zero-retention
   with an UPDATE after creating it, and the trigger refuses it: "run retention receipt is immutable". Test setup
   must create the run with its retention receipt from the start (or use the store's own creation path for a ZDR run).
2. memory_control::tests::pause_blocks_inline_and_dreaming_extraction (memory_control.rs ~1224): "the inline row
   appears while on" expected 1, got 2. Either the test counts rows another writer adds, or inline extraction writes
   twice while the user is on: investigate before changing the assertion.

Re-run: bash "apps/Model Plane/scripts/tests/session-core-db-tests.sh" record_scheduled_run_result_refuses_zdr_run=1 pause_blocks_inline_and_dreaming_extraction=1

Re-run 2026-10-26 ~15:10 after the db-harness-two-failures packet: both tests pass through the harness (record_scheduled_run_result_refuses_zdr_run, pause_blocks_inline_and_dreaming_extraction), exit 0.

## Lead proofs, 2026-10-26 afternoon
- session-core harness, Round A filters (thread_off_blocks=2, thread_resume_watermark, dreaming_reads_nothing_a_thread_said,
  a_manual_save_into_a_thread_set_off, pause_blocks_inline_and_dreaming_extraction, resume_does_not_mine_messages_written_while_paused): exit 0.
  Log: .plan-work/depth/db-harness-roundA.log
- audit-core, every package on a throwaway postgres:15.18 (scratchpad go-pg-tests.sh): 429 passed, 3 failed -> fixed (922d96f79, D1-732:
  NULLIF int4 typing + the chained-purge test's backdate) -> internal/store 78 passed, 0 failed. Logs: audit-core-db.log, audit-core-store-db.log
- promtool v2.54.1: check rules (19 rules) and test rules pass for audit-core, audit-delivery, zdr-attestation.
- core-gateway: internal/config and internal/health vet clean and pass; the module-wide vet failure is pre-existing
  (internal/auth/token_manager.go is empty since b83214338; a Windows-only go.sum entry for go-winio).
- org-core, every package on a throwaway org_lifecycle postgres with the fixture marker (scratchpad lifecycle-pg-tests.sh): 348 passed,
  4 failed. SIEM exporter test literal fixed (611206b80, D1-753) -> 7/7 SIEM tests pass. D1-751 (default privileges, history tables not
  append-only) sent to a fix packet (round-C). D1-752 (pre-existing interactive-retention outbox test) logged for an owner decision.
  Log: org-core-lifecycle-db.log
- capability-core `go test -tags integration ./...` (testcontainers postgres:16): 27 packages ok, 5 FAIL. Cause 1 (D1-754): migration
  0021_skill_bundles uses the reserved word `binary` as a column -> every test applying migrations fails (api, registry, server,
  skillbundle). Cause 2 (D1-755): taskexec TestScheduleTemplateDigestSurvivesJSONB scratch schema stale vs the WS20 sweeper (25P02).
  Both sent to round-D (task wsrzg2hpx, lock DEPTH-5). Re-run after: go test -tags integration -count=1 -p 2 ./... in capability-core.
  Log: capability-core-integration.log
- org-core D1-751 fixed (c695e3030, migration 024 least privilege): without 024 the new privilege test and both RLS tests fail;
  with 024 the full org-core suite (-p 1) passes 352, only D1-752 (pre-existing) fails. Logs: org-core-024-without.log, org-core-024-with.log.
  Deploy owed: rebuild org-core via scripts/run-control-plane.sh so 024 applies on the live DB (not done).
- capability-core integration run 2 after round D (794a548cb D1-754, 77af40718 D1-755): 31 packages ok, 0021-0024 apply on Postgres.
  One failure left: tool_policies accepts allow with NULL digest (D1-757). Sweeper design defect D1-756 (errors discarded, one tx per
  batch, poison schedule blocks every sweep). Both in round-E (task wx7wxdhqz, lock DEPTH-6). Log: capability-core-integration-2.log
- Round E committed (D1-756 sweeper savepoints + errors, D1-757 tool_policies 0025): capability-core integration run 3 = all 32 packages
  ok (first fully green run). Old sweeper.go fails the new isolation test (fired=0, rollback). Logs: capability-core-integration-3.log,
  round-e-targeted.log, round-e-old-sweeper.log.
- Other Model Plane Go services: browser-broker (testcontainers + TEST_DATABASE_URL) and letta-bridge green; sandbox-manager (process
  integration tests did not compile since S4.2) and cost-core (miss-reason test keyed by the raw idempotency key) fixed in 21af9fecd
  (D1-767/768) -> sandbox-manager 9/9, cost-core 7/7 packages ok.
- W1-25: WS09-S9 974f9f18d, WS09-S8 5c81e7f52 + shared edits 4e9d5ffe2, WS09-S7 ChatPage edit 59af16e00. W1-25 proof: model-gateway paging 4,
  zdr matrix 6, e2e build, session-core paging 3 + DB 3, BFF 746, go drift clean. model-gateway --lib: 2357 pass, 2 FAIL in
  idempotency_registry (renewal from e05ace637; deterministic) -> D1-778, round-G (task wu4mtcfxx, lock DEPTH-7).
- Full session-core DB harness re-run started (bv01frr4z), log db-harness-full-2.log.
- Full session-core DB harness re-run: 867 passed, 0 failed (recorded in the harness, commit 6c11f5277).
- D1-778 corrected and fixed (7f2a72aa1): the two idempotency renewal tests had paused-clock timing bugs; renew already answered true on
  an outage. model-gateway --lib 2361/0 (lead-run), idempotency_registry 27/27 three runs in a row.
- Verevon v3 full vitest (--pool=threads): 354 files / 3831 tests pass; 19 files hit worker-start timeouts under load and were re-run with
  --maxWorkers=2: 17 pass (453 tests), 2 fail to import workspace packages not linked in this worktree (@quarry/client in quarry-client.ts,
  @verevon/remote-core in RemoteSupportPage.tsx; pre-existing env, same files as the 34 tsc errors, untouched since Aug/Sep).
  Total 371/373 files, 4284 tests green. Logs: verevonv3-vitest-full.log, verevonv3-vitest-missed.log
- WAVE 1 END CHECK (fresh Rust target C:/dev/mp-target-final, deleted afterwards): cargo test --workspace --lib 4697 passed, 0 failed;
  go vet + go test green for bridge-core, browser-broker, capability-core, cost-core, letta-bridge, nats-provisioner, orchestrator-core,
  sandbox-manager, go/gen, audit-core, org-core, integration-corev2 (after d67c5a43f, D1-788 golden test). Summary: final-verify/summary.txt
