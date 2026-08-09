# Model Plane Improvements 2026

**Status:** Architecture and implementation proposal  
**Scope:** `apps/Model Plane` and its contracts with Control, Application, Data, and Ingestion planes  
**Research date:** 2026-08-03  
**Expanded:** 2026-08-03 — loop/harness engineering, AI runtime efficiency, TOON/JSON, MCP code mode, skills, LLM Wiki, multimodal RAG, A2A, adaptive compute, self-improvement, world state, containment, and eval science  
**Verified against the running stack:** 2026-08-03 (same day) — every major proposed component cross-checked against actual Rust/Go source with file:line citations. See §1a for the full ledger and three corrections that change how the rest of this document should be read: the approval-continuation work (§14) is ~70% done and one dispatcher away, `cost-core`'s ledger is already durable (an earlier "in-memory" note is stale), and the GraphPlan model (§6) is confirmed 100% greenfield — the single biggest bet in this document. Re-verify before trusting anything below without a citation next to it; this stack changes fast enough that claims documented as "live" have been found completely inert in production before (see `verevon-roadmap.md` §1).  
**Primary rule:** Quarry captures evidence. Data Plane knows. Model Plane reasons. App Shell presents.

---

## 1. Purpose

This document defines the next improvements for Verevon's Model Plane based on:

- the current CoreSystem plane ownership model;
- the existing Rust and Go Model Plane services;
- the open-source systems and frameworks reviewed for Verevon;
- newer agent-runtime, durable-execution, graph-orchestration, routing, memory, evaluation, context-codec, tool-use, skill, multimodal-RAG, and interoperability patterns available in 2026;
- the requirement that Verevon owns its control plane and does not depend on another product's UI, tenant model, or workflow editor.

This is **not** a replacement architecture. The existing Model Plane already has the correct major building blocks:

- `model-gateway`;
- `session-core`;
- `inference-core`;
- `execution-core`;
- `orchestrator-core` on Temporal;
- `capability-core`;
- `sandbox-manager`;
- `browser-broker`;
- `letta-bridge`;
- `cost-core`;
- `bridge-core`;
- PostgreSQL, NATS/JetStream, MinIO, Dragonfly, and OpenTelemetry.

The objective is to **converge, harden, and improve** those components.

---

## 1a. Verification pass against the running stack — 2026-08-03, re-run 2026-08-09

This document was written as a research proposal. This section is the
difference between that and reality: every major component below was
checked against the actual current Rust/Go source (file:line cited) and
against fixes that landed this same week, so the rest of the document can be
read as **verified-and-prioritized**, not just proposed. Re-run this check
before treating anything below as still accurate — it has already drifted
twice (§18 on the first pass, §14 on this one).

**2026-08-09 re-verification.** `git log --since=2026-08-03` on
`session-core`, `execution-core`, `model-gateway`, `capability-core`,
`cost-core`, and `orchestrator-core` was checked commit-by-commit against
this section's claims. One item moved, from the single most-cited gap in
the whole document to shipped:

- **§14 (durable approval continuation) is now DONE**, not PARTIAL ~70%. The
  missing piece named below — "nothing calls it,
  `execution-core/src/runtime_loop/agent.rs:2237` stubs the client side" —
  is fixed: `approval_delivery_worker.rs` (committed `34f30ee4`, 1331 lines)
  is the real dispatcher. It leases a row via `ClaimApprovalDeliveries`,
  resumes the exact suspended tool call (provider action or shipment
  booking), executes it, and records+acknowledges the outcome. It grew past
  what this section speculated in three ways not anticipated here:
  - **Deterministic postcondition verification** (roadmap P1.3, `d7a7b01d`
    through `61c46594`): a completed dispatch is independently re-checked
    against the system of record — shipping-core, 6 provider read-backs
    (LinkedIn/Meta by-id, LinkedIn posts by bounded listing), 4 mutation
    verifiers with field-level value comparison, and browser procedures
    against their own final observation. `PostconditionOutcome`
    (Confirmed/Refuted/Inconclusive) can *override* a structural success
    into `verified_failure`, or *strengthen* it to `method: "postcondition"`
    — never the reverse, and never on an unreachable check.
  - **The receipt gate is lifted** (`214902ff`): a provider write that
    integration-corev2 durably completed but returned no id-like field
    (common for LinkedIn partial updates and Okta lifecycle calls, which
    answer `204 No Content`) used to be recorded as a terminal
    `invalid_continuation` regardless. It now resolves through the same
    postcondition machinery — `CompletedByPostcondition` only on a
    `Confirmed` outcome, `postcondition_refuted` as a new allowlisted
    failure code otherwise, silence never promoted to success.
  - **This is the first live producer of `pb::VerificationResult`**,
    i.e. the first real instance of §1's "Verified Outcome Foundation"
    concept this whole document assumes. The Proof Bundle (roadmap P1.2,
    `b86b2447`/`fa49b65d`) reads exactly this data end-to-end into the Agent
    Run Console.

  Everything else in this section's original per-component ledger was
  re-checked directly against source rather than assumed carried-forward,
  and is confirmed **unchanged**:
  - §8.1's "attest exact execution request" step: still no `Attest*`
    function in `capability-core` scoped to a specific call (only runtime
    *health* attestation exists — `AttestAvailabilityForOrg`/
    `AttestAvailabilityGlobal`, `capabilities_store.go:344-425` — a
    different concept from signing a specific execution request). Still
    GREENFIELD as originally scoped.
  - §9: no `RoutingDecision` type or step-level routing function exists
    anywhere in `model-gateway`. Still request-level only.
  - §13: `skill_promotion.go` last touched 2026-07-31 (before the original
    pass); unchanged, 141 lines, same shape.
  - §18: `cost-core` still has no `Reserve`/reservation type anywhere in the
    service. Durable ledger claim and the reservation/commit gap both stand.
  - §21: `MAX_TOOL_ROUNDS` is still resolved once from env, clamped
    `1..=32`, defaulting to 12 (`tool_loop.rs:50-67`) — unchanged.
  - §22: TOON usage and prompt-cache-key handling are still confined to
    `http_routes.rs`; no second provider's cache_control equivalent has been
    added. Still narrower than proposed, still Anthropic-only.
  - §23.1/23.2 (progressive MCP disclosure) and §23.4 (MCP code mode): still
    no `server/discover`-equivalent staged disclosure in `capability-core`.
    Still GREENFIELD — see the 2026-07-28 MCP release-candidate note added
    to §23 below, which changes *how* this should be built, not whether it
    still needs building.
  - §23.9: `CapabilitiesStore.RankedList` (`capabilities_store.go:553`)
    still scores on the same measured fields (`success_rate`,
    `p95_latency_ms`, `mean_cost_usd`, `approval_rate`, `incident_count`,
    `operator_rating`) via `sort.SliceStable` — real, unchanged, still the
    thing to extend rather than replace.

**Read this first: three corrections to how the rest of the document should
be read.**

1. **Post-approval continuation (§14, Phase 0 item 1) is not a 6-item
   greenfield build — it is ~70% done and one dispatcher away from done.**
   The tenant-scoped IDOR requirement in §14.2 is already met:
   `session-core/src/orchestration_store.rs:343-347`'s `decide_approval` runs
   a compare-and-set transaction — `WHERE id = $1 AND org_id = $2 AND status
= 'requested' AND ($6::text IS NULL OR user_id = $6)` — with dedicated
   tests (`approval_decision_query_is_tenant_scoped_and_compare_and_set`,
   cross-tenant rejection tests in `orchestration_grpc.rs:1824,1899,2027`).
   Approval grant already atomically writes an identifier-only,
   leaseable/retryable/terminally-settleable row to a real
   `approval_delivery_outbox` table (migration `0014_approval_delivery_outbox.sql`;
   `claim_approval_deliveries`/`acknowledge_approval_delivery` RPCs exist
   server-side, `orchestration_grpc.rs:1487-1560`). **The one thing missing,
   confirmed directly in source**: nothing calls it —
   `execution-core/src/runtime_loop/agent.rs:2237` stubs the client side as
   `Err(Status::unimplemented("claim_approval_deliveries not used"))`. This
   is now the single most concrete, smallest, highest-leverage item in this
   entire document: build the execution-core dispatcher that leases a row,
   resumes the exact suspended graph node, executes it, and acknowledges —
   everything durable it needs to talk to already exists.
2. **§18's "cost-core should become a durable enforcement service" is
   already half true and should not be read as a from-scratch task.**
   `cost-core/cmd/main.go:205-213` requires `DATABASE_URL` unless an explicit
   `COST_CORE_ALLOW_EPHEMERAL=true` dev opt-out is set; the durable path
   (`internal/postgres/store.go`) is append-only `cost_entries` with
   SQL-aggregate rollups, and is the default in production today. An earlier
   audit's "cost ledger in-memory" note is **stale as of this pass** — flag
   it for correction wherever else it's repeated. What genuinely does not
   exist: reservation/commit (hold-then-settle) semantics — grepping the
   service for `Reserve`/`reservation` returns nothing; today's model is
   append-then-check-against-cap. §18.2's "reserved verification cost" idea
   has no implementation to reserve against yet — that is the real
   remaining gap, not durability.
3. **§6's GraphPlan/GraphNode model is confirmed 100% greenfield — this is
   the single biggest architectural bet in the document, size it
   accordingly.** Today's actual persistence is a flat `plans`/`plan_steps`
   schema (`session-core/migrations/0003_orchestration_tables.sql:29-107`)
   with ONE linear, trigger-assigned ordinal per plan (`assign_plan_step_ordinal`,
   MAX+1) — not a DAG — plus a flat one-parent-per-child `subagent_edges`
   relation, not typed dependency edges between arbitrary nodes. No
   per-node state machine exists beyond `approvals.status`
   (requested/granted/denied/timed_out) and a coarse overall `runs.status`
   (queued/completed/failed/cancelled). Every single Temporal workflow in
   `orchestrator-core/cmd/workflows/` — `autoresearch.go`, `deep_task.go`,
   `evaluator_optimizer.go`, `interactive_run.go`, `feedback_promotion.go`,
   `memory_consolidation.go`, `wide_research.go`, `skill_promotion.go` —
   uses the **identical** vanilla `temporal.RetryPolicy{BackoffCoefficient:
2.0, MaximumAttempts: 3}`, copy-pasted, with zero semantic recovery
   levels (no `escalat`/`RecoveryLevel`/`replan`/`alternate_capability`
   anywhere in either service). §6's proposal is real, smart, and the
   correct direction — but do not schedule it as a quick win next to item 1
   above. It has no existing scaffolding to extend.

**Per-component ledger** (EXISTS / PARTIAL / GREENFIELD), cross-referenced
inline at each relevant section below rather than only here. Re-verified
2026-08-09 against source; changes from the 2026-08-03 pass are marked.

| Doc section | Proposed component                                | Verified state                                                             |
| ----------- | ------------------------------------------------- | -------------------------------------------------------------------------- |
| §8          | Capability attestation                            | GREENFIELD — see §8 note (unchanged 08-09)                                 |
| §9          | Step-level model routing + RoutingDecision record | PARTIAL (request-level only) — see §9 note (unchanged 08-09)               |
| §13         | Skill promotion / failure learning                | PARTIAL, and a real live instance already ships — see §13 note (unchanged 08-09) |
| §14         | Durable approval continuation                     | **DONE 08-09** (was PARTIAL ~70% on 08-03) — see the 2026-08-09 correction above |
| §18         | Durable cost ledger                               | **DONE** — see the correction above (unchanged 08-09)                      |
| §21         | Loop engineering (goal loop specifically)         | The acute pain this would fix is **already fixed**, simpler — see §21 note (unchanged 08-09) |
| §22         | TOON usage                                        | EXISTS, narrower than proposed — see §22 note (unchanged 08-09)            |
| §22.6       | Anthropic prompt caching                          | EXISTS, Anthropic-only — see §22 note (unchanged 08-09)                    |
| §23.1–23.2  | Progressive MCP disclosure                        | GREENFIELD — mechanism now specified by the 2026-07-28 MCP RC, see §23 note |
| §23.4       | MCP code mode                                     | GREENFIELD, confirmed (unchanged 08-09)                                    |
| §23.9       | Outcome-driven capability ranking                 | **Real ranking already exists, extend it** — see §23 note (unchanged 08-09) |
| §23 (new)   | MCP client migration to the 2026-07-28 RC          | **NEW 08-09** — our client is legacy stateful; see §23 note                |

---

## 2. Terminology note: GraphPloop / GraphLoop

No established public project named **GraphPloop** was found during the GitHub and web research pass. This document therefore treats the term as **GraphLoop-style execution**:

> An explicit, versioned execution graph that separates planning, execution, verification, and recovery instead of hiding all control flow inside an open-ended LLM loop.

The design borrows from graph runtimes such as LangGraph, Microsoft Agent Framework workflows, Mastra workflows, deterministic orchestration systems such as Microsoft Conductor, and newer research on structured graph harnesses and execution lineage.

---

## 3. Non-goals

The improvements must not:

- replace Temporal with LangGraph, Mastra, Pydantic AI, or another workflow runtime;
- replace `execution-core` with OpenHands, Claude Agent SDK, OpenAI Agents SDK, or a hosted managed-agent platform;
- let Letta become the source of truth for sessions, company knowledge, or permissions;
- let LangChain or Langflow enter the production hot path;
- create a second capability, approval, memory, or workflow UI;
- allow Model Plane to bypass Control Plane identity or Ingestion Plane provider execution;
- allow a model to directly invoke arbitrary tools merely because an MCP server exposes them;
- duplicate canonical Data Plane retrieval, embeddings, graph, or wiki ownership.

---

## 4. Current strengths to preserve

### 4.1 Clear plane ownership

Model Plane owns:

- authenticated inference and streaming;
- thread, run, checkpoint, and context assembly;
- planning and execution loops;
- agent and subagent behavior;
- capability selection and policy checks;
- approval pauses;
- Temporal agent workflows;
- provider routing;
- Model Plane-local artifacts and events.

It does not own:

- identity and organization truth;
- canonical business records;
- document, embedding, graph, or wiki stores;
- raw connector acquisition and browser evidence;
- frontend projection state.

### 4.2 Correct language split

Keep the current rule:

