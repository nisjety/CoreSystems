# Model Plane v2 — Gap Analysis

> **Date:** 2025-07  
> **Scope:** `apps/Model Plane v2/` against the desired end state described in `Future roadmap.md`  
> **Method:** Static analysis of all service directories, schema files, configuration, and roadmap narrative

---

## Executive Summary

Model Plane v2 has a solid initial service skeleton — five services are wired to shared infrastructure (NATS, Redis, MinIO, Postgres, Temporal) and a 10-layer reasoning pipeline is operational in `ai-core`. However, **every phase of the v2 roadmap (Phase 0–12) has at least partial gaps**, and three phases (1, 7, 11–12) have zero implementation. The most critical structural problem is that `agent-core` has grown into a 60+ module monolith that conflates session authority, runtime execution, policy, skill management, cost accounting, voice, and Letta integration — responsibilities that the roadmap explicitly assigns to separate services. Until session-core is introduced and the agent-core scope is bounded, no downstream phase can make clean progress.

**14 gaps are identified below.** Each is rated **HIGH / MEDIUM / LOW** severity based on blast radius and dependency blocking.

---

## Gap Index

| # | Gap | Severity | Phase |
|---|-----|----------|-------|
| 1 | Phase 0 contract work not started | HIGH | 0 |
| 2 | `agent-core` is an unbounded monolith | HIGH | 0–2 |
| 3 | `session-core` does not exist | HIGH | 1 |
| 4 | `capability-core` boundary violations | HIGH | 5 |
| 5 | Go and Rust runtimes entirely absent | MEDIUM | 0 |
| 6 | `research-core` entirely absent | HIGH | 7 |
| 7 | Quarry not integrated into MP v2 | MEDIUM | 8 |
| 8 | Canonical identifiers not enforced at schema level | HIGH | 0–1 |
| 9 | Run state machine (9-state) not enforced | HIGH | 2 |
| 10 | Compaction / memory tiers not implemented | MEDIUM | 3 |
| 11 | Convex is still authority, not projection | HIGH | 4 |
| 12 | Connector / outbox layer absent | MEDIUM | 6 |
| 13 | Phases 9–12 not started | LOW | 9–12 |
| 14 | `docs/` is effectively empty | MEDIUM | 0 |

---

## Gap 1 — Phase 0 contract work not started

**Severity:** HIGH  
**Blocking:** All other phases depend on frozen contracts.

### Current state

`docs/` contains only `Future roadmap.md`. There are no:
- OpenAPI or Protobuf spec files checked in
- ADRs (Architecture Decision Records)
- Ownership matrix
- Canonical event envelope definition
- Identifier glossary
- Replay contract
- Cross-service compatibility matrix

### Desired state

Phase 0 exits with the following artefacts locked:

1. **Ownership matrix** — one table listing every domain (session, run, tool call, approval, skill, etc.) and the single service authoritative for writes.
2. **Canonical event envelope** — frozen Protobuf/JSON schema with fields: `event_type`, `event_id`, `event_timestamp`, `producing_service`, `org_id`, `correlation_id`, `causation_id`, `actor_type`, `actor_id`, all canonical resource IDs that apply, `idempotency_key`, `schema_version`.
3. **Identifier glossary** — definitions and format rules for all 17 canonical identifiers (see Gap 8).
4. **Replay contract** — specification for how `session-core` replays a thread timeline from raw NATS events.
5. **Compatibility matrix** — which callers consume which service's events/APIs and which breaking changes are allowed during migration phases.
6. **ADRs** — one per major architectural decision (service split rationale, identifier strategy, state machine choice, Convex projection model).

### Impact if not fixed

Every subsequent phase ships on quicksand. Teams add code to `agent-core` because the boundary is not written down. Schema drift accumulates silently.

### Remediation steps

1. Create `docs/adr/` and write ADR-001 (service ownership) as the first action.
2. Define the canonical event envelope in `shared/contracts/event_envelope.proto`.
3. Publish the identifier glossary as `docs/identifiers.md`.
4. Write the ownership matrix as `docs/ownership_matrix.md`.
5. Do not start Phase 1 implementation until Phase 0 artefacts are reviewed and merged.

---

## Gap 2 — `agent-core` is an unbounded monolith

