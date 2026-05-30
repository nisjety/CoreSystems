# ADR-002: Event Envelope

- **Status**: Accepted
- **Date**: 2025-01-XX
- **Deciders**: Model Plane v2 architecture working group
- **Supersedes**: none
- **Superseded by**: none

## Context

Model Plane v2 is polyglot (Go control plane, Rust hot paths, Python ML
workers) and emits events to three sinks: NATS JetStream subjects, the
Postgres `events` table, and the analytics pipeline. Today each service
invents its own payload shape: some include `tenant_id`, some `org_id`,
some omit timestamps, and none carry a correlation identifier.

The consequences are concrete:

- Cross-service tracing is impossible — a `run.started` in `agent-core`
  cannot be stitched to a `llm.completion` in `llm-worker`.
- Idempotency cannot be enforced at the bus layer because there is no
  stable dedup key.
- Replay and dead-lettering are per-service ad hoc code, not a platform
  capability.
- Schema evolution has no versioning contract — a breaking payload change
  silently corrupts downstream consumers.

Phase 0 of the roadmap requires a single event envelope before any
observability, orchestration, or replay work can begin.

## Decision

Every event emitted to NATS, persisted to the `events` table, or shipped
to analytics MUST carry the uniform envelope defined in
`docs/event-envelope.md`. That document is **normative**; this ADR ratifies
its enforcement and scope.

### Always-required fields

| Field | Type | Notes |
|---|---|---|
| `schema_version` | string | SemVer, e.g. `1.0.0` |
| `event_id` | UUIDv7 | Globally unique, time-ordered |
| `event_type` | string | Regex `^[a-z]+\.[a-z_]+\.[a-z_]+$` (domain.entity.action) |
| `event_timestamp` | string | RFC3339 nanosecond UTC |
| `producing_service` | enum | `session-core`, `agent-core`, `capability-core`, `research-core`, `execution-core`, `llm-worker`, `quarry` |
| `org_id` | UUID | Tenant boundary |
| `correlation_id` | UUID | Traces logical operation across services |
| `actor_type` | enum | `user`, `agent`, `system`, `scheduler` |
| `actor_id` | string | Opaque ID of the actor |
| `payload` | object | Event-specific body |
| `payload_schema_ref` | string | URI to JSON Schema for `payload` |

### Contextual-required fields

Per `docs/event-envelope.md` §3, the following are required when the
event is produced in the corresponding scope:

- `run_id` — required for any event emitted inside a run
- `session_id` — required for any event tied to a session
- `action_id` — required for action-scoped events (execution-core)
- `idempotency_key` — required on any event that may be retried by its
  producer; forms the dedup tuple `(producing_service, org_id, idempotency_key)`
- `causation_id` — the `event_id` of the event that caused this one, when applicable

### Boundary rules

1. **Emitters MUST populate the envelope.** No service may publish to NATS
   or insert into `events` without it.
2. **Consumers MUST validate `schema_version`.** A missing or
   unrecognised version MUST route the message to the dead-letter stream.
3. **Unknown `event_type` → dead-letter.** Consumers MUST NOT silently
   drop events they do not recognise.
4. **Evolution is additive-only within a major version.** New optional
   fields may be added; existing fields MUST NOT change type or semantics.
5. **Breaking changes bump `schema_version` major and require a new ADR.**

## Alternatives Considered

- **CloudEvents v1.0** — rejected: no first-class `org_id`, `run_id`, or
  `session_id`; extensions are untyped and unvalidated in existing
  tooling; would still require a superset envelope.
- **Minimal envelope (id + type + payload)** — rejected: provides no
  schema evolution path, no idempotency, no tenant scoping. Revisits the
  exact problem this ADR solves within a quarter.
- **Per-service schemas with shared header** — rejected: breaks cross-
  service tracing and makes dead-lettering per-service, reintroducing the
  status quo.

## Consequences

### Positive
- Cross-service tracing via `correlation_id` / `causation_id` becomes
  mechanical.
- Idempotency is enforceable at the bus layer via the `(producing_service,
  org_id, idempotency_key)` tuple.
- Replay and dead-lettering become platform features, not per-service
  rewrites.
- Schema evolution has a documented contract, unblocking future payload
  changes without downstream surprise.

### Negative
- ~200 bytes of envelope overhead per event.
- Existing emitters (agent-core, llm-worker, quarry) require a migration
  pass to add envelope fields; dual-write is required during rollout.

### Neutral
- Unblocks ADR-003 (run state machine), which depends on reliable
  `run.*` events.
- Powers Phase 3 observability work (distributed traces, SLO dashboards).

## Compliance

- CI schema validation gate: every event fixture under `test/events/`
  MUST validate against `schemas/event-envelope.schema.json`.
- Lint rule: any `nats.Publish` / `Conn.Publish` / `jetstream.Publish`
  call whose payload does not include the envelope fields MUST fail CI.
- Each service's `README.md` MUST declare the `event_type` values it
  emits and consumes, in a machine-parseable block.

## References

- `docs/event-envelope.md` — normative field specification
- `docs/identifiers.md` — canonical ID definitions
- ADR-001 — Service ownership matrix (defines `producing_service` enum)
- ADR-003 — Run state machine (consumer of `run.*` events)
