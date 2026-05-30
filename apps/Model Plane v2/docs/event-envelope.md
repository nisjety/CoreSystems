# Event Envelope — Model Plane v2

**Status:** Accepted (Phase 0 Contract Lock)
**Owner:** Platform / agent-core
**Last updated:** 2025

Every event emitted onto NATS, persisted to Postgres `events`, or forwarded to
downstream analytics MUST conform to this envelope. The envelope is a stable
contract — additive changes only. Breaking changes require bumping
`schema_version` and an ADR.

---

## 1. Envelope fields

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | string (SemVer) | ✅ | Envelope schema version. Starts at `1.0.0`. |
| `event_id` | UUIDv7 | ✅ | Globally unique event identifier. Minted by producer. |
| `event_type` | string, regex `^[a-z]+\.[a-z_]+\.[a-z_]+$` | ✅ | `<domain>.<resource>.<verb>`, e.g. `run.action.started`. |
| `event_timestamp` | RFC3339 nano (UTC) | ✅ | Producer clock at emission. |
| `producing_service` | string | ✅ | One of: `session-core`, `agent-core`, `capability-core`, `research-core`, `execution-core`, `llm-worker`, `quarry`. |
| `producing_instance` | string | ✅ | Hostname or pod name of the emitting instance. |
| `org_id` | UUIDv7 | ✅ | Tenant root. Required on every event. |
| `correlation_id` | UUIDv7 | ✅ | Trace-level correlation; equals root `run_id` when inside a run, else a synthetic ID. |
| `causation_id` | UUIDv7 | ⚠️ | `event_id` of the direct cause. NULL only for user-initiated root events. |
| `actor_type` | enum | ✅ | `user`, `agent`, `system`, `connector`. |
| `actor_id` | string | ✅ | `user_id`, `agent_id`, service name, or connector id per `actor_type`. |
| `session_id` | UUIDv7 | ⚠️ | Required if event occurs inside a session. |
| `thread_id` | UUIDv7 | ⚠️ | Required if event is attached to a conversation thread. |
| `run_id` | UUIDv7 | ⚠️ | Required for any event inside a run. |
| `parent_run_id` | UUIDv7 | ❌ | Present on sub-run events. |
| `action_id` | UUIDv7 | ⚠️ | Required on action-scoped events. |
| `tool_call_id` | string | ❌ | Present on tool-invocation events. |
| `approval_id` | UUIDv7 | ❌ | Present on approval lifecycle events. |
| `workspace_id` | string (`twk_…`) | ❌ | Present on sandbox-scoped events. |
| `sandbox_id` | UUIDv7 | ❌ | Present on sandbox-scoped events. |
| `skill_id` | string (`sk_…`) | ❌ | Present on skill-invocation events. |
| `skill_version` | SemVer | ❌ | Required whenever `skill_id` is present. |
| `idempotency_key` | string, max 255 | ⚠️ | Required on all state-mutating events. Scope: `(producing_service, org_id, idempotency_key)`. |
| `payload` | object | ✅ | Event-type-specific body. Validated against `payload_schema_ref`. |
| `payload_schema_ref` | string | ✅ | URI or registry key for the payload schema, e.g. `core/v1/run.action.started`. |
| `trace_context` | object | ❌ | W3C `traceparent` / `tracestate` propagated from inbound request. |

"⚠️ Required" means required when contextually applicable per the table in
§3; producers MUST NOT emit the event without it.

---

