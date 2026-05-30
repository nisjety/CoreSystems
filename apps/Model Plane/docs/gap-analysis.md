# Model Plane v2 — Gap Analysis

Planning note: this file captures the earlier foundation-gap view.

For the expanded parity plan against `Model Plane v2`, `claude-code-fork`, and the external ecosystem references, use:

- `docs/GOAL.md`
- `docs/ROADMAP.md`
- `docs/PLAN.md`
- `docs/REFERENCE-MATRIX.md`

> Reconciled against `docs/VERIFICATION.md` (Phase F closed 4/4). Status reflects the 18 / 30 verification gates currently green. Evidence pointers link each claim back to VERIFICATION.md or the owning source file.

---

## 1. Rust Services

### execution-core
- **Status:** gRPC server + checkpoint path wired; runtime loop and sandbox dispatch still scaffolded.
- ✅ Checkpoint gRPC handler live at `rust/services/execution-core/src/grpc.rs:67-91`.
- ✅ Temporal activity contracts registered (see § 7).
- **Gaps:**
  - No pre-persist **secret scrub** on checkpoint payloads — tracked as Security gate "No secret material in sandbox payloads" (**PR-2**).
  - No WASM/container dispatch loop.
  - No artifact capture / streaming output.
  - No resource-limit enforcement beyond Temporal retries.

### inference-core
- **Status:** Minimal stub.
- **Gaps:**
  - No provider adapters (OpenAI, Anthropic, local).
  - No streaming SSE/gRPC relay.
  - No token-counting / cost tracking.
  - No retry/fallback logic.
  - No model-capability resolution.

### session-core
- **Status:** gRPC `GetContextAssembly` implemented; transactional outbox emits `THREAD_CREATED` + `MESSAGE_APPENDED` in-tx.
- ✅ Assembly path at `rust/services/session-core/src/grpc.rs:611-884`.
- ✅ Transactional outbox: `create_thread` (`rust/services/session-core/src/grpc.rs:78-149`) and `append_message` (`rust/services/session-core/src/grpc.rs:151-220`) write the domain row and the corresponding event envelope inside the same Postgres transaction, using `mp_events::derive_idempotency_hash` for cross-language parity. Tests: `rust/services/session-core/tests/outbox_emission.rs` 4/4 PASS. Real-DB end-to-end verification deferred to the Postgres integration harness.
- **Gaps:**
  - No latency SLO harness on `GetContextAssembly` — tracked as Performance gate "Context assembly completes within token budget" (**PR-8**).
  - No session state-machine persistence or multi-turn history.
  - No tool-call orchestration.

### model-gateway
- **Status:** Auth/JWKS + rate-limiter + header scrubbing wired.
- ✅ JWT/JWKS middleware (Gate 7): `cargo test -p model-gateway --lib auth::` → 10/10 PASS.
- ✅ Rate limiter at `rust/services/model-gateway/src/rate_limit.rs` (156 LOC).
- ✅ PR-4 (header scrubbing) + PR-5 (gateway rate-limit interceptor) closed — see VERIFICATION.md §Security.
- **Gaps:**
  - No upstream health checks / circuit breaker.
  - No per-provider cost telemetry in streaming path.

---

## 2. Go Control Plane

### Existing services (6)
`browser-broker`, `capability-core`, `letta-bridge`, `model-gateway`, `orchestrator-core`, `sandbox-manager`

### Status
- ✅ **Envelope round-trip** (Go ⇄ Rust ⇄ Python): 14/14 Go, 26/26 Rust, 6/6 Python. Evidence: VERIFICATION.md § Contract.
- ✅ **Cross-language idempotency parity**: `TestDeriveIdempotencyHash_MatchesRust` matches golden hex `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637`.
- ✅ **Temporal registrations**: `pkg/temporalreg` exposes 5 workflows + 10 activities across 3 task queues (`mp-session`, `mp-inference`, `mp-execution`). Go test 4/4 PASS.
- ✅ **Workflow supervision**: Phase F closed — cancel propagation, compensation (SAGA), approval durability (`cmd/workflows/...` 11 PASS + 2 SKIP, fixture-gated).
- ✅ **browser-broker grant primitives**: `ErrGrantRevoked` + `Revoke()` at `go/services/browser-broker/internal/grant/grant.go`.
- **Gaps:**
  - No **post-revoke denial test** in `browser-broker/server_test.go` — Security gate "Browser lease revocation prevents further use" (**PR-3**).
  - `envelope`, `idempotency`, `natsx` packages lack fuzz tests.
  - No standardized `/healthz` / `/readyz` across services.

