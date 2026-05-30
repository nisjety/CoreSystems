# Event Schemas — Phase 0 Freeze

**Envelope source of truth:** [`rust/crates/mp-events/src/envelope.rs`](../../rust/crates/mp-events/src/envelope.rs)
**Proto source of truth:** [`proto/model_plane/v1/events.proto`](../../proto/model_plane/v1/events.proto)
**Idempotency helper:** [`rust/crates/mp-events/src/idempotency.rs`](../../rust/crates/mp-events/src/idempotency.rs)

## 1. Envelope (frozen)

Every event published on `mp.v1.*` is wrapped in the canonical envelope. JSON encoding on NATS,
Postgres `jsonb` in the append-only events table.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `event_id` | ULID string | ✅ | Globally unique event identifier |
| `event_type` | string | ✅ | Discriminator (e.g. `SESSION_START`, `RUN_COMPLETED`) |
| `schema_version` | uint32 | ✅ | Schema version for `payload` of this `event_type` (≥ 1) |
| `ts` | RFC3339 timestamp (UTC) | ✅ | Producer wall-clock |
| `producer` | string | ✅ | Emitting service name (e.g. `session-core`) |
| `correlation_id` | string | ⚠️ | Trace correlation ID; empty string allowed |
| `causation_id` | string | ⚠️ | `event_id` of the direct cause; empty string allowed |
| `idempotency_key` | string | ⚠️ | Client-supplied dedup key; empty string allowed |
| `org_id` | string | ✅ | Tenant identifier |
| `user_id` | string | ⚠️ | Acting user; empty string allowed for system events |
| `resource_ref` | string | ⚠️ | Canonical resource reference (e.g. `thread/01HZ...`) |
| `payload` | JSON value | ✅ | Type-specific payload, shape governed by `event_type` + `schema_version` |

Validation (`Envelope::validate`) rejects empty `event_id`, `event_type`, `producer`, `org_id`,
and requires `schema_version ≥ 1`.

## 2. Encoding

- Wire format on NATS: JSON (`Envelope::to_json_bytes` / `from_json_bytes`).
- At-rest in Postgres: `jsonb`. Columns mirror top-level envelope fields; `payload` stored nested.
- Proto message `model_plane.v1.Event` is the strongly-typed mirror used for gRPC streaming.

## 3. Idempotency

- Publishers MUST set `idempotency_key` for any event that may be retried.
- Consumers MUST dedupe by `(producer, event_type, idempotency_key)` when key is non-empty,
  falling back to `event_id` otherwise.
- Helper: `mp_events::idempotency` exposes the canonical dedup key builder. Dedup window is a
  consumer-side policy, not part of the envelope.

## 4. Event catalogue (v1)

Payload schemas are versioned independently per `event_type` via `schema_version`. Every entry
below is **v1 unless noted**. Additive fields within a version are allowed; removals or
renames bump `schema_version`.

### 4.1 Session domain — `mp.v1.session.*` / `mp.v1.run.*`

| `event_type` | Producer | `resource_ref` | Payload keys |
|--------------|----------|----------------|--------------|
| `SESSION_START` | session-core | `session/{session_key}` | `agent_id`, `inputs`, `policy` |
| `SESSION_END` | session-core | `session/{session_key}` | `reason`, `final_state` |
| `RUN_STARTED` | session-core | `run/{run_id}` | `agent_id`, `session_key`, `input` |
| `RUN_COMPLETED` | session-core | `run/{run_id}` | `status`, `output`, `usage` |
| `RUN_FAILED` | session-core | `run/{run_id}` | `error_code`, `error_message`, `retryable` |
| `CHECKPOINT_CREATED` | session-core | `run/{run_id}` | `checkpoint_id`, `step_id` |

### 4.2 Execution domain — `mp.v1.run.{run_id}.event` (step sub-events)

| `event_type` | Producer | `resource_ref` | Payload keys |
|--------------|----------|----------------|--------------|
| `STEP_STARTED` | execution-core | `step/{step_id}` | `run_id`, `tool_or_model`, `input` |
| `STEP_COMPLETED` | execution-core | `step/{step_id}` | `output`, `latency_ms`, `usage` |
| `STEP_FAILED` | execution-core | `step/{step_id}` | `error_code`, `error_message`, `retryable` |
| `TOOL_CALL_REQUESTED` | execution-core | `step/{step_id}` | `tool`, `args` |
| `TOOL_CALL_RESULT` | execution-core | `step/{step_id}` | `output`, `error?` |

### 4.3 Inference domain — `mp.v1.run.{run_id}.event`

| `event_type` | Producer | Payload keys |
|--------------|----------|--------------|
| `INFERENCE_STARTED` | inference-core | `model`, `prompt_tokens`, `params` |
| `INFERENCE_TOKEN` | inference-core | `delta`, `seq` *(optional streaming)* |
| `INFERENCE_COMPLETED` | inference-core | `completion_tokens`, `latency_ms`, `usage` |
| `INFERENCE_FAILED` | inference-core | `error_code`, `error_message`, `retryable` |

### 4.4 Ingress / gateway — `mp.v1.ingress.*`

| `event_type` | Subject | Payload keys |
|--------------|---------|--------------|
| `INGRESS_ACCEPTED` | `mp.v1.ingress.accepted` | `request_id`, `route`, `org_id` |
| `INGRESS_REJECTED` | `mp.v1.ingress.rejected` | `request_id`, `reason` |
| *compat bridges* | see [`nats-subjects.md`](./nats-subjects.md#4-legacy-compatibility) | legacy pass-through |

### 4.5 Usage — `mp.v1.usage.{org_id}`

| `event_type` | Payload keys |
|--------------|--------------|
| `USAGE_RECORDED` | `meter`, `amount`, `unit`, `run_id?`, `model?`, `ts` |
| `QUOTA_EXCEEDED` | `meter`, `limit`, `current` |

### 4.6 Capability / lease — `mp.v1.stream.*` + per-lease subjects

| `event_type` | Payload keys |
|--------------|--------------|
| `SANDBOX_LEASED` | `sandbox_lease_id`, `expires_at` |
| `SANDBOX_RELEASED` | `sandbox_lease_id`, `reason` |
| `BROWSER_LEASED` | `browser_lease_id`, `expires_at` |
| `BROWSER_RELEASED` | `browser_lease_id`, `reason` |

## 5. Evolution policy

1. Adding a new `event_type`: additive, document it in §4 before first publish.
2. Adding an optional field to an existing payload at the same `schema_version`: allowed.
3. Removing / renaming / changing semantics of a field: bump `schema_version`; support the
   previous version for at least one release cycle.
4. Changing the envelope itself: requires a new proto major version and coordinated migration.

## 6. Review checklist

- [x] Every publisher in the codebase uses `mp_events::envelope::Envelope` (or the proto mirror).
- [x] Every event type in code is listed in §4 with producer, subject mapping, and payload keys.
- [x] Required fields in §1 cannot be empty in any fixture or publisher call-site.
- [x] `schema_version` is set explicitly by every producer; no implicit defaults.
