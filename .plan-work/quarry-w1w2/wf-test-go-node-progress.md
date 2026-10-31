# wf-test-go-node progress (wave test phase)

Unit: test-go-node. Phase: run once, report, no fixes, no retries. No commits and no edits by this unit.

## Preflight (done)
- Disk: C: 457.8 GB free (threshold 8 GB).
- Docker: `timeout 25 docker info` answered (rc 0). No container or image command is run by this unit.
- No earlier progress file existed, so this is a fresh run.
- `git log -n 50 --format='%h %B' -- .` read (history only).
- Toolchain: go1.26.5 windows/amd64, node v24.19.0, GNU timeout 8.32.
- Quarry-v2 has scripts/tests/ with 3 .test.mjs files (openapi-route-coverage, openapi-scrape-parity, openapi-challenge-parity).
- Root scripts/tests/ has 5 .test.mjs files (ownership-docs-contract, zdr-contract-schema, grant-contract-overlay, browser-grant-refusal, browser-grant-contract).
- The only docker use in the root node tests is grant-contract-overlay (docker compose version and docker compose config, which are renders, not containers). The bootstrap honors CORE_ROOT_OVERRIDE and runs no docker.

## Jobs started (logs in scratchpad/WT/, each wrapped in timeout 1000 s)
- BROKER: go vet ./... then go test -count=1 ./... in apps/Model Plane/go/services/browser-broker (GOWORK not set, as specified; apps/Model Plane/go/go.work applies). Log: broker.log, marker WT_DONE_BROKER.
- ORCH: GOWORK=off go test -count=1 ./... in apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator. Log: orch.log, marker WT_DONE_ORCH.
- CONTROL: GOWORK=off go test -count=1 ./... in apps/Ingestion Plane/Quarry-v2/services/quarry-control. Log: control.log, marker WT_DONE_CONTROL.
- NODE-ROOT: node --test scripts/tests/*.test.mjs at the worktree root. Log: node-root.log, marker WT_DONE_NODE_ROOT.
- NODE-QUARRY: node --test scripts/tests/*.test.mjs in apps/Ingestion Plane/Quarry-v2. Log: node-quarry.log, marker WT_DONE_NODE_QUARRY.
- ZDR: bash scripts/tests/zdr-contract-sync.test.sh at the root. Log: zdr-sync.log, marker WT_DONE_ZDR.
- PARITY: bash scripts/tests/principal-registry-parity.test.sh at the root (stub docker, CORE_ROOT_OVERRIDE sandboxes). Log: parity.log, marker WT_DONE_PARITY.

## Results
- BROKER: go vet rc 0; go test rc 0. 4 packages ok (cmd, internal/authz, internal/grant, internal/server); internal/telemetry has no test files; 0 failed. Log broker.log.
- ORCH: GOWORK=off go test rc 0. 7 packages ok; internal/controlauth has no test files; 0 failed. Log orch.log.
- CONTROL: GOWORK=off go test rc 0. 15 packages ok; notify, store/storetest, temporal have no test files; 0 failed. Log control.log.
- NODE-ROOT: node --test rc 0. 138 tests: 138 pass, 0 fail, 0 skipped. Log node-root.log.
- NODE-QUARRY: node --test rc 0. 23 tests: 23 pass, 0 fail. Log node-quarry.log.
- ZDR: zdr-contract-sync.test.sh rc 0. 65 passed, 0 failed (the pending consumers it lists are informational, not failures). Log zdr-sync.log.
- PARITY: TIMEOUT. Killed by timeout 1000 s (exit 124). 21 ok and 0 not ok before the kill; the shadowing-launcher check and the three bootstrap runs were not reached. Log parity.log. Depth bullet appended to .plan-work/quarry-w1w2/depth/W1-tests.md.
- Leftover processes after the kill: none found (process listing).
- go.work.sum mtime is 19 Oct 2026, before this run, so these tests did not rewrite it.
- No commits. No tracked file was edited by this unit. Nothing was run against Docker containers; the root node suite ran docker compose render commands only.

## Continuation: re-run on the current tree (2026-10-24, second agent of this unit)
- Found the section above from the 2026-10-23 run (logs 17:12 to 17:28). Since then 370 commits landed (HEAD 226a7f2ff, 2026-10-24 04:16), among them Go changes in browser-broker, quarry-orchestrator and quarry-control and changes to the Quarry-v2 OpenAPI and docs that the node contract tests read. The earlier results described an older tree, so every command was run once more on the current tree. The earlier results stay above for comparison.
- Preflight: `timeout 25 docker info` rc 0; HEAD 226a7f2ff; no uncommitted changes under apps/Model Plane/go, apps/Ingestion Plane/Quarry-v2/services, Quarry-v2/scripts and scripts. Disk: about 305 GB free on C.
- Jobs, all started together (logs in scratchpad/WT/t2-*.log, each under timeout 1000, end markers WT_DONE_*):
  - BROKER (t2-broker.log): go vet ./... then go test -count=1 -v ./... in apps/Model Plane/go/services/browser-broker (GOWORK not set). rc 0. 4 packages ok, internal/telemetry has no test files. 115 top-level PASS, 0 FAIL.
  - ORCH (t2-orch.log): GOWORK=off go test -count=1 -v ./... in quarry-orchestrator. rc 1. 126 top-level PASS, 5 FAIL. Failing: internal/schedules (2), internal/workflows (2), internal/timeouts (1, package new since the earlier run).
  - CONTROL (t2-control.log): GOWORK=off go test -count=1 -v ./... in quarry-control. rc 1. 289 top-level PASS, 8 FAIL, 28 SKIP (Postgres tests skip, QUARRY_TEST_DSN unset). Failing: internal/resources only, 8 top-level tests (TestCreateScheduleStampsZdr has subtests zdr_org and standard_org, both failing).
  - NODE-ROOT (t2-node-root.log): node --test scripts/tests/*.test.mjs at the repo root. rc 0. 143 pass, 0 fail, 0 skipped.
  - NODE-QUARRY (t2-node-quarry.log): node --test scripts/tests/*.test.mjs in apps/Ingestion Plane/Quarry-v2. rc 1. 23 tests, 19 pass, 4 fail: openapi-route-coverage (every_registered_route_is_documented_or_allowlisted_with_a_reason, the_allowlist_holds_no_stale_entries, the_route_count_is_pinned) and openapi-scrape-parity (formats_and_extractor_enums_equal_the_rust_enums, TypeError: formatsDefs is not iterable).
  - ZDR (t2-zdr.log): bash scripts/tests/zdr-contract-sync.test.sh at the repo root. rc 0. 71 passed, 0 failed.
  - PARITY (t2-parity.log): bash scripts/tests/principal-registry-parity.test.sh at the repo root, timeout 1000. rc 124, outcome timeout. 21 ok and 0 not ok before the kill; the shadowing-launcher check and the bootstrap runs did not appear. Same as the 2026-10-23 run. Depth bullet appended (recurrence; low; pre-existing per bea28d6ca, 2026-10-08). No leftover processes from this run (process listing at 04:38).
- Status after the run: all seven jobs have end markers (WT_DONE_*). Git: HEAD still 226a7f2ff; no change under the test paths; go.work.sum mtime unchanged (2026-10-19). No commits by this unit.
- Depth: 17 bullets appended to depth/W1-tests.md, one per failing test, severity high, each in a package that passed in the 2026-10-23 run, except internal/timeouts (new since then). Causes not investigated (wave test phase). The earlier parity bullet stays.
- Attribution (git log on each failing test file, one command each): the Go failures sit in files last changed by the WS15 commits of 2026-10-23 (1aa3d0a7f, 233b61994, 12aae7ed0, 31a2f7928, 369f55674), except TestCreateScheduleStampsZdr and TestUnstampedScheduleArgsAreUnchanged (file last changed 2a7759f69, WS4.H, 2026-10-21). The node failures are in files last changed 2026-10-18 (bee604d47, WS5.H); the suite reads docs/openapi.yaml and the route sources, which changed since.
- Notes: no separate hand-over file was written; this progress file is the hand-over (notes_file). No commits. No tracked file was edited by this unit.
