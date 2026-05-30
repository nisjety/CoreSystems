# Session-Core API Reference

> **Updated**: May 2026  
> **Version**: 0.3.0 — JetStream SSE, 004 approvals, pagination

---

## Overview

Session-core provides a unified HTTP API for managing interactive agent sessions, including:
- **Sessions**: User session lifecycle and state
- **Events**: Session event streaming (SSE)
- **Approvals**: Human approval workflows
- **Plans**: Execution plans and plan steps
- **Todos**: Task management within sessions
- **Lineage**: Subagent spawn relationships

### Base URL

```
http://localhost:3000/v1
```

### Authentication

All endpoints (except `/health`) require:
- **Header**: `Authorization: Bearer <token>`
  OR
- **Context**: User ID must be set in `user_id` context (set by auth middleware)

---

## Health Check

### GET /health

Quick liveness check.

**Response** (200 OK):
```json
{
  "status": "healthy",
  "service": "session-core",
  "version": "0.1.0"
}
```

---

## Sessions

### POST /sessions

Create a new user session.

**Request**:
```json
{
  "tenant_id": "tenant-xyz",
  "workspace_id": "ws-123",
  "org_id": "org-456",
  "plan_mode": false,
  "metadata": {
    "model_plane_version": "v2"
  }
}
```

**Response** (201 Created):
```json
{
  "session": {
    "id": "sess_abcd1234",
    "tenant_id": "tenant-xyz",
    "workspace_id": "ws-123",
    "user_id": "user-123",
    "org_id": "org-456",
    "status": "active",
    "plan_mode": false,
    "model_plane_version": "v1",
    "created_at": "2026-05-02T10:30:00Z",
    "updated_at": "2026-05-02T10:30:00Z"
  }
}
```

**Errors**:
- **400**: Missing required fields
- **403**: Org membership validation failed
- **500**: Internal error

---

### GET /sessions/:id/state

Get the current session state including pending approvals.

Approval resolution and pending approval reads are backed by the 004 `approvals` table. For compatibility, pending approvals are returned in the same envelope shape as before.

**Response** (200 OK):
```json
{
  "session": {
    "id": "sess_abcd1234",
    "status": "active",
    ...
  },
  "pending_approvals": [
    {
      "id": "appr_5678",
      "session_id": "sess_abcd1234",
      "tool_name": "file_delete",
      "status": "pending",
      "created_at": "2026-05-02T10:35:00Z"
    }
  ],
  "event_cursor": 42
}
```

---

### GET /sessions/:id/events

Subscribe to session events via Server-Sent Events (SSE).

**Query Parameters**:
- `after_sequence` (optional): Resume from event sequence number

**Response** (200 OK, `text/event-stream`):
```
id: 1
event: session.created
data: {"id":"evt_1","session_id":"sess_abc","type":"session.created","payload":{...},"created_at":"2026-05-02T10:30:00Z"}

id: 2
event: message.sent
data: {"id":"evt_2","session_id":"sess_abc","type":"message.sent","payload":{...},"created_at":"2026-05-02T10:31:00Z"}
```

**Transport behavior**:
- Replays missed events from Postgres first
- Streams live events from NATS/JetStream subject `velion.session.<session_id>.event`
- Falls back to Postgres polling if NATS subscription is unavailable

**Heartbeat**: Sent every 15 seconds (`: heartbeat\n\n`).

---

### POST /sessions/:id/messages

Send a message in a session.

**Request**:
```json
{
  "role": "user",
  "content": "Please analyze this dataset"
}
```

**Response** (200 OK):
```json
{
  "event": {
    "id": "evt_2",
    "session_id": "sess_abc",
    "sequence": 2,
    "event_type": "message.sent",
    "payload": {
      "role": "user",
      "content": "Please analyze this dataset"
    },
    "created_at": "2026-05-02T10:31:00Z"
  }
}
```

---

### POST /sessions/:id/approvals/:approval_id

