# W2-U16 hand-over (WS15.S1, tenant-safe schedule persistence and creation)

## What was built (committed)
- 369f55674 control: migration 017 (definition columns, org index, one-time disable of pre-017 scrape/crawl/batch rows); org-scoped schedule store and handlers (resources/schedules.go); tenant view; validate per kind; cron.MinInterval (cron/interval.go); cycle23 trigger/backfill org check.
- 81854809c edge + core: schedule_routes.rs maps scrape/crawl/batch, refuses with details.reason; ScheduleSummary timezone/jitter_s/needs_activation; tests/schedule_create.rs.
- d1c861872 gateway: needsActivation, timezone, overlapPolicy in the item; timezone forwarded.
- 1ed8d3ff2 docs: openapi additive properties and the RUNBOOKS WS15.S1 section (hunk-only commit).
- 99c61dfd9 docs: SCHEDULES.md rewritten to the code.

## Checks actually run
- quarry-control: `go build ./...` rc 0; `go vet ./...` rc 0 (vet compiles the tests).
- quarry-core and quarry-edge: `cargo check -p quarry-core -p quarry-edge --tests` in the Quarry container, rc 0 (after one attempt blocked by another agent's untracked quarry-transform/src/parse_cache.rs).
- gateway: native `cargo check --tests` (apps/Frontend Plane/verevonv3/apps/gateway), exit 0.
- Nothing was run as a test (wave rule). All new tests are written.

## Traps for the next agent
- The control store's `Validate` now rejects a schedule with no TargetKind or a non-executable kind. Any new caller that builds a `store.Schedule` and calls Validate must set TargetKind and config.
- `POST /v1/schedules` on control takes org from the query only. Old bodies with `org_id` only will 400 "org_id required" unless the caller passes `?org_id=`. The edge already does this; the test helper postSchedule derives the query from the body for old-style cases.
- Control answers 201 but the edge answers 200 (WS4.H tests pin 200).
- Migration 017 runs once per DB (schema_migrations). The DB test `TestMigration017_*` runs the file again by hand; it needs QUARRY_TEST_DSN.
- Two shared files carry other units' uncommitted hunks: openapi.yaml (RequestAsk, WS7.C) and RUNBOOKS.md (a WS17.S0 deletion). They were committed hunk-only with qcommit-patch.sh; the other hunks are still uncommitted in the working tree.
- The gateway test in schedules.rs is a pure function test; it does not need the Quarry edge.

## Next (WS15.S2, U38)
- The executor reads these rows. It must map `kind` (scrape|crawl|batch) to its workflow and must not treat `needs_activation` as a fire signal.
- The orchestrator still reads GET /v1/schedules unscoped. Move it to GET /v1/internal/schedules/desired, then set QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG=1.

Depth items: .plan-work/quarry-w1w2/depth/W2-U16.md.