---

## 3. Python Lab

### Status
- ✅ **mp-events-py** shipped (Slice 8): pydantic v2 model + canonical idempotency hash; 6/6 pytest PASS; Python 3.14.3.
- **Gaps:**
  - No producer helpers (Slice 8 deferred).
  - No working FastAPI lab services.
  - No NATS consumer integration.
  - No experiment-tracking logic, notebook pipeline, or model-registry CRUD.

---

## 4. Protobuf / Contracts

| Item | Status | Evidence / Gap |
|------|--------|----------------|
| `buf lint` / `buf breaking` | ✅ CI-enforced (Slice 6) | `.github/workflows/buf.yml` + `pkg/cibuf` gate test |
| Event envelope golden fixture | ✅ Round-trips Rust ⇄ Go ⇄ Python | `tests/fixtures/envelope_valid.json` |
| Required-field validation (10) | ✅ Parity all three languages | Go 14/14, Rust 26/26, Python 6/6 |
| Local `buf` CLI | ❌ Absent | `prost-build` + `tonic-build` remain the local proxy |
| `.proto` coverage for mp-events types | 🟡 Partial | Rust structs canonical; not all types in `.proto` |

---

## 5. Event System (mp-events)

| Required field | Status |
|----------------|--------|
| `event_id`, `event_type`, `schema_version`, `producer`, `org_id`, `ts`, `correlation_id`, `idempotency_key`, `user_id`, `resource_ref` | ✅ All 10 validated on Go, Rust, Python |

| Behaviour | Status |
|-----------|--------|
| `schema_version == 0` rejected | ✅ |
| Replay determinism (4 gates) | ✅ `replay_deterministic.rs` — cursor/limit/tiebreak/final-state |
| Runtime lifecycle (4 gates) | ✅ `runtime_lifecycle.rs` — canonical order, taxonomy coverage, failure path, subagent lineage |
| Ordered delivery e2e | ❌ Not yet covered at transport layer |
| Dead-letter handling | ❌ Missing |

---

## 6. NATS Subjects

| Item | Status |
|------|--------|
| `velion.*` subject parity (Go ⇄ Rust) | ✅ Byte-identical constants + helpers |
| `aqencia.*` subject parity (Go ⇄ Rust) | ✅ 5 constants + `LEGACY_AQENCIA_WILDCARD` |
| Active legacy wildcard subscribers (Go) | ✅ `orchestrator-core/cmd/main.go:112` subscribes to `LegacyRunEventsWildcard`, `LegacySessionCommandWildcard`, `LegacyAqenciaWildcard` |
| `CompatMode` (off / legacy_only / dual_read / dual_write) | ✅ Go `pkg/natsx/mode.go` + `compat.go` `LegacyMappings`; Rust mirror in `mp-events::subjects` |
| Rust compat subscriber runtime wire-up | ❌ Constants present; no active Rust subscriber / dual-read owner |
| JetStream StreamSpec + ConsumerSpec parity | ✅ 10 + 7 fields; enum tokens lowercase-identical |

- **Gap:** Service-level **dual-write / dual-read correctness** in orchestrator-core (**PR-6**).
- **Gap:** **4-mode feature-flag toggle matrix** (off / legacy_only / dual_read / dual_write) (**PR-7**).

---

## 7. Temporal Workflows

| Item | Status |
|------|--------|
| Task queues | ✅ `[mp-session, mp-inference, mp-execution]` |
| Workflow definitions | ✅ 5 total — `SessionWorkflow`, `RunWorkflow`, `InferenceWorkflow`, `ExecutionWorkflow`, + interactive run |
| Activity implementations | ✅ 10 total (8 unique sorted in registry) — `PersistRunStart/End`, `EmitEvent`, `CreateCheckpoint`, `InvokeModel`, `RecordUsage`, `ExecuteStep`, `ToolCallActivity` |
| Registry parity (Go ⇄ Rust) | ✅ `pkg/temporalreg` 4/4 PASS; `mp-events::temporal` 4/4 PASS |
| Supervision: cancel + compensation + approval durability | ✅ Phase F (`cmd/workflows/...`) |
| Retry/saga policies | 🟡 Compensation path proven; dedicated retry-policy catalog not yet canonical |