## 2. JSON Schema (envelope only, excluding `payload`)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://model-plane-v2/schemas/event-envelope-1.0.0.json",
  "title": "ModelPlaneV2 Event Envelope",
  "type": "object",
  "required": [
    "schema_version", "event_id", "event_type", "event_timestamp",
    "producing_service", "producing_instance", "org_id",
    "correlation_id", "actor_type", "actor_id",
    "payload", "payload_schema_ref"
  ],
  "properties": {
    "schema_version": { "type": "string", "pattern": "^\\d+\\.\\d+\\.\\d+$" },
    "event_id": { "type": "string", "format": "uuid" },
    "event_type": { "type": "string", "pattern": "^[a-z]+\\.[a-z_]+\\.[a-z_]+$" },
    "event_timestamp": { "type": "string", "format": "date-time" },
    "producing_service": {
      "type": "string",
      "enum": ["session-core","agent-core","capability-core","research-core","execution-core","llm-worker","quarry"]
    },
    "producing_instance": { "type": "string", "minLength": 1, "maxLength": 253 },
    "org_id": { "type": "string", "format": "uuid" },
    "correlation_id": { "type": "string", "format": "uuid" },
    "causation_id": { "type": ["string","null"], "format": "uuid" },
    "actor_type": { "type": "string", "enum": ["user","agent","system","connector"] },
    "actor_id": { "type": "string", "minLength": 1, "maxLength": 255 },
    "session_id": { "type": ["string","null"], "format": "uuid" },
    "thread_id": { "type": ["string","null"], "format": "uuid" },
    "run_id": { "type": ["string","null"], "format": "uuid" },
    "parent_run_id": { "type": ["string","null"], "format": "uuid" },
    "action_id": { "type": ["string","null"], "format": "uuid" },
    "tool_call_id": { "type": ["string","null"], "maxLength": 128 },
    "approval_id": { "type": ["string","null"], "format": "uuid" },
    "workspace_id": { "type": ["string","null"], "pattern": "^twk_[0-9A-HJKMNP-TV-Z]{26}$" },
    "sandbox_id": { "type": ["string","null"], "format": "uuid" },
    "skill_id": { "type": ["string","null"], "pattern": "^sk_[0-9A-HJKMNP-TV-Z]{16}$" },
    "skill_version": { "type": ["string","null"], "pattern": "^\\d+\\.\\d+\\.\\d+(-[\\w\\.\\-]+)?$" },
    "idempotency_key": { "type": ["string","null"], "maxLength": 255 },
    "payload": { "type": "object" },
    "payload_schema_ref": { "type": "string", "minLength": 1 },
    "trace_context": { "type": ["object","null"] }
  },
  "additionalProperties": false
}
```

---

## 3. Contextual requirement matrix

| Event category | `session_id` | `thread_id` | `run_id` | `action_id` | `idempotency_key` |
|---|---|---|---|---|---|
| `session.*` | ✅ | ❌ | ❌ | ❌ | ✅ |
| `thread.*` | ✅ | ✅ | ❌ | ❌ | ✅ |
| `run.created` / `run.state_changed` | ✅ | ✅ | ✅ | ❌ | ✅ |
| `run.action.*` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `run.tool_call.*` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `run.approval.*` | ✅ | ✅ | ✅ | ❌ | ✅ |
| `capability.connector.*` | ❌ | ❌ | ⚠️* | ❌ | ✅ |
| `execution.sandbox.*` | ❌ | ❌ | ⚠️* | ❌ | ✅ |
| `research.thread.*` | ✅ | ⚠️** | ❌ | ❌ | ✅ |

\* Required when the operation was triggered from inside a run.
\*\* Use `research_thread_id` instead; carried inside `payload` until promoted.

---

## 4. Idempotency rules

- All state-mutating events MUST carry `idempotency_key`.
- Consumers dedupe on `(producing_service, org_id, idempotency_key)` with a
  minimum retention of 24h.
- Producers SHOULD derive the key deterministically from the causal inputs
  (e.g. `sha256(run_id || action_id || retry_attempt)`).
- Purely informational events (e.g. `*.heartbeat`) MAY omit the key.

---

## 5. Compatibility & evolution

- Additive changes (new optional fields) do not bump `schema_version`.
- Removals, renames, or semantic changes bump minor (`1.1.0`) or major
  (`2.0.0`) and require an ADR.
- Consumers MUST ignore unknown fields.
- `additionalProperties: false` is enforced on the envelope only; payloads
  govern their own extensibility.

---

## 6. Cross-references

- Identifiers — `identifiers.md`
- Run state machine — `run-state-machine.md`
- Ownership — `adr/ADR-001-service-ownership-matrix.md`
- Envelope decision — `adr/ADR-002-event-envelope.md`