**Severity:** HIGH  
**Blocking:** Phase 1 (session-core extraction), Phase 5 (capability-core separation).

### Current state

`agent-core/` contains 60+ modules organised into deeply nested subdirectories. Confirmed responsibility clusters inside a single service:

| Cluster | Modules found |
|---------|---------------|
| Session / thread management | session handling, message storage, thread state |
| Runtime execution | turn execution, tool dispatch, hooks |
| Policy / permissions | role checks, permission gates |
| Skill management | skill registry, versioning, evaluation |
| Cost accounting | budget tracking, spend aggregation |
| Voice | voice session management |
| Letta integration | direct Letta client coupling |
| Compaction | context window management |
| Subagent orchestration | nested agent spawning |

### Desired state

After Phase 1–5 refactors:

- `session-core` owns all durable session, thread, message, and run lineage.
- `capability-core` owns all policy, permissions, budget decisions, and tool eligibility.
- `agent-core` retains only: turn execution loop, tool dispatch, hook invocation, compaction trigger, subagent spawning, trajectory capture.
- Letta and voice are either extracted to dedicated adapters or clearly documented as bounded plugins.

### Impact if not fixed

- Every feature request lands in `agent-core`, compounding the problem.
- `session-core` and `capability-core` cannot be built cleanly while `agent-core` is the authority for their domains.
- Testing any single concern requires standing up the full monolith.

### Remediation steps

1. Complete Gap 1 (ownership matrix) first.
2. Identify all durable writes in `agent-core` that belong to `session-core` and tag them with `# TODO: session-core`.
3. Identify all policy/budget branches in `agent-core` that belong to `capability-core` and tag them with `# TODO: capability-core`.
4. Build `session-core` (Gap 3) and migrate durable writes in one phase per domain.
5. Do not add new domains to `agent-core` from this point forward.

---

## Gap 3 — `session-core` does not exist

**Severity:** HIGH  
**Blocking:** Phase 1 is the foundation for Phases 2, 3, 4, 6, 7.

### Current state

There is no `session-core` service directory. The `apps/Control Plane/` directory exists but contains no session-core.

### Desired state

`session-core` is the sole durable authority for:

- Sessions (`session_id`)
- Threads (`thread_id`, `research_thread_id`)
- Messages (append-only)
- Run events (`run_id`, `parent_run_id`)
- Research events (`research_thread_id`)
- Approval records (`approval_id`)
- Compaction lineage
- Trajectory metadata (`trajectory_id`)

**Required gRPC surface:**

```protobuf
service SessionCore {
  rpc CreateSession(CreateSessionRequest) returns (Session);
  rpc AppendMessage(AppendMessageRequest) returns (MessageAck);
  rpc CreateThread(CreateThreadRequest) returns (Thread);
  rpc AppendRunEvent(AppendRunEventRequest) returns (EventAck);
  rpc AppendResearchEvent(AppendResearchEventRequest) returns (EventAck);
  rpc GetThreadTimeline(GetThreadTimelineRequest) returns (ThreadTimeline);
  rpc ReplayThread(ReplayThreadRequest) returns (stream ReplayEvent);
  rpc BackfillProjection(BackfillProjectionRequest) returns (BackfillAck);
}
```

**Required storage:** append-only event log per thread, queryable by `thread_id` / `run_id` / `session_id`.

**Phase 1 exit criteria (from roadmap):**
- `session-core` is authoritative for message and run history.
- `agent-core` and `ai-core` stop performing durable writes for session data.
- Convex projections are fed by `session-core` events only.
- Existing sessions can be replayed end-to-end from `session-core` event history.

### Impact if not fixed

Phases 2, 3, 4, 6, and 7 all emit or consume events that must be durably stored somewhere. Without `session-core`, those events accumulate in `agent-core` (worsening Gap 2) or Convex (worsening Gap 11).

### Remediation steps

1. Create `apps/Control Plane/session-core/` as a Python FastAPI + gRPC service (or Go, per Gap 5 plan).
2. Define schemas for sessions, threads, messages, run_events, research_events.
3. Implement append-only writes with idempotency keys.
4. Implement `GetThreadTimeline` and `ReplayThread` RPCs.
5. Wire to NATS for downstream event fan-out.
6. Migrate `agent-core` session writes to call `session-core` (use dual-write with flag until stable).

---

