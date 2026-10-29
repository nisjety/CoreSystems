# Model Plane Architecture

> **Runtime/source correction — 2026-07-13:** the component topology below is architectural intent, not current availability. Live gateway/inference gRPC are absent; source now restores authenticated inference and is restoring gateway/session compatibility, but caller tokens and rollback are incomplete. Live cost/session/capability auth and Letta semantic readiness fail. Stdio MCP is quarantined in current source, inline MCP dispatch is denied, unowned records fail closed, and no Visma server/bridge is deployed. See [MODEL_PLANE_STATUS.md](../MODEL_PLANE_STATUS.md) and the [dated audit](core-research/plane-audit-2026-07-13.md).

> **Verified 2026-07-11 (Phase 4 audit).** Core loop and service topology re-checked against live containers + source. Confirmed live via host-curl: model-gateway `/healthz` 200 / `/health` 401 (JWT), capability-core :8085, cost-core :8089, bridge-core :8091. MCP proxying is real: model-gateway exposes `RegisterMcpServer` / `ListMcpServers` / `ProxyMcpTool` (`runtime_registries.rs`) wired into `tool_loop.rs`, `bridges/mcp-bridge` is a working reference bridge, and execution-core routes MCP tools as `mcp__<server_id>__<tool>`. execution-core's pre-tool permission gate (`runtime_loop/mod.rs` → `permission::evaluate_call` / `is_risky_tool`) is enforced, not decorative: risky tools pause the run and create a durable approval via session-core. **No Visma MCP server is registered anywhere in the Model Plane** — Visma appears only in prose docs; the generic MCP plumbing exists but ships with example configs (fs/ddg) only. Corrections below: bridge-core is live (was listed "not started"); cost-core added.

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

### cost-core (Go) ✅  _(Verified 2026-07-11)_
Cost/pricing authority. HTTP :8089 (`/healthz` 200 live) + gRPC :9098. model-gateway reads `COST_CORE_URL` for the price catalogue that backs the durable cost ledger and the budget gate during model selection. Live and deployed in `deploy/docker-compose.yml` + production/override. (Was previously undocumented in this Service Boundaries list.)

### bridge-core (Go) ✅  _(Verified 2026-07-11 — was listed "not started")_
`/v1/bridge/*` operator ingress. HTTP :8091 (`/healthz` 200 live) + gRPC :9100; deployed in `deploy/docker-compose.production.yml` + override, running 2+ days. Fronts the reference bridges under `bridges/` (`mcp-bridge` Node reference on :9201, `lsp-bridge` on :9202). MCP tool calls are proxied by the model-gateway's `ProxyMcpTool` RPC, not by bridge-core directly.

### Future services (not started)

- **ai-core** (Go) — multimodal `/v1/ai/*` ingress (PLAN.md Phase 5).
- **memory-core** (Go + Rust) — `/v1/memory/*` + `/v1/knowledge/*` graph + wiki layers (PLAN.md Phase 7). letta-bridge becomes a sub-adapter.
- **task-core** (Go) — `/v1/tasks/*` + `/v1/cron/*` durable work (PLAN.md Phase 4).
- **~~bridge-core~~** — now live, see the bridge-core section above. (`/v1/voice/*` + `/v1/channels/*` sub-surfaces remain future.)

## Cross-Plane Constraints

1. Identity is read-only. Model Plane validates Control Plane JWT tokens but never issues or stores them.
2. Billing is event-based. Usage flows via NATS events to Control Plane.
3. Data Plane access is API-only. No direct DB connections.
4. Artifact ownership is local. MinIO buckets and Temporal workflows are owned exclusively by Model Plane.
