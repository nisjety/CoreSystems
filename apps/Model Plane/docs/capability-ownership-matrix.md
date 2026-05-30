# Capability Ownership & Harmonization Matrix

> **Purpose.** The single source of truth for *who owns what* in the Model Plane, so we never build two systems that do the same job. Every capability has **exactly one canonical owner**; everything else either relays to it or is consolidated into it. This operationalizes the directive: *right tool for the right job, small but powerful, not over-engineered.*
>
> **Grounded in code, not docs** (audited 2026-05-30 via codegraph + source inspection of `apps/Model Plane/`). Where this matrix and `ARCHITECTURE.md`/`ROADMAP.md` disagree, **this matrix wins** until those are reconciled (see §1).
>
> **Scope rule:** canonical target is `apps/Model Plane/` (**v1**). `apps/Model Plane v2/` is **deprecated** — never extend or duplicate it (per MEMORY: "v1 is forward target; v2 deprecated").

---

## 1. Reality reconciliation — the docs are stale

The roadmap marks most phases ❌. **The code says otherwise.** This is the root cause of duplication risk: building "missing" features that already exist.

| Roadmap claim | Actual state in code | Evidence |
|---|---|---|
| P1 orchestration shell ❌ | **Largely DONE** | `session-core/orchestration_store.rs` (25 pub fns: plans/steps/todos/approvals/lineage), `orchestration_grpc.rs`, `orchestration_nats.rs`; gateway routes `/v1/orchestration/{plans,todos,runs/:id/cancel,resume,plans}`, `/v1/runs/:id/events`; migration `0003_orchestration_tables` |
| P4 tasks/cron ❌ | **DONE (with overlap)** | session-core migration `0004_tasks_cron_hooks_skills_memory` (tasks, task_events/assignments/dependencies/artifacts, cron_schedules/fires); Go `task-core` service (store+cron); gateway `/v1/tasks`,`/v1/cron` |
| P5 multimodal ❌ (chat-only) | **DONE** | inference-core `provider/{vision,video,speech,doc_intel,realtime}.rs`; gateway routes for all 8 groups: `/v1/ai/{chat,embeddings,images,documents,speech,translate,video,realtime,language,models}` |
| P2 capability registries ❌ (all Unimplemented) | **Mostly DONE** | capability-core packages: `registry/`(models), `commands/`, `tasks/`, `schedule/`, `coordination/`, `modalities/`, `sandbox/`, `policy/`(RBAC), `artifacts/`, `failover/`, `streaming/`, `roadmap/` |
| P7 knowledge plane ❌ | **Partial — routes live** | gateway `/v1/graph/entities`,`/v1/graph/expand`,`/v1/wiki/pages`; dataplane protos `graph_v1`,`wiki_v1`,`knowledge_v2` |
| P8 token efficiency ❌ | **Partial** | gateway `/v1/toon/encode`; execution-core compaction hook; `session-core/compaction.rs` |
| MCP / plugins "not started" | **Registries EXIST (in-mem)** | gateway `runtime_registries.rs`: mcp (Wave 10g), plugins (10h), commands+hooks+permissions (10i) |
| Plan mode / approvals ❌ | **EXIST (in-mem, gateway)** | `coordinator.rs` (PlanModeStore, TeamWorkerStore), `approvals.rs` (ApprovalStore) |

**Conclusion:** Model Plane v1 is ~80–90% feature-complete against its own roadmap. The remaining work is **harmonization + a few genuine gaps**, not greenfield phases.

---

## 2. The layering principle (resolves most duplication)

Three tiers. A capability appearing in more than one tier is **correct layering** *only if* the lower tiers defer durability upward. Same-tier duplication is a **defect**.