## Gap 4 — `capability-core` boundary violations

**Severity:** HIGH  
**Blocking:** Phase 5 cleanup, and any deterministic policy testing.

### Current state

`capability-core/` exists at `:8004` but:

- Policy and permission logic **duplicated** in `agent-core` — live policy branches exist alongside calls to `capability-core`.
- `ai-core` performs inline model routing decisions rather than receiving a pre-resolved route from `capability-core`.
- Budget decisions are tracked partially in `agent-core` (cost accounting cluster) and partially in `capability-core`.
- There are no auditable decision logs keyed by `run_id` / `thread_id` / `org_id`.

### Desired state

`capability-core` is the **single governance surface** for:

- Model routing decisions
- Tool eligibility (allow / deny with reason)
- Approval mode requirements
- Budget status and fallback policy
- Fallback routing

**Required gRPC surface:**

```protobuf
service CapabilityCore {
  rpc ResolveExecutionPolicy(ExecutionPolicyRequest) returns (ExecutionPolicy);
  rpc ResolveModelRoute(ModelRouteRequest) returns (ModelRoute);
  rpc ResolveToolEligibility(ToolEligibilityRequest) returns (ToolEligibility);
  rpc ResolveApprovalPolicy(ApprovalPolicyRequest) returns (ApprovalPolicy);
  rpc ResolveBudgetDecision(BudgetDecisionRequest) returns (BudgetDecision);
}
```

Every decision must produce an auditable log entry keyed by `run_id`, `thread_id`, `org_id`.

### Impact if not fixed

- Policy behaviour cannot be unit-tested deterministically because it is spread across services.
- Budget and tool eligibility decisions are invisible in the run timeline.
- `capability-core` is bypassed silently when inline checks in `agent-core` return early.

### Remediation steps

1. Audit every policy branch in `agent-core` and `ai-core` (grep for permission/role/budget checks).
2. Model each decision as a `capability-core` RPC.
3. Replace inline branches with `capability-core` calls plus a bounded TTL cache.
4. Add decision audit logging to `session-core` event log.
5. Write deterministic policy tests for allow, deny, ask, fallback, budget-exhausted cases.

---

## Gap 5 — Go and Rust runtimes entirely absent

**Severity:** MEDIUM  
**Blocking:** Limits throughput on hot paths; does not block correctness, but blocks the target architecture.

### Current state

All five services are Python. The roadmap specifies:

- **Go** — control plane, API gateways, NATS routers, capability policy engine
- **Rust** — hot execution paths (tool dispatch, context window slicing, high-throughput event fans)
- **Python** — ML workers, LLM invocation, embedding, research synthesis

No Go modules (`go.mod`) or Rust crates (`Cargo.toml`) exist under `apps/Model Plane v2/`.

### Desired state

Target runtime layout:
```
ai-core/          Python FastAPI — inference facade
agent-core/       Python FastAPI — turn execution loop
session-core/     Go or Python — append-only event authority
capability-core/  Go — policy engine (deterministic, low-latency)
llm-worker/       Python — stateless ML execution
execution-core/   Rust or Python — task graph, hot tool dispatch path
research-core/    Python — research orchestration
```

Rust is highest value in `execution-core` for the tool dispatch hot path. Go is highest value in `capability-core` (policy decisions need determinism and low latency) and `session-core` (append throughput).

### Impact if not fixed

Python GIL limits concurrent tool execution throughput. Policy decisions introduce Python startup overhead on every turn. This is an architectural debt item, not a correctness blocker today.

### Remediation steps

1. Decide and document which services will be Go / Rust in ADR-002.
2. Start with `capability-core` in Go as a greenfield rewrite (bounded scope, well-defined gRPC surface).
3. Add Rust for `execution-core` tool dispatch once Phase 2 execution spine is stable.
4. Keep Python workers for all LLM/ML calls — do not port ML code to Go/Rust.

---

## Gap 6 — `research-core` entirely absent

**Severity:** HIGH  
**Blocking:** Manus-style wide research is impossible without this service.

### Current state

No `research-core` directory exists anywhere in `apps/Model Plane v2/` or `apps/`.

### Desired state

`research-core` is a standalone orchestration service owning:

