# Phase 0 — Frozen Specifications

This directory contains the authoritative, **frozen** specifications for the Model Plane v2.
Any change to these documents requires a documented ADR and a version bump.

| Document | Scope | Source of truth |
|----------|-------|-----------------|
| [`ids.md`](./ids.md) | Typed identifier formats | `rust/crates/mp-ids/src/lib.rs` |
| [`nats-subjects.md`](./nats-subjects.md) | NATS subject tree + compat | `rust/crates/mp-events/src/subjects.rs` |
| [`event-schemas.md`](./event-schemas.md) | Event envelope + payloads | `rust/crates/mp-events/src/envelope.rs`, `proto/model_plane/v1/events.proto` |
| [`temporal-workflows.md`](./temporal-workflows.md) | Workflow + activity contracts | `go/cmd/*` (session/inference/execution cores) |
| [`proto-contracts.md`](./proto-contracts.md) | gRPC + proto surface review | `proto/model_plane/v1/*.proto` |

## Freeze policy

- **Identifiers** (`mp-ids`): ULID format, types, ownership — locked.
- **Subjects** (`mp-events::subjects`): `mp.v1.*` tree — locked. Legacy mirrors remain until cutover.
- **Envelope** (`mp-events::envelope`): field set — locked. Payloads may evolve per event type via `schema_version`.
- **Proto**: `model_plane/v1/*` is the v1 wire contract. Breaking changes require `v2`.

Downstream phases (1–10) MUST consume these specs as immutable inputs.
