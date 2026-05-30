# session-core — Current Gap Analysis

> Updated: 2026-05-02
> Module: github.com/I-Dacosta/CoreSystem/apps/session-core
> Build/Test: `go build ./...` and `go test ./...` passing

---

## 1. End-to-End Implemented

The following are fully wired (migration + repository + service + handler + route):

- Sessions lifecycle: create/state/messages/resume
- Event streaming: SSE with Postgres replay + live NATS/JetStream fan-out
- Approval resolution path: migrated to 004 `approvals` repository model
- Plans API: plan CRUD/state + plan steps API
- Todos API: create/get/list/update/delete
- Lineage API: create/list/delete edges
- Org membership validation in session creation
- Convex mirroring for session + message sync

---

## 2. Schema/Runtime Alignment

### Legacy vs current approval model

- Legacy table: `approval_queue` (001)
- Current runtime model: `approvals` (004)

Status:
- Session state pending approvals now read from `approvals` via `ApprovalRepository`.
- Approval decisions now write to `approvals` via `DecideApproval`.
- Legacy shape is still returned in state payload for API compatibility.

Remaining:
- Optional cleanup/deprecation plan for `approval_queue` table writes/reads in repository layer.

---

## 3. Pagination Status

List endpoints now support:
- `limit` (default 50, max 200)
- `offset` (default 0)

Applied to:
- `GET /v1/plans/thread/:thread_id`
- `GET /v1/plans/:plan_id/steps`
- `GET /v1/todos/thread/:thread_id`
- `GET /v1/todos/run/:run_id`
- `GET /v1/lineage/:run_id/children`
- `GET /v1/lineage/:run_id/parents`

Responses include a `pagination` object with `limit`, `offset`, and `count`.

---

## 4. SSE Architecture Status

Current behavior:
1. Parse cursor (`Last-Event-ID` / `after_sequence`)
2. Replay missing events from Postgres
3. Subscribe to live NATS subject: `velion.session.<session_id>.event`
4. Stream events in real-time over SSE
5. Fallback to Postgres polling if NATS subscription fails

Status: implemented and active.

---

## 5. Testing Status

Automated:
- Unit tests added for pagination parsing and SSE message decoding
- Unit tests added for approval model mapping compatibility
- Full Go test suite passes

Integration:
- `scripts/smoke_test_api.sh` now includes:
  - Pagination checks on list endpoints
  - SSE real-time check (`event: message.sent` observed)

Remaining:
- Add CI job to run smoke/integration tests against ephemeral environment.

---

## 6. Remaining Gaps (Prioritized)

### High
- Add dedicated endpoint set for creating/listing `approvals` (currently only resolve + state projection)
- Add validation for state transitions (currently handlers pass states through)

### Medium
- Add total-count support for pagination metadata where needed
- Add idempotency support beyond `POST /sessions`

### Medium/Low
- Add explicit migration/deprecation strategy for legacy `approval_queue`
- Add load test for SSE fan-out under concurrent sessions

---

## 7. Practical Completion Estimate

- API/runtime feature completion: ~95%
- Operational hardening (CI integration tests, transition validation, deprecation cleanup): remaining ~5%