| Work type                                                                          | Preferred runtime |
| ---------------------------------------------------------------------------------- | ----------------- |
| Latency-sensitive inference, context packing, protocol boundaries, execution loops | Rust              |
| Durable orchestration, registries, policy, resource lifecycle, approvals, budgets  | Go                |
| Provider experiments, model research, evals, benchmark adapters                    | Python            |

### 4.3 Correct infrastructure split

| Dependency     | Canonical Model Plane role                                              |
| -------------- | ----------------------------------------------------------------------- |
| PostgreSQL     | Durable sessions, plans, approvals, lineage, capabilities, cost records |
| Temporal       | Long-running workflows, recovery, approvals, schedules, research loops  |
| NATS/JetStream | Events and asynchronous cross-service notifications                     |
| MinIO          | Large artifacts, execution outputs, snapshots, generated files          |
| Dragonfly      | Cache, leases, limits, temporary coordination                           |
| Letta          | Optional memory adapter only                                            |
| OpenTelemetry  | Neutral tracing and metrics                                             |

---

## 5. External stack audit and disposition

The correct question is not "Which framework should Verevon use?" It is:

> Which implementation ideas should be incorporated into Verevon's first-party runtime, and which libraries are useful only in labs or adapters?

| Project or technology        | Strongest pattern                                                                        | Verevon decision                                                              |
| ---------------------------- | ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| OpenHands Software Agent SDK | Typed actions/observations, workspaces, condensers, security checks, remote agent server | Learn from and benchmark; do not replace Model Plane                          |
| OpenRAG                      | Packaged ingestion/retrieval contracts, Docling, knowledge MCP                           | Reuse parser ideas; do not deploy as Model Plane                              |
| ECC                          | Plan-test-review-verify-remember-improve, skills, rules, hooks, security scanning        | Implement natively in `capability-core`                                       |
| OpenWork                     | `search_capabilities` then `execute_capability`                                          | Implement natively in capability discovery                                    |
| OpenWorker                   | Approval inbox, completed deliverables, unattended approval parking                      | Implement in Model/Application planes                                         |
| Orca                         | Parallel attempts, isolated execution, annotated review                                  | Use selectively for high-risk tasks and evals                                 |
| Buzz                         | Humans and agents as actors in one event model                                           | Standardize Verevon event envelopes and actor identity                        |
| OpenAI Agents SDK            | Handoffs, tool guardrails, sessions, traces, structured outputs, sandbox-agent concepts  | Provider reference adapter and compatibility test suite                       |
| Claude Agent SDK             | Isolated SDK configuration, hooks, tools, MCP, local process ownership                   | Provider reference adapter; avoid managed-agent dependency for core execution |
| Microsoft Agent Framework    | Harness, graph workflows, type-safe routing, checkpoints, HITL                           | Strong reference for GraphPlan and orchestration patterns                     |
| Microsoft Conductor          | Deterministic YAML/graph orchestration with explicit context flow                        | Borrow deterministic routing and diffable workflow ideas                      |
| Pydantic AI                  | Typed agents and official Temporal durability integration                                | Python lab and contract reference only                                        |
| Mastra                       | Workspaces, tool permissions, workflow snapshots, suspend/resume                         | TypeScript reference; borrow workspace capability concepts                    |
| LangGraph                    | Explicit state graphs, interrupts, checkpointing, time travel                            | Research/reference only; Temporal remains canonical                           |
| LangChain / Deep Agents      | Fast prototype loops, middleware, HITL patterns                                          | Python lab only                                                               |
| Langflow                     | Visual prototyping                                                                       | Optional local profile only; no production authority                          |
| Letta                        | Persistent memory blocks and attach/detach semantics                                     | Optional adapter; Verevon owns memory schema and policy                       |
| MCP                          | Tool interoperability                                                                    | Use protocol SDK; gate through capability-core                                |
| ACP                          | Agent-client interoperability                                                            | Add when external agent clients need Model Plane access                       |
| GraphLoop pattern            | Immutable plan version, explicit nodes, verification, recovery edges                     | Build as native Model Plane execution model                                   |

---

## 6. Target execution model

**Verified 2026-08-03 — see §1a correction 3 for the full detail: this is
confirmed 100% greenfield**, the single biggest architectural bet in this
document. Today's `plans`/`plan_steps` schema is one linear ordinal list,
not a DAG; no per-node state machine exists; every Temporal workflow uses
an identical copy-pasted vanilla retry policy with zero semantic recovery
levels. Sequence this as real, multi-quarter work (as §36 Phase 1 already
does) — do not let it get scheduled alongside the much smaller, much
closer-to-done items in §1a (the approval dispatcher, cost reservation
semantics).

### 6.1 Replace the implicit loop as the only execution model

The current action loop remains useful for short, bounded tasks. Longer or effectful tasks should compile into a `GraphPlan`.

```text
Task request
    -> task classification
    -> planning policy
    -> GraphPlan vN
    -> Temporal supervision
    -> node execution
    -> node verification
    -> recovery or continuation
    -> outcome verification
    -> result and learning record
```

The key principle:

> The model proposes control flow; Model Plane stores and executes an explicit graph.

### 6.2 GraphPlan

```ts
interface GraphPlan {
	planId: string;
	version: number;
	tenantId: string;
	runId: string;

	objective: string;
	successCriteria: SuccessCriterion[];
	constraints: Constraint[];

	nodes: GraphNode[];
	edges: GraphEdge[];

	createdBy: ActorRef;
	createdAt: string;
	supersedesVersion?: number;

	policySnapshotId: string;
	capabilitySnapshotId: string;
	modelRoutingSnapshotId: string;
}
```

A plan version is immutable after execution starts. Replanning creates a new version linked to the previous version.

### 6.3 GraphNode

```ts
interface GraphNode {
	nodeId: string;
	kind:
		| "reason"
		| "retrieve"
		| "tool"
		| "browser"
		| "human"
		| "verify"
		| "transform"
		| "subagent"
		| "join"
		| "finish";

	inputSchema: JsonSchema;
	outputSchema: JsonSchema;

	dependencies: string[];
	capabilityQuery?: CapabilityQuery;
	selectedCapabilityId?: string;

	riskClass: "none" | "read" | "low" | "medium" | "high";
	approvalPolicyId?: string;

	modelPolicy?: ModelPolicy;
	retryPolicy: RetryPolicy;
	timeoutPolicy: TimeoutPolicy;
	verificationPolicy: VerificationPolicy;
	recoveryPolicy: RecoveryPolicy;
}
```

### 6.4 Node state machine

```text
PENDING
  -> READY
  -> RUNNING
  -> AWAITING_APPROVAL
  -> SUCCEEDED
  -> VERIFYING
  -> VERIFIED
  -> FAILED_RETRYABLE
  -> RECOVERING
  -> FAILED_FINAL
  -> SKIPPED
  -> CANCELLED
```

Temporal owns durable progress. PostgreSQL stores the canonical plan, node state, and user-visible execution record. NATS announces state changes but is not the authority.

### 6.5 Recovery levels

```text
Level 0: retry same call with idempotency protection
Level 1: retry with richer context or repaired arguments
Level 2: choose an alternate capability/provider
Level 3: replan the current subgraph
Level 4: create a new full plan version
Level 5: request human intervention
```

Recovery must be bounded. Open-ended self-retry loops are prohibited.

---

## 7. Planner, executor, verifier separation

Every important step should distinguish three roles.

### Planner

- defines the intended outcome;
- selects the next node or creates a graph;
- identifies risks and evidence needs;
- does not directly claim that execution succeeded.

### Executor

- resolves authorized capabilities;
- calls the selected tool, browser, sandbox, or provider;
- returns typed observations and receipts;
- never invents approval authority.

### Verifier

- checks deterministic postconditions first;
- checks provider state or rereads resources when possible;
- uses a model only when deterministic verification is insufficient;
- emits `verified`, `failed`, or `unknown`.

Example:

```text
Planner: change customer assignment to user_42
Executor: call CRM update capability with idempotency key
Verifier: reread customer and compare assignee
Result: VERIFIED only after the reread matches
```

This pattern should apply to browser actions, provider writes, file changes, notifications, and business workflows.

---

## 8. Capability discovery, attestation, and execution

### 8.1 Two-step capability use

Implement the OpenWork-style pattern inside `capability-core`:

```text
search_capabilities
    -> policy filtering
    -> ranked candidate list
    -> select capability
    -> attest exact execution request
    -> execute capability
    -> verify outcome
```

### 8.2 Capability definition

**Verified 2026-08-03 — PARTIAL.** No type named `CapabilityDefinition`
exists, but two real structs cover much of this: `models.Capability`
(`internal/models/capability.go:45-69` — ID, Name, Kind, Version,
Description, RiskLevel, LazyLoad, Scope, Enabled, IdempotencyKey, OrgID,
EnabledForScopes, RolloutState, AvailabilityState, ExecutionMode,
CostClass) and the richer persisted `registry.CapabilityRow`
(`internal/registry/capabilities_store.go:24-58` — adds SchemaInput/Output
JSON schemas plus _measured_, not estimated, P95LatencyMS/MeanCostUSD).
`RiskLevel`/`RolloutState`/`AvailabilityState` are rough proxies for this
proposal's `riskClass`/`readiness`. Genuinely missing: `ownerService`,
`intents`, `sideEffect`, `reversible`, `requiredScopes`, `residencyClasses`,
`privacyClasses`, `verificationCapabilityId`, `approvalPolicyId`,
`attestationKeyId` — none of these fields exist today. Extend the existing
structs rather than introducing a parallel schema.

```ts
interface CapabilityDefinition {
	capabilityId: string;
	version: number;
	ownerService: string;

	description: string;
	intents: string[];
	inputSchema: JsonSchema;
	outputSchema: JsonSchema;

	sideEffect: boolean;
	reversible: boolean;
	riskClass: "read" | "low" | "medium" | "high";

	requiredScopes: string[];
	residencyClasses: string[];
	privacyClasses: string[];

	estimatedLatencyMs: number;
	estimatedCost: CostEstimate;

	verificationCapabilityId?: string;
	approvalPolicyId?: string;

	readiness: "disabled" | "experimental" | "ready" | "degraded";
	attestationKeyId: string;
}
```

### 8.3 Capability attestation

