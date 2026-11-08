# W2-CP-S7b status: audit-before-ack for ignore-robots jobs (lane c, branch w23/lane-c)

## Built
All of the scope. Code only, not compiled, no tests run (owner order 2026-11-03). Only `gofmt -d` was run on the touched Go files (LF copies): no diffs, so they parse.

Checked first: nothing of it existed. There was no `quarry-control/internal/audit` package, no audit call in `createJob`, no `AUDIT_CORE_*` read in `cmd/control/main.go`. The audit-core credential row for `quarry-control` (audit:write, planes ["ingestion"]) and its compose contract tests already exist from W2-CP-R13-2. audit-core's `parseServiceCredentialRegistry` already accepts a second ingestion writer (only `integration-corev2` is required at startup), so the audit-core half is tests only, no code change.

- `quarry-control/internal/audit/client.go` (NEW): `New`, `FromEnv` (`AUDIT_CORE_URL`, `AUDIT_CORE_SERVICE_TOKEN`; both unset is `(nil, nil)`, one set or a bad URL is an error that names the setting, never the value), `RecordAudit` (POST `/v1/audit`, `X-Service-Id: quarry-control`, `X-Service-Token`, producer `quarry-control` and plane `ingestion` forced by the client, outcome defaults to `ok`, occurred_at in UTC, 2xx only, redirects not followed, 5 s default timeout). Follows integration-corev2's `AuditClient.RecordAudit`.
- `quarry-control/internal/audit/client_test.go` (NEW): headers and body, forced producer/plane, 200 and 202 succeed, every non-2xx and a redirect fail, transport error, incomplete events never sent, nil client, URL/token validation, FromEnv cases (and the error never contains the token).
- `quarry-control/internal/resources/resources.go`: `JobsOption` now sets a private `jobsConfig` (`WithAdmission` keeps its signature; new `WithRobotsAudit(RobotsAuditor)`). `createJob` parses the robots request after origin, and writes `crawl.robots_ignored` after the backlog check and BEFORE `db.Jobs().Create`. A missing sink or an audit failure answers 503 (`RUNTIME_NOT_READY`, details `reason: robots_override_audit_unavailable`, the upstream error is logged, not returned) and stores nothing. An idempotent retry of an existing job returns it before the audit, so one job gets one row. Event: `event_id = quarry-control:crawl.robots_ignored:<job_id>` (audit-core dedupes a repeat), `resource_id = job_id`, `user_id`, `subject = quarry.job`, details `{seed_host, authority, rev, kind}`, `request_id`. The audit write has its own 5 s deadline.
- `quarry-control/internal/resources/resources_robots_audit_test.go` (NEW): the plan's three tests (`TestCreateJob_RobotsIgnore_WritesAuditBeforeAck` also proves the job did not exist when its row was written, `TestCreateJob_RobotsIgnore_AuditFailureCreatesNoJob`, `TestCreateJob_Respect_WritesNoAudit`) plus string-mode and casing, refused with no sink while an ordinary job still passes, 400 without user_id, an idempotent retry writes one row, and a retry after a failed audit stores one job.
- `quarry-control/cmd/control/main.go`: builds the client with `audit.FromEnv`. Unset means off and a misconfiguration means off: it is logged, never fatal. The client is passed as `WithRobotsAudit` only when it is configured.
- `audit-core/internal/api/service_auth_test.go`: five tests. A second ingestion writer passes parse and startup validation and authorizes POST `/v1/audit` as its own principal, ingestion only. Neither writer can use the other's token (401). quarry-control cannot read (403). A second writer without planes (key missing, empty, blank) is still rejected. quarry-control alone does not satisfy the `integration-corev2` startup requirement.

## Commits (lane c)
- 752948af7 feat(quarry-control): audit-core client for POST /v1/audit as the quarry-control ingestion producer (WS14.S7b)
- acf31cc62 feat(quarry-control): audit crawl.robots_ignored before acknowledging a robots.txt override job (WS14.S7b)
- 712385106 test(audit-core): a second ingestion writer (quarry-control) is accepted and a writer without planes is still rejected (WS14.S7b)
- docs(plan-work) commit with this file and the depth rows

## Code only, not compiled
No `go build`, `go vet` or `go test` was run. The integration step compile-checks. Commands for it: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./internal/resources ./internal/audit ./cmd/control -count=1`; `cd "apps/Control Plane/audit-core" && go test ./internal/api -count=1`.

## Flag and default
No new flag. Nothing changes unless a job asks to ignore robots.txt. Today the edge always stamps `robots: "respect"` and refuses `ignore_robots_txt` (the edge side is W2-Q-S7a). With `AUDIT_CORE_URL` and `AUDIT_CORE_SERVICE_TOKEN` unset (the default; the Ingestion compose is not touched), every override job is refused with 503. To enable it, the owner: (1) copies `QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN` from Control's `.env.generated-secrets` into Ingestion's store; (2) sets `AUDIT_CORE_URL` (audit-core on the shared network) and `AUDIT_CORE_SERVICE_TOKEN` on `quarry-control` in `apps/Ingestion Plane/docker-compose.yml`; (3) recreates quarry-control. Rollback: unset them, and overrides are refused again.

## Assumed decisions
- Two shapes count as an override: `params.robots == "ignore"` (the string the edge stamps today and the one the orchestrator honours, `workflows/inputs.go` `stringFromParam`) and `params.robots == {mode:"ignore", ...}` (the S7a object in the plan). The mode is compared case-insensitively, so a casing variant is audited (or refused) rather than skipped. Any other value is not an override.
- An override with no `params.user_id` is a 400: the audit row must name the person (the edge refuses service principals, so a human user_id is always there).
- The audit row is written before `Create`. If `Create` then fails (an idempotency race, a store error), a row exists for a job id that was never stored. This is accepted: the plan requires "no audit row, no job", not the reverse. A retry gets a new job id and a new row.
- `event_id` is derived from the job id (`quarry-control:crawl.robots_ignored:<job_id>`), so audit-core's idempotency makes a repeated write a duplicate.
- Only the seed host is audited (lower-cased host of `params.url`, then `seed_url`), never the path or query.
- 503 uses `RUNTIME_NOT_READY` (the only Go error code that maps to 503).
- A half-set or invalid audit config is logged and treated as off, not fatal. This differs from admission, which is fatal: here nothing else depends on the client, and the refusal is fail-closed anyway.
- No Ingestion compose edit: the file is outside the unit's list, and the default must stay off.

## MP asks
None. Nothing appended to MP-ASKS.md.

## Not verified
- Nothing compiled or run (rule). The handler tests use the memory store only.
- No live audit-core round trip: the 202/200 handling and the producer == principal check are proven only against a test double and against audit-core's own handler code.
- The plan's deployment evidence (fixture-a with an org granted the override, `GET /v1/audit` shows the row, audit-core stopped gives 503 and no fixture request) needs W2-Q-S7a, WS7.H/I and the compose env. Not done.
- Lane-c depth rows: `.plan-work/quarry-w1w2/W23-depth-lane-c.md` (tag W2-CP-S7b).
