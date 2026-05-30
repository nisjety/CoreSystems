# Model Plane Architecture

## Pyramid Placement

The Model Plane sits at Layer 4 of the CoreSystem pyramid. It is the AI reasoning and agent-orchestration layer. It receives identity and org context from the Control Plane (L1), retrieves documents and embeddings from the Data Plane (L2), and ingests raw content via the Ingestion Plane (L3).

## Authority Rules

- Canonical owner of reasoning pipelines, agent orchestration, execution graphs, capability definitions, and LLM routing.
- Canonical owner of its own databases: session-core Postgres, Redis hot cache, Temporal task queues, MinIO artifacts.
- May read Data Plane embeddings and documents via gRPC/REST.
- May read Control Plane identity tokens to authorize requests.
- Does NOT own user identity, org hierarchy, or billing (Control Plane).
- Does NOT own document storage, embeddings, or retrieval indexes (Data Plane).
- Does NOT own raw-content ingestion (Ingestion Plane).

## One-Writer-Per-Resource Table

| Resource | Owner Service | Storage | Status | Notes |
|----------|--------------|---------|--------|-------|
| Thread timeline | session-core | Postgres `threads`, `messages` | ✅ live | Append-only messages, transactional outbox |
| Run metadata | session-core | Postgres `runs` | ✅ live | Status transitions only by session-core |
| Run state (step loop) | execution-core | In-memory + events | ✅ live | Delegates to session-core for durable writes |
| Checkpoint snapshots | session-core | Postgres `checkpoints` + MinIO | 🟡 Postgres live, MinIO not provisioned | Pre-persist secret scrub live (PR-2) |
| Memory index | session-core | Postgres `memory_index` | ✅ live | Topic metadata for context assembly |
| Event log | session-core | Postgres `events` | ✅ live | Append-only, ordered by (ts, event_id) |
| Plans / todos / approvals / subagent lineage / run-event log | session-core | Postgres (target) | 🟡 module `orchestration_store.rs` scaffolded; migrations + handlers pending | See gap-analysis.md § 13.1 — 12 RPCs Unimplemented |
| Agent definitions | capability-core | Registry | 🟡 in-memory only | Postgres backing pending — gap-analysis.md § 13.2 |
| Tool/skill metadata | capability-core | Registry | 🟡 in-memory only | Same |
| Routing policies | capability-core | Registry | 🟡 in-memory only | Same |
| Sandbox leases | sandbox-manager | In-memory + Redis | 🟡 in-memory; Redis not wired | Lease lifecycle real; some create paths Unimplemented |
| Browser grants | browser-broker | In-memory + Redis | 🟡 in-memory; Redis not wired | `ValidateGrant` live; create paths Unimplemented pre-codec |
| Memory blocks (bridge) | letta-bridge | In-memory store | 🟡 stub; real Letta upstream not wired | gap-analysis.md § 13.2 |
| Workflow state | orchestrator-core | Temporal | ✅ live | 5 workflows + 10 activities |
| Artifacts (bytes) | execution-core | MinIO | ❌ bucket not provisioned | Object key contract documented in CONTRACTS.md |
| Inference cache | inference-core | In-memory (`cache.rs`) | 🟡 in-memory; Redis target | Stateless except cache |
| Rate-limit state | model-gateway | In-process | 🟡 process-local; Redis target | Per-peer fixed-window |

## Service Boundaries

Status legend: ✅ live · 🟡 partial / stubbed backing · ❌ not started

### model-gateway (Rust) ✅
Public boundary for HTTP/gRPC/SSE/WebSocket. Auth (JWKS-backed JWT), request normalization, rate limiting (in-process — Redis target), header scrubbing. SSE for `/v1/invoke/stream`. No authoritative state.

### session-core (Rust) ✅ + 🟡
Authoritative state for thread timeline, run metadata, checkpoints, context assembly, memory index. Postgres-backed with event replay. Transactional outbox emits `THREAD_CREATED` / `MESSAGE_APPENDED` in-tx. Module `orchestration_store.rs` (371 LOC) scaffolds plan/approval/todo/subagent-lineage durability — schema migrations + RPC wiring still pending (gap-analysis.md § 13.1).

### inference-core (Rust) ✅
Provider routing (OpenAI + Anthropic adapters), retries, fallback chain, token streaming, prompt cache (in-memory — Redis target). `Infer` and `InferStream` gRPC live. Streaming p95 first-token < 200 ms gate green (PR-8).

### execution-core (Rust) ✅
Runtime loop: tool planning, permission gates, hook gates (pre-tool, post-tool, post-step, compaction-trigger), subagent hook, secret scrub (PR-2), `ExecuteStep` + `ResumeRun` RPCs. Step throughput + checkpoint recovery + context-assembly SLO gates green (PR-8).

### orchestrator-core (Go) ✅ + 🟡
Temporal-based outer workflow envelope. 5 workflows (`InteractiveRunSupervision`, `DeepTaskWorkflow`, `MemoryConsolidationWorkflow`, `SkillPromotionWorkflow`, `WideResearchWorkflow`). 10 activities with real downstream gRPC dispatch + `Unavailable`-tolerant fallbacks. `internal/orchestration/handlers.go` exposes 12 RPCs all returning `codes.Unimplemented` (gap-analysis.md § 13.1) — to be replaced as Phase 1 lands.

### capability-core (Go) 🟡
Policy and capability authority. Skill registry, connector normalization, model/tool eligibility. Six RPCs implemented (`ListCapabilities`, `GetCapability`, `EvaluatePolicy`, `ValidateSkillBundle`, `CheckSkillPromotion`, `PromoteSkill`) — all backed by an **in-memory** registry + policy engine. Postgres backing pending. No tool / command / plugin / MCP / model / memory-adapter / safety registry yet (PLAN.md Phase 2).

### sandbox-manager (Go) 🟡
Thread-scoped and agent-scoped sandbox lifecycle. `ValidateLease` + lease-state methods live; create paths Unimplemented pre-codec (gap-analysis.md § 13.3).

### browser-broker (Go) 🟡
Trusted browser grants with per-session revocation. `ValidateGrant` returns `FailedPrecondition` for revoked/expired grants (PR-3 closed). Create paths Unimplemented pre-codec.

### letta-bridge (Go) 🟡
Optional async bridge for memory block synchronization. Currently backed by an **in-memory `memstore`** — no real Letta upstream wired. `internal/{client,retrieval,sync}` packages scaffolded but unconnected. `SearchMemory` / `IndexMemory` / `Health` work against the in-memory store only.

### Future services (not started)

- **ai-core** (Go) — multimodal `/v1/ai/*` ingress (PLAN.md Phase 5).
- **memory-core** (Go + Rust) — `/v1/memory/*` + `/v1/knowledge/*` graph + wiki layers (PLAN.md Phase 7). letta-bridge becomes a sub-adapter.
- **task-core** (Go) — `/v1/tasks/*` + `/v1/cron/*` durable work (PLAN.md Phase 4).
- **bridge-core** (Go) — `/v1/bridge/*` + `/v1/voice/*` + `/v1/channels/*` operator ingress (PLAN.md Phase 6).

## Cross-Plane Constraints

1. Identity is read-only. Model Plane validates Control Plane JWT tokens but never issues or stores them.
2. Billing is event-based. Usage flows via NATS events to Control Plane.
3. Data Plane access is API-only. No direct DB connections.
4. Artifact ownership is local. MinIO buckets and Temporal workflows are owned exclusively by Model Plane.