---

## 8. Verification Gates (VERIFICATION.md)

**Checked: 30 / 31** — 1 open gate (PR-2.5 cross-plane vendor secret redaction). Remaining work is feature delivery from `PLAN.md` (orchestration-shell parity, capability platform parity, multimodal runtime, knowledge plane, operator shell, token efficiency).

### Closed since last revision

**Security (all 6 closed)** — see `VERIFICATION.md` §Security Tests.
- ✅ No secret material in sandbox payloads — pre-persist scrub live in `execution-core`.
- ✅ Browser lease revocation (PR-3) — `ValidateGrant` gRPC maps revoked/expired to `FailedPrecondition`; `internal/server/server_test.go::TestValidateGrant_*` PASS.
- ✅ Internal headers not leaked (PR-4) — `ScrubInternal`/`SendSafeHeader` + `UnaryHeaderScrubInterceptor` in `go/services/browser-broker/internal/server/headers.go`; tests PASS.
- ✅ Rate limiting at gateway boundary (PR-5) — `UnaryRateLimitInterceptor` in `internal/server/ratelimit.go` returns `ResourceExhausted`; tests PASS.
- ✅ Auth middleware rejects missing/invalid Bearer — `cargo test -p model-gateway --lib auth::` 10/10 PASS.

**Performance (1 closed, 3 open)**
- ✅ Streaming p95 first-token latency < 200 ms — measured in `inference-core` streaming path.

### Open gates (5) — grouped by domain

**Performance (3 open of 4)** — harness crate still pending (tracked under **PR-8**).
- Step throughput > 10 steps/second per run
- Checkpoint recovery time < 5 s
- Context assembly completes within token budget

**Migration (4 open of 4)**
- Compat adapter translates all legacy subjects correctly
- Dual-write consistency verified where adapters active
- Dual-read returns identical results from old and new paths
- Feature flag toggles cleanly between old and new service paths

---

## 9. Testing

| Category | Status | Notes |
|----------|--------|-------|
| Rust unit + integration | ✅ 57+ passing | per-crate, per-gate evidence in VERIFICATION.md |
| Go unit | ✅ Green | envelope 14, jetstream N, temporalreg 4, workflows 11 PASS + 2 SKIP |
| Python unit | ✅ 6/6 | mp-events-py |
| Cross-service integration | ❌ None | Covered by **PR-6** (orchestrator dual-write/read) |
| Performance / load | ❌ None | **PR-8** SLO harness crate |
| Migration toggle matrix | ❌ None | **PR-7** 4-mode test |
| Fuzz tests (envelope / idempotency / natsx) | ❌ None | Deferred |

---

## 10. Infrastructure

| Item | Status |
|------|--------|
| Postgres schemas | 🟡 Partial (mp-contracts sqlx migrations) |
| MinIO bucket setup | ❌ Not configured |
| Redis caching layer | ❌ Not integrated |
| NATS JetStream streams | ✅ StreamSpec + ConsumerSpec parity (Go + Rust) |
| Temporal namespace `model-plane` | ✅ Referenced by registry; runtime provisioning pending |
| Docker Compose for Model Plane | ❌ Not unified |
| CI pipeline | 🟡 `buf.yml` live; broader test matrix pending |

---

## 11. Priority Actions (Phase F+1 closure)

Phase 0 freezes landed. Remaining closure is execution-focused, tracked in § 12.

1. ✅ **ID formats** — `mp-ids` ULID newtypes (Rust 4/4 PASS).
2. ✅ **Event envelope required fields** — all 10 validated in 3 languages.
3. ✅ **NATS subject registry** — `velion.*` + `aqencia.*` byte-identical Go ⇄ Rust.
4. ✅ **Temporal workflow names** — canonical in `pkg/temporalreg` + `mp-events::temporal`.
5. 🟡 **Protobuf schemas** — `buf` CI live; local `.proto` expansion pending.
6. 🟡 **Verification gates** — 30/31 closed; PR-2.5 cross-plane vendor secret redaction open.

