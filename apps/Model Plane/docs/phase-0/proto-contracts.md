# Proto Contracts — Phase 0 Review

**Location:** [`proto/model_plane/v1/`](../../proto/model_plane/v1/)
**Build:** `buf.yaml` + `buf.gen.yaml` at proto root.
**Consumers:** Rust (`tonic`/`prost`), Go (`grpc-go`), TypeScript (`connect-es`) via generated clients.

## 1. File inventory (v1)

| File | Scope | Depends on |
|------|-------|------------|
| `ids.proto` | Typed ID wrapper messages mirroring `mp-ids` | — |
| `events.proto` | `Event` envelope + event-type enum | `ids.proto` |
| `sessions.proto` | Session + run RPC surface (session-core) | `ids.proto`, `events.proto` |
| `runs.proto` | Run lifecycle types shared between cores | `ids.proto`, `events.proto` |
| `inference.proto` | Inference RPC + streaming messages | `ids.proto`, `events.proto` |
| `execution.proto` | Step execution + tool-call messages | `ids.proto`, `events.proto` |
| `capabilities.proto` | Capability registry (agents, tools) | `ids.proto` |
| `sandboxes.proto` | Sandbox manager RPCs | `ids.proto` |
| `browser.proto` | Browser broker RPCs | `ids.proto` |
| `memory.proto` | Memory / checkpoint store RPCs | `ids.proto` |
| `gateway.proto` | Public model-gateway RPC + auth claims | `ids.proto`, `sessions.proto` |

## 2. Package + versioning conventions

- Proto package: `model_plane.v1`.
- Go package: `model_plane/v1;modelplanev1`.
- Rust crate: `mp-contracts` (`prost`/`tonic` generated).
- All v1 messages are frozen. Breaking changes move to `model_plane.v2` in a new directory;
  v1 remains compilable and wire-compatible for the deprecation window.

## 3. Contract checklist (per file)

Every proto file MUST satisfy:

- [x] `syntax = "proto3";`
- [x] `package model_plane.v1;`
- [x] `option go_package = "…/model_plane/v1;modelplanev1";`
- [x] No `optional` on scalar fields unless absence is semantically distinct from the default.
- [x] Enums: first value is `*_UNSPECIFIED = 0`. Receivers treat unknown values as `UNSPECIFIED`.
- [x] IDs are `string` fields named after the Rust newtype (e.g. `session_key`, `run_id`).
- [x] Timestamps are `google.protobuf.Timestamp`.
- [x] Arbitrary payloads are `google.protobuf.Struct` or `bytes` with a documented encoding.
- [x] Every RPC documents the streaming mode and idempotency expectations in its comment.

## 4. RPC surface (authoritative list)

| Service | RPC | Streaming | Owner |
|---------|-----|-----------|-------|
| `GatewayService` | `SubmitRun`, `StreamRun`, `GetRun` | server-streaming on `StreamRun` | model-gateway |
| `SessionService` | `StartSession`, `EndSession`, `ListRuns`, `GetSession` | unary | session-core |
| `InferenceService` | `Invoke` | server-streaming (tokens) | inference-core |
| `ExecutionService` | `ExecuteStep` | server-streaming (step sub-events) | execution-core |
| `CapabilityService` | `RegisterAgent`, `GetAgent`, `ListTools` | unary | capability-core |
| `SandboxService` | `LeaseSandbox`, `ReleaseSandbox`, `ExecInSandbox` | `ExecInSandbox` bidi | sandbox-manager |
| `BrowserService` | `LeaseBrowser`, `ReleaseBrowser`, `Navigate` | unary + server-streaming | browser-broker |
| `MemoryService` | `PutCheckpoint`, `GetCheckpoint`, `ListCheckpoints` | unary | session-core (memory substore) |

The exact RPC signatures live in the `.proto` files; this table is the **freeze contract** for
which services own which surface.

## 5. Auth + metadata

All RPCs require the following gRPC metadata headers:

| Header | Semantics | Required |
|--------|-----------|----------|
| `authorization` | `Bearer <JWT>` with JWKS-verified claims (see model-gateway) | ✅ (except health) |
| `x-org-id` | Tenant assertion, cross-checked against JWT `org` claim | ✅ |
| `x-request-id` | Client-supplied correlation ID; propagated to envelopes | ⚠️ recommended |
| `traceparent` | W3C Trace Context | ✅ |
| `x-idempotency-key` | Optional dedup key mapped onto `Envelope.idempotency_key` | ⚠️ |

Services MUST reject requests missing required headers with `UNAUTHENTICATED` or `INVALID_ARGUMENT`
as appropriate — no silent defaults.

## 6. Evolution policy

- Add a new field: assign next free field number, default-empty compatible — **allowed in v1**.
- Add a new RPC: new method on an existing service — **allowed in v1**.
- Add a new service: new `.proto` file under `model_plane/v1/` — **allowed**.
- Remove / rename / renumber a field: **v2**.
- Change wire type of a field: **v2**.
- Change streaming mode of an RPC: **v2**.

## 7. Generation

- `buf generate` runs `buf.gen.yaml` plugins: Go gRPC, Go proto, TS connect.
- Rust is generated separately: `mp-contracts/build.rs` runs `tonic_build`/`prost-build`
  directly at crate build time. Generated artifacts are **not** checked in for Rust.
- Go generated code is checked in under `go/internal/genproto/`.
- TS clients are built on demand in the frontend plane.

## 8. Review checklist

- [x] All 11 v1 proto files compile with `buf lint` and `buf breaking` against the last release.
- [x] Every ID field in proto matches an `mp-ids` newtype name (snake_case).
- [x] Every streaming RPC documents its back-pressure / keepalive expectations in a trailing comment.
- [x] `GatewayService.SubmitRun` is the only public ingress; everything else is intra-mesh.
- [x] No `google.protobuf.Any` usage in v1 — payloads are typed or `Struct`/`bytes` with explicit docs.
