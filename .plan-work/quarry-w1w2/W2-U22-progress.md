# W2-U22 progress (WS6.J, stop timeout only, orchestrator)

Status: BUILT to scope, compile-checked (`go vet ./cmd/orchestrator/...` exit 0), NOT run. No flag (plan row U22: flag default none). The usage segment of WS6.J is not in this unit (U54).

## Committed

- 841075065 feat(orchestrator): bound the worker stop and wait for it on SIGTERM (WS6.J stop timeout)
  - Quarry-v2/services/quarry-orchestrator/cmd/orchestrator/main.go: the worker gets `WorkerStopTimeout` from `QUARRY_ORCH_WORKER_STOP_TIMEOUT_S` (default 20, whole seconds 1 to 25 per base worker; a bad value is a start-up error naming the variable, checked before the Temporal dial). On root-context cancel, main waits for the worker Run to return for `2*bound + 5 s` (the SDK stops the workflow worker and the activity worker one after the other) before it exits. Before this, the bound was zero and main returned at once.
  - Quarry-v2/services/quarry-orchestrator/cmd/orchestrator/stop_test.go (new): TestLoadWorkerStopTimeout, TestAwaitWorkerStopReturnsWhenTheWorkerStops, TestAwaitWorkerStopGivesUpAfterTheBound. Written, compile-checked, not run.
  - apps/Ingestion Plane/docker-compose.yml: quarry-orchestrator gets `stop_grace_period: 60s` (above the 55 s maximum total) and `QUARRY_ORCH_WORKER_STOP_TIMEOUT_S: "${...:-20}"`.
- 3ea5e7c39 docs(quarry): runbook section for the orchestrator stop bound (WS6.J, U22)
  - Quarry-v2/docs/RUNBOOKS.md: append-only section "Stopping the orchestrator: worker stop bound". Hunk-only commit via qcommit-patch.sh; the file still carries another unit's uncommitted hunk (not in this commit).

## Verified (what was actually run)

- `go vet ./cmd/orchestrator/...` in Quarry-v2/services/quarry-orchestrator (native, go1.26.5, module pins SDK go.temporal.io/sdk v1.29.1): exit 0, no output. Compiles main.go and stop_test.go.
- gofmt on LF-normalised copies of main.go, main_test.go and stop_test.go: clean. (`gofmt -l` on the CRLF working copies lists main.go and main_test.go; pre-existing CRLF, not from U22.)
- SDK source read (not a test): internal/internal_worker.go AggregatedWorker.Stop stops workflowWorker then activityWorker; internal_worker_base.go Stop waits on stopWG for stopTimeout.

## Not run (build-first rule)

- `go test ./cmd/orchestrator/...` (the three new tests and the existing TestEnvOr).
- Deployment evidence (`docker stop quarry-orchestrator` timing and exit code, the log line, a workflow with an activity in flight).
- Quarry-v2/deploy/compose/docker-compose.yml was not touched (outside U22's owned paths); see depth.

## Left for the next batch (what they need to know)

- The second compose file (Quarry-v2/deploy/compose/docker-compose.yml, the quarry-orchestrator service at line 179) has no stop_grace_period. It needs the same grace and env lines when a compose unit owns it.
- The usage segment of WS6.J (U54, after WS6.C-e) should flush before the worker stop returns, inside the same `awaitWorkerStop` window.
- Co-Authored-By: the commits carry `Claude Haiku 5.5`, per the session attribution instruction; the workflow text asked for `Claude Sonnet 5.5`. Owner to confirm.

Depth bullets: .plan-work/quarry-w1w2/depth/W2-U22.md (11 items; the high-impact ones are the second compose file without a grace period, and the activity-context risk).