- Research job lifecycle (8 states: created → planning → dispatching → running → synthesizing → completed → failed → cancelled)
- Hypothesis tree storage keyed by `research_thread_id`
- Parallel task fan-out (each unit receives fresh context)
- Source ranking, deduplication, contradiction tracking
- Provenance packaging (each finding traceable to evidence)
- Job resume and export
- Compatibility adapter for existing MP v2 callers

**Required gRPC surface:**

```protobuf
service ResearchCore {
  rpc CreateResearchJob(CreateResearchJobRequest) returns (ResearchJob);
  rpc PlanHypotheses(PlanHypothesesRequest) returns (HypothesisTree);
  rpc DispatchResearchTasks(DispatchRequest) returns (DispatchAck);
  rpc CollectEvidence(CollectEvidenceRequest) returns (EvidenceAck);
  rpc SynthesizeFindings(SynthesizeRequest) returns (SynthesisResult);
  rpc ResumeResearchJob(ResumeRequest) returns (ResearchJob);
  rpc ExportResearchReport(ExportRequest) returns (ResearchReport);
}
```

Progress events must be published back to `session-core` and Convex projections.

### Impact if not fixed

Wide research tasks currently either land in `agent-core` (inflating the monolith) or are not supported. The `research_thread_id` identifier is defined in the roadmap but has no owning service.

### Remediation steps

1. Create `apps/Model Plane v2/research-core/` after Phase 1 and 2 are stable.
2. Define research job schema and hypothesis tree model.
3. Implement fan-out using Temporal workflows (already provisioned on `velion` namespace).
4. Emit `research.*` events to NATS → `session-core`.
5. Add compatibility adapter so `agent-core` can delegate to `research-core` without a hard cutover.

---

## Gap 7 — Quarry not integrated into MP v2

**Severity:** MEDIUM  
**Blocking:** Browser execution, crawling, and deterministic extraction are unsupported.

### Current state

Quarry exists as a separate service under `apps/Ingestion Plane/Quarry/` with its own postgres schema and job queue. There is no integration contract, shared identifier, or event bridge between Quarry and MP v2 services.

### Desired state

Quarry is the **deterministic execution substrate** for browser operations, crawling, and content extraction. MP v2 services interact with Quarry via a defined gRPC/REST surface:

```protobuf
service Quarry {
  rpc CreateSandbox(CreateSandboxRequest) returns (Sandbox);
  rpc ResumeSandbox(ResumeSandboxRequest) returns (Sandbox);
  rpc CheckpointSandbox(CheckpointSandboxRequest) returns (CheckpointAck);
  rpc TerminateSandbox(TerminateSandboxRequest) returns (TerminateAck);
  rpc Browse(BrowseRequest) returns (BrowseResult);
  rpc Extract(ExtractRequest) returns (ExtractionResult);
  rpc Crawl(CrawlRequest) returns (CrawlResult);
  rpc CaptureArtifact(CaptureArtifactRequest) returns (Artifact);
}
```

Sandbox events (`sandbox_id`) must use the canonical identifier format and be published to NATS.

**Note from roadmap:** Quarry should not receive MP v2 agent semantics. It is a substrate. `research-core` sits above Quarry and manages the orchestration layer.

### Impact if not fixed

- `agent-core` tools that require browser execution have no clean execution surface.
- `research-core` (Gap 6) cannot fan out crawling tasks without a stable Quarry integration contract.
- The `sandbox_id` canonical identifier has no owning service.

### Remediation steps

1. Define the Quarry integration contract (gRPC proto) in `shared/contracts/quarry.proto`.
2. Ensure Quarry uses the canonical `sandbox_id` format (see Gap 8).
3. Wire Quarry sandbox lifecycle events to NATS using the canonical event envelope.
4. Build `research-core` on top of the Quarry contract, not directly into Quarry.
5. Plan connector mode for trusted-browser execution separately (roadmap Phase 6 note).

---

## Gap 8 — Canonical identifiers not enforced at schema level

**Severity:** HIGH  
**Blocking:** Replay, tracing, and cross-service correlation are unreliable without this.

### Current state

The roadmap defines **17 canonical identifiers** but none are enforced via shared schema validators, Protobuf type aliases, or database check constraints. Services generate IDs independently with inconsistent prefixes and formats.

### Required canonical identifiers (from roadmap)

