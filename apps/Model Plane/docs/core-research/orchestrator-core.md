# orchestrator-core Research Dive

Generated: 2026-07-11 (supersedes 2026-06-09 draft)
Auditor pass: Phase 4 / Model Plane. Evidence grades: **[live-curl]** host-side HTTP, **[inspect]** `docker ps`/`docker inspect` (no exec — containerd content store corrupted this pass), **[source-only]** read from disk / host build.

Scope: `apps/Model Plane/go/services/orchestrator-core`

---

## Snapshot

`orchestrator-core` is the Go **Temporal worker + a thin gRPC proxy**. It plays two very different roles, and the distinction is the headline of this audit:

1. **`OrchestrationCoreService` gRPC proxy on `:9080` — LIVE and used.** A pass-through to the Rust `session-core` for Plans / Todos / Approvals / SubagentLineage / `StreamRunEvents`. `model-gateway` dials it (`ORCHESTRATOR_CORE_ADDR`) and it is the system-of-record hop for the chat approvals surface. This is real and on a hot path. [source-only + inspect]

2. **Temporal worker (6 workflows + 11 activities) — REAL code, DORMANT in prod.** The worker registers durable workflows that call sibling services over typed gRPC (no mocks). But **nothing in the repo starts these workflows.** The live agentic/chat loop bypasses Temporal entirely and runs through `execution-core`'s `RunAgent` (confirmed by a source comment in the gateway, see below). The durable machinery is built, unit-tested, and idle. [source-only]

Health is live: `GET :8084/healthz` and `/readyz` both return `200 "ok"` via host curl, even though `docker ps` shows `(unhealthy)` — that is the broken exec-based healthcheck, not a down service. [live-curl][inspect]

Non-generated Go LOC: ~5,211 across 39 files. `go build`, `go vet`, and the full `go test` suite all pass on the host toolchain (go1.26.2). [source-only]

---

## Live verification

| Check | Result | Grade |
|---|---|---|
| `GET http://localhost:8084/healthz` | `200` body `ok` | [live-curl] |
| `GET http://localhost:8084/readyz` | `200` body `ok` | [live-curl] |
| `GET http://localhost:8084/health` | `404` (only `/healthz`,`/readyz` exist) | [live-curl] |
| `GET http://localhost:8084/metrics` | `404` (no scrape endpoint; OTEL is push/OTLP) | [live-curl] |
| Container `model-plane-orchestrator-core-1` | `running`, health=`unhealthy` (broken exec HC), restarts=10, started 2026-07-09 | [inspect] |
| `go build ./services/orchestrator-core/...` | exit 0 | [source-only] |
| `go vet ./services/orchestrator-core/...` | exit 0 | [source-only] |
| `go test ./services/orchestrator-core/...` | PASS (activities, workflows, compat, orchestration) | [source-only] |
| git status (service dir) | **clean** — no uncommitted WIP | [source-only] |
| Last commit touching service | `da136db3` 2026-07-07 "thread user_id through ExecuteStep to close cross-user leak (Phase 4)" | [source-only] |

---

## Runtime shape (verified against source)

- `cmd/main.go` — builds gRPC clients (non-blocking dial), starts the `:8084` health HTTP, dials Temporal (`temporal:7233`), registers workflows + activities on task queue `model-plane-orchestrator`, optionally starts NATS subscriptions, and serves the `OrchestrationCoreService` gRPC on `:9080`.
- `cmd/workflows/*` — `InteractiveRunSupervision`, `DeepTaskWorkflow`, `WideResearchWorkflow`, `MemoryConsolidationWorkflow`, `SkillPromotionWorkflow`, `FeedbackPromotionWorkflow` (all registered), **plus `AutoresearchWorkflow` (NOT registered — see finding 2).**
- `cmd/activities/*` — 11 registered activities. `StartRunActivity`→session-core, `ExecuteStepLoopActivity`→execution-core `ExecuteStep`, memory activities→letta-bridge `SearchMemory`/`IndexMemory`, skill activities→capability-core `ValidateSkillBundle`/`CheckSkillPromotion`/`PromoteSkill`, `AggregateFeedbackActivity`→in-memory `FeedbackStore`. Real typed proto clients, no JSON shim, no mocks.
- `internal/orchestration/*` — the gRPC proxy handlers (`handlers.go`), an envelope-publishing `Service`, and a NATS `Subscriber` fed by a **`LoggingPersister`** (self-documented no-op).
- `internal/compat/*`, `internal/natsadapter/*` — legacy NATS subject bridging.
- `internal/grpcclient/*`, `internal/config/*`, `internal/telemetry/*`.