```
INGRESS (ephemeral, run-loop-scoped)   model-gateway (Rust)        — HTTP/SSE/WS, auth, rate-limit, in-mem caches that MUST defer to durable owners
DURABLE STATE (one writer per resource) session-core (Rust+PG), capability-core (Go+PG), task-core (Go+PG)
COORDINATION (recovery, retries)        orchestrator-core (Temporal), execution-core (step loop + enforcement)
```

**Rule:** model-gateway's `runtime_registries.rs`, `coordinator.rs`, `approvals.rs` stores are **ephemeral caches**, not systems of record. Each must (a) hydrate from its durable owner on miss, and (b) publish mutations to NATS for the owner to persist. They are allowed to exist as latency caches; they are **not** allowed to be the source of truth.

---

## 3. Capability Ownership Matrix

Legend — **Owner** = single system of record. **Relay** = ingress/enforcement that defers to owner. **Protocol**: gRPC (internal sync), NATS (async events), HTTP (public), Temporal (durable coordination).

| Capability | Canonical Owner | Storage | Relay / Enforcement | Async | Duplication verdict |
|---|---|---|---|---|---|
| Identity / org / billing | **Control Plane** (external) | — | model-gateway validates JWT | — | ✅ clean (read-only) |
| Thread / message timeline | **session-core** | Postgres | gateway ingress | outbox | ✅ |
| Run metadata + lifecycle | **session-core** | Postgres | execution-core transitions | NATS run-events | ✅ |
| Plans / steps / todos / approvals / subagent-lineage | **session-core** (`orchestration_store.rs`) | Postgres | gateway `coordinator.rs`/`approvals.rs` are **ephemeral caches** | `orchestration_nats.rs` | ⚠️ **CONSOLIDATE**: gateway stores must defer to session-core (see §4.1) |
| Checkpoints / context assembly / compaction | **session-core** | Postgres + MinIO | execution-core triggers | — | ✅ |
| Memory index (`agent_memory`) | **session-core** | Postgres | — | — | ⚠️ vs letta-bridge (see §4.4) |
| Durable tasks + cron | **task-core** (Go) ↔ **session-core** tables | Postgres | gateway `/v1/tasks`,`/v1/cron`; capability-core `tasks/`,`schedule/` = **metadata only** | NATS | ⚠️ **RESOLVE**: one durable task store (see §4.2) |
| Tool / command / skill / model / plugin / MCP / routing / safety registries | **capability-core** | Postgres | gateway `runtime_registries.rs` = **ephemeral cache** | NATS reconcile | ⚠️ **CONSOLIDATE**: gateway registries defer to capability-core (see §4.3) |
| Policy / permissions / RBAC | **capability-core** (`policy/engine.go`) | Postgres | gateway `permissions` ACL cache; execution-core `permission/` = **enforcement** | — | ⚠️ enforcement OK; gateway ACL must hydrate from capability-core |
| Hook config (registry) | **capability-core** (`hook_configs`) | Postgres | execution-core `hook/` = **firing/enforcement**; gateway `hooks` = cache | — | ✅ layering OK once gateway hydrates |
| Lifecycle hook firing | **execution-core** (`hook/mod.rs`) | — | — | — | ✅ enforcement owner |
| Provider routing / inference / multimodal | **inference-core** (`provider/*`) | in-mem cache (Redis target) | gateway `/v1/ai/*` ingress | usage NATS | ✅ (add capabilities struct — §5) |
| Secret scrub / redaction | **execution-core** (`scrub.rs`) | — | called at output/error/checkpoint boundary | — | ✅ single owner (extend regexes — §5) |
| Sandbox lease lifecycle | **sandbox-manager** (Go) | in-mem + Redis target | execution-core requests lease | — | ✅ owner; **no real isolation yet (gap §5)** |
| Real OS isolation (bwrap/Landlock/seccomp/egress) | **execution-core** (NEW) + sandbox-manager (provisioning) | — | — | — | ❌ **GENUINE GAP** — nothing exists |
| Subagent spawn / message / coordinate | **orchestrator-core** (Temporal) | session-core lineage | execution-core `subagent/` = **exec**; gateway `coordinator.rs` TeamWorkerStore = **cache** | NATS | ⚠️ **CONSOLIDATE**: 3 locations → orchestrator owns lifecycle (see §4.1) |
| Channels (Slack/Discord/…) / voice | **bridge-core** (Go) | session-core | gateway ingress | NATS | ✅ (`channel/`,`voice/` exist) |
| IDE bridge / remote sessions | **bridge-core** (`session/`) | — | gateway WS | — | 🟡 partial; reuse Velion JWT |
| Browser grants | **browser-broker** (Go) | in-mem + Redis target | — | — | ✅ |
| Cost / usage ledger | **cost-core** (Go) | Postgres | gateway emits `usage.*` | NATS | ✅ |
| Fine-tuning jobs | **session-core** (persist) + **model-gateway** (Azure) | Postgres | poller | NATS finetune | ✅ (Wave 7) |
| Knowledge: graph / wiki extraction | **Data Plane** (read) + session-core context | Data Plane | gateway `/v1/graph`,`/v1/wiki` relay | NATS | 🟡 extraction service is P7 gap |
| Compact transport (TOON) | **model-gateway** (`/v1/toon`) + shared crate | — | — | — | ✅ (benchmark/reversibility = P8 finish) |