| Identifier | Owning service |
|------------|---------------|
| `session_id` | session-core |
| `thread_id` | session-core |
| `research_thread_id` | research-core / session-core |
| `run_id` | agent-core → session-core event |
| `parent_run_id` | agent-core |
| `action_id` | agent-core |
| `tool_call_id` | agent-core |
| `approval_id` | capability-core / session-core |
| `connector_delivery_id` | connector gateway |
| `workspace_id` | control-plane |
| `sandbox_id` | Quarry |
| `skill_id` | capability-core |
| `skill_version` | capability-core |
| `trajectory_id` | agent-core / session-core |
| `org_id` | control-plane |
| `user_id` | control-plane |
| `query_depth` | research-core |

### Desired state

- All 17 identifiers have a documented format (e.g. `ses_<ulid>`, `run_<ulid>`).
- Shared Protobuf well-known types or Pydantic validators enforce format at every service boundary.
- Database schemas use typed columns (not bare `TEXT`) with check constraints where appropriate.
- A test asserts that every canonical ID passing through the event bus matches the registered format.

### Impact if not fixed

- Cross-service correlation breaks silently when an ID format drifts.
- Replay produces gaps when IDs used in events do not match IDs stored in the database.
- Debugging requires manual ID format guessing.

### Remediation steps

1. Define all 17 ID formats in `docs/identifiers.md` (part of Phase 0).
2. Create `shared/identifiers.py` (and Go/Rust equivalents) with constructor and validator functions.
3. Add database migrations to add check constraints on existing ID columns.
4. Add a shared Protobuf enum or string-type alias for each canonical identifier family.
5. Add a CI lint rule that fails if a raw `str` is used for a canonical ID field without type annotation.

---

## Gap 9 — Run state machine (9-state) not documented or enforced

**Severity:** HIGH  
**Blocking:** Phase 2 execution spine depends on this.

### Current state

Runs transition through states implicitly in `agent-core`. The state set is not documented, not enforced at the database level, and not validated on transitions. Observers cannot determine the current state of a run without reading internal `agent-core` memory.

### Required state machine (from roadmap)

```
created → queued → running → {
  waiting_for_approval,
  waiting_for_tool,
  compacting
} → running → {completed, failed, cancelled}
```

All nine states: `created`, `queued`, `running`, `waiting_for_approval`, `waiting_for_tool`, `compacting`, `completed`, `failed`, `cancelled`.

### Desired state

- `run_state` column in every run-related table uses an enum limited to the 9 valid states.
- All state transitions are validated with an explicit allowed-transitions table (no free-form state setting).
- Every transition emits a `run.state_changed` event to NATS with `from_state`, `to_state`, `run_id`, `timestamp`.
- `session-core` can reconstruct the full state history of any run from events.

### Impact if not fixed

- Cannot deterministically resume a run after restart.
- Approval and tool-wait paths have no canonical observable state.
- `session-core` replay (Gap 3) cannot produce a consistent timeline without state machine events.

### Remediation steps

1. Define the 9 states and allowed transitions in `docs/run_state_machine.md`.
2. Add `run_state` enum migration to `agent-core` and `session-core` schemas.
3. Replace all raw state string assignments in `agent-core` with a `RunStateMachine.transition(to_state)` call that validates the transition.
4. Emit `run.state_changed` NATS events on every transition.
5. Add unit tests asserting all valid and invalid transitions.

---

## Gap 10 — Compaction and memory tiers not implemented

**Severity:** MEDIUM  
**Blocking:** Phase 3; agents degrade silently under long context without this.

### Current state

`agent-core` has compaction code but it is not modelled as a first-class state machine event. Memory tiers are not defined. Post-compact cache invalidation does not happen. Compaction metadata is not stored in any authoritative log.

### Desired state (Phase 3)

**Five memory tiers, each with explicit load rules:**

| Tier | Contents | Load trigger |
|------|----------|--------------|
| Bootstrap | Org instructions, system prompt | Session start |
| Org memory | Persistent org-level facts | Session start |
| Thread memory | Summary of prior turns in thread | Thread resume |
| Recent execution context | Last N turns, tool calls | Always |
| Semantic memory | Retrieved embeddings | On similarity threshold |

