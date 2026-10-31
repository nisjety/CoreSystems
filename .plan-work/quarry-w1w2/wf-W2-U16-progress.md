# W2-U16 progress: WS15.S1 tenant-safe schedule persistence and creation (migration 017)

Status: BUILT to the plan's file table (all commits landed); tests WRITTEN and compile-checked only (wave rule). Nothing fires (WS15.S2 is next).

## Committed (local branch quarryv2-next)
- 369f55674 feat(control): tenant-safe schedule persistence and creation (WS15.S1). Migration 017, store (Schedule fields, org-scoped SchedulesStore methods, pg SQL, mem store), schedule_validate.go rewrite, cron/interval.go (MinInterval), resources/schedules.go (org-scoped handlers moved from resources.go), tenant view in schedule_wire.go, cycle23 trigger/backfill org check, tests.
- Verified: `go build ./...` and `go vet ./internal/...` pass in quarry-control (vet compiles the tests). Not run.

## Committed after the control slice
- 81854809c edge + core: schedule_routes.rs create mapping and reasons, ScheduleSummary fields (quarry-core resources.rs), tests/schedule_create.rs (new), zdr_posture_stamp.rs legacy fixture (config.url added). Verified: cargo check -p quarry-core -p quarry-edge --tests in the container, rc 0.
- d1c861872 gateway: ingestions/schedules.rs (needsActivation, timezone, overlapPolicy; timezone forwarded on create; one pure test). Verified: native cargo check --tests, exit 0.
- 1ed8d3ff2 docs: openapi.yaml additive properties and the RUNBOOKS WS15.S1 section (hunk-only commit via qcommit-patch; the other hunks in those files are NOT committed here).
- 99c61dfd9 docs: SCHEDULES.md rewritten to the code.

## Not done (and why)
- WS15.S2 executor and the orchestrator read of the new fields: next slice (U38). The orchestrator still reads the unscoped list until it moves to /v1/internal/schedules/desired.
- Deployment evidence (five steps): needs a live stack.
- Migration DB tests: need QUARRY_TEST_DSN (CI Postgres lane).
- Error codes are details.reason, not new ErrorCode variants (H8 deferred).
- Edge answers 200 on create; the plan says 201 (WS4.H tests pin 200).

## For the next batch
- Control's legacy unscoped list is still live for the orchestrator. Do not set QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG until the orchestrator reads /v1/internal/schedules/desired.
- Migration 017 is applied once per database (schema_migrations). Its guard (updated_at = 0) disables pre-017 scrape, crawl and batch rows on that first run only.
- Depth items: .plan-work/quarry-w1w2/depth/W2-U16.md. Hand-over: .plan-work/quarry-w1w2/wf-W2-U16.md.