---

## 4. Duplication ledger — explicit rulings

### 4.1 Orchestration & coordination (plan mode, approvals, subagents) — **3-way shadow**
- **Owner:** session-core (durability) + orchestrator-core (Temporal lifecycle/recovery).
- **Shadows:** model-gateway `coordinator.rs` (PlanModeStore, TeamWorkerStore), `approvals.rs` (ApprovalStore) — all in-memory, no backend calls (verified: no `session_core.*` calls in these files), explicitly documented as "promote to postgres later."
- **Ruling:** Keep the gateway stores as **run-loop latency caches only**. Wire them to: (a) read-through to session-core `orchestration_grpc` on miss, (b) write-through via NATS (`orchestration_nats`) so session-core persists. execution-core `subagent/` stays as the **executor**; orchestrator-core owns spawn/lifecycle. **No new coordinator system.**
- **Verified 2026-05-30 — precise gap (3 parts, all needed; do not half-fix):**
  1. **Subject divergence:** the gateway publishes orchestration mutations to **ad-hoc subjects** — `agents.run.plan_mode.{entered,exited}` (`coordinator.rs`), `agents.approval.{requested,resolved}` (`approvals.rs`) — NOT the canonical `mp.v1.orchestration.*` tree (`SUBJECT_PLAN`/`SUBJECT_APPROVAL` in `mp-events`). No in-repo consumer subscribes to the ad-hoc subjects, so they are effectively dead-ends today.
  2. **Payload mismatch:** session-core's `orchestration_nats.rs` decodes a protobuf `OrchestrationEvent`; the gateway publishes a generic JSON envelope. A subject rename alone would fail to decode — payloads must be aligned to `OrchestrationEvent`.
  3. **No persistence:** `orchestration_nats.rs` is **fan-out only** ("broadcasts to in-process gRPC subscribers… subscribers re-read") — it does **not** write to the durable `orchestration_store`. Durability requires a persisting consumer.
  **Correct fix (stack-verified):** gateway emits `OrchestrationEvent` on `mp.v1.orchestration.{plan,approval}`; a session-core consumer persists via `orchestration_store::{create_plan,request_approval,decide_approval,…}`; gateway reads-through on cache miss. All three parts need the running stack (NATS+Postgres) to verify decode + persistence round-trip — a cosmetic subject rename is explicitly **not** the fix.
