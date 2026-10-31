# D-D-B2-go progress (2026-10-27)

Status: COMPLETE. Both Go modules green. Four local commits on `quarryv2-next`, nothing pushed.

Commits
- c4375fbce test(quarry-control): read workflow args from the desired endpoint and unwrap the envelope (8 control failures; test-side)
- d4d7f9410 fix(quarry-orchestrator): scheduled fires lost their schedule id and could not read control's answer (5 orchestrator failures; two production bugs, a floor bug, two test fixes)
- d12aa5ead fix(quarry-control): an event takes its per-run seq in the same transaction as its insert (D2-016)
- 17971f060 fix(deploy): compose stop_grace_period + worker stop bound (D2-062); desired read pinned under REQUIRE_ORG (D2-042)

Results (native, GOWORK=off)
- services/quarry-orchestrator: go vet clean; go test -count=1 -v ./...: 134 top-level pass, 0 fail (baseline 126 pass, 5 fail).
- services/quarry-control: go vet clean; go test -count=1 -v ./...: 299 top-level pass, 0 fail, 28 skip (baseline 289 pass, 8 fail, 28 skip). The skips are the Postgres tests without QUARRY_TEST_DSN.
- Postgres: throwaway postgres:16-alpine under the docker lock, QUARRY_REQUIRE_DB=1: TestPostgres_EventSeq (4 subtests), TestPostgres_JobAdmission (7), TestPostgres_ConcurrentClaimsNeverExceedTheCap all PASS. Mutation (advisory lock replaced by a no-op) makes the concurrent subtest fail.
- Pause-twice test ran 20 times in a row: pass.
- cargo check --workspace --all-targets through qtest: Finished, rc 0 (only the existing warnings; no Rust was changed).
- docker compose -f Quarry-v2/deploy/compose/docker-compose.yml config (dummy env): rc 0, stop_grace_period 1m0s, QUARRY_ORCH_WORKER_STOP_TIMEOUT_S "20".

Re-run commands
- cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && GOWORK=off go vet ./... && GOWORK=off go test -count=1 ./...
- cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && GOWORK=off go vet ./... && GOWORK=off go test -count=1 ./...
- pg: scratchpad/D-B2-go/pgtest.sh under with-lock.sh docker (start postgres:16-alpine on 54329, run the pg tests, remove it).

Uncommitted files of mine: none. Strays left alone: quarry-edge lib.rs, main.rs, fixture_site.rs, quarry-runtime step_receipts.rs, .plan-work/next-slices.md, rb.patch, other units' notes.

Open findings: depth/D-D-B2-go.md (desired read has no cursor and caps at 500; RUNBOOKS floor sentence; controlCall envelope trap; REQUIRE_ORG deploy step).
