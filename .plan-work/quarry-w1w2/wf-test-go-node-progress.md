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