- **2026-05-30 — write-through path de-risked + THE crux found.** Good news: the gateway **already holds `orchestration_client: OrchestrationCoreServiceClient` in `AppState`** (used by `sse.rs`), and session-core's `OrchestrationCoreService` is the canonical durable path (`CreateApproval`→`store::request_approval`, `DecideApproval`→`store::decide_approval`). So the fix is a **gRPC write-through** at `grpc.rs:642/655/668` — **no NATS consumer, no double-write.** **Crux (why it's not a 10-line drop-in):** `approvals.rs` mints its **own** `approval_id` in-memory, but `store::request_approval` mints a **different** id server-side — a naive write-through would create **divergent durable records the later `DecideApproval` can't target** (orphaned un-resolvable approvals — *worse* than the current no-persistence state, and a new inconsistency). **Required:** let `CreateApprovalRequest` carry a client-supplied id (small additive contract change) **or** have the gateway store the returned durable id keyed by its own; then best-effort write-through (in-memory stays authoritative for the response). Enum/field map is known: kind `tool_execution`→`APPROVAL_KIND_TOOL_CALL`; status `approved`→`APPROVAL_STATE_GRANTED`, `denied`→`APPROVAL_STATE_DENIED`. Turnkey once the id-minting decision is made + verified against the running pair.

### 4.2 Tasks & cron — **task-core is an orphaned in-memory duplicate** (RESOLVED diagnosis 2026-05-30)
- **Verified by code-read:**
  - `task-core/internal/store/store.go` is **explicitly in-memory** — `map[string]*Task` + mutex, doc comment "implements in-memory storage". Persists nothing; data lost on restart.
  - `task-core/cmd/main.go` registers **only a gRPC health server** — no task RPC service is wired. Its cron scheduler ticks every 30s over a store no external caller populates.
  - **Nothing imports task-core** (caller grep across `rust/`+`go/` hit only generated proto). Effectively dead/orphaned.
  - Durable task/cron state already lives in session-core migration `0004` (`tasks`/`cron_schedules`/`cron_fires`); gateway `/v1/tasks`+`/v1/cron` are ingress; **Temporal (orchestrator-core) is already the durable scheduler**; capability-core `tasks/`+`schedule/` hold metadata.
