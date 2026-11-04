# W2-FE-L5a status: WS15.L5 contract and SPA job controls

Branch w23/lane-d. Date 2026-11-02. Built, tested (vitest), dark: no control shows until the gateway reports `jobControl` (gateway half is W2-FE-L5b).

## Built
Check first: none of this existed (the registry had only `run_schedule_action`; the client had no job-control calls; the job row had no controls). All of it is new.
- Contract (`src/shared/actions/action-registry.ts`): `ingestions.get_run_status` (low), `pause_run` and `resume_run` (medium, no approval), `cancel_run` (medium, requiresApproval, not reversible). `run_schedule_action` is split into `pause_schedule` (no approval) and `change_schedule_state` (unpause, trigger, backfill, delete; requiresApproval; not reversible). `create_schedule` gains `urls`, `maxPages`, `overlapPolicy` (skip, cancel, allow, the owner enum); `targetUrl` is now required except for `batch`, which needs `urls`; the input is strict, so `activate` is rejected.
- Side files the registry forces (outside the stated file list, unavoidable): `model-access.ts` (every registry id needs a side-map row; all six new ids are closed to the model) and the generated `agent-action-admission.json` golden.
- Client (`src/shared/api/ingestions-client.ts`): `getIngestionRunStatus`, `pauseIngestionRun`, `resumeIngestionRun`, `cancelIngestionRun` against `/api/v1/knowledge/crawl/{id}` (GET, POST /pause, POST /resume, DELETE; the paths the L5b queue row names). `ScheduleCapabilities.jobControl` (true only for exactly `true`; off in the all-off default and on read failure). `RunItem` gains optional `origin`, `phase`, `desiredState`. `ScheduleCreateRequest` gains the new fields and has no `activate`. Schedule verbs are typed (`SchedulePauseAction`, `ScheduleStateAction`) and a backfill window can be passed.
- SPA: `RunJobControls.tsx` on the runs table row (new Actions column, drawn only when `jobControl`): Pause on a live crawl, Resume on a paused one, Cancel with an inline second confirmation; a 501 shows a plain not-enabled message; clicks do not select the row. Only `crawl` jobs get controls.

## Commits (w23/lane-d)
- f8d81c23f feat(frontend): ingestions run-control and schedule-state action contracts (WS15.L5)
- 31f2a00e0 feat(frontend): ingestions client job-control calls and jobControl capability (WS15.L5)
- 1dc42dfe5 feat(frontend): pause, resume and cancel controls on the ingestions job row (WS15.L5)
- the status, asks and depth commit follows.

## Tests (vitest on the host, node_modules borrowed by junction from another worktree)
- New: `ingestions-job-control-actions.test.ts` 10, `ingestions-client.test.ts` 9, `RunJobControls.test.tsx` 9 (28 new).
- `src/shared/actions src/shared/api src/features/ingestions src/shared/context-packs`: 115 files passed, 1092 tests passed; 1 file failed to load: `quarry-client.test.ts` cannot resolve `@quarry/client` (the borrowed install does not link the SDK; unrelated to this unit).
- `tsc6 -p tsconfig.app.json`: 34 errors (21 quarry-client.ts, 12 RemoteSupportPage.tsx, 1 schedule-contract.test.ts), none in a touched file; the queue baseline is 33 (see depth row).

## Flag and default
No new flag. The edge flag `QUARRY_EDGE__JOB_CONTROL` stays off. The SPA shows nothing unless the gateway capabilities answer carries `jobControl: true`; absent or failed read means off.

## Assumed decisions
- Capability name and carrier: `jobControl` on the existing `GET /api/ingestions/schedules/capabilities` (same `QUARRY_SCHEDULE_CAPABILITIES` JSON; L5b adds the field). No new endpoint.
- Run control addresses crawl only (`kind` enum is just `crawl`, optional), because the L5b row lists only the knowledge crawl routes. Batch is a one-line enum extension when the gateway proxies it.
- Pause and resume act immediately (registry: no approval); only cancel gets the confirmation ("pause/resume/cancel with confirm" read as: confirm where the registry requires approval).
- `change_schedule_state` keeps the existing wire verbs (`unpause_schedule`, `trigger_schedule`, `backfill_schedule`, `delete_schedule`) so L5b only routes; `pause_schedule` keeps its verb.
- Overlap values are the owner enum skip, cancel, allow (quarry-core `OverlapPolicy`).
- The page existing schedule buttons were left as they are (type-narrowed only).

## Model Plane asks
Yes, one: re-vendor the admission golden (canonical copy, pin, three vendored copies, one digest in `scripts/tests/zdr-contract-sync.test.sh`). Exact steps and the new digest are in `.plan-work/quarry-w1w2/MP-ASKS.md`. Until done, the repo drift check fails; do not merge this lane to main without it.

## Not verified
- Not run in a browser or against a gateway: the L5b routes do not exist yet, so every control call would 404 today (hidden by the capability gate).
- Gateway Rust parity: the dispatchers for the new action ids and the removal of `run_schedule_action` are L5b. The gateway still dispatches the old id until then.
- `zdr-contract-sync.test.sh` not run (it needs the Model Plane copies updated first).
- Layout of the Actions column was not looked at (no CSS added).