Resolve a pending approval (approve or deny).

**Request**:
```json
{
  "approve": true,
  "feedback": "Approved with conditions"
}
```

**Response** (200 OK):
```json
{
  "status": "resolved"
}
```

---

### POST /sessions/:id/resume

Resume a paused session.

**Response** (200 OK):
```json
{
  "status": "resumed"
}
```

---

## Plans

### POST /plans

Create a new plan.

**Request**:
```json
{
  "run_id": "run-123",
  "thread_id": "thread-456",
  "author": "user-789",
  "state": "DRAFT",
  "summary": "Execute data pipeline",
  "metadata": {
    "priority": "high"
  }
}
```

**Response** (201 Created):
```json
{
  "plan": {
    "id": "plan_abcd1234",
    "run_id": "run-123",
    "thread_id": "thread-456",
    "author": "user-789",
    "state": "DRAFT",
    "summary": "Execute data pipeline",
    "metadata": {
      "priority": "high"
    },
    "created_at": "2026-05-02T10:40:00Z",
    "updated_at": "2026-05-02T10:40:00Z"
  }
}
```

**Note**: If `id` is not provided, a ULID will be auto-generated. If `author` is not provided, it defaults to the authenticated user.

---

### GET /plans/:plan_id

Get a plan by ID.

**Response** (200 OK):
```json
{
  "plan": { ... }
}
```

---

### GET /plans/thread/:thread_id

List all plans for a thread.

**Query Parameters**:
- `limit` (optional, default `50`, max `200`)
- `offset` (optional, default `0`)

**Response** (200 OK):
```json
{
  "plans": [
    { ... },
    { ... }
  ],
  "pagination": {
    "limit": 50,
    "offset": 0,
    "count": 2
  }
}
```

---

### PATCH /plans/:plan_id/state

Update a plan's state.

**Request**:
```json
{
  "state": "PROPOSED"
}
```

**Response** (200 OK):
```json
{
  "status": "updated"
}
```

**Valid States**: `DRAFT`, `PROPOSED`, `APPROVED`, `REJECTED`, `EXECUTING`, `COMPLETED`, `FAILED`, `SUPERSEDED`, `ARCHIVED`

---

### POST /plans/:plan_id/steps

Create a step within a plan.

**Request**:
```json
{
  "step_order": 0,
  "title": "Validate input data",
  "operation": "validate_dataset",
  "state": "PENDING"
}
```

**Response** (201 Created):
```json
{
  "step": {
    "id": "step_xyz789",
    "plan_id": "plan_abcd1234",
    "step_order": 0,
    "title": "Validate input data",
    "operation": "validate_dataset",
    "state": "PENDING",
    "created_at": "2026-05-02T10:41:00Z",
    "updated_at": "2026-05-02T10:41:00Z"
  }
}
```

---

### GET /plans/:plan_id/steps

List steps in a plan (sorted by step_order).

**Query Parameters**:
- `limit` (optional, default `50`, max `200`)
- `offset` (optional, default `0`)

**Response** (200 OK):
```json
{
  "steps": [
    { ... },
    { ... }
  ],
  "pagination": {
    "limit": 50,
    "offset": 0,
    "count": 2
  }
}
```

---

### PATCH /plans/:plan_id/steps/:step_id/state

Update a plan step's state.

**Request**:
```json
{
  "state": "RUNNING"
}
```

**Response** (200 OK):
```json
{
  "status": "updated"
}
```

**Valid States**: `PENDING`, `RUNNING`, `DONE`, `SKIPPED`, `FAILED`

---

## Todos

### POST /todos

Create a new todo.

**Request**:
```json
{
  "thread_id": "thread-456",
  "run_id": "run-123",
  "assignee": "user-999",
  "title": "Review data quality report",
  "description": "Check outliers and anomalies",
  "state": "PENDING",
  "priority": "HIGH",
  "blocked_by": [],
  "metadata": {
    "category": "review"
  }
}
```

