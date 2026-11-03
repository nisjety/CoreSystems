# W2-Q-S5 status: WS15.S5 schedules runbook and go-live gates (docs) (lane d, branch w23/lane-d)

## Built / partly

Built, docs only. No code, no flag and no default changed. Nothing was flipped.

- `apps/Ingestion Plane/Quarry-v2/docs/SCHEDULES.md`: rewritten from the stub to the real behaviour. Covers the four kinds and their create checks, the Temporal overlap mapping, trigger and backfill through `schedule_requests` (flag off), the edge's fire-time posture (observe by default) with its skip reasons, the X-9 ZDR consumer, the known gaps (13 items) and the test files.
- `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md`: a new section "Schedules go-live: gates, the observe week and the flips (WS15.S5, docs only, 2026-11-03)", placed after the WS15.S2 record. It holds the switches and their defaults, Gates 0 to 5 (preconditions, the observe week, enforce, the first fire, the request path, the gateway capabilities), rollback, and the explicit not-done list (21 rows, each with what it blocks and where it lives).
- RUNBOOKS, existing sections corrected: the driver-cascade pause and unpause steps and the "Recovery: scrape backlog from a queue freeze" steps used routes that do not exist (`/resume`, `?status=paused`, `?backfill=skip|missed`) and control's routes without `org_id`. They now call the edge's pause, unpause and list. The one `/resume` left in RUNBOOKS is a runs route, not a schedule route.

Scope items: SCHEDULES.md (built), RUNBOOKS go-live checklist (built), explicit not-done list (built), no flip (held), the test named by the scope (not run, see Code only).

## Commits (w23/lane-d)

- 497655f7a docs(quarry): SCHEDULES.md describes the real kinds, overlap, request rows and fire-time posture (W2-Q-S5)
- 2fc19ec73 docs(quarry): RUNBOOKS schedules go-live gates and not-done list (W2-Q-S5)
- The commit that adds this file: docs(plan-work): W2-Q-S5 status and lane d depth rows

## Code only, not compiled

Docs only, so nothing to compile. The test named in the scope, `node --test scripts/tests/docs-contract.test.mjs`, was not run: the owner order of 2026-11-03 says no test runs in unit work. The integration step runs it. The checks I did instead are listed under Not verified.

## Flag and default

None added or changed. The switches the docs name keep their defaults in the code and the compose:
- `QUARRY_SCHEDULED_JOBS_ENABLED` 0 (control, compose)
- `QUARRY_EDGE__SCHEDULE_POSTURE` observe (edge)
- `QUARRY_CONTROL__ZDR_EVENT_CONSUMER` off (control)
- `QUARRY_CONTROL__SCHEDULE_REQUESTS` off (control); `QUARRY_SCHEDULE_REQUESTS` off (orchestrator)
- `QUARRY_SCHEDULE_CAPABILITIES` all false (gateway)
- `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` false, observe (edge)

## Verified against the code (what the docs state)

Each claim was read from source before it was written. The files:
- Control: `resources/schedules.go`, `schedule_requests.go`, `schedule_runs.go`, `schedule_wire.go`, `cycle23.go`; `store/schedule_validate.go`, `store/pg/resources.go` (`UpdateRuntime` has no caller), `store/pg/migrations/026_schedule_requests.sql`; `posture/posture.go`.
- Orchestrator: `schedules/schedules.go` (reconcile, the org-scoped reaper, `triggerWith` SKIP, `backfillWith` ALLOW_ALL), `schedules/spec.go` (overlap mapping, `prepareSpec`), `schedules/requests.go`; `workflows/scheduled.go`, `workflows/workflows.go` (`defaultActivityOpts`), `errs/errs.go` (`Retryable`), `activities/scheduled.go`.
- Edge: `schedule_routes.rs` (create, the `forward_json` status mapping, the ZDR refusal on trigger, backfill and unpause), `schedule_preflight.rs` (modes, check order, log lines), `routes.rs` (schedule route table), `zdr_refusal.rs` (observe by default), `resource_routes.rs`.
- Compose (`apps/Ingestion Plane/docker-compose.yml`): `QUARRY_SCHEDULED_JOBS_ENABLED`, `QUARRY_EDGE__SCHEDULE_POSTURE`, the org-core URL and token lines.
- Gateway (Frontend Plane, `domains/ingestions/schedules.rs`): the capability defaults.
- Test files named in SCHEDULES.md: each path exists in the lane.

