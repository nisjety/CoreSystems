# W2-U16 status: tenant-safe schedules, unscoped list removed (D2-042)

Branch w23/lane-a. Date 2026-11-01.

## Built
- `GET /v1/schedules` without `org_id` is a 400 in every environment. The legacy all-orgs branch (wire-shape list, deprecation log, `os.Getenv` switch) is deleted from `schedules.go`; the `os` and `zerolog` imports went with it. A tenant request is the keyset page of the caller's own rows, as before.
- `EnvSchedulesRequireOrg` is kept as a retired constant (documented as a no-op). Compose sets `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG` to `${...:-1}` so an older control image rolled back onto the stack is fenced too.
- Already existed (not rebuilt): org-scoped list, 404 for foreign ids on get/delete/enable/disable/runs, and the orchestrator's `GET /v1/internal/schedules/desired` read (the orchestrator already calls it, `services/quarry-orchestrator/internal/schedules/schedules.go:227`). Nothing in the live tree calls the unscoped list.
- Docs: RUNBOOKS (flag retired, D2-043 note with a read-only SELECT), SCHEDULES.md tenancy section.

## Commits
See `git log w23/lane-a` (code and tests, docs/compose, this status file).

## Tests (Go host, GOWORK=off)
- `go test ./internal/resources/`: ok. Targeted run: TestListSchedules_WithoutOrgIsRefused (400 for /v1/schedules, ?org_id= and ?limit=50, with the env var unset, 1 and 0; no org rows in the body; desired read still answers both orgs), TestListSchedules_NeverCrossesOrgs (3 rows org A, 2 rows org B, paged at limit 2, an empty org gets an empty page), TestListSchedules_OrgScoped, TestInternalDesired_ReturnsAllOrgs: 4 PASS.
- TestPgListSchedules_NeverCrossesOrgs: written, SKIPs without `QUARRY_TEST_DSN` (fails if `QUARRY_REQUIRE_DB=1`). 1 SKIP here.
- `go vet ./internal/resources/` clean; `go test -count=1 ./...` for quarry-control: all packages ok.
- Replaced TestListSchedules_WithoutOrgIsLegacyUnlessRequired (it pinned the removed behaviour).

## Flag and default
No new flag (leak fix). `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG` is retired and has no effect; compose default 1.

## Assumed decisions
- Refusal is 400 (matching get/delete in the same file), not 401: the edge has already authenticated; control only sees a missing `org_id`.
- Kept the constant and the compose variable rather than deleting them, for rollback fencing.
- D2-043: migration 017's disabled rows stay disabled; owners re-enable after review (RUNBOOKS).

## Model Plane asks
None.

## Not verified
- The Postgres variant has not run (no DSN here; live databases are SELECT-only). It belongs to the W2-T gate with a `QUARRY_TEST_DSN`.
- Compose was edited but `docker compose config` was not run.
- No live stack check that the edge never forwards a list without `org_id`.