**Compaction behaviour:**
- Auto-compact triggers at a configurable token threshold.
- Pre-compact snapshot is recorded (allows reproducible re-summarisation).
- Post-compact: stale tool cache and retrieval cache are invalidated.
- Compaction metadata (before/after token counts, summary hash) stored in `session-core`.
- Failure modes are explicit: if compaction fails, the agent does not silently continue with an over-full context.

### Remediation steps

1. Define memory tier load rules in `docs/memory_tiers.md`.
2. Add compaction event types to the canonical event envelope.
3. Emit `compaction.started` and `compaction.completed` events with metadata to `session-core`.
4. Add post-compact cache invalidation hooks in `agent-core`.
5. Add fallback behaviour spec (fail-safe vs. degrade) for compaction failures.

---

## Gap 11 — Convex is still authority, not projection

**Severity:** HIGH  
**Blocking:** Data consistency and replay correctness.

### Current state

Convex mutations in the Application Plane perform direct business-logic writes (session creation, run state updates, message appends). Convex is treated as a source of truth for agent state, not a reactive projection.

### Desired state (Phase 4)

Convex owns only:

- `agentThreads` — reactive read model
- `researchThreads` — reactive read model
- `threadMessages` — latest N messages for display
- `runStatus` — current run state for UI
- `approvalQueue` — pending approvals for display
- `researchSummaries` — latest research synthesis

All writes flow through authoritative services → NATS events → projection workers → Convex. Convex can be deleted and repopulated from `session-core` event history. Frontend code paths tolerate projection lag explicitly (no business decisions depend on Convex being current).

### Impact if not fixed

- Convex state can diverge from `session-core` with no reconciliation path.
- Deleting and replaying Convex produces different UI state.
- Business-logic bugs in Convex mutations are invisible during replay.

### Remediation steps

1. Audit all Convex mutations for business logic (state transitions, ID generation, budget checks).
2. Move each piece of business logic to the appropriate authoritative service.
3. Build NATS → Convex projection workers (one per read model).
4. Add parity checks between `session-core` and Convex projection state.
5. Remove direct mutations from frontend code; route all writes through service APIs.

---

## Gap 12 — Connector and outbox layer absent

**Severity:** MEDIUM  
**Blocking:** Phase 6; persistent agents cannot receive/send via external channels.

### Current state

No connector gateway, outbound delivery outbox, or connector registry exists. External channel integration (webhooks, long-lived sockets) is not supported.

### Desired state (Phase 6)

**Normalized connector envelope fields:**
- `connector_name`, `connector_auth_reference`, `sender_identity`
- `thread_mapping_metadata`, `normalized_content_blocks`, `attachments`
- `delivery_metadata`, `idempotency_key`, `connector_delivery_id`

**Capabilities:**
- Connector registry mapping transport capabilities to auth references.
- Gateway service owning inbound webhook and socket connectivity.
- Outbound delivery outbox with retry and idempotent delivery semantics.
- Connector events published to NATS using canonical identifiers.
- Connector secrets isolated — never leak into `agent-core` or Convex.
- Future extension point for trusted-browser connector mode (Manus Browser Operator style).

### Remediation steps

1. Define connector envelope schema in `shared/contracts/connector.proto`.
2. Create gateway service (new or extend existing `apps/Control Plane/` gateway).
3. Implement outbound outbox with retry semantics backed by existing Postgres.
4. Add connector registry table.
5. Wire `connector_delivery_id` to canonical identifier format (Gap 8).

---

## Gap 13 — Phases 9–12 not started

**Severity:** LOW (deferred; not blocking current phases)

These phases are not expected to be started before Phases 0–8 are complete but are catalogued here to avoid scope amnesia.

| Phase | Description |
|-------|-------------|
| 9 | Skill evaluation pipeline and promotion/rollback machinery |
| 10 | Trajectory capture and training data export |
| 11 | Multi-org isolation and tenant boundary enforcement |
| 12 | Observability plane (structured traces, span export, cost attribution per `org_id`) |

**Required contract surfaces for Phase 9 (skill lifecycle events):**
- `skill.evaluated`, `skill.promoted`, `skill.rolled_back` (canonical events)
- `capability-core` owns `skill_id`, `skill_version`

**Required contract surfaces for Phase 10:**
- `trajectory_id` with pointer to raw turn inputs/outputs
- Export pipeline writing to MinIO (already provisioned)

### Remediation steps

No immediate action. Ensure Phase 0 contract work reserves the identifier and event namespace for these phases.