**Verified 2026-08-03 — GREENFIELD.** No per-call attestation exists.
`capability-core/internal/models/availability.go` implements only a runtime
**health heartbeat** (`AvailabilityAttestationTTL = 5*time.Minute`), gated by
a coarse scope check (`HealthWriteScope`/`GlobalHealthWriteScope`,
`internal/authz/authz.go:22-27`) — not a short-lived signed token bound to
tenant/actor/run/node/capability-version/payload-digest/idempotency-key.
`IdempotencyKey` in the capability catalog (`registry/models.go:89`) is a
**static, per-capability fingerprint set at registration time**, not a
runtime, per-invocation signed grant. This section's proposal is the
correct fix for a real gap, not a restatement of something already covered
by the health-attestation heartbeat (§34's capability-core plan and the
2026-07-30 code-interpreter/canvas health-attestation work are a different,
narrower thing: "is this capability's _backend_ alive," not "is _this
specific call_ authorized").

Before an effectful call, `capability-core` should mint a short-lived, exact attestation bound to:

- tenant;
- actor;
- run and graph node;
- capability ID and version;
- payload digest;
- target resource;
- idempotency key;
- approval record;
- expiration;
- allowed provider effect.

The executor must reject any mismatch.

### 8.4 Capability retrieval ranking

Do not send every tool schema to the model. Rank capabilities using:

```text
intent match
x tenant enablement
x actor permission
x task grant
x risk compatibility
x provider readiness
x historical success
x latency score
x cost score
x residency compatibility
```

Return only the most relevant candidates.

---

## 9. Harness-native model routing

Model routing must move from request-level routing to **step-level routing**.

**Verified 2026-08-03 — PARTIAL, request-level only.** `inference-core`'s
`intent::resolve` (`provider/intent.rs:236-270`) is called exactly once per
turn, at the top of `FallbackChain::infer`/`infer_stream`
(`provider/fallback.rs:579-609,692,929`) — it rewrites `req.model` once for
the whole request. A tool-bearing turn is floored to `Complexity::Complex`
as a whole (`intent.rs:148-160`, part of this week's fix for the
tool-routing bug), not per sub-step. The only "different model within one
turn" behavior that exists today is the **provider fallback ladder**
(`fallback.rs:659-681,770-778`) — it tries alternate models sequentially
only when the chosen model fails or is undeployed, which is reactive
failover, not the deliberate multi-model plan this section proposes. The
`Decision` struct that already exists (`intent.rs:222-228`: `model`, `mode`,
`complexity`, `posture`) is only ever `info!`-logged
(`fallback.rs:598-605`) as unstructured tracing text — it is not persisted
and has none of the `RoutingDecision` fields below (`fallbackModelIds`,
`reasonCodes`, `confidence`, `predictedQuality/Cost/Latency`,
`requiresIndependentVerifier`). This section's proposal is real, unbuilt
work with a genuine hook to extend from (`FallbackChain` already threads a
`Decision` through every call) — it is not a green field, but it is also
not close to done.

A planning step, structured extraction step, tool-selection step, browser visual step, verification step, and summarization step may need different models.

### 9.1 Routing input

```ts
interface AgentRoutingContext {
	tenantId: string;
	runId: string;
	nodeId: string;

	taskClass: string;
	stepClass: string;
	riskClass: string;

	contextTokens: number;
	expectedOutputTokens: number;
	modality: "text" | "vision" | "audio" | "browser" | "code";

	availableModels: ModelProfile[];
	previousAttempts: AttemptSummary[];
	currentBudget: BudgetState;
	latencySloMs: number;
	privacyPolicy: PrivacyPolicy;
}
```

### 9.2 Routing strategies

```text
Cold start:
  deterministic rules + model capability profiles

Warm routing:
  historical success by task/step/provider

Learned routing:
  contextual bandit or ranker using traces and verified outcomes

High-risk ensemble:
  complementary planners or planner + verifier
```

### 9.3 Routing output

```ts
interface RoutingDecision {
	modelId: string;
	fallbackModelIds: string[];
	reasonCodes: string[];
	confidence: number;
	predictedQuality: number;
	predictedCost: number;
	predictedLatencyMs: number;
	requiresIndependentVerifier: boolean;
}
```

### 9.4 Data flywheel

Every model decision should create a training record:

```text
harness state
+ selected model
+ prompt/context profile
+ tool/capability choices
+ retries and failures
+ verified outcome
+ latency
+ cost
+ human correction
= routing training example
```

The router should learn from **verified environmental outcomes**, not only model self-ratings.

---

## 10. Smart agentic decisions

The runtime should classify decisions by what should control them.

| Decision              | Preferred control                     |
| --------------------- | ------------------------------------- |
| Tenant permission     | Deterministic policy                  |
| Approval requirement  | Deterministic policy                  |
| Idempotency and retry | Runtime policy                        |
| Known workflow branch | Deterministic graph edge              |
| Capability relevance  | Retrieval/ranker with policy filter   |
| Ambiguous plan        | LLM planner                           |
| Model selection       | Router based on task, outcomes, cost  |
| Completion            | Deterministic verifier first          |
| Recovery escalation   | Bounded runtime policy                |
| Memory promotion      | Review workflow, not automatic append |
| Skill promotion       | Eval and approval gate                |

### 10.1 Decision confidence

Every nontrivial model decision should return:

- decision type;
- selected option;
- alternatives considered;
- confidence;
- evidence references;
- reason codes;
- predicted risk;
- whether a verifier is required.

Do not expose private chain-of-thought. Store concise, inspectable decision summaries.

### 10.2 Stop conditions

Each graph must define explicit stop conditions:

- success criteria met;
- insufficient evidence;
- budget exhausted;
- repeated failure limit;
- denied capability;
- approval expired;
- unsafe state;
- user cancellation.

---

## 11. Multi-agent patterns

Multiple agents should be used only when they improve correctness enough to justify cost.

### Sequential

Use when outputs naturally feed later steps.

```text
research -> analysis -> draft -> verify
```

### Parallel

Use for independent evidence gathering, competing plans, or specialist analysis.

```text
policy specialist
customer-context specialist
risk specialist
        -> join/verifier
```

### Handoff

Use when one specialist should take control of the remaining conversational task.

### Manager/supervisor

Use when a coordinator needs to assign subtasks dynamically, but bound the number of agents and turns.

### Debate or ensemble

Use only for high-risk ambiguity, not as a default.

### Context isolation

Each subagent receives an explicit context packet:

```ts
interface SubagentContextPacket {
	objective: string;
	allowedCapabilities: string[];
	evidenceRefs: string[];
	relevantArtifacts: string[];
	constraints: string[];
	outputSchema: JsonSchema;
	tokenBudget: number;
}
```

Do not share full conversation history by default.

---

## 12. Memory architecture

Memory should be split into distinct authorities.

| Memory class                     | Owner                                   | Purpose                                   |
| -------------------------------- | --------------------------------------- | ----------------------------------------- |
| Working memory                   | `session-core`                          | Current run state and active context      |
| Conversation memory              | `session-core` / Application projection | Multi-turn continuity                     |
| Verified company knowledge       | Data Plane                              | Durable facts with provenance             |
| Episodic task memory             | Model Plane                             | Previous runs, outcomes, corrections      |
| Procedural memory                | `capability-core`                       | Skills, workflows, successful procedures  |
| Optional semantic memory backend | `letta-bridge`                          | Experimental retrieval/compaction adapter |

### 12.1 Memory record

```ts
interface AgentMemoryRecord {
	memoryId: string;
	tenantId: string;
	subjectId?: string;
	agentId?: string;

	class: "episodic" | "procedural" | "preference" | "working_summary";
	content: string;

	sourceRefs: string[];
	runId?: string;
	confidence: number;

	status: "proposed" | "verified" | "rejected" | "superseded";
	validFrom?: string;
	validUntil?: string;
	supersedesId?: string;

	privacyClass: string;
	retentionClass: string;
	createdAt: string;
}
```

### 12.2 Memory promotion

```text
run outcome
    -> candidate memory extraction
    -> duplicate/contradiction check
    -> privacy and retention check
    -> confidence/evidence check
    -> human or policy review when required
    -> promote or reject
```

Letta may index or retrieve approved memory, but it must not define the canonical schema or bypass Verevon policy.

---

## 13. Skills and continuous improvement

**Verified 2026-08-03 — a real, live, working piece of this already
shipped, and it is leaner than the checklist below.** As of 2026-08-01,
`orchestrator-core`'s `internal/quality` package computes a Wilson-95%-lower-bound
score per skill from weighted evidence (explicit thumbs ratings at full
weight, implicit signals — Regenerate/EditResubmit/NearDuplicate/Correction
— discounted to 0.25× and further scaled by detector confidence). Below a
threshold it **quarantines** the skill — disables its injection into future
prompts via a dedicated `SetAgentSkillEnabled` RPC to session-core,
deliberately NOT the generic upsert (which would have overwritten the
skill's content) — with hysteresis to prevent flapping and a per-sweep cap.
This is a genuine, narrower, already-running instance of §13.3's "failure
learning" loop: it does not do the full promotion checklist below
(regression tests, security scan, tenant/global promotion policy), but it
is live, catching real signal today, not a proposal. Recommendation: treat
§13.2's full promotion checklist as the target state for _turning a learned
behavior into a new active skill_, and treat the quarantine system as the
already-solved _demoting a bad skill_ half of the lifecycle — extend it,
do not replace it, and note recovery from quarantine is still deliberately
manual (the system can't yet distinguish a policy quarantine from a human
disabling a skill on purpose).

Borrow ECC's operating principle:

```text
plan -> execute -> review -> verify -> remember -> improve
```

Implement it as a native skill lifecycle.

### 13.1 Skill definition

```ts
interface AgentSkill {
	skillId: string;
	version: number;
	tenantId?: string;

	description: string;
	triggerExamples: string[];
	instructions: string;

	requiredKnowledge: string[];
	allowedCapabilities: string[];

	inputSchema: JsonSchema;
	outputSchema: JsonSchema;

	approvalRules: ApprovalRule[];
	verificationSteps: VerificationStep[];

	evalSuiteId: string;
	status: "draft" | "candidate" | "active" | "deprecated";
}
```

### 13.2 Skill promotion

A learned behavior may become an active skill only after:

- repeated successful outcomes;
- deterministic or human verification;
- no unresolved policy violations;
- regression tests;
- cost and latency checks;
- security scan;
- tenant/global promotion policy.

### 13.3 Failure learning

Store structured failure categories:

- wrong capability selected;
- correct capability, invalid arguments;
- provider unavailable;
- policy denied;
- approval rejected;
- incomplete context;
- model hallucinated completion;
- verification failed;
- duplicate side effect prevented;
- budget exceeded.

The router and skill system should use these categories rather than raw free-text logs.

---

## 14. Durable approvals

Approval continuation is a release-critical feature.

**Verified 2026-08-03 — see §1a correction 1 for the full detail.** §14.2's
tenant/actor/payload-digest validation requirement is already met in
`session-core`. The durable, leaseable delivery-outbox this section implies
already exists. The one gap is a dispatcher in `execution-core` — its
client-side handler for the exact RPC needed
(`claim_approval_deliveries`) is a literal `unimplemented!()` stub today.
Do not scope this section as "build durable approvals" — scope it as
"build the one missing consumer of durable approvals that already exist."

### 14.1 Approval object

```ts
interface ApprovalRequest {
	approvalId: string;
	tenantId: string;
	runId: string;
	graphNodeId: string;

	actionSummary: string;
	capabilityId: string;
	exactArgumentsDigest: string;
	semanticDiff?: SemanticDiff;

	evidenceRefs: string[];
	riskClass: string;
	reversible: boolean;

	allowedDecisions: Array<"approve" | "edit" | "reject" | "respond">;
	requiredRoles: string[];

	expiresAt: string;
	status: "pending" | "approved" | "edited" | "rejected" | "expired";
}
```

### 14.2 Continuation rules

- Approval must be durable before returning `awaiting_approval`.
- Resume must validate the exact run, node, payload digest, tenant, and approver.
- Edited actions create a new payload digest and re-run policy checks.
- Approval is single-use.
- Expired approval cannot execute.
- Temporal resumes the exact workflow point.
- Effectful execution still uses idempotency and provider receipts.

---

## 15. Context assembly and compaction

Context should be assembled from typed sections, not one uncontrolled transcript.

```text
system policy
agent definition
current objective
GraphPlan node input
verified evidence
authorized memory blocks
selected skill
capability candidates
recent action/observation window
budget and stop conditions
```

### 15.1 Context manifest

```ts
interface ContextManifest {
	contextId: string;
	runId: string;
	nodeId: string;

	sections: Array<{
		kind: string;
		sourceRef: string;
		tokenCount: number;
		trustClass: string;
		retentionClass: string;
	}>;

	totalTokens: number;
	compactionVersion: string;
	redactionVersion: string;
}
```

### 15.2 Compaction

Borrow OpenHands condenser and modern harness patterns:

- compact by semantic section, not arbitrary message count;
- preserve unresolved commitments;
- preserve tool receipts and verification state;
- preserve citations and approval records;
- remove repeated tool schemas;
- store compacted source references so details can be rehydrated;
- never compact away an active policy constraint or pending approval.

---

## 16. Sandbox and workspace model

Borrow OpenHands and Mastra workspace separation without adopting their control planes.

```text
Workspace
  - mounted files/artifacts
  - allowed commands
  - network policy
  - package policy
  - secrets by reference
  - resource limits
  - snapshot policy
  - approval policy
```

### 16.1 Sandbox lease

```ts
interface SandboxLease {
	leaseId: string;
	tenantId: string;
	runId: string;
	nodeId: string;

	backend: string;
	capabilities: string[];
	networkPolicyId: string;
	secretRefs: string[];

	acquiredAt: string;
	expiresAt: string;
	snapshotRef?: string;
}
```

Critical lease state must be durable or reconstructable. In-memory-only lifecycle state is not acceptable for production resumability.

---

## 17. Tracing and execution lineage

OpenAI Agents SDK, OpenHands, LangGraph, and Buzz all reinforce the value of complete event lineage.

### 17.1 Canonical trace hierarchy

```text
workflow/run
  plan version
    graph node
      model turn
      capability search
      policy decision
      approval
      tool execution
      verification
      recovery
```

### 17.2 Span fields

Each span should include:

- tenant, run, plan, node, and actor IDs;
- model and provider;
- capability and version;
- input/output schema versions;
- evidence and artifact refs;
- privacy and retention classes;
- token usage;
- cost;
- latency;
- retry count;
- verification outcome;
- failure class;
- route decision ID.

Do not store hidden model chain-of-thought. Store concise decision summaries, tool calls, observations, and outcomes.

### 17.3 Execution lineage

Artifacts should record dependencies:

```text
source evidence -> research notes -> analysis -> draft -> approval -> final artifact
```

When one source changes, the system can identify which downstream artifacts require replay instead of regenerating everything.

---

## 18. Cost and budget enforcement

**Verified 2026-08-03 — durability is DONE, correcting an earlier stale
audit note.** `cost-core` already requires `DATABASE_URL` in production
(`cmd/main.go:205-213`; only an explicit `COST_CORE_ALLOW_EPHEMERAL=true`
dev opt-out falls back to memory) and persists an append-only `cost_entries`
ledger with SQL-aggregate rollups (`internal/postgres/store.go`). Separately,
this week's fix to cost-aware model selection (Budget/Balance/Genius) closed
a real gap where the budget-check call carried no auth header and 401'd on
every call since it was built — that enforcement path is real and live now
too. What genuinely does not exist, confirmed by an empty grep for
`Reserve`/`reservation` in the service: **reservation/commit (hold-then-settle)
semantics.** Today's model is append-then-check-against-cap, not
reserve-before-spend. Read §18.1/§18.2 below as "add reservation semantics
to an already-durable ledger," not "make the ledger durable."

`cost-core` should become a durable enforcement service rather than an observability-only ledger.

### 18.1 Budget dimensions

- per tenant;
- per user;
- per agent;
- per skill;
- per run;
- per graph node;
- per model/provider;
- browser and sandbox cost;
- external API cost.

### 18.2 Runtime enforcement

Before a step:

```text
remaining budget
>= predicted step cost
+ reserved verification cost
+ bounded recovery reserve
```

If not, the runtime must:

- select a cheaper route;
- reduce fan-out;
- request increased budget;
- return partial results;
- or stop safely.

### 18.3 Cost-aware verification

Do not save money by skipping verification on effectful operations. Reserve verification cost at plan time.

---

## 19. Evaluation architecture

### 19.1 Three eval layers

| Layer          | Question                                                |
| -------------- | ------------------------------------------------------- |
| Component eval | Does one model/tool/retriever work correctly?           |
| Harness eval   | Does routing, context, recovery, and verification work? |
| Product eval   | Does the full task satisfy a customer outcome safely?   |

### 19.2 Required metrics

- task success rate;
- verified success rate;
- false success rate;
- approval correctness;
- policy violation rate;
- capability-selection accuracy;
- tool argument validity;
- recovery success;
- average retries;
- p50/p95 latency;
- cost per verified task;
- human correction rate;
- memory precision;
- skill regression rate.

### 19.3 Comparative harnesses

Use provider SDKs and external frameworks as benchmark adapters:

```text
Verevon native runtime
OpenAI Agents SDK adapter
Claude Agent SDK adapter
OpenHands SDK adapter
Pydantic AI lab adapter
LangGraph lab adapter
Mastra lab adapter
```

Run the same tasks with the same tools, budgets, and success criteria.

---

## 20. Security improvements

**New findings, 2026-08-02/03, from a full down-the-stack code-health
audit — these are exactly the class of gap this section exists to close,
and should be treated as Phase 0 release-correctness items alongside §14's
approval dispatcher, not filed separately:**

- **A documented HITL safety invariant is violated in
  `execution-core/src/browser_agent/browser_risk.rs:148`.** Three separate
  doc comments in this codebase (`browser_risk.rs:144-147`,
  `browser_agent.rs:147-151`, `llm_planner.rs:41-44`) assert that
  `classify_action_risk` ORs the model's self-reported risk category with a
  deterministic keyword/URL backstop, "so a model that omits or
  under-reports risk cannot silently bypass the gate." The code does not do
  this: a self-report of ANY category short-circuits the backstop entirely
  — the "omits" case (no self-report) correctly falls through, but
  "under-reports" (e.g. the model says `risk_category='login'` for an
  action that is actually checkout/destructive) never runs the backstop at
  all. The approval gate still fires either way, but the audited "why"
  shown to the human reviewer, and persisted to the durable Approval
  record, can be wrong for exactly the highest-stakes actions this gate
  exists to protect. This directly undermines §7's planner/executor/verifier
  separation and §31's containment philosophy — fix the OR logic before
  building more on top of this gate.
- **`capability-core`'s PATCH/DELETE handlers silently no-op on a
  wrong/foreign/nonexistent id.** 13 call sites across
  `internal/api/workplane_apis.go` and `internal/api/registry_apis.go`
  (`TasksHandler`, `CronHandler`, `SkillsHandler`, `RoutingHandler`,
  `SafetyHandler`) discard both the SQL error and the rows-affected count,
  so a mutation against the wrong org/id still returns 200. For
  `safety_policies`/`routing_policies` specifically, an operator
  "disabling" a policy can get a success response while the policy stays
  active — a correctness gap this document's own capability-lifecycle
  proposals (§8, §23) would silently inherit if built on top of it
  unfixed. Fix first.
- **`letta-bridge`'s Postgres memory tier makes `DeleteMemory` a permanent
  silent no-op** (no per-user ownership column to verify against;
  `Deleted: false` with no error, indistinguishable on the wire from
  "already deleted"). This is directly the concern §34's letta-bridge plan
  raises ("memory provenance and deletion propagation") — it is not
  hypothetical, it is the live fallback tier whenever the semantic
  agent-memory client isn't configured. Any erasure/DSAR workflow that
  trusts this response leaves rows behind indefinitely.
- **`session-core` is the one internal Go service whose HMAC
  service-delegation has no anti-replay nonce**, unlike every sibling
  (org-core, billing-core, user-core, audit-core all added a
  `delegationNonceCache` — session-core did not). An identical signed
  request can be replayed any number of times inside its ~35s validity
  window. Worth closing given how much of this document's proposed work
  (§14, §8, §31) routes through session-core's trust boundary.

### 20.1 Tool and MCP security

- MCP registration is not trust.
- Import tool schemas into capability-core.
- Scan tools for risky names, broad schemas, secret exposure, and unbounded network access.
- Require owner, version, attestation, readiness, and policy metadata.
- Apply input and output guardrails around every effectful capability.

### 20.2 Prompt injection boundaries

Treat all external content as untrusted evidence. The runtime must distinguish:

- user instructions;
- system policy;
- verified company policy;
- tool output;
- web/document content;
- suspected injected instructions.

External content must never grant permissions or override capability policy.

### 20.3 Secret handling

- Models receive secret references, never raw credentials.
- Executors resolve secrets at the final trusted boundary.
- Tool results redact secrets before returning to Model Plane.
- Traces store secret metadata only.

### 20.4 Subagent containment

Each subagent receives a least-privilege task grant and bounded capabilities. A parent agent cannot grant more authority than it possesses.

---

## 21. Loop engineering and harness lifecycle

Graph execution defines the durable shape of a run. **Loop engineering** defines how an agent repeatedly makes progress, detects failure, spends additional compute, and decides when to stop.

Model Plane should provide a small library of typed loop templates rather than expressing every recurring behavior as an unbounded model-controlled cycle.

### 21.1 Canonical loop families

| Loop                                                   | Purpose                                                      | Typical owner                  |
| ------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------ |
| Goal loop _(acute pain already fixed, see note below)_ | Move an objective toward verified completion                 | `execution-core` + Temporal    |
| Research loop                                          | Find gaps, search, assess coverage, and synthesize           | `orchestrator-core`            |
| Repair loop                                            | Classify a failure, gather better evidence, and retry safely | `execution-core`               |
| Review loop                                            | Generate, independently evaluate, revise, and re-evaluate    | `execution-core`               |
| Knowledge-maintenance loop                             | Detect changed evidence and update dependent knowledge       | Model Plane + Data Plane       |
| Skill-improvement loop                                 | Turn repeated verified corrections into candidate procedures | `capability-core`              |
| Routing-learning loop                                  | Use execution feedback to improve model/tool selection       | `inference-core` + `cost-core` |

**Verified 2026-08-03 — the goal loop's acute failure mode is already
fixed, and simpler than this section proposes.** `MAX_TOOL_ROUNDS` was 3,
which meant a self-describing MCP server's own tool-discovery calls left
zero rounds to act on results — fixed 2026-07-28 by raising it to 12
(configurable), with tool-result truncation, a larger answer-token budget,
context-assembly reservation, and honest signaling when the tool phase is
cut short. Live-verified before/after. **This means the full
`LoopDefinition`/stall-detection apparatus below is not needed to fix the
problem that most urgently motivated it** — the config/budget fix already
did that. The apparatus remains genuinely valuable for the _other_ loop
families in the table above (research, repair, review,
knowledge-maintenance, skill-improvement, routing-learning), none of which
have anything like it today — size and sequence this section for those,
not as an urgent fix for goal-loop starvation, which is closed.

### 21.2 Loop definition

```ts
interface LoopDefinition {
	loopId: string;
	version: number;
	kind:
		| "goal"
		| "research"
		| "repair"
		| "review"
		| "knowledge_maintenance"
		| "skill_improvement"
		| "routing_learning";

	initialGraphTemplateId: string;
	progressFunctionId: string;
	completionPolicyId: string;
	stallPolicyId: string;
	escalationPolicyId: string;

	maxIterations: number;
	maxRepeatedState: number;
	maxWallClockMs: number;
	maxCost: number;
	verificationReserve: number;
}
```

Every loop requires:

- explicit state;
- an observable progress function;
- deterministic or independently judged completion;
- iteration, time, and cost ceilings;
- repeated-state detection;
- bounded recovery;
- an escalation path;
- a durable handoff artifact.

### 21.3 Progress and stall detection

Do not let the model decide progress from prose alone. Track concrete deltas:

```ts
interface LoopProgress {
	iteration: number;
	newEvidenceCount: number;
	unresolvedRequirementCount: number;
	verifiedRequirementCount: number;
	repeatedStateCount: number;
	newArtifactCount: number;
	verifierScoreDelta?: number;
	estimatedRemainingWork?: number;
}
```

A loop is stalled when one or more conditions hold:

- the same state hash repeats;
- no new evidence or artifact is produced;
- verifier score does not improve across the configured window;
- the same capability fails with the same failure class;
- the planner proposes a previously rejected action without new evidence;
- expected remaining work increases repeatedly;
- marginal quality gain falls below the cost threshold.

### 21.4 Harness contract

The harness should be an explicit object, not an implicit collection of prompts and middleware.

```ts
interface AgentHarness {
	harnessId: string;
	version: number;

	environment: EnvironmentContract;
	contextPolicy: ContextPolicy;
	capabilityView: CapabilityView;
	skillView: SkillView;
	memoryView: MemoryView;
	policySnapshot: PolicySnapshot;
	workspaceLease?: WorkspaceLease;
	verifierProfile: VerifierProfile;
	budget: BudgetState;
	autonomyLevel: AutonomyLevel;
	traceContext: TraceContext;
}
```

### 21.5 Harness assumption registry

A major 2026 lesson is that harnesses encode assumptions about what models cannot do. Those assumptions become stale as models improve.

Store every nontrivial harness component as an explicit assumption:

```ts
interface HarnessAssumption {
	assumptionId: string;
	statement: string;
	compensatingComponent: string;
	evidenceRefs: string[];
	introducedAt: string;
	lastValidatedAt: string;
	modelFamilies: string[];
	removalEvalSuiteId: string;
	status: "active" | "challenged" | "obsolete";
}
```

Examples:

- "This model needs sprint-level decomposition for long tasks."
- "Context compaction is insufficient; clean resets are required."
- "The generator cannot reliably judge visual quality."
- "This model needs full tool schemas rather than summaries."

Run regular **harness ablations** that remove or simplify one component at a time. Keep a component only if it improves verified outcomes, safety, cost, or latency.

### 21.6 Agent legibility

The environment must be easy for agents to understand programmatically.

Provide:

- machine-readable architecture manifests;
- stable service and capability names;
- generated contract indexes;
- exact run/test commands;
- examples of correct artifacts;
- failure reason codes with remediation hints;
- dependency and ownership maps;
- current plan, progress, and remaining-work artifacts;
- explicit definitions of done.

Repository and platform knowledge should be a versioned system of record, not scattered across conversations.

### 21.7 Structured handoffs and context resets

Long-running tasks should support both compaction and clean context reset.

A handoff packet must contain:

```ts
interface AgentHandoff {
	objective: string;
	planVersion: number;
	completedNodeIds: string[];
	activeNodeId?: string;
	remainingNodeIds: string[];

	decisions: DecisionSummary[];
	failedApproaches: FailureSummary[];
	unresolvedQuestions: string[];

	evidenceRefs: string[];
	artifactRefs: string[];
	verificationState: VerificationState;
	budgetState: BudgetState;
	recommendedNextAction: string;
}
```

Use compaction when continuity matters and the context remains healthy. Use a clean reset when:

- attention quality degrades;
- repeated instructions accumulate;
- the model begins prematurely wrapping up;
- the task changes phase significantly;
- a fresh independent evaluator is required.

### 21.8 Brain/hands separation

Keep the existing architectural principle and make it a stable interface:

```text
Brain
  planning, interpretation, model routing, uncertainty, synthesis

Hands
  deterministic capability execution, sandbox, browser, connectors, artifacts

Control
  identity, policy, approval, budgets, durable workflow
```

Models and harnesses may change quickly. Execution, authorization, artifact, and receipt contracts should remain stable.

---

## 22. AI runtime efficiency and context encoding

Verevon should treat context encoding as a routed optimization problem. No single notation is best for every payload.

### 22.1 Canonical representation policy

```text
Durable storage and internal contracts
  Protobuf, JSON, JSON Schema, SQL records

Model-facing structured context
  compact JSON, TOON, Markdown, or terse text selected by benchmark

Effectful tool arguments
  strict JSON Schema / Protobuf-derived schema

Human-facing output
  natural language, Markdown, or domain artifact
```

TOON is a model-facing codec, not a canonical service or storage format.

**Verified 2026-08-03 — TOON exists and is used, correcting an earlier
"unwired" note.** A real encoder crate (`mp-toon`) is wired at three fixed
call sites: a standalone `/v1/toon/encode` debug endpoint
(`model-gateway/src/http_routes.rs:156,6346-6355`), an admin `compact_now`
RPC summary (`session-core/src/grpc.rs:2107`), and — the significant one —
session-core's RAG-fallback path, which requests `context_format: "toon"`
from the Data Plane retrieval RPC (`grpc.rs:2743-2790,2821`). What does
**not** exist: any shape/model-based encoder selection (JSON vs
compact-JSON vs TOON vs CSV vs Markdown) — TOON is invoked at fixed sites,
never dynamically chosen per §22.3's `EncodingCandidate`/`EncodingDecision`
router. Read this section as "add the routing layer on top of an encoder
that already ships," not "adopt TOON."

### 22.2 TOON versus JSON policy

Use TOON when:

- payloads contain uniform arrays of records;
- field names repeat heavily;
- the model only needs to read the data;
- context cost dominates serialization latency;
- benchmarked accuracy is non-inferior for the selected model;
- explicit row counts and field declarations improve validation.

Prefer compact JSON when:

- structures are small, deeply nested, heterogeneous, or sparse;
- data will be signed, hashed, executed, or passed directly to a tool;
- provider-native structured output requires JSON;
- the model performs faster or more accurately with JSON;
- the cost of explaining TOON exceeds its savings.

Prefer CSV when the payload is purely tabular and no nested structure is needed.

### 22.3 Context encoding router

```ts
interface EncodingCandidate {
	encoding: "json" | "compact_json" | "toon" | "csv" | "markdown" | "terse";
	estimatedTokens: number;
	encodeLatencyMs: number;
	expectedAccuracy: number;
	modelCompatibility: number;
}

interface EncodingDecision {
	encoding: EncodingCandidate["encoding"];
	codecVersion: string;
	reasonCodes: string[];
	originalTokenEstimate: number;
	encodedTokenEstimate: number;
	expectedSavings: number;
	schemaHash?: string;
}
```

The router should use payload shape, model profile, task type, latency target, and historical accuracy.

### 22.4 Required codec benchmark

For each payload class, evaluate:

- JSON;
- compact JSON;
- TOON;
- CSV where valid;
- Markdown table;
- Verevon terse format.

Measure:

- tokens;
- time to first token;
- total latency;
- field-retrieval accuracy;
- argument validity;
- malformed output rate;
- truncation behavior;
- encoding CPU cost;
- model/provider variance.

Do not adopt a compact format based on token count alone.

### 22.5 Structured-output hierarchy

```text
1. Provider-native constrained JSON Schema
2. Grammar-constrained output
3. Validated JSON with bounded repair
4. TOON output only after model-specific evaluation
5. Free text only for human-facing prose
```

Effectful actions must never depend on unvalidated free text.

### 22.6 Prompt and KV-cache engineering

**Verified 2026-08-03 — real and more sophisticated than "absent," but
Anthropic-only.** `inference-core`'s `anthropic.rs:244-336`
(`apply_prompt_caching`) places `cache_control: {"type":"ephemeral"}`
breakpoints in hierarchy order (tools → system → last message), respects
each model's minimum cacheable-token floor, caps at 4 breakpoints, is
unconditionally disabled under ZDR, and has extensive tests
(`anthropic.rs:846-1069`). This is exactly "stable prefix ordering." Two
real gaps remain: no equivalent wiring for OpenAI/Azure OpenAI (`grep
cache_control` finds only `anthropic.rs`), and no cache-hit-rate/savings
metrics anywhere — the `PromptCacheProfile` tracking struct below does not
exist yet. Scope this section as "extend to other providers + add
observability," not "build prompt caching."

Design context for provider prompt caching:

- stable system prefix;
- deterministic capability ordering;
- immutable policy and skill version blocks;
- static content before volatile content;
- schema hashes and cache version IDs;
- separate stable instructions from run-specific evidence;
- invalidate on policy, skill, model, or capability version change;
- record cache hit, cached tokens, and savings per node.

```ts
interface PromptCacheProfile {
	provider: string;
	model: string;
	stablePrefixHash: string;
	policyVersion: string;
	skillVersions: string[];
	capabilitySchemaHashes: string[];
	cacheTtlSeconds?: number;
}
```

### 22.7 Context allocation

Every graph node receives a context budget divided by category:

```text
system and policy
objective and node contract
selected skill
capability schemas
retrieved evidence
working state
recent action/observation window
verification state
output reserve
```

Unused categories should not consume a fixed allocation.

### 22.8 Terse internal mode

Implement the earlier TOON/Caveman concept as a formal internal response policy:

```ts
type InternalVerbosity = "normal" | "compact" | "terse" | "machine_structured";
```

Use terse mode for:

- subagent handoffs;
- loop progress;
- tool receipts;
- capability candidate lists;
- repeated observations;
- trajectory summaries.

Do not use it for customer-facing communication, legal records, or explanations where readability matters.

### 22.9 Artifact references instead of repeated payloads

Large results should move by immutable artifact reference:

```ts
interface ArtifactHandle {
	artifactRef: string;
	mediaType: string;
	schemaId?: string;
	contentHash: string;
	sizeBytes: number;
	summary: string;
	accessPolicyId: string;
}
```

Models receive summaries and selected projections. Runtime components transfer the underlying artifact directly.

---

## 23. Capability intelligence and MCP tool-call optimization

MCP is an interoperability protocol. Verevon's canonical capability model remains `capability-core`.

### 23.1 Progressive capability disclosure

**Verified 2026-08-03 — GREENFIELD.** `capability-core`'s `MCPHandler`
(`internal/api/registry_apis.go:227-362`) is pure CRUD over `mcp_servers`
with a **manually-specified, exact `tool_allowlist`** required at
registration (`registry_apis.go:654`) — there is no Level 0-3 staged
disclosure anywhere in the service, and no code path that calls out to an
MCP server to enumerate schemas progressively. (This week's separate
model-gateway fix — "auto-discover tools instead of requiring a manual
allowlist," 2026-07-28 — happens at connect-time in a different service and
does not change what capability-core stores or discloses to the model at
inference time; do not conflate the two.) This section's proposal is real,
unbuilt work — **see §23.11 for the 2026-07-28 MCP spec release candidate,
which supplies a real staged-disclosure primitive (`server/discover`) for
exactly this Level 0-3 model, rather than requiring one to be invented from
scratch.**

Do not load every connected tool schema into model context.

```text
Level 0: provider/server
  identity, trust, readiness, ownership

Level 1: capability summary
  name, intent, risk, cost, short description

Level 2: full schema
  loaded for shortlisted candidates

Level 3: examples and extended docs
  loaded only when argument construction requires them
```

### 23.2 Capability search flow

```text
Task
  -> semantic capability retrieval
  -> deterministic tenant/policy/scope filter
  -> readiness and health filter
  -> historical outcome ranker
  -> load 3-10 full schemas
  -> model selects or runtime dispatches
```

`search_capabilities` should support:

```ts
interface CapabilitySearchRequest {
	query: string;
	taskClass: string;
	desiredEffect: "read" | "write" | "execute";
	requiredResourceTypes?: string[];
	maxRiskClass?: string;
	maxCandidates: number;
	detailLevel: "summary" | "schema" | "examples";
}
```

### 23.3 Tool schema normalization

Normalize imported MCP schemas into a stable internal form:

- canonical names;
- normalized enums and required fields;
- side-effect classification;
- secret-field marking;
- output size estimates;
- pagination contract;
- idempotency support;
- verification method;
- schema hash;
- owner and version;
- residency and privacy classes.

Detect semantically duplicate tools and expose one internal capability with multiple provider implementations.

### 23.4 Code mode for MCP

**Verified 2026-08-03 — GREENFIELD, confirmed.** No `code_mode`/generated-code
execution concept exists anywhere in `capability-core`; its
`internal/sandbox/sandbox.go` only defines a generic execution-sandbox
policy (syscalls/resources/network/filesystem) used for tool/code execution
generally, with nothing specific to compiling MCP tool calls. This is pure
new work, exactly as scoped in this section.

For multi-tool or high-volume tasks, compile selected MCP capabilities into a sandboxed code API.

```text
MCP server definitions
  -> generated typed wrappers
  -> scoped virtual filesystem/package
  -> agent loads only required wrapper files
  -> sandbox executes control flow and transformations
  -> model receives summary, artifacts, and receipts
```

Benefits:

- tool definitions load on demand;
- large intermediate values stay outside model context;
- filtering, joining, loops, and conditionals execute deterministically;
- fewer model turns;
- lower token use;
- reduced copying errors;
- sensitive intermediate data can remain in the sandbox.

### 23.5 Code-mode safety

Generated code must run with:

- capability allowlist;
- no ambient credentials;
- secret references only;
- network egress policy;
- CPU, memory, file, and wall-clock limits;
- deterministic package allowlist;
- read/write effect separation;
- approval attestation for effectful calls;
- complete call receipts;
- output limits and redaction.

### 23.6 Tool result handles

```ts
interface ToolResultHandle {
	handleId: string;
	capabilityId: string;
	artifactRef?: string;
	schemaId?: string;
	rowCount?: number;
	sizeBytes: number;
	summary: string;
	projectionHints: string[];
	expiresAt?: string;
}
```

Allow:

- field projection;
- server-side filtering;
- runtime pagination;
- direct tool-to-tool transfer;
- artifact-to-tool transfer;
- aggregate-only model visibility.

### 23.7 Tool-call graph compilation

When the plan is sufficiently deterministic, compile tool steps into a typed call graph:

```ts
interface ToolCallGraph {
	nodes: ToolCallNode[];
	edges: DataDependency[];
	parallelGroups: string[][];
	compensationPlan?: CompensationStep[];
}
```

The model approves the plan or fills ambiguous parameters; the runtime executes known dataflow without another LLM turn per edge.

### 23.8 Argument repair

Invalid arguments should trigger a narrow repair path:

```text
validation failure
  -> return exact field errors
  -> expose only selected capability schema
  -> repair arguments
  -> revalidate
```

Do not rerun the full planner unless the selected capability is unsuitable.

### 23.9 Outcome-driven capability ranking

**Verified 2026-08-03 — a real version of this already exists; extend it,
do not rebuild it.** `capability-core`'s `ListCapabilities`
(`internal/server/server.go:112`) already ranks via
`rankLocalCapabilities`/`internal/scoring/scoring.go` on a composite score
covering **success rate, schema-fail rate, p95 latency, mean cost,
approval rate, incident count, operator rating, and a rollout-state
multiplier** — most of this section's bullet list is already implemented.
What is missing: this ranking is local/deterministic only; semantic
(intent-based) matching is delegated entirely to an **optional external
Letta tool-search service** (`internal/lettatools/client.go`, vector/FTS/
hybrid modes) that only engages when configured AND the tenant's ZDR
posture is verified-non-ZDR AND the query is non-empty — it is not
authoritative and is not part of `capability-core` itself. The correct
framing for this section: add a first-party semantic/intent layer that
composes with the existing outcome-driven score, rather than treating
outcome-driven ranking as unbuilt.

Update ranking from verified outcomes:

- success/failure by task class;
- argument-repair rate;
- latency;
- cost;
- provider errors;
- rate-limit frequency;
- verification success;
- human correction;
- rollback/compensation rate.

Recent failures should reduce rank only for relevant contexts, not globally disable a useful capability.

### 23.10 Capability bundles

Expose common groups as discoverable bundles:

- email triage;
- customer support;
- SharePoint research;
- browser research;
- invoice processing;
- meeting preparation.

A bundle reduces discovery cost but grants no additional authority.

### 23.11 The 2026-07-28 MCP specification release candidate

**New 2026-08-09.** The `2026-07-28` MCP spec RC ([Anthropic's own
announcement](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/))
is the largest revision since MCP launched, and it lands in two places this
document already names.

**What changed, precisely.** The `initialize`/`initialized` handshake is
removed (SEP-2575): protocol version, client info, and capabilities now
travel in `_meta` on every request instead of being negotiated once, and a
new `server/discover` method lets a client fetch server capabilities
on-demand rather than only at connection time. The `Mcp-Session-Id` header
and the protocol-level session it implied are also removed (SEP-2567) — any
request can now land on any server instance, so the sticky routing and
shared session store a horizontal deployment used to need are no longer
required at the protocol layer. Full JSON Schema 2020-12 is now supported
for tool schemas (previously a constrained subset). Roots, sampling, and
`logging/setLevel` move to Deprecated under a new feature-lifecycle policy
(minimum twelve months before removal); MCP Apps (server-rendered UI) and
Tasks graduate into first-class, opt-in Extensions.

**What this means for our client.** `model-gateway/src/mcp_http.rs` — the
one production Streamable HTTP client, called from `runtime_registries.rs`,
proven live against Visma Net per §140 of `VEREVON.md` — is fully legacy
stateful: it opens with `initialize`, tracks a server-assigned
`Mcp-Session-Id` (`mcp_http.rs:37`, `MCP_SESSION_HEADER`), and negotiates
`MCP_PROTOCOL_VERSION = "2024-11-05"` (`mcp_jsonrpc.rs:17`) once at
connection time. It uses exactly four methods: `initialize`,
`notifications/initialized`, `tools/list`, `tools/call`. The first two are
removed outright by the RC; the two that carry the real work
(`tools/list`/`tools/call`) are structurally unchanged, just re-framed as
single self-contained requests. We use zero roots, sampling, or logging
calls, so none of the deprecations touch us.

The migration this implies is a rewrite of `mcp_http.rs`'s session-opening
logic into a single-request-per-call shape — drop the `initialize`
round-trip and `Mcp-Session-Id` tracking, move client identity into `_meta`,
send `MCP-Protocol-Version`/`Mcp-Method`/`Mcp-Name` per request — bounded to
that one file plus its call site, net code *removed* rather than added
(no session state to hold). Not urgent under the new twelve-month
deprecation floor, but not optional either: third-party MCP servers
(Visma's included) will move to the new spec on their own timeline, and our
client needs to speak it before they drop the legacy handshake.

**What this means for §23.1-23.2 (progressive capability disclosure).**
That proposal is GREENFIELD specifically because nothing today "calls out to
an MCP server to enumerate schemas progressively." `server/discover` is
exactly that primitive on the wire — a client can fetch Level 0/1
(identity, capability summary) cheaply and defer Level 2/3 (full schema,
examples) until a candidate is shortlisted, without a bespoke discovery
protocol of our own. This does not reduce the size of the §23.1-23.2 build
(the staged-disclosure logic, ranking, and caching in `capability-core` are
still entirely unbuilt) — it removes one design question (what wire
primitive to build discovery on) by supplying a standard answer.

**A capability the RC opens that no Verevon document currently proposes:
exposing Verevon as an MCP server, not only consuming one.** Statelessness
is what made Simon Willison's `datasette-mcp` ship after ["the fourth time
[he'd] tried building this
plugin"](https://simonwillison.net/2026/Jul/31/stateless-mcp/) — no session
store, no sticky routing, so a plugin can add an `/-/mcp` endpoint to an
existing service cheaply. The same removal applies to us: a stateless
`/mcp` endpoint over Verevon's grounded retrieval and governed actions
surface could be wired directly into a user's own ChatGPT or Claude, the
same way `datasette-mcp` wires into either today. This is a distribution
question for `verevon-vision.md` and a roadmap-sequencing question for
`verevon-roadmap.md`, not a Model Plane implementation detail — flagged here
because the RC is what makes it newly cheap, not because Model Plane owns
the decision.

### 23.12 Agent Plugins — a portable packaging format for skills and MCP servers

**New 2026-08-09.** [Agent Plugins](https://agent-plugins.org)
([spec repo](https://github.com/agentplugins/agent-plugins-spec)) is a new
open, vendor-neutral standard (v1.0.0; steering committee includes Amazon,
Cursor, Microsoft, OpenAI, Vercel) for packaging Agent Skills and MCP servers
into one portable directory: a closed `plugin.json` manifest, a fixed
`skills/` location holding `SKILL.md`-plus-scripts packages, a fixed
`mcp.json` describing MCP servers (stdio, Streamable HTTP, legacy SSE), and
reverse-domain-namespaced directories for client-specific extras. VS Code and
Cursor are confirmed compatible clients as of this writing (both: skills +
all three MCP transports); Claude Code was not confirmed present in what was
checked.

**This is a different layer from §23.1-23.2, not a replacement for it — do
not conflate the two the same way the 2026-07-28 note above warns against
conflating auto-discovery with capability-core's own disclosure.** Agent
Plugins governs how a *human's editor client* discovers and loads a *plugin
bundle* at install time. §23.1-23.2 governs how *capability-core* discloses
capability information to *the model* at inference time, staged by risk/cost
summary before full schema. Nothing in Agent Plugins changes what
capability-core stores or how model-gateway assembles context — the
GREENFIELD status of §23.1-23.2 is unchanged by this finding.

**Where it is genuinely relevant — two narrow, real connections:**

1. **As an import/export format for `capability-core`'s `mcp_servers` table.**
   `MCPHandler` (`registry_apis.go:227-362`) has its own bespoke registration
   shape today. `mcp.json`'s three closed server variants (stdio /
   streamable-http / sse) are a real, already-adopted-by-two-editors
   alternative to inventing another bespoke import format, if Verevon ever
   wants to accept a plugin-packaged MCP server as a registration source
   rather than only a bare URL. Worth noting precisely what the spec pins
   down that our own registration path does not yet: `command` MUST be a
   single executable token (never a shell string — no injection surface from
   a malformed launch command), remote `url` MUST be HTTPS except for
   loopback, and **the spec is explicit, twice, that `env`/`headers` are
   "visible package data, not a portable secret mechanism"** — authorization
   is entirely client-managed, which is exactly the shape `mcp_oauth.rs`'s
   OAuth 2.1 + Dynamic Client Registration flow already takes. No conflict;
   if anything, external validation that auth belongs at the client layer,
   not in portable config.
2. **As a distribution shape for §24's `SkillPackage`.** §24.2's
   `SkillPackage` interface is Verevon's own independently-designed governed
   skill envelope — versioned, signed, with `allowedCapabilities` and
   `policyRequirements`. Agent Plugins' `skills/<name>/SKILL.md` is
   structurally the same primitive at its core (name, description, packaged
   instructions/scripts/references) but carries none of `SkillPackage`'s
   governance fields — the spec's `extensions` field (reverse-domain
   namespaced, e.g. a hypothetical `com.verevon.skill-package` key) is
   precisely where those Verevon-specific fields would travel if a
   `SkillPackage` were ever exported as a portable Agent Plugin, or imported
   from one authored elsewhere. The two are complementary: §24 is the
   richer internal governance model; Agent Plugins is a candidate external
   interchange shape for the portion of it that has cross-client meaning.

Neither connection is scheduled work — both are noted so a future build of
§23.1-23.2 or §24 does not reinvent a portable format that already has real
adoption, without being talked into building against it before there is a
concrete need to interoperate with an external client.

---

## 24. Skills as versioned software packages

Skills should be treated as governed, composable packages of procedural knowledge, not large prompt fragments.

### 24.1 Progressive skill disclosure

Use one simple progressive-disclosure layer initially:

```text
Startup/task routing context
  skill ID, name, description, triggers, risk

After selection
  full SKILL.md / instructions

On demand
  examples, scripts, references, schemas, tests
```

Research indicates that progressive disclosure becomes valuable as the skill/document corpus grows, while extra routing depth can harm accuracy. Avoid a deeply nested skill-navigation hierarchy until evals justify it.

### 24.2 Skill package

```ts
interface SkillPackage {
	skillId: string;
	version: string;
	publisher: PublisherIdentity;
	contentHash: string;
	signature?: string;

	metadata: SkillMetadata;
	instructionsRef: string;
	resourceRefs: string[];
	scriptRefs: string[];
	graphTemplateRefs: string[];
	evalSuiteId: string;

	dependencies: SkillDependency[];
	conflictsWith: string[];
	allowedCapabilities: string[];
	policyRequirements: string[];
	status: "draft" | "candidate" | "active" | "deprecated" | "revoked";
}
```

See §23.12 for a candidate external interchange shape (Agent Plugins'
`skills/SKILL.md` packaging) this internal envelope could export to or
import from — a distribution question, not a change to this model.

### 24.3 Skill compilation

Compile a skill into:

```text
procedural instructions
+ GraphPlan template
+ capability shortlist
+ context retrieval plan
+ policy/approval rules
+ verification plan
+ output contract
+ eval suite
```

This makes behavior inspectable and testable.

### 24.4 Skill composition

Support typed dependencies rather than copy-pasting instructions.

```ts
interface SkillDependency {
	skillId: string;
	versionRange: string;
	required: boolean;
	inputMapping?: Record<string, string>;
	outputMapping?: Record<string, string>;
}
```

Detect:

- dependency cycles;
- incompatible policies;
- capability conflicts;
- contradictory instructions;
- overlapping triggers;
- redundant skills.

### 24.5 Skill quality scorecard

Evaluate each version on:

- trigger precision and recall;
- verified task success;
- skill uptake/usefulness;
- unnecessary tool calls;
- argument validity;
- recovery success;
- latency and cost;
- human correction;
- safety violations;
- cross-model portability;
- regression against previous version.

### 24.6 Skill candidate generation

Agents may propose skills from repeated trajectories, but proposals remain non-authoritative.

```text
repeated verified workflow or correction
  -> distill candidate procedure
  -> generate tests and threat model
  -> run eval suite across models
  -> compare with existing skills
  -> security and policy review
  -> human/tenant promotion
```

Self-evolving-skill research is promising, especially when skill generation and verification co-evolve. Verevon should use it in an offline candidate pipeline, never as automatic production publication.

### 24.7 Skill supply-chain security

Treat metadata and instructions as executable security surfaces.

Require:

- publisher identity;
- signature/content hash;
- permission manifest;
- network and filesystem requirements;
- dependency lock;
- capability diff on update;
- risk diff on update;
- static scan;
- semantic instruction scan;
- adversarial trigger tests;
- approved version and rollback path.

Protect against:

- manipulative descriptions designed to win semantic routing;
- hidden capability escalation;
- poisoned examples;
- malicious external references;
- trigger collisions;
- name squatting;
- dependency substitution;
- unsafe auto-update.

### 24.8 Skill optimization lab

Use DSPy/GEPA-style optimization as an offline compiler:

```text
skill/prompt program
+ representative train/eval trajectories
+ executable metric
  -> propose instruction/example changes
  -> evaluate candidates
  -> Pareto frontier: quality, cost, latency, safety
  -> reviewed versioned candidate
```

Production runtime consumes approved compiled artifacts. It does not continuously mutate prompts in place.

---

## 25. LLM Wiki and knowledge gardening

Traditional RAG derives an answer from raw chunks at query time. An LLM Wiki incrementally compiles durable, linked, source-grounded knowledge that can be maintained between queries.

### 25.1 Ownership

```text
Quarry / Ingestion Plane
  captures immutable source evidence and source versions

Data Plane
  owns wiki pages, versions, links, source logs, graph/indexes,
  review records, retention, and maintenance history

Model Plane
  owns analysis, proposed updates, contradiction reasoning,
  stale/orphan/gap agents, and research planning

Application Plane
  owns browsing, editing, review, graph, and diff UX
```

### 25.2 Three layers

```text
Raw sources
  immutable evidence and source versions

Wiki
  compiled pages, entities, concepts, procedures, summaries

Purpose and schema
  why the wiki exists and how pages must be structured/maintained
```

### 25.3 Core operations

- `ingest`: analyze source, create/update proposed pages, preserve provenance;
- `query`: retrieve compiled pages plus original evidence as needed;
- `lint`: find contradictions, missing provenance, stale content, duplicates, and broken links;
- `research`: fill identified knowledge gaps;
- `review`: approve or reject proposed changes;
- `rebuild`: deterministically regenerate derived indexes without losing accepted page versions.

### 25.4 Wiki page contract

```ts
interface WikiPageVersion {
	pageId: string;
	version: number;
	title: string;
	pageType: string;
	bodyArtifactRef: string;

	sourceRefs: SourceEvidenceRef[];
	wikilinks: string[];
	claims: ClaimRecord[];

	validFrom?: string;
	validUntil?: string;
	supersedes?: string[];

	generatedByRunId?: string;
	reviewedBy?: string;
	status: "proposed" | "accepted" | "superseded" | "rejected";
}
```

### 25.5 Knowledge-maintenance loops

#### Changed source loop

```text
source version changed
  -> identify dependent claims/pages/artifacts
  -> compare old and new evidence
  -> propose minimal diffs
  -> classify contradiction/supersession
  -> review according to policy
  -> publish page version
  -> selectively re-index/replay downstream artifacts
```

#### Contradiction loop

```text
claim conflict detected
  -> collect exact evidence and validity windows
  -> determine coexistence, supersession, or unresolved conflict
  -> update contradiction index
  -> request review when confidence is insufficient
```

#### Knowledge-gap loop

Use graph structure, unanswered queries, low-confidence answers, and repeated external searches to identify:

- isolated pages;
- weak communities;
- missing bridge concepts;
- frequently requested absent facts;
- entities with stale sources;
- procedures without verification evidence.

### 25.6 Progressive disclosure for wiki retrieval

Expose:

1. page metadata and one-line summaries;
2. selected page bodies;
3. exact source evidence;
4. neighboring graph context only when needed.

Progressive disclosure should be benchmarked against hybrid retrieval; capable agents may skip indexes and infer paths directly, while large corpora benefit from targeted access.

### 25.7 Human/LLM division

```text
Human
  defines purpose, approves sensitive changes, resolves ambiguity

LLM
  performs maintenance, proposes structure, links, and updates

Data Plane
  preserves accepted history, provenance, permissions, and retention
```

---

## 26. Multimodal RAG and multimodal memory

Model Plane should route retrieval by evidence modality rather than reducing every source to text.

### 26.1 Visual RAG

Visual RAG must preserve:

- full page images;
- layout and reading order;
- figures, charts, tables, diagrams, and slide composition;
- region bounding boxes;
- OCR/text spans linked to regions;
- page and document sequence;
- source version and provenance.

```ts
interface VisualEvidenceRef {
	documentId: string;
	sourceVersion: string;
	pageNumber: number;
	region?: BoundingBox;
	artifactRef: string;
	textSpanIds: string[];
	visualEmbeddingRef?: string;
	evidenceType: "page" | "figure" | "chart" | "table" | "diagram" | "slide";
}
```

Retrieval flow:

```text
query analysis
  -> determine text/layout/visual requirements
  -> hybrid text + page-image + region retrieval
  -> rerank evidence bundles
  -> reason over selected pages/regions
  -> validate claims against cited visual regions
```

### 26.2 Voice and audio RAG

Support four paths:

1. speech query over text knowledge;
2. text query over transcripts and audio segments;
3. native audio query over audio embeddings;
4. retrieval-assisted ASR using company terms and prior segments.

```ts
interface AudioKnowledgeUnit {
	sourceId: string;
	sourceVersion: string;
	audioArtifactRef: string;
	startMs: number;
	endMs: number;

	transcript?: string;
	speakerId?: string;
	language?: string;
	textEmbeddingRef?: string;
	audioEmbeddingRef?: string;
	soundEvents?: string[];
	confidence?: number;
}
```

Answers must cite exact timestamps and preserve speaker attribution.

### 26.3 Video RAG

Ingestion should produce:

- scene/shot boundaries;
- keyframes;
- transcript alignment;
- speaker and audio events;
- object/action tracks;
- temporal summaries;
- frame/region evidence refs.

The planner retrieves relevant temporal windows rather than sending entire videos to a model.

### 26.4 Cross-modal retrieval

Allow:

- text query -> image/chart/audio/video evidence;
- screenshot -> relevant documentation;
- spoken query -> related calls and documents;
- chart region -> source table and explanatory text;
- audio segment -> related CRM/customer records under policy.

### 26.5 Discourse-aware RAG

Do not represent long arguments as unrelated chunks. Preserve:

- claim/support/contrast relations;
- section hierarchy;
- rhetorical links;
- speaker turns;
- cross-passage dependencies.

Use discourse graphs to retrieve evidence that forms a coherent argument, not only semantically similar passages.

### 26.6 Temporal RAG

Every durable fact should be able to carry:

- event time;
- ingestion time;
- valid-from/valid-until;
- supersession relationship;
- current/expired/disputed state.

The retriever should answer both "what is true now?" and "what was believed at time T?"

### 26.7 Multimodal memory lifecycle

Memory quality must be measured across four stages:

```text
write
  Was the right observation stored?

maintain
  Was stale information revised or superseded?

retrieve
  Was the correct memory surfaced?

use
  Did the agent apply it correctly to the action?
```

High memory recall is not sufficient if maintenance or use is poor.

---

## 27. Agent interoperability: MCP, A2A, ACP, and internal contracts

Use distinct protocols for distinct boundaries.

```text
MCP
  agent-to-tool/resource interoperability

A2A
  independent agent-to-agent discovery and task exchange

ACP/IDE protocols
  interactive agent clients and coding shells where useful

Verevon internal RPC/events
  trusted first-party execution, state, and policy contracts
```

### 27.1 A2A adapter

A2A 1.0 provides agent discovery, Agent Cards, task lifecycle, streaming, files, and structured data across JSON-RPC, HTTP+JSON, and gRPC bindings.

Verevon should add A2A only as an edge adapter when external or separately governed agents need to collaborate.

### 27.2 Agent Card import

Treat an Agent Card as an untrusted capability advertisement.

```ts
interface ImportedAgentProfile {
	agentId: string;
	provider: string;
	endpoint: string;
	protocolVersion: string;
	skills: ImportedAgentSkill[];
	inputModes: string[];
	outputModes: string[];
	authSchemes: string[];

	trustState: "unverified" | "verified" | "approved" | "revoked";
	attestationRef?: string;
	policyId: string;
}
```

Import into `capability-core`, then apply:

- identity verification;
- tenant allowlist;
- skill/capability normalization;
- data residency and privacy policy;
- cost and timeout policy;
- output scanning;
- delegation scope;
- task receipt and verification.

### 27.3 Delegation contract

```ts
interface AgentDelegationRequest {
	taskId: string;
	objective: string;
	inputArtifactRefs: string[];
	outputSchema: JsonSchema;
	allowedEffects: string[];
	forbiddenEffects: string[];
	dataPolicy: DataHandlingPolicy;
	budget: DelegationBudget;
	deadline: string;
	verificationRequirements: VerificationRequirement[];
}
```

Remote agents do not receive Verevon internal memory, tools, or credentials unless explicitly projected by policy.

### 27.4 Internal subagents versus external agents

Use native GraphPlan subgraphs for internal subagents. Use A2A only when the remote system is independently deployed, opaque, or separately governed.

Avoid turning every capability into an agent.

---

## 28. Adaptive test-time scaling and trajectory reuse

The next performance frontier is not always a larger model. It is deciding **when and how to spend more inference-time compute**.

### 28.1 Dynamic compute policy

```ts
interface TestTimeScalingPolicy {
	baseRouteId: string;
	maxExplorations: number;
	maxParallelAttempts: number;
	maxVerifierCalls: number;
	maxCost: number;
	stopConfidence: number;
	minimumExpectedValueGain: number;
}
```

The orchestrator may choose to:

- request another independent solution;
- use a different model or prompt strategy;
- gather more evidence;
- run a specialized verifier;
- refine the current solution;
- stop and return the best verified result.

### 28.2 When to scale

Spend additional compute when:

- task value or risk is high;
- verifier confidence is low;
- candidates disagree;
- evidence coverage is incomplete;
- the first attempt fails deterministically;
- predicted value gain exceeds additional cost;
- the task class historically benefits from ensemble or refinement.

Do not fan out for low-risk, easily verified tasks.

### 28.3 Trajectory distillation

Long-horizon attempts should be converted into compact, reusable representations:

```ts
interface TrajectorySummary {
	taskClass: string;
	approach: string;
	keyHypotheses: string[];
	progress: string[];
	successfulActions: string[];
	failureModes: string[];
	unresolvedIssues: string[];
	evidenceRefs: string[];
	verifiedOutcome: string;
	cost: number;
}
```

Use summaries for:

- selecting among parallel attempts;
- conditioning a refinement attempt;
- router learning;
- skill candidate generation;
- human review.

Do not inject full raw trajectories unless debugging requires them.

### 28.4 Verifier scaling

A verifier should actively search for counterexamples rather than only assign a score.

Examples:

- generate tests that distinguish candidate code paths;
- re-read provider state after an action;
- find evidence contradicting a research conclusion;
- compare visual output against explicit criteria;
- probe boundary and adversarial cases.

### 28.5 Adaptive solver selection

The test-time controller can select:

- model;
- reasoning effort;
- prompt/skill variant;
- retrieval strategy;
- number of attempts;
- verifier type.

Keep this bounded by `cost-core` and policy.

---

## 29. Self-improving platform flywheel

Verevon should improve from verified use without allowing production agents to silently rewrite themselves.

### 29.1 Correction-to-eval loop

```text
human correction or verified failure
  -> structured failure record
  -> minimal reproducible task/eval
  -> classify root cause
  -> propose fix to prompt, skill, router, tool, or policy
  -> run regression suite
  -> canary/shadow deployment
  -> promote or rollback
```

### 29.2 Root-cause classes

- missing knowledge;
- bad retrieval;
- wrong capability selection;
- invalid arguments;
- provider failure;
- poor plan;
- insufficient verification;
- stale memory;
- ambiguous policy;
- unsafe autonomy;
- weak skill instructions;
- context truncation;
- model mismatch.

Do not solve every failure by editing the system prompt.

### 29.3 Offline agent-program optimization

Use DSPy/GEPA-style methods to optimize:

- task prompts;
- skill instructions;
- examples;
- retrieval queries;
- judge criteria;
- routing policy features;
- context allocation.

Requirements:

- representative task set;
- executable or independently reviewed metric;
- held-out regression set;
- cost and latency objective;
- safety constraints;
- versioned output;
- human approval for production.

### 29.4 Router feedback loop

Adopt execution-grounded routing:

```text
Context
  task, risk, modality, prior attempts, model/tool statistics

Action
  model, tool, skill, retrieval, and compute allocation

Feedback
  verified outcome, cost, latency, correction, failure class

Updated context
  learned route priors for future tasks
```

Use contextual bandits/rankers only after deterministic logging and counterfactual evaluation are reliable.

### 29.5 Shadow and counterfactual evaluation

For selected production tasks:

- run the chosen route normally;
- evaluate alternative routes in shadow mode where safe;
- compare verified outcomes and costs;
- avoid duplicate external effects;
- use recorded evidence/sandbox snapshots for replay.

This produces routing data without exposing users to untested actions.

### 29.6 Self-play and synthetic tasks

Use self-play to create difficult research, retrieval, and planning tasks, but:

- keep synthetic tasks separate from product evals;
- detect duplicates and benchmark leakage;
- validate task solvability;
- require executable or expert verification;
- prevent the generator and solver from sharing hidden answers.

### 29.7 Platform hill-climbing

Every important product failure should become a measurable "hill to climb":

```text
named capability gap
+ eval set
+ baseline
+ owner
+ target
+ regression gate
```

Improvement is complete only when the metric rises without unacceptable cost, latency, or safety regressions.

---

## 30. World-state ledger and epistemic control

Agents need a controlled representation of what is known, believed, proposed, authorized, and actually changed.

### 30.1 State classes

```ts
type EpistemicState =
	| "observed"
	| "retrieved"
	| "inferred"
	| "assumed"
	| "proposed"
	| "approved"
	| "executed"
	| "verified"
	| "disputed"
	| "superseded";
```

Never collapse these into one generic memory record.

### 30.2 World-state record

```ts
interface WorldStateRecord<T> {
	recordId: string;
	subject: string;
	predicate: string;
	value: T;
	epistemicState: EpistemicState;

	evidenceRefs: string[];
	actorId: string;
	observedAt?: string;
	validFrom?: string;
	validUntil?: string;
	supersedes?: string[];

	confidence?: number;
	verificationMethod?: string;
	privacyClass: string;
}
```

### 30.3 State transition rules

Examples:

```text
model draft
  proposed, not executed

human approval
  approved, not executed

provider call accepted
  executed, not necessarily verified

provider reread/receipt
  verified

new contrary evidence
  disputed or superseded
```

### 30.4 Plan-state reconciliation

Before resuming a long-running workflow:

- revalidate permissions;
- re-read effectful external state;
- check whether assumptions are stale;
- invalidate completed nodes whose preconditions no longer hold;
- selectively replay dependent nodes.

This prevents a durable workflow from continuing based on an obsolete world model.

---

## 31. Containment, authority budgets, and adaptive autonomy

As model capability rises, safety should rely less on repeated permission prompts and more on enforced limits to what an agent can do.

### 31.1 Two risk dimensions

```text
Failure probability
  how likely the agent is to act incorrectly

Blast radius
  the maximum damage possible if it does
```

Model improvements reduce failure probability but do not automatically limit blast radius.

### 31.2 Authority budget

```ts
interface AuthorityBudget {
	autonomyLevel: 0 | 1 | 2 | 3 | 4 | 5;
	allowedCapabilities: string[];
	allowedResourceScopes: string[];
	maxExternalEffects: number;
	maxMonetaryValue?: number;
	maxDataRows?: number;
	allowedDomains?: string[];
	egressPolicyId: string;
	approvalPolicyId: string;
	expiresAt: string;
}
```

Suggested levels:

| Level | Meaning                                                    |
| ----: | ---------------------------------------------------------- |
|     0 | Answer only                                                |
|     1 | Draft or recommend                                         |
|     2 | Prepare exact action for approval                          |
|     3 | Execute reversible low-risk actions                        |
|     4 | Execute specifically approved effects                      |
|     5 | Run bounded unattended workflow within an authority budget |

### 31.3 Containment controls

- sandbox/VM isolation;
- ephemeral or explicitly persistent filesystem;
- egress allowlists;
- capability-specific credentials;
- short-lived leases;
- no ambient tenant-wide secrets;
- resource ceilings;
- effect counters;
- monetary and data-volume limits;
- immutable receipts;
- emergency revocation;
- tenant kill switch.

### 31.4 Approval fatigue

High-frequency prompts become rubber stamps. Reduce fatigue by:

- auto-allowing deterministically safe actions;
- grouping related effects into one precise approval;
- showing semantic diffs and maximum impact;
- requiring approval only at meaningful boundaries;
- using containment for routine actions;
- expiring approvals when plan, payload, actor, or policy changes.

### 31.5 Dynamic autonomy

Autonomy may increase only after:

- stable verified success;
- low correction rate;
- bounded effects;
- mature verification;
- tenant policy approval.

It should decrease automatically on:

- model/provider change;
- new skill version;
- increased risk or scope;
- repeated failure;
- verification uncertainty;
- anomalous behavior.

---

## 32. Evaluation science and production experimentation

Agent evaluation must separate the model, harness, and environment.

### 32.1 Evaluation dimensions

```text
Benchmark
  tasks, success criteria, datasets

Harness
  prompts, tools, memory, routing, skills, loops

Environment
  browser/sandbox, CPU/RAM, network, timeouts, dependencies
```

Record all three as versioned inputs.

### 32.2 Resource envelopes

Agentic performance changes with:

- CPU and RAM;
- wall-clock limits;
- network latency and bandwidth;
- concurrency;
- tool/provider availability;
- sandbox image;
- browser/runtime version.

Every eval report should include guaranteed resources and hard ceilings. Treat small leaderboard differences skeptically when configurations differ.

### 32.3 Noise and robustness testing

Inject controlled:

- tool timeouts;
- malformed responses;
- stale results;
- user typos and contradictory instructions;
- duplicate events;
- delayed webhooks;
- partial provider failure;
- noisy retrieval;
- prompt injection;
- missing permissions;
- browser UI changes.

Measure graceful degradation, not only ideal-environment success.

### 32.4 Executable outcome evaluation

Prefer:

- provider-state rereads;
- tests/simulators;
- artifact validation;
- schema and constraint checks;
- exact business-state verification;
- expert review for subjective outputs.

Avoid grading an action solely from the agent's explanation.

### 32.5 Multi-run statistics

Report:

- number of trials;
- confidence intervals;
- infra error rate;
- model/provider error rate;
- false success;
- task variance;
- cost distribution;
- latency distribution;
- resource configuration.

### 32.6 Model and harness drift

Run continuous canaries when:

- provider model version changes;
- prompt or skill version changes;
- capability schema changes;
- retrieval index changes;
- sandbox/browser image changes;
- policy changes.

Automatically freeze or roll back routes that exceed regression thresholds.

### 32.7 Unified evaluation adapter

Build a lab interface separating benchmark, harness, and environment so Verevon can compare:

- native Model Plane;
- provider SDK harnesses;
- alternate models;
- skill versions;
- routing policies;
- browser/data environments.

The production runtime remains first-party; external frameworks are evaluation adapters.

---

## 33. Emerging priorities and recommended bets

The important shift is from "pick the best model" to **design an adaptive, measurable, governed system around changing models**.

### 33.1 What experts and developers are converging on

1. **Harness engineering is a primary product advantage.** Model capability alone does not determine agent performance.
2. **Harnesses must get simpler as models improve.** Regular ablation prevents stale scaffolding.
3. **Code mode replaces long chains of direct tool calls** for large tool libraries and datasets.
4. **Skills are becoming a portable software layer** with progressive disclosure, resources, scripts, tests, and governance.
5. **Execution-grounded routing beats static model selection.** The router must learn from verified outcomes.
6. **Adaptive test-time compute is replacing fixed best-of-N.** Spend extra inference only when uncertainty and value justify it.
7. **Verifier quality is often more valuable than another generator.** Independent, adversarial verification reduces false success.
8. **Artifact-first long-running work is replacing conversation-first continuity.** Plans, progress, evidence, and handoffs must be durable.
9. **Multimodal memory must maintain an evolving world**, not only retrieve old text.
10. **Agent protocols are specializing.** MCP handles tools; A2A handles independent agents; internal RPC remains the trusted core.
11. **Containment is replacing approval spam** as the scalable safety mechanism.
12. **The product correction loop becomes the training loop.** Corrections should become evals, fixes, and regression gates.

### 33.2 Do now

**Reordered 2026-08-03 to put verified-smallest/verified-highest-leverage
first** (see §1a); the rest of the original list follows unchanged:

- **build the execution-core approval-delivery dispatcher** (§14/§36
  Phase 0 item 1 — the smallest, most concrete item in this document);
- fix the 4 newly-found Phase 0 security items (§20/§36: browser-risk OR
  logic, capability-core swallowed errors, letta-bridge delete no-op,
  session-core replay nonce) before building new capability/memory/
  approval features on top of those services;
- add reservation/commit semantics to `cost-core`'s already-durable ledger
  (not "make it durable" — done);
- implement the harness contract and assumption registry;
- add TOON/JSON encoding _routing_ (the TOON encoder itself already ships
  at 3 fixed call sites — this is the dynamic-selection layer on top);
- implement progressive capability disclosure (confirmed greenfield);
- add sandboxed MCP code mode and result handles (confirmed greenfield);
- complete skill compilation, signing, and regression gates (build on top
  of the already-live implicit-dissatisfaction/quarantine system, §13 —
  don't replace it);
- add artifact-first handoffs and clean context reset support;
- implement execution-grounded router feedback (extend `FallbackChain`'s
  existing per-turn `Decision`, don't start from nothing, §9);
- add authority budgets and autonomy levels;
- improve eval reproducibility and noise testing;
- add LLM Wiki maintenance contracts between Model and Data Plane;
- add visual/audio evidence contracts before building broad multimodal features.

### 33.3 Research behind feature flags

- adaptive test-time scaling controller;
- trajectory tournament/refinement;
- DSPy/GEPA offline optimization;
- self-evolving skill candidate generation;
- A2A adapter and Agent Card discovery;
- discourse-aware retrieval;
- native audio embeddings;
- multimodal world-state memory;
- counterfactual route replay;
- agent teams for highly parallel, decomposable work.

### 33.4 Avoid for now

- uncontrolled online self-modification;
- automatic publication of generated skills;
- exposing every MCP tool to every model turn;
- converting every service or capability into an agent;
- using A2A for internal subagents;
- default multi-agent fan-out;
- replacing deterministic workflow execution with a free-form LLM loop;
- storing canonical business state in model memory;
- adopting TOON as an internal service contract;
- trusting benchmark differences without matched harness/environment settings.

### 33.5 Proposed new first-party components

| Component                | Responsibility                                                    |
| ------------------------ | ----------------------------------------------------------------- |
| `harness-registry`       | Version harnesses, assumptions, ablations, and compatibility      |
| `codec-rs`               | JSON/TOON/terse encoding, token estimation, codec benchmarks      |
| `tool-code-runtime`      | Sandboxed generated code over selected capability wrappers        |
| `skill-compiler`         | Compile skill packages to graph/context/policy/verification plans |
| `optimization-lab-py`    | DSPy/GEPA, route and skill optimization experiments               |
| `world-state-core`       | Epistemic states, temporal validity, reconciliation               |
| `agent-interoperability` | Optional A2A/ACP edge adapters                                    |
| `multimodal-context`     | Visual/audio/video evidence planning and context assembly         |

These may begin as modules inside existing services. Do not create a microservice unless scaling, isolation, language/runtime, or ownership requires it.

---

## 34. Service-specific improvement plan

### `model-gateway`

Add or complete:

- stable structured-output schema passthrough;
- GraphPlan invoke/start/resume APIs;
- route-decision propagation;
- strict tenant delegation;
- unified event streaming;
- no in-memory approval authority.

### `session-core`

**Verified 2026-08-03**: durable approval continuation (tenant-scoped
compare-and-set decisions + leaseable delivery-outbox) is already built
here — the gap is the execution-core consumer, not session-core. Also add:
the missing anti-replay nonce on the HMAC service-delegation path (§20 —
every sibling Go service already has this, session-core does not).

Add or complete:

- canonical GraphPlan and GraphNodeRun persistence (today: a flat
  single-ordinal `plans`/`plan_steps` list, not a DAG — confirmed
  greenfield, see §1a);
- immutable plan versions;
- ~~durable approval continuation~~ **mostly done** — see above;
- context manifests;
- artifact dependency lineage;
- event replay with schema versions;
- bounded compaction records.

### `inference-core`

Add or complete:

- model capability profiles;
- step-level route interface;
- provider health and quality statistics;
- structured-output normalization;
- multimodal consistency;
- prompt-cache metadata;
- model-independent usage records.

### `execution-core`

**Verified 2026-08-03 — highest-priority concrete item in this whole
document lives here**: implement a real client for session-core's
`claim_approval_deliveries`/`acknowledge_approval_delivery` RPCs (today
`runtime_loop/agent.rs:2237` stubs it as
`Err(Status::unimplemented("claim_approval_deliveries not used"))`) that
leases a delivery row, resumes the exact suspended graph node, executes it,
and acknowledges. Everything durable this needs already exists in
session-core. Also fix the browser-risk classifier's self-report/backstop
OR-logic violation (`browser_agent/browser_risk.rs:148`, §20) before
building the planner/executor/verifier separation below on top of it.

Add or complete:

- **the approval-delivery dispatcher above (do first)**;
- **the browser-risk OR-logic fix above (do before the rest of this list)**;
- planner/executor/verifier interfaces;
- typed tool guardrails;
- exact execution attestations;
- postcondition verification hooks;
- bounded recovery levels;
- outcome receipts;
- model-independent action/observation contracts.

### `orchestrator-core`

Add or complete:

- Temporal GraphRun workflow;
- graph-node scheduling;
- parallel joins;
- cancellation and compensation;
- approval expiration;
- graph replanning/version transitions;
- skill and memory promotion workflows;
- budget-reservation workflow.

### `capability-core`

**Verified 2026-08-03**: `tool outcome history` and `contextual ranking`
below are **already real** (`internal/scoring/scoring.go` — success rate,
p95 latency, mean cost, approval rate, incidents, operator rating,
rollout-state multiplier) — extend with a semantic layer, do not rebuild.
Fix the swallowed-error PATCH/DELETE handlers (§20, 13 call sites in
`workplane_apis.go`/`registry_apis.go`) before adding the skill-lifecycle
and attestation work below, so new features don't inherit the same
silent-failure pattern.

Add or complete:

- **fix swallowed-error PATCH/DELETE handlers (do first, §20)**;
- `search_capabilities` (semantic layer — the outcome-ranking half already
  exists, see above);
- ~~capability semantic index~~ / ~~contextual ranking~~ — **partially
  done**, extend `scoring.go`, don't replace;
- capability attestation (confirmed greenfield, §8);
- provider readiness;
- skill lifecycle;
- ~~tool outcome history~~ — **done**, see above;
- MCP import/scanning (progressive disclosure is confirmed greenfield, §23);
- policy snapshotting.

### `sandbox-manager`

Add or complete:

- durable lease metadata;
- backend abstraction;
- network policies;
- snapshots and restoration;
- per-run workspace mounts;
- resource accounting;
- cleanup reconciliation.

### `browser-broker`

Add or complete:

- durable grants;
- exact domain/action scope;
- Quarry lease binding;
- revocation propagation;
- credential reference policy;
- browser cost and session metadata.

### `letta-bridge`

Keep optional and add:

- provider-neutral memory adapter contract;
- native PostgreSQL implementation;
- Letta implementation;
- no fallback that silently changes production semantics;
- memory provenance and deletion propagation.

### `cost-core`

**Verified 2026-08-03**: durable PostgreSQL ledger is **done** (default in
production, dev-only opt-out flag). Route-level budget enforcement for
Budget/Balance/Genius is also **done** as of this week (was silently
inert for months due to a missing auth header on the budget-check call —
now fixed and live). The real remaining item is reservation/commit
semantics — confirmed absent (zero `Reserve`/`reservation` hits in the
service).

Add or complete:

- ~~durable PostgreSQL ledger~~ — **done**;
- real NATS/usage subscriptions;
- **reservation and commit semantics (the real remaining gap)**;
- ~~route-level budget enforcement~~ — **done** (2026-07-29 fix);
- browser/sandbox/external provider costs.

### `bridge-core`

Add or complete concrete adapters only when there is a real caller. Remove or clearly label no-op adapters so availability is not overstated.

---

## 35. Deployment profiles

### Minimal production

- gateway;
- session;
- inference;
- execution;
- orchestration;
- capabilities;
- cost;
- PostgreSQL;
- Temporal;
- NATS;
- MinIO;
- Dragonfly;
- OpenTelemetry.

### Browser-enabled

Adds:

- browser-broker;
- Quarry contract;
- browser runtime backends.

### Sandbox-enabled

Adds:

- sandbox-manager;
- one isolated sandbox backend.

### Research/lab

Adds:

- LangChain/LangGraph;
- Pydantic AI;
- Mastra;
- OpenHands adapters;
- provider SDK adapters;
- Letta external backend;
- evaluation datasets.

Lab dependencies must not be required for normal production startup.

---

## 36. Implementation phases

### Phase 0: release correctness

**Resequenced 2026-08-03 against verified reality (see §1a).** Original
items renumbered/annotated; two newly-found items added from a
down-the-stack security audit (§20).

1. **Build the approval-delivery dispatcher in `execution-core`** —
   ~70% of "durable approval continuation" already exists (tenant-scoped
   compare-and-set decisions, a leaseable delivery-outbox); the only
   missing piece is a real client for `claim_approval_deliveries` (today a
   literal `unimplemented!()` stub) that resumes and executes the exact
   suspended graph node. Smallest, most concrete item in this whole
   document — do this first.
2. Fix the browser-risk classifier's self-report/backstop OR logic
   (`browser_risk.rs:148`, §20) — a documented safety invariant is
   currently violated.
3. Fix `capability-core`'s swallowed-error PATCH/DELETE handlers (§20) —
   13 call sites return false success today.
4. Fix `letta-bridge`'s silent `DeleteMemory` no-op (§20) — a real
   DSAR/erasure-relevant gap, not hypothetical.
5. Add the anti-replay nonce to `session-core`'s HMAC delegation (§20) —
   every sibling Go service already has this.
6. Complete tenant delegation and capability attestation (§8 — confirmed
   greenfield, the real remaining lift in this phase).
7. Remove in-memory authority for critical production state (mostly
   verified already resolved — `cost-core` and the approval path are both
   durable now; re-audit `sandbox-manager`/`browser-broker` lease state
   specifically, per §16's own durability requirement, before marking this
   item closed).
8. ~~Make cost records durable~~ — **DONE** (§18): `cost-core` is
   Postgres-backed by default in production. Replace this item with
   _add reservation/commit semantics to the existing durable ledger_,
   which is the real remaining gap.
9. Make provider/capability readiness truthful.
10. Standardize event envelopes and schema versions.

### Phase 1: GraphLoop foundation

1. Define `GraphPlan`, `GraphNode`, and `GraphEdge` contracts.
2. Add immutable plan versions in session-core.
3. Implement Temporal `GraphRunWorkflow`.
4. Implement bounded node recovery.
5. Add plan/node APIs and event streams.
6. Add deterministic success criteria.

### Phase 2: smart routing

1. Build model capability profiles.
2. Add step-level routing records.
3. Add capability semantic search.
4. Add outcome-based route scoring.
5. Add cheap fallback and high-risk ensemble policies.
6. Add budget reservation.

### Phase 3: verifier and learning

1. Add deterministic verifier interfaces.
2. Add provider reread/receipt verification.
3. Add structured failure categories.
4. Add memory candidate workflow.
5. Add skill candidate and promotion workflow.
6. Add human-correction ingestion.

### Phase 4: advanced orchestration

1. Parallel subgraphs and join policies.
2. Artifact execution lineage.
3. Incremental graph replay.
4. Cross-agent context packets.
5. Selective multi-model ensembles.
6. ACP bridge for external agent clients.

---

## 37. Acceptance criteria

The Model Plane improvement program is successful when:

- every effectful run can pause and resume after process restart;
- no approval is held only in memory;
- every provider action has exact authorization, idempotency, and verification;
- plan versions and recovery decisions are inspectable;
- model and capability routing are measurable by verified outcomes;
- failed/expensive tools reduce their future ranking for similar tasks;
- external frameworks can be benchmarked without becoming runtime authorities;
- Letta can be disabled without losing canonical memory;
- Langflow and LangChain are absent from the production hot path;
- every run has a complete actor, capability, evidence, cost, and outcome lineage;
- cost per verified task decreases without increasing false-success rate;
- large MCP estates use progressive disclosure rather than loading all schemas;
- high-volume multi-tool tasks can use sandboxed code mode and result handles;
- TOON is used only where model-specific benchmarks beat compact JSON;
- skills are signed, versioned, progressively disclosed, evaluated, and rollbackable;
- harness assumptions are versioned and regularly ablated;
- long-running runs leave durable handoff artifacts and can reset context safely;
- world-state records distinguish proposed, approved, executed, and verified effects;
- multimodal evidence preserves page/region/timestamp provenance;
- external agents can be integrated through A2A without exposing internal memory or authority;
- autonomy is bounded by explicit authority budgets and containment;
- every production correction can become a reproducible eval and regression gate.

---

## 38. Recommended final posture

```text
Verevon-owned control plane
  - graph execution
  - capability registry
  - policy and approval
  - memory and skills
  - model routing
  - cost and evaluation
  - harness and loop registry
  - skill compiler and optimization pipeline
  - world-state and multimodal context policy

Headless infrastructure
  - PostgreSQL
  - Temporal
  - NATS
  - MinIO
  - Dragonfly
  - OpenTelemetry
  - sandboxed tool-code runtime

Optional adapters and labs
  - Letta
  - OpenHands
  - OpenAI Agents SDK
  - Claude Agent SDK
  - Microsoft Agent Framework
  - Pydantic AI
  - Mastra
  - LangGraph/LangChain
  - DSPy/GEPA optimization lab
  - A2A interoperability adapter
```

The Model Plane should become a **governed agent harness with explicit graph execution and verified outcomes**, not another generic multi-agent framework.

---

## 39. Research references

### Verevon and reviewed repositories

- OpenHands: https://github.com/OpenHands/OpenHands
- OpenHands Software Agent SDK: https://github.com/OpenHands/software-agent-sdk
- OpenRAG: https://github.com/langflow-ai/openrag
- ECC: https://github.com/affaan-m/ECC
- OpenWork: https://github.com/different-ai/openwork
- OpenWorker: https://github.com/andrewyng/openworker
- Orca: https://github.com/stablyai/orca
- Buzz: https://github.com/block/buzz

### Agent runtimes and orchestration

- OpenAI Agents SDK: https://openai.github.io/openai-agents-python/
- Microsoft Agent Framework: https://learn.microsoft.com/en-us/agent-framework/overview/
- Microsoft Conductor: https://opensource.microsoft.com/blog/2026/05/14/conductor-deterministic-orchestration-for-multi-agent-ai-workflows/
- Pydantic AI durable execution: https://pydantic.dev/docs/ai/capabilities/durable_execution/overview/
- LangGraph: https://docs.langchain.com/oss/python/langgraph/overview
- Mastra workflows: https://mastra.ai/ai-workflows
- Mastra workspaces: https://mastra.ai/blog/introducing-mastra-workspaces
- Letta memory blocks: https://docs.letta.com/guides/core-concepts/memory/memory-blocks

### Research informing GraphLoop and routing

- Harness-native agentic routing: https://arxiv.org/abs/2607.11399
- Structured graph harness: https://arxiv.org/abs/2604.11378
- Execution lineage: https://arxiv.org/abs/2605.06365
- OpenHands SDK paper: https://arxiv.org/abs/2511.03690

### Harness, loops, context, and containment

- OpenAI Harness Engineering: https://openai.com/index/harness-engineering/
- Anthropic long-running harness design: https://www.anthropic.com/engineering/harness-design-long-running-apps
- Anthropic effective long-running harnesses: https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents
- Anthropic managed agents / brain-hands separation: https://www.anthropic.com/engineering/managed-agents
- Anthropic agent containment: https://www.anthropic.com/engineering/how-we-contain-claude
- Anthropic agent evals: https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents
- Anthropic infrastructure noise in agent evals: https://www.anthropic.com/engineering/infrastructure-noise

### Tool use, MCP, skills, and encoding

- MCP code execution / code mode: https://www.anthropic.com/engineering/code-execution-with-mcp
- Advanced dynamic tool use: https://www.anthropic.com/engineering/advanced-tool-use
- Agent Skills: https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills
- TOON format: https://github.com/toon-format/toon
- SkillJuror: https://github.com/zhiyuchen-ai/skill-juror
- Progressive disclosure study: https://arxiv.org/abs/2607.17598
- Agent Skills ecosystem analysis: https://arxiv.org/abs/2602.08004
- EvoSkills: https://arxiv.org/abs/2604.01687

### Routing, optimization, and test-time scaling

- Agent-as-a-Router: https://arxiv.org/abs/2606.22902
- Agent-as-a-Router code: https://github.com/LanceZPF/agent-as-a-router
- ATLAS adaptive test-time scaling: https://arxiv.org/abs/2606.01667
- Agentic coding trajectory scaling: https://arxiv.org/abs/2604.16529
- Agentic Verifier: https://arxiv.org/abs/2602.04254
- DSPy: https://dspy.ai/
- GEPA: https://arxiv.org/abs/2507.19457
- GEPA code: https://github.com/gepa-ai/gepa
- OpenAI self-improving product/eval loop: https://openai.com/index/building-self-improving-tax-agents-with-codex/

### Knowledge, memory, and multimodal agentic RAG

- Agentic RAG systematization: https://arxiv.org/abs/2603.07379
- WorldMemArena: https://arxiv.org/abs/2605.29341
- LLM Wiki implementation and pattern: https://github.com/nashsu/llm_wiki
- Progressive disclosure for LLM-maintained wikis: https://arxiv.org/abs/2607.04576

### Agent interoperability and evaluation infrastructure

- A2A Protocol: https://a2a-protocol.org/latest/
- A2A v1.0 specification: https://a2a-protocol.org/dev/specification/
- A2A and MCP: https://a2a-protocol.org/dev/topics/a2a-and-mcp/
- AgentCompass: https://arxiv.org/abs/2607.13705
- General Agent Evaluation / Exgentic: https://arxiv.org/abs/2602.22953
- AgentNoiseBench: https://arxiv.org/abs/2602.11348

### Visual, audio, and cross-modal retrieval

- VLD-RAG for long visually rich documents: https://arxiv.org/abs/2607.24748
- Multimodal Graph RAG for visually rich documents: https://arxiv.org/abs/2606.28780
- Visual Document Retrieval survey: https://arxiv.org/abs/2602.19961
- ViDoRAG: https://arxiv.org/abs/2502.18017
- PlanRAG-Audio: https://aclanthology.org/2026.findings-acl.1304/
- WavRAG: https://aclanthology.org/2025.acl-long.613/