- **Three systems can fire scheduled work** (task-core's hand-rolled scheduler, Temporal cron, session-core tables). task-core is the redundant one — and the *wrong tool* (in-memory, non-durable) for a job Temporal already does durably.
- **Ruling:** **Retire task-core's hand-rolled scheduler/store.** Durable cron = **Temporal**; durable task state = **session-core**; metadata/templates = **capability-core**; ingress = **gateway**. If task-core is kept, gut it to a thin gateway→session-core/orchestrator relay — it must NOT own a parallel store or scheduler.
- **✅ RETIRED 2026-05-30.** Safety conditions verified (not blind): no `depends_on: task-core` in compose; nothing imports the package; it held no durable state. Removed the service dir, its `go.work` entry, and both compose blocks; `go work sync` + all Go modules build clean; stale gateway comments fixed. **Remaining (stack):** point gateway `/v1/tasks`+`/v1/cron` durability at session-core `tasks` tables and cron firing at Temporal (orchestrator-core) — integration-tested.

### 4.3 Capability registries — **gateway in-mem vs capability-core durable**
- gateway `runtime_registries.rs` (mcp/plugins/commands/hooks/permissions) vs capability-core durable packages.
- **Ruling:** capability-core is the **registry system of record**. Gateway registries become **read-through caches** that subscribe to capability-core reconcile events (`skill.registered`, `plugin.installed`, `mcp.server.enabled`, `routing.policy.updated`). **No second registry implementation.**
- **2026-05-30 — emission COMPLETE across all 4 registries** (commits 888e82a, 3ba4e4e): `internal/reconcile.Emit` publishes `mp.v1.capability.<kind>.<action>` envelopes via the shared `go/pkg/publisher` (nil-safe), wired into the **MCP, Skills, Routing, and Safety** create paths through a non-rippling `WithPublisher` setter, best-effort. 3 unit tests via `InMemoryPublisher`; capability-core builds + vets clean. **Two runtime-bound halves remain:** (a) **prod activation** — inject `publisher.NewNATSPublisher(nats.Connect(NATS_URL))` in `cmd/main.go` (`nats.Conn` satisfies `RawPublisher`; needs the `nats.go` dep + running bus); (b) **gateway-subscribe consumer** — the Rust `runtime_registries` caches subscribe to `mp.v1.capability.>` and invalidate/refresh. Both verified against the live stack.

### 4.4 Memory — **session-core `agent_memory` vs letta-bridge**
- **Ruling:** session-core owns the **canonical memory index**. letta-bridge becomes **one adapter** behind a capability-core `memory-adapters` registry (hermes `MemoryProvider` shape). Rust calls only `Prefetch`/`SyncTurn`. **letta-bridge is not a parallel memory system.**

### 4.5 Skills — registry vs promotion vs learning
- **Ruling:** capability-core owns the **skills registry** (`agent_skills`); orchestrator-core `SkillPromotionWorkflow` **writes into it**; the hermes-style learning loop is a **producer** of skill candidates, not a store. One registry.

---

## 5. Genuine gaps (net-new from harvest — these are the ONLY things to build fresh)

After harmonization, the truly-missing items are small and targeted:

| # | Gap | Owner | Source (license) | Why it's real (not duplicate) |
|---|---|---|---|---|
| G1 | **OS isolation** (bwrap/Landlock/seccomp/no_new_privs) + egress allowlist | execution-core (policy) + a real executor (NEW) | codex `linux-sandbox`/`bwrap`/`network-proxy` (Apache) | **Deeper than expected:** `tool_bridge` is a *deterministic stub* (canned outputs, spawns nothing) and `sandbox-manager` is *lease bookkeeping only* (no `exec.Command`/docker). Nothing executes sandboxed processes yet. **Foundation laid** (`execution-core/sandbox.rs`: policy→bwrap argv, 6 tests); a real process executor is the prerequisite (bigger build, needs stack+Linux). |
| G2 | **MCP transport client** behind the existing registry | capability-core | codex `rmcp-client` (Apache, fork) | Registry exists; the actual stdio/HTTP MCP client connection layer does not |
| G3 | **Sandbox-policy vocabulary** (`MpSandboxPolicy`/`PermissionProfile`/network) | execution-core + `safety.proto` | codex enums (Apache) | No sandbox-policy type exists; G1 needs it |
| G4 | **Provider capabilities struct** (`supports_vision/tools/thinking`, ctx window) | inference-core | codex `ProviderCapabilities` (Apache) | Providers exist but aren't introspectable; needed to gate routing |
| G5 | **Secret-scrub hardening** (sk-*/AKIA*/PG-DSN/NATS-creds) | execution-core `scrub.rs` | codex `redact_secrets` (Apache) | `scrub.rs` exists — this is a **regex extension**, not new system |
| G6 | ~~Keyring store for infra secrets~~ | — | codex `keyring-store` (Apache) | **SKIPPED — wrong tool.** OS keychain is a desktop-CLI pattern; this server plane uses env vars + secret-manager. Adding it = over-engineering. |
| G7 | **Closed learning loop** (post-session skill review) | orchestrator-core activity → capability-core | hermes `background_review` + prompt (MIT) | Promotion workflow exists; the *review→candidate* producer does not |
| G8 | **Real durability** for gateway ephemeral stores (§4.1/4.3 wiring) | gateway ↔ session-core/capability-core | — | Consolidation work, not new feature |

Everything else the harvest proposed **already exists** — do not rebuild it.

**Implementation status (2026-05-30):**
- ✅ **G5** — `execution-core/scrub.rs` extended (connection-string passwords, inline `KEY=value`, Slack/Google keys); 7 tests pass.
- ✅ **G4** — `inference-core/provider/mod.rs` `ProviderCapabilities` + `capabilities()` trait method (2 tests); OpenAI + Anthropic providers now **override it with accurate self-description** (tools/vision/thinking/streaming/embeddings, 128k/200k context). Providers are now introspectable; the router/policy can gate modality use against real capability instead of hardcoded assumptions.
- ✅ **G3** — `execution-core/policy.rs` `MpSandboxPolicy`/`MpNetworkPolicy` (Rust vocabulary); 5 tests pass. Proto promotion deferred to G1 (when it crosses execution-core↔sandbox-manager).
- ⏭️ **G6** — skipped (rationale above).
- 🟡 **G1 foundation** — `execution-core/sandbox.rs` (`MpSandboxPolicy`→bubblewrap argv, Linux-gated syscalls, transparent passthrough without bwrap; 6 tests). Blocked on a **real tool executor** — `tool_bridge` is currently a deterministic stub and `sandbox-manager` is lease-only, so there is no process to isolate yet. The executor is the next prerequisite (needs stack+Linux to verify).
- ✅ **G2 stdio transport** — `model-gateway/mcp_jsonrpc.rs` (pure JSON-RPC 2.0 framing + `stdio://` URL parse, 8 tests) wired into `runtime_registries.rs::handle_proxy_mcp_tool`: spawns the MCP server subprocess, does the `initialize` handshake + `tools/call` over newline-delimited JSON-RPC, 30s timeout, child always killed. Compiles clean. (HTTP transport pre-existed; sse still returns Unimplemented. Integration test vs a real MCP server needs the stack.)
- 🟡 **G7 producer core + activity logic** — `capability-core/internal/learning`: retention policy (`SelectForPersistence`: provenance protection, dedup, threshold) **and** the activity control flow (`RunReview` over injected `Reviewer`/`Sink` interfaces); MIT-adapted prompt; 12 tests, `go vet` clean.
  - **Sink ownership RESOLVED + durable endpoint BUILT (2026-05-30):** skill bodies live in session-core `agent_skills` (matches `SkillCandidate`); capability-core registry is the catalog. ✅ **`UpsertAgentSkill` RPC now exists** (commit 1e06eb0): migration 0006 (`origin` column + check constraint), `sessions.proto` RPC, session-core handler with **DB-level provenance protection** (a `background_review` upsert can't overwrite a `user` skill → `skipped_protected`). Compiles clean. ✅ **Go codegen migration done** (3d62d66 — `go/gen` regenerated to nested paths, all 7 Go modules build) and ✅ **concrete `Sink` built** (`internal/skillsink`, commit d4b31c9 — `SessionCoreSink` → `UpsertAgentSkill`, session-scoped, skipped_protected handling, 3 tests). **G7 spine complete: producer + activity + endpoint + sink, all built & tested.** **Only remaining:** concrete `Reviewer` (inference-core LLM call + structured-output parse) and the Temporal session-end trigger (orchestrator-core) — both irreducibly need the live model + workflow engine to be meaningful/verifiable.
- ⚠️ **Codegen hygiene finding (2026-05-30, empirical):** the checked-in `go/gen` is **stale vs the current buf proto-path scheme** — a scoped `buf generate` rewrote 10 unrelated dataplane files (flat `dataplane/graph_v1.proto` → nested `dataplane/graph/v1/graph_v1.proto`, renaming internal symbols ~75 lines/file). Rust regenerates fine at build time (`tonic_build`); **Go does not** (checked-in, drifted). Consequence: **any new Go RPC consumer** (e.g. G7's `Sink` calling `UpsertAgentSkill`) is blocked until a deliberate **repo-wide `buf generate` Go migration** is run + reviewed in a controlled env — it must not ride along inside a feature commit. This is the precise, verified reason the Go-consumer side of G7/§4.3 is gated. (Reverted the experimental regen; Rust endpoints stand.)
- ✅ **G8 approval write-through — IMPLEMENTED** (commit a8aeeee). The id-minting divergence crux is solved: added `client_approval_id` to `CreateApprovalRequest`; session-core honors it; the gateway approval handlers (`request`/`approve`/`deny`) now **write through best-effort** to session-core's canonical `OrchestrationCoreService` via the already-present `orchestration_client` (in-memory store stays authoritative, so no regression). Pure `GatewayApproval→Create/Decide` mappings (kind/state enums) unit-tested; both `MockSessionCore` impls updated; all compiles clean. **Remaining:** plan-mode (`coordinator.rs`) write-through (same pattern) + on-stack round-trip verification.
- ✅ **P8 correctness — CLOSED by verification.** `mp-toon` is encode-only and lossy by construction (unquoted scalars conflate `"30"`/`30`/bools), so no decoder can losslessly invert it. **But every caller was verified display-only:** session-core `grpc.rs` uses it for the on-demand compaction *summary* (source events remain in the log — GOAL.md compaction-never-replaces-source holds); gateway `/v1/toon/encode` is a prompt-utility returning a string + token estimate. **Nothing round-trips TOON back to structured data.** So TOON is already the right tool for its one job (display/prompt compaction), the roadmap's "TOON reversibility gate" is **not applicable** (it should be removed from ROADMAP P8, not built), and the inaccurate "losslessly round-trippable" module-doc claim was corrected. No `decode` should be written; if a future feature needs reversible compact transport, use JSON or add a *separate* type-tagged codec — do not retrofit TOON.
  - Residual (cheap, stack to confirm): assert the session-core compaction summary is stored *additively* (never overwriting source events) — usage is display-shaped, so low risk.

---

## 6. Protocol map — right tool for each job

| Job | Protocol | Rationale |
|---|---|---|
| Public client ↔ gateway | HTTP/JSON + SSE (+WS for realtime/bridge) | Browser-native, Velion proxy-friendly |
| gateway ↔ core services | **gRPC (Tonic/Go)** | Typed, low-latency, internal trust |
| Async fan-out (events, reconcile, usage, finetune) | **NATS JetStream** `mp.v1.*` | Decouple slow workers from request path; replay |
| Durable multi-step coordination + retries + cron(durable) | **Temporal** (orchestrator-core) | Crash-safe, deterministic replay |
| In-process latency cache | dashmap/in-mem (gateway) | Must defer durability upward (§2) |
| Live UI projection | Convex (read-only) | Reactive dashboards |
| **Not used:** GraphQL | — | REST+NATS+Convex cover it; no schema-stitch cost |

---

## 7. Action plan (harmonize first, then fill gaps — keeps it small)

**Phase H — Harmonize (no new features, removes duplication):**
1. [§4.2] Resolve task durable-store overlap (task-core delegates persistence to session-core).
2. [§4.1] Wire gateway coordinator/approvals stores to read/write-through session-core.
3. [§4.3] Wire gateway registries to capability-core reconcile events.
4. [§4.4] Reframe letta-bridge as a memory-adapter behind capability-core.
5. Reconcile `ARCHITECTURE.md`/`ROADMAP.md`/`gap-model.md` to actual state (mark P1/P4/P5 DONE).

**Phase G — Fill genuine gaps (Apache-vendored, small):**
6. [G5] Extend `scrub.rs` regexes (S).
7. [G6] Keyring-store crate (S).
8. [G3] `MpSandboxPolicy` enum + `safety.proto` (S).
9. [G4] `ProviderCapabilities` on inference-core ProviderRouter (S).
10. [G1] execution-core OS isolation via codex `linux-sandbox` + egress (L, Linux-gated).
11. [G2] Fork codex `rmcp-client` behind capability-core MCP registry (M).
12. [G7] hermes learning-loop activity feeding capability-core skills + Wave 7 (M).

Each item: one owner, respects §3, ADR records source+license. **No item creates a second system for an existing job.**