### Config (compose vs. code defaults) [source-only + inspect]

Compose (`deploy/docker-compose.yml`) sets: `TEMPORAL_ADDRESS=temporal:7233`, `NATS_URL=nats://nats:4222`, `SESSION_CORE_ADDR=session-core:9091`, `INFERENCE_CORE_ADDR=inference-core:9092`, `EXECUTION_CORE_ADDR=execution-core:9093`, `CAPABILITY_CORE_ADDR=capability-core:9097`. `SANDBOX_MANAGER_ADDR`/`BROWSER_BROKER_ADDR`/`LETTA_BRIDGE_ADDR` are left to code defaults (`:9094`/`:9095`/`:9096`), which match the sibling compose gRPC ports.

---

## Findings (ranked)

### 1. Temporal durable orchestration is REAL but not dispatched — nothing in the repo starts the workflows. [source-only + inspect] — HIGH
`grep` for `ExecuteWorkflow`/`StartWorkflow`/`SignalWithStartWorkflow` across all Model Plane Go returns **nothing outside orchestrator-core itself**; the task-queue name `model-plane-orchestrator` appears only in `config.go`; no Rust service uses a Temporal client to start these workflows. The gateway confirms the live path bypasses Temporal — `services/model-gateway/src/sse.rs:2151`:
> "The live agentic path runs through execution-core's RunAgent, not orchestrator-core's Temporal workflow, so nobody was emitting run lifecycle events…"
So the 6 registered workflows + 11 activities are genuine, compile, and pass unit + replay tests, but they are **idle** — no production traffic reaches them. `coordinator.rs` documents an *aspiration* ("the orchestrator restarts the run from its Temporal checkpoint") that the current wiring does not realize. This is the answer to "real Temporal vs in-memory stubs": the Temporal engine is real; its role in the live product is not yet wired.