---

## 12. Open Gate Ownership Matrix (remaining)

| Gate | Owner plane(s) | Scope | Status |
| ---- | -------------- | ----- | ------ |
| **PR-2.5 — Cross-plane vendor secret redaction** | Control Plane (billing-core, auth-core), Ingestion Plane (integration-core) | Vendor-specific secret redaction at the boundary that owns each vendor: Stripe API keys (billing-core `adapter.go`, `config.go`), Twilio Verify tokens (auth-core `twilio-verify.service.ts`), Nango/provider-catalog credentials (integration-core `provider-catalog.ts`, `app-config.ts`). Stripe-in-Ingestion mismatch: read-only mirror or remove. Complements PR-2 execution-core generic last-defense scrubber. | 🟡 Open |

### Closed

| Gate | PR | Evidence |
|------|----|----------|
| Feature-flag toggle matrix | **PR-7** | `go/pkg/natsx/mode_matrix_test.go` — `TestCompatMode_{PublishFanoutMatrix,SubscribeFanoutMatrix,RoundTripHandlerReceivesCanonical,DualReadDeduplication,LegacyOnlyRequiresMapping}` — all PASS (0.203s, `pkg/natsx`). Covers 4 modes × both legacy-mapped subjects + dedup + rollback-guard. |
| Compat adapter translates all legacy subjects | **Gate #4** | `go/pkg/natsx/compat_matrix_test.go` — `TestLegacyMappings_{EveryEntryTranslates,PatternsAreUnique,ReversibleSubjectsRoundTrip,UnknownSubjectIsIdentity}`. Data-driven over `LegacyMappings`; every declared entry is exercised automatically. PASS (0.210s). |
| Dual-write consistency (mechanism + service) | **PR-6** | `go/pkg/natsx/compat_matrix_test.go::TestPR6_DualWriteDeliversIdenticalPayloadToBothSubjects` (byte equality at publisher layer) PLUS `go/services/orchestrator-core/internal/compat/e2e_test.go::TestServiceE2E_DualWriteConsistency` + `TestServiceE2E_DualWriteLegacyArrivalStillTranslated` (end-to-end through production wiring). Self-loop guard in `internal/compat/subscriber.go::HandleLegacyMessage` (producer≡compat-adapter OR schema_version>0) regression-guarded by `TestServiceE2E_SelfLoopGuard`. |
| Dual-read equivalence (mechanism + service) | **PR-6** | `go/pkg/natsx/compat_matrix_test.go::TestPR6_DualReadEquivalentToLegacyOnlyAndV1Only` (handler-contract equivalence across modes) PLUS `go/services/orchestrator-core/internal/compat/e2e_test.go::TestServiceE2E_DualReadEquivalence` — v1 consumer observes both native-v1 and bridged-legacy events exactly once, canonical subject, preserved EventID. Full orchestrator-core suite PASS. |
| Step throughput > 10/s per run | **PR-8** | `rust/services/execution-core/tests/slo.rs::step_throughput_meets_slo` drives 200 `runtime_loop::execute_step` invocations through `mp-slo::harness::step_throughput`; p99 inter-step < 100 ms. `synthetic_slow_step_surfaces_breach` is the regression guard. `cargo test -p execution-core --test slo` 4/4 PASS. |
| Checkpoint recovery < 5 s | **PR-8** | `rust/services/execution-core/tests/slo.rs::checkpoint_recovery_cpu_path_meets_slo` wraps a 512-step checkpoint build → scrub → serde round-trip in `mp-slo::harness::checkpoint_recovery_ms`; elapsed < 5 s. Covers CPU-bound recovery path. |
| Context assembly within token budget | **PR-8** | `rust/services/session-core/src/grpc.rs::tests::context_assembly_meets_slo` wires `mp-slo::harness::context_assembly` against real `assemble_segments` over 50 iterations on a large fixture; asserts p95 latency ≤ SLO target AND `estimated_tokens ≤ max_tokens` every iteration. |
| SLO harness primitives | **PR-8** | `rust/crates/mp-slo` — 19 unit/integration tests PASS (`cargo test -p mp-slo`). `first_token_latency`, `step_throughput`, `checkpoint_recovery_ms`, `context_assembly` harnesses plus named SLO catalog + deterministic nearest-rank percentiles. |