**Response** (201 Created):
```json
{
  "todo": {
    "id": "todo_pqrs5678",
    "thread_id": "thread-456",
    "run_id": "run-123",
    "assignee": "user-999",
    "title": "Review data quality report",
    "description": "Check outliers and anomalies",
    "state": "PENDING",
    "priority": "HIGH",
    "blocked_by": [],
    "metadata": { ... },
    "created_at": "2026-05-02T10:45:00Z",
    "updated_at": "2026-05-02T10:45:00Z",
    "completed_at": null
  }
}
```

---

### GET /todos/:todo_id

Get a todo by ID.

**Response** (200 OK):
```json
{
  "todo": { ... }
}
```

---

### GET /todos/thread/:thread_id

List todos for a thread.

**Query Parameters**:
- `limit` (optional, default `50`, max `200`)
- `offset` (optional, default `0`)

**Response** (200 OK):
```json
{
  "todos": [ ... ],
  "pagination": {
    "limit": 50,
    "offset": 0,
    "count": 1
  }
}
```

---

### GET /todos/run/:run_id

List todos for a run.

**Query Parameters**:
- `limit` (optional, default `50`, max `200`)
- `offset` (optional, default `0`)

**Response** (200 OK):
```json
{
  "todos": [ ... ],
  "pagination": {
    "limit": 50,
    "offset": 0,
    "count": 1
  }
}
```

---

### PATCH /todos/:todo_id/state

Update a todo's state.

**Request**:
```json
{
  "state": "IN_PROGRESS"
}
```

**Response** (200 OK):
```json
{
  "status": "updated"
}
```

**Valid States**: `PENDING`, `IN_PROGRESS`, `BLOCKED`, `COMPLETED`, `CANCELLED`

---

### DELETE /todos/:todo_id

Delete a todo.

**Response** (200 OK):
```json
{
  "status": "deleted"
}
```

---

## Lineage

### POST /lineage

Create a lineage edge (parent → child run relationship).

**Request**:
```json
{
  "parent_run_id": "run-123",
  "child_run_id": "run-456",
  "role": "CODER"
}
```

**Response** (201 Created):
```json
{
  "edge": {
    "parent_run_id": "run-123",
    "child_run_id": "run-456",
    "role": "CODER",
    "spawned_at": "2026-05-02T10:50:00Z"
  }
}
```

**Roles**: `CODER`, `REVIEWER`, `RESEARCHER`, `EXPLORER`, `GENERIC`

---

### GET /lineage/:run_id/children

Get all child runs spawned by a parent run.

**Query Parameters**:
- `limit` (optional, default `50`, max `200`)
- `offset` (optional, default `0`)

**Response** (200 OK):
```json
{
  "children": [
    {
      "parent_run_id": "run-123",
      "child_run_id": "run-456",
      "role": "CODER",
      "spawned_at": "2026-05-02T10:50:00Z"
    }
  ],
  "pagination": {
    "limit": 50,
    "offset": 0,
    "count": 1
  }
}
```

---

### GET /lineage/:run_id/parents

Get all parent runs that spawned a child run.

**Query Parameters**:
- `limit` (optional, default `50`, max `200`)
- `offset` (optional, default `0`)

**Response** (200 OK):
```json
{
  "parents": [ ... ],
  "pagination": {
    "limit": 50,
    "offset": 0,
    "count": 1
  }
}
```

---

### DELETE /lineage

Delete a lineage edge.

**Request**:
```json
{
  "parent_run_id": "run-123",
  "child_run_id": "run-456"
}
```

**Response** (200 OK):
```json
{
  "status": "deleted"
}
```

---

## Error Responses

All endpoints return error responses in this format:

```json
{
  "error": "Human-readable error message",
  "details": "Optional additional context"
}
```

### HTTP Status Codes