---

## Gap 14 — `docs/` is effectively empty

**Severity:** MEDIUM  
**Blocking:** Onboarding, Phase 0 contract freeze.

### Current state

`apps/Model Plane v2/docs/` contains one file: `Future roadmap.md`. No ADRs, no service READMEs, no schema documentation, no runbooks.

### Desired state

Minimum documentation corpus:

```
docs/
  adr/
    ADR-001-service-ownership.md
    ADR-002-language-runtime-selection.md
    ADR-003-identifier-strategy.md
    ADR-004-convex-projection-model.md
  identifiers.md
  ownership_matrix.md
  run_state_machine.md
  memory_tiers.md
  event_envelope.md
  GAP_ANALYSIS.md          ← this file
  Future roadmap.md
```

Each service should have its own `README.md` describing: purpose, ports, dependencies, key APIs, local dev instructions.

### Remediation steps

1. Write ADR-001 as part of Phase 0 kickoff (not optional).
2. Generate service READMEs from current code; they do not need to be comprehensive initially.
3. Maintain `ownership_matrix.md` as a living document — update on every PR that touches a domain boundary.

---

## Phased Remediation Roadmap

The table below maps gaps to the roadmap phase where each should be resolved.

| Roadmap Phase | Gaps closed | Key deliverables |
|---------------|-------------|-----------------|
| **Phase 0** (Baseline) | 1, 8, 14 | ADRs, ownership matrix, identifier glossary, event envelope, compatibility matrix |
| **Phase 1** (session-core) | 3, partial 2 | session-core service, durable authority migration, Convex feed from events |
| **Phase 2** (Execution spine) | 9, partial 2 | 9-state run machine, canonical execution context, full tool call observability |
| **Phase 3** (Compaction) | 10 | Memory tiers, compaction events, post-compact cache invalidation |
| **Phase 4** (Convex projection) | 11 | Projection workers, parity checks, business-logic removal from Convex |
| **Phase 5** (capability-core) | 4, partial 2 | Governance surface, policy audit log, deterministic policy tests |
| **Phase 6** (Connectors) | 12 | Connector envelope, gateway, outbox |
| **Phase 7** (research-core) | 6, partial 7 | research-core service, hypothesis tree, Quarry adapter |
| **Phase 8** (Quarry integration) | 7 | Full Quarry contract, sandbox events |
| **Parallel / ongoing** | 5 | Go capability-core, Rust execution-core hot path |
| **Phases 9–12** | 13 | Skills, trajectories, multi-org, observability plane |

---

## Quick Wins (can start immediately, unblock later phases)

| Item | Effort | Unblocks |
|------|--------|---------|
| Write ADR-001 (service ownership) | 1 day | All phases |
| Write `docs/identifiers.md` with 17 ID formats | 0.5 day | Gap 8, replay correctness |
| Tag `agent-core` with `# TODO: session-core` / `# TODO: capability-core` comments | 1 day | Phase 1 scoping |
| Add `run_state` enum migration + transition validator | 2 days | Gap 9, replay |
| Define canonical event envelope proto | 1 day | Gap 1, all event-emitting work |
| Write service READMEs (generated from code) | 1 day | Gap 14, onboarding |

---

## Appendix — Canonical Event Families (13 required)

These event types must be defined in the shared event envelope schema before Phase 1:

1. `session.created`, `session.closed`
2. `thread.created`, `thread.archived`
3. `run.created`, `run.state_changed`, `run.completed`, `run.failed`, `run.cancelled`
4. `action.started`, `action.completed`, `action.failed`
5. `tool_call.requested`, `tool_call.result`
6. `approval.requested`, `approval.resolved`
7. `hook.invoked`, `hook.outcome`
8. `compaction.boundary` (before/after markers)
9. `subagent.spawned`, `subagent.completed`, `subagent.failed`
10. `research.hypothesis_proposed`, `research.evidence_collected`
11. `sandbox.created`, `sandbox.terminated`, `sandbox.checkpointed`
12. `connector.delivery_received`, `connector.delivery_sent`
13. `skill.evaluated`, `skill.promoted`, `skill.rolled_back`

---

*This document should be treated as a living artefact. Update gap status as phases complete. Each gap should link to its corresponding ADR and tracking issue when available.*