## Assumed decisions

1. Gate order follows the queue and the X-9 decision: S4 observed for one week on a live stack before enforce; counsel answers prospective or retroactive ZDR before enforce; the request path (S3) waits for the edge half. Sources: the W2-queue WS15.S5 row and `DECISIONS-ROUND-2026-10-27.md` section 3.7.
2. The observe week runs with `QUARRY_SCHEDULED_JOBS_ENABLED=1`, because observe needs fires to observe. The switch is global, so the week cannot be limited to some orgs. The owner chooses the start day. This is written as a procedure; nothing is decided.
3. The explicit not-done list sits in RUNBOOKS, in the go-live section. SCHEDULES.md keeps the behaviour-level Known gaps and points to RUNBOOKS. Each page cross-references the other's numbering.
4. Operators reach schedules through the edge with a user token, not through control. Control's routes take `org_id` and need an HMAC signature, so the RUNBOOKS commands use the edge.
5. The S2 deployment evidence asks for a one-minute cron, which the 5-minute scrape floor refuses. Gate 3 uses `*/5` for scrape. `QUARRY_SCHEDULE_MIN_INTERVAL_S` is named as an evidence-only override, and the docs say it applies to every kind. The S2 record itself is not edited.
6. The S3 status file says the edge has no ZDR refusal on trigger, backfill or unpause. The code calls `refuse_for_claims` on all three (WS4.H, observe by default). SCHEDULES.md follows the code. The S3 record is stale on this point; it is outside this unit's files, so it is not edited.
7. The older RUNBOOKS records (WS15.S1 "trigger and backfill are unavailable", the WS15.S2 "replay fixture is owed") are left as recorded. Current state is in SCHEDULES.md and in the go-live section. The unit records are history and were not rewritten.
8. The edge's 502 mapping (control's 409 and 501 reach the client as `DRIVER_FAILED`) is documented as a Known gap, not fixed. A fix is an edge code change and sits outside this unit.
9. `scripts/tests/docs-contract.test.mjs` is not extended to read SCHEDULES.md. The file is outside this unit's list. The gap is recorded as RUNBOOKS not-done item 20.
10. Items already recorded in the W2-Q-S3 and W2-CP-X9EVENT depth rows (org erasure of `schedule_requests`, the backfill overlap, the at-least-once request, the consumer's missing wiring) are referenced from the docs, not repeated in the depth rows.

## MP asks

None. No Model Plane file was touched. Schedules do not call the Model Plane.

## Not done

- The test run (`node --test scripts/tests/docs-contract.test.mjs`): the integration step runs it.
- Code fixes for the gaps in SCHEDULES.md Known gaps and RUNBOOKS Not done (21 rows). They are listed, not built.
- Stack evidence for any unit. None has run. The go-live section lists the evidence still owed.
- The cross-unit check: another unit in this lane is editing `quarry-core/src/error.rs`, `fallback_driver.rs`, `docs/ERROR_REASONS.md` and adding `services/quarry-orchestrator/internal/errs/driver_failed.go` (DRIVER_FAILED sub-codes, E3.8.1). If that changes the 502 mapping, Known gap 1 and depth row 1 need a re-read. I did not touch those files.

## Not verified

- `docs-contract.test.mjs` was not run. Instead I read its assertions and grepped RUNBOOKS for each string it requires: the rerank default line and the shadow smoke-run text, the AI-formats reason tokens, the answer-synthesis heading and reasons, the webhook and change-text switch names, and the sections named by `sectionOf`. All are present once, and my edits add none of the names the test scans for (`RERANK_WINDOW`, `ModelPlaneSearchReranker`, `QUARRY_EDGE__AI_FORMATS` variants, and so on).
- Nothing was compiled or executed.
- Temporal behaviour was not checked against a live cluster. Three points are stated from the SDK's semantics or left open: a trigger during a running fire under `SKIP` starts nothing; whether a paused schedule's missed ticks are caught up on unpause (the RUNBOOKS says it depends on the catch-up window and does not rely on it); the name of Temporal's overlap-skip counter (described in words, not as a field name).
- The edge's 202-as-200 pass-through and the 502 mapping were read from `forward_json`, not exercised.
- The org-core token copy is manual (depth row in W2-CP-R13-2). Not checked on any stack.