### Closed (moved out of matrix — see VERIFICATION.md)

| Gate | PR | Evidence |
|------|----|----------|
| No secret material in sandbox payloads | PR-2 | Checkpoint scrub live in execution-core |
| Browser lease revocation prevents further use | PR-3 | `TestValidateGrant_RevokedReturnsFailedPrecondition` PASS |
| Internal headers not leaked | PR-4 | `TestScrubInternal` / `TestSendSafeHeader_StripsInternalPrefixes` PASS |
| Rate limiting at gateway boundary | PR-5 | `TestRateLimit_*` + `UnaryRateLimitInterceptor` PASS |
| Streaming p95 < 200 ms first token | part of PR-8 | measured; remaining 3 perf gates still need harness |

---

## Summary

| Area | Done | Remaining |
|------|------|-----------|
| Rust crate structure | ✅ 100% | — |
| Rust service logic | ~70% | inference-core adapters live; execution-core runtime loop live; SLO harness live (PR-8); `session-core/src/orchestration_store.rs` 371 LOC scaffolded — needs schema migration + handler wiring |
| Go control plane | ~75% | Dual-write/read wiring, post-revoke enforcement test, fuzz, plus 12 orchestration RPC handlers Unimplemented (see § 13) |
| Python lab | ~20% | mp-events-py ✅; producer helpers + FastAPI services pending |
| Protobuf contracts | ~60% | `buf` CI ✅; `.proto` expansion pending |
| Event system | ~75% | Required fields ✅; dead-letter + ordered-delivery pending |
| NATS integration | ~80% | Go subscribers + CompatMode ✅; dual-read wiring (PR-6/7) |
| Temporal workflows | ~80% | 5 wf + 10 act registered; retry-policy catalog pending |
| Verification gates | **30/31 foundation closed** | PR-2.5 cross-plane vendor secret redaction open; 8 Phase 9 parity gates open (lint+test all TBD) |
| Testing | ~60% | Unit ✅; integration / perf / fuzz pending |
| Infrastructure | ~25% | Compose unification + MinIO + Redis pending |

---

## 13. Stub Replacement Inventory (added 2026-04-27)

This section is the canonical list of in-tree stubs that must be replaced with real implementations. Each row names the file, what is fake about it today, and what "real" means for cutover.

### 13.1 Orchestration RPC handlers — 12 Unimplemented

Owner: `go/services/orchestrator-core/internal/orchestration/handlers.go`

| RPC | Status | Replacement target |
|---|---|---|
| `ListPlans` | Unimplemented | Read from `session-core/orchestration_store` plan table |
| `GetPlan` | Unimplemented | Same |
| `TransitionPlan` | Unimplemented | Write through Temporal signal + emit `PLAN_TRANSITIONED` event |
| `ListTodos` | Unimplemented | Read from orchestration_store todo table |
| `GetTodo` | Unimplemented | Same |
| `TransitionTodo` | Unimplemented | Write + emit `TODO_TRANSITIONED` event |
| `ListApprovals` | Unimplemented | Read from orchestration_store approval table |
| `GetApproval` | Unimplemented | Same |
| `DecideApproval` | Unimplemented | Write + emit `APPROVAL_DECIDED` event + signal Temporal workflow |
| `GetSubagentLineage` | Unimplemented | Read subagent_lineage table |
| `AttachSubagent` | Unimplemented | Write + emit `SUBAGENT_ATTACHED` event |
| `StreamRunEvents` | Unimplemented | JetStream consumer multiplexed onto gRPC server stream |

Source comment: `// All RPCs are stubbed with codes.Unimplemented for now; T4–T6 will fill in`.

### 13.2 In-memory backings that must move to durable stores