| Code | Meaning |
|------|---------|
| 200 | Success (GET, PATCH, DELETE) |
| 201 | Created (POST) |
| 400 | Bad request (validation error) |
| 401 | Unauthorized (authentication required) |
| 403 | Forbidden (authorization failed, org membership denied) |
| 404 | Not found |
| 500 | Internal server error |

---

## Idempotency

The `POST /sessions` endpoint supports idempotent requests via the `X-Idempotency-Key` header.

```bash
curl -X POST http://localhost:3000/v1/sessions \
  -H "X-Idempotency-Key: key-12345" \
  -H "Content-Type: application/json" \
  -d '{...}'
```

If the same key is used within 24 hours, subsequent requests return a **409 Conflict** response.

---

## Implementation Status

| Feature | Status | Notes |
|---------|--------|-------|
| Sessions | ✅ Implemented | Full lifecycle + real-time SSE (NATS/JetStream with DB replay) |
| Plans | ✅ Implemented | Full API, CRUD + state transitions |
| Plan Steps | ✅ Implemented | Full API, ordered within plans |
| Todos | ✅ Implemented | Full API, filterable by thread/run |
| Lineage | ✅ Implemented | Parent/child spawn relationships |
| Approvals | ✅ Implemented | `ResolveApproval` and pending reads migrated to 004 `approvals` model |
| NATS JetStream SSE | ✅ Implemented | Live subject `velion.session.<session_id>.event` |
| Convex Mirror | ✅ Implemented | Auto-syncs sessions & messages to frontend |
| Org Validation | ✅ Implemented | Service-layer checks, can add middleware |

---

## Example Workflows

### Creating and Executing a Plan

```bash
# 1. Create a session
SESSION_ID=$(curl -X POST http://localhost:3000/v1/sessions \
  -H "Content-Type: application/json" \
  -d '{
    "tenant_id": "tenant-xyz",
    "workspace_id": "ws-123",
    "org_id": "org-456"
  }' | jq -r '.session.id')

# 2. Create a plan in the session
PLAN_ID=$(curl -X POST http://localhost:3000/v1/plans \
  -H "Content-Type: application/json" \
  -d "{
    \"run_id\": \"run-123\",
    \"thread_id\": \"$SESSION_ID\",
    \"state\": \"DRAFT\",
    \"summary\": \"Execute workflow\"
  }" | jq -r '.plan.id')

# 3. Add steps to the plan
curl -X POST http://localhost:3000/v1/plans/$PLAN_ID/steps \
  -H "Content-Type: application/json" \
  -d '{
    "step_order": 0,
    "title": "Validate",
    "state": "PENDING"
  }'

# 4. Propose the plan
curl -X PATCH http://localhost:3000/v1/plans/$PLAN_ID/state \
  -H "Content-Type: application/json" \
  -d '{"state": "PROPOSED"}'

# 5. Approve the plan
curl -X PATCH http://localhost:3000/v1/plans/$PLAN_ID/state \
  -H "Content-Type: application/json" \
  -d '{"state": "APPROVED"}'

# 6. Execute the plan
curl -X PATCH http://localhost:3000/v1/plans/$PLAN_ID/state \
  -H "Content-Type: application/json" \
  -d '{"state": "EXECUTING"}'
```

### Streaming Events

```bash
# Subscribe to session events with curl
curl -N http://localhost:3000/v1/sessions/$SESSION_ID/events?after_sequence=0

# In another terminal, send a message
curl -X POST http://localhost:3000/v1/sessions/$SESSION_ID/messages \
  -H "Content-Type: application/json" \
  -d '{
    "role": "user",
    "content": "Analyze the data"
  }'

# The event stream will receive:
# id: 1
# event: message.sent
# data: {...}
```

---

## Version History

- **0.3.0** (May 2026): JetStream-backed SSE, 004 approval migration, pagination on list endpoints
- **0.2.0** (May 2026): Added Plans, Todos, Lineage APIs
- **0.1.0** (Mar 2026): Initial release with Sessions, Events, Approvals