### 2. `AutoresearchWorkflow` is defined + tested but NOT registered. [source-only] — HIGH
`cmd/workflows/autoresearch.go` implements a budgeted program-artifact loop and has `autoresearch_test.go` + a replay test, but `main.go` registers `WideResearchWorkflow` and never registers `AutoresearchWorkflow`. It therefore cannot be started on the worker. `docs/gap-model.md:46` claims "✅ orchestrator-core … autoresearch (budgeted program-artifact loop) … " as live — **overstated**; it is unregistered/dead. (Its helpers also carry stale internals: `estimateCost` is a hardcoded `$0.10/step` proxy the comment admits is a placeholder, and `parseEvalDecision`'s comment describes summary-string matching but the code checks `step.Completed`.)

### 3. `CAPABILITY_CORE_ADDR` default is wrong (port collision). [source-only + inspect] — MEDIUM
`internal/config/config.go` defaults `CapabilityCoreAddr` to `capability-core:9092` — the **inference-core** gRPC port. Capability-core's real gRPC port is `9097`. Production compose overrides it correctly (`capability-core:9097`), so live routing is fine, but any deploy relying on the code default would send `ValidateSkillBundle`/`CheckSkillPromotion`/`PromoteSkill` RPCs to inference-core. Latent misconfiguration; fix the default to `:9097`.

### 4. `ENABLE_COMPAT_ADAPTER` is a dead env var. [source-only + inspect] — MEDIUM
Base compose sets `ENABLE_COMPAT_ADAPTER: "false"`, the override sets `"true"`, yet **no Go source reads it**. The compat/orchestration/feedback NATS subscriptions are gated solely on `NATS_URL` being non-empty (always `nats://nats:4222`). So the legacy compat adapter and the `mp.v1.orchestration.>` / `mp.v1.feedback.rated` subscribers run in **every** deploy regardless of the flag. The `"false"` toggle is misleading — either wire the flag or delete it.

### 5. HITL: honored in InteractiveRun, silently skipped in the multi-step workflows. [source-only] — LOW (latent; not a write-bypass)
`InteractiveRunSupervision` correctly detects `NeedsApproval` (execution-core status `awaiting_approval`), blocks on the `approval` signal, and only resumes after approval — real HITL. But `DeepTaskWorkflow`, `WideResearchWorkflow`, and the unregistered `AutoresearchWorkflow` never inspect `NeedsApproval`; they record the partial `StepLoopOutput` and move on. This is **not** a bypass that executes risky writes — execution-core returns `awaiting_approval` *without* running the tool — but these workflows drop approval-gated steps instead of pausing. Impact is latent because the workflows are dormant (finding 1). Actual HITL enforcement lives in execution-core/session-core, not here.

### 6. Prior "cross-user leak" fix is present and committed. [source-only] — INFO (resolved)
`StepLoopInput` carries `UserID`; `executeStepRequest` sets `ExecuteStepRequest.UserId` (proto field 8); `DeepTask`/`Autoresearch`/`WideResearch`/`InteractiveRun` all thread `input.UserID`. Committed as `da136db3` (2026-07-07). At the orchestrator layer the org-only scoping is closed; enforcement of viewer-scoping remains execution-core's responsibility.

### 7. `LoggingPersister` is an honest no-op, not a fake. [source-only] — INFO
The `mp.v1.orchestration.>` subscriber decodes + validates envelopes correctly, then hands them to a `LoggingPersister` whose doc-comment says "no-op persister … used until a real session-core projection is wired in." Consumed events are logged, not projected/persisted. Correctly labelled placeholder.

### 8. Cosmetic: stale health-port comment / unused config field. [source-only] — INFO
`main.go` comment says "Health server on :8082" but it listens on `:8084`; `config.HealthAddr` (default `:8082`) is never used (the health server hardcodes `:8084`).

---

## The Phase-4 headline questions, answered for this service

1. **Is the model-gateway → execution-core agent/tool loop real and non-mocked?** — Yes, but it does **not** run through orchestrator-core. The live loop is `model-gateway → execution-core RunAgent` (gateway source comment, finding 1). orchestrator-core's `ExecuteStepLoopActivity` *does* call the same `execution-core.ExecuteStep` with real gRPC, but only inside Temporal workflows that are not dispatched in production.
2. **Are tools registered/dispatched (shipping → shipping-core :3156)?** — Not an orchestrator-core concern. Tool registration/dispatch (shipping, info, integration tools) lives in execution-core; orchestrator-core only forwards a run id + org/user to `ExecuteStep`. No shipping-core reference exists here.
3. **THE MCP / Visma question.** — **orchestrator-core has ZERO `mcp`/`visma`/`bridge` references** (grep confirmed). MCP bridging is `bridge-core` + `bridges/mcp-bridge`, not this service. "Test the Visma MCP" is not an orchestrator-core capability — route that question to bridge-core.
4. **Is HITL enforced or decorative?** — See finding 5: genuinely enforced in `InteractiveRunSupervision`; skipped-not-bypassed in the other (dormant) workflows; core enforcement is downstream in execution-core/session-core.

---

## Doc-staleness corrections (for STALE_DOC_DELETION_REGISTER follow-up)

- `docs/gap-model.md:46` — "autoresearch … ✅" is **false** (unregistered; finding 2).
- `docs/gap-model.md:359` — "orchestrator-core … **fails**: go.mod replace paths … not present in Docker build context (context is only the service dir)" is **STALE/FIXED**. The Dockerfile build context is `../go` (workspace root; `COPY . .` from `/workspace`), the header documents the replace-path requirement, and the container has been `Up 2 days` — the build succeeds. [source-only + inspect]
- `docs/ARCHITECTURE.md:34` — "5 workflows + 10 activities" undercounts: **6 registered workflows + 11 registered activities** (7 workflow funcs exist including the unregistered autoresearch).
- The 2026-06-09 draft's "several activities degrade gracefully → placeholder-like" framing is imprecise: degradation is deliberate (`codes.Unavailable` → warn + fallback for `StartRun`; hard `Unavailable` error for memory/skill activities). Not placeholders.

---

## Bottom line

orchestrator-core is **two services in one shell**. The `OrchestrationCoreService` gRPC proxy to session-core is real, wired, and used by the gateway for approvals/plans/todos. The Temporal worker is real, well-tested Go — but it is **dormant**: no service starts its workflows, and the live agent loop deliberately runs through execution-core's `RunAgent` instead. The debt is not fakery; it is (a) a durable-orchestration engine that product traffic never enters, (b) one unregistered workflow that docs claim is live, and (c) two misleading config knobs (`CAPABILITY_CORE_ADDR` default, `ENABLE_COMPAT_ADAPTER`). Health, build, vet, and tests are all green.