| Component | Today | Target |
|---|---|---|
| `capability-core/internal/registry` | In-process map; lost on restart | Postgres-backed registry with migrations |
| `capability-core/internal/policy` | In-process engine with hardcoded checks | Postgres-stored policy rules + scope resolution |
| `letta-bridge/internal/memstore` | In-memory `Store` (no real Letta) | Real Letta gRPC/HTTP client + retry + circuit breaker |
| `letta-bridge/internal/{retrieval,sync,client}` | Stubbed packages exist | Wire real upstream Letta endpoints |
| `session-core/src/orchestration_store.rs` | Module scaffolded (371 LOC), no migrations applied at runtime | Postgres tables (`plans`, `todos`, `approvals`, `subagent_lineage`, `run_events`) with sqlx migrations + transactional outbox emission |

### 13.3 Stub-mode service paths

| Service | Stub trigger | What it does today | Replacement |
|---|---|---|---|
| `go/services/model-gateway/cmd/main.go` | `session-core` or `inference-core` dial fails | Falls back to `Unimplemented` for `Invoke` / `InvokeStream` (`internal/server/server.go:36,48`) | Required-dependency mode: fail fast at startup if downstream not reachable in non-dev profile |
| `go/services/model-gateway/internal/proxy/proxy.go:170` | Request without `thread_id` | Returns error: "thread_id is required (thread autocreation not yet implemented)" | Implement gateway-side thread autocreation against `session-core.CreateThread` |
| `go/services/sandbox-manager/internal/server/server.go:82` | Pre-codec phase | Some handlers Unimplemented per cmd note | Wire generated codecs + real lease lifecycle |
| `go/services/browser-broker/internal/server/server.go:80` | Hand-authored ServiceDesc | Handlers return Unimplemented before generated stubs | Same — buf-generated codecs |

### 13.4 Concepts described in docs but not implemented

| Concept | Doc claim | Reality | Action |
|---|---|---|---|
| Redis hot cache | ARCHITECTURE.md "Redis hot cache, idempotency dedup, rate limiter state" | Not integrated end-to-end; rate limiter in-process only | Wire Redis adapter; move idempotency dedup to Redis |
| MinIO artifact bucket layout | CONTRACTS.md MinIO Object Key Conventions | No bucket provisioning; no client wired in execution-core | Provision bucket + add MinIO client in execution-core artifact path |
| Temporal namespace `model-plane` | ARCHITECTURE.md, registry references | Registered in `pkg/temporalreg`; runtime namespace provisioning still pending | Add Temporal namespace bootstrap to deploy compose |
| Unified docker compose | README references `deploy/` | No unified compose covering Postgres+NATS+Redis+MinIO+Temporal+all 9 services | Build single `deploy/docker-compose.yml` parity with v2 |
| Dead-letter handling | implied by event system | Missing | Add DLQ stream + handler |
| Ordered delivery e2e | implied by event system | Replay-tier proven; transport-tier not | Add e2e ordering test |
| Fuzz tests on envelope / idempotency / natsx | implied by gates | Missing | Add `go test -fuzz` and Rust `cargo fuzz` corpora |

### 13.5 Phase 9 gates — fill-in tracker

All 8 gates currently "TBD foundation only". Implementation order should follow PLAN.md sequencing (Phase 2 → 1 → 4 → 5 → 3 → 6 → 7 → 8 → 9).

| Gate | Owner | Lint | Test | Wire-up |
|---|---|---|---|---|
| Orchestration shell parity | orchestrator-core + execution-core | TBD | TBD | Replace § 13.1 stubs |
| Capability platform parity | capability-core | TBD | TBD | Replace § 13.2 in-memory registry |
| Multimodal breadth | ai-core (new) + inference-core | TBD | TBD | New `proto/ai.proto` |
| Graph/wiki memory correctness | memory-core (new) + letta-bridge | TBD | TBD | Replace § 13.2 memstore |
| Tasks / cron / coordinator | task-core (new) | TBD | TBD | New protos |
| Bridge / voice / channel | bridge-core (new) | TBD | TBD | New protos |
| Compact transport correctness | shared | TBD | TBD | `buf breaking` CI across 4 protos |
| Cross-plane vendor secret redaction (PR-2.5) | Control / Ingestion / ai-core | TBD | TBD | See VERIFICATION.md PR-2.5 |
