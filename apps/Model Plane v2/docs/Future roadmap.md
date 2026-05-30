# Future roadmap

## Purpose

This document is the execution roadmap for turning Model Plane v2 into a durable, research-capable, policy-governed agent backend.

It is intentionally written as a delivery plan, not a product vision note. Each phase defines:

- what changes
- which service owns it
- which contracts must exist first
- what can run in parallel
- how the phase is verified before the next one starts

## Desired end state

Model Plane v2 should converge on this service split:

- `session-core` is the system of record for sessions, messages, approvals, run lineage, and replay.
- `agent-core` is the runtime engine for turns, tools, hooks, compaction, subagents, and trajectory capture.
- `capability-core` is the only policy and routing authority for model selection, tool governance, budgets, and approval decisions.
- `ai-core` is the inference facade, not the durable state owner and not the hidden policy layer.
- `research-core` is a new standalone orchestrator for wide research, evidence gathering, hypothesis trees, and result synthesis.
- `Quarry` stays the deterministic execution substrate for browser automation, crawling, extraction, and sandbox lifecycle.
- `Application Plane / Convex` is the reactive projection and collaboration read model for frontend experiences, not a primary write authority.
- learning remains built on existing MP v2 trajectory, cron, memory, and skills work rather than introducing a second self-improvement system.

## Why this split

The roadmap borrows proven patterns from current external systems while preserving the existing CoreSystem architecture:

- Claude Code contributes runtime discipline: lifecycle hooks, tool governance, compaction boundaries, instruction loading, and session-aware memory behavior.
- Convex contributes durable threads, message persistence, reactive projections, hybrid context retrieval, and durable workflow semantics.
- OpenClaw contributes gateway-centered channel normalization, connector envelopes, session routing, and skill registry ideas.
- Manus contributes parallel wide-research orchestration, persistent per-task execution environments, and browser actions tied to trusted local context or isolated sandboxes.
- Hermes contributes closed-loop learning: persistent memory, trajectory search, skill evolution, reviewable promotions, and long-lived agent environments.

## Manus match strategy

MP v2 should not try to copy Manus as a single product. It should copy the operating concepts that fit our existing service boundaries.

### Manus concepts to copy directly

- Wide Research: a main orchestrator breaks one research goal into many independent branches, each branch runs with fresh context, and results are synthesized centrally.
- Persistent task environments: every long-running research or execution task gets a resumable workspace with artifacts, journals, and checkpoint references.
- Branch isolation: sub-runs should not share one growing context window; they should converge only through structured findings and evidence.
- Resume semantics: long-running tasks must survive restarts and continue from durable state rather than restarting from prompt-only context.
- Evidence-first outputs: reports, datasets, and conclusions should be traceable to fetches, artifacts, and source-level provenance.

### Manus concepts to adapt, not clone

- Browser Operator: MP v2 should first implement server-side browser execution through Quarry sandboxes. A later optional local-browser mode should live behind the connector or gateway layer, not inside Quarry core.
- Cloud computer model: MP v2 already has the right split for this. `execution-core` should manage runner lifecycle and workspace semantics, while Quarry stays focused on browser and extraction execution.
- Productized website builder behavior: this is not the first Manus parity target. If added later, it should be expressed as durable execution workflows and artifact pipelines, not as a separate monolithic builder service.

### Manus concepts to avoid

- putting research planning into Quarry
- letting `ai-core` become the durable owner of task state
- letting branch workers write directly to projection state
- coupling trusted-user browser access with server-side browser infrastructure

### MP v2 interpretation

In MP v2, Manus parity means:

- `research-core` becomes the planner, fan-out coordinator, evidence aggregator, and synthesis layer
- `session-core` becomes the durable owner of research threads, run lineage, and resumable task state
- Quarry becomes the resumable browser and extraction substrate
- `capability-core` decides the model, tool, budget, and approval profile for each research branch
- `execution-core` remains the generic runner and artifact workspace substrate for non-browser execution
- Convex mirrors research progress and final outputs as a projection, never as truth

## Current starting point

The roadmap starts from real code that already exists, not from zero.

### Already landed

- `agent-core` has first-class lifecycle hook primitives.
- `SESSION_START` is wired into run creation.
- lifecycle hook tests and run-lineage tests exist and are passing.
- `agent-core` already contains trajectory capture, memory seams, auto-skill registry, and cron scheduling surfaces.
- `session-core` already exists as the natural durable authority boundary.
- Convex already exists as an Application Plane read/projection system.
- Quarry already has browser session and session-manager surfaces that can evolve into resumable sandboxes.
- the current compose stack already includes the right enabling substrate for Manus-like durability: NATS, MinIO, Temporal, Letta, isolated worker services, and separate execution and capability services.

### Architectural decisions already made

- `research-core` should be a standalone service between MP v2 and Quarry.
- `session-core` remains the authority for durable threads and lineage.
- Convex remains a projection and subscription layer.
- `capability-core` stays the single governance surface.
- Quarry should not absorb research planning responsibilities.

## Program rules

1. Change authority before adding sophistication.
2. Persist facts once, then project them many ways.
3. Keep runtime state rebuildable from authoritative events.
4. Add new services behind compatibility adapters first.
5. Do not merge research orchestration into browser execution.
6. Do not auto-promote learned behavior without evaluation gates.
7. Every phase must end with a replay or rebuild proof, not only green unit tests.

## Shared contracts to freeze early

These contracts unblock most of the roadmap and should be frozen early enough that other teams can build in parallel.

### Canonical identifiers

- `session_id`
- `thread_id`
- `research_thread_id`
- `run_id`
- `parent_run_id`
- `action_id`
- `tool_call_id`
- `approval_id`
- `connector_delivery_id`
- `workspace_id`
- `sandbox_id`
- `skill_id`
- `skill_version`
- `trajectory_id`
- `org_id`
- `user_id`
- `query_depth`

### Core event families

- session lifecycle events
- thread lifecycle events
- run lifecycle events
- action lifecycle events
- tool call request and result events
- approval request and resolution events
- hook invocation and outcome events
- compaction boundary events
- subagent lifecycle events
- research hypothesis and evidence events
- sandbox lifecycle events
- connector inbound and outbound delivery events
- skill evaluation, promotion, rollback, and retirement events

### Required envelope fields

Every cross-service event should carry at least:

- event type
- event id
- event timestamp
- producing service
- org id
- correlation id
- causation id
- actor type and actor id
- canonical resource ids
- idempotency key
- schema version

### Proposed contract surfaces

These names are proposed so workstreams can converge around a shared vocabulary.

#### session-core

- `CreateSession`
- `AppendMessage`
- `CreateThread`
- `AppendRunEvent`
- `AppendResearchEvent`
- `GetThreadTimeline`
- `ReplayThread`
- `BackfillProjection`

#### agent-core

- `CreateRun`
- `ResumeRun`
- `CancelRun`
- `ExecuteTool`
- `InvokeSubagent`
- `CompactContext`
- `RecordTrajectory`

#### capability-core

- `ResolveExecutionPolicy`
- `ResolveModelRoute`
- `ResolveToolEligibility`
- `ResolveApprovalPolicy`
- `ResolveBudgetDecision`

#### research-core

- `CreateResearchJob`
- `PlanHypotheses`
- `DispatchResearchTasks`
- `CollectEvidence`
- `SynthesizeFindings`
- `ResumeResearchJob`
- `ExportResearchReport`

#### Quarry

- `CreateSandbox`
- `ResumeSandbox`
- `CheckpointSandbox`
- `TerminateSandbox`
- `Browse`
- `Extract`
- `Crawl`
- `CaptureArtifact`

## Phase map

## Phase 0 - Baseline and contract lock

### Goal

Turn the current partial implementation into the stable baseline for the rest of the program.

### Scope

- record what is already landed in `agent-core`
- freeze the service ownership model
- freeze identifiers and event envelopes
- define the first compatibility rules so later phases can ship incrementally

### Work items

1. Document the current `agent-core` lifecycle hook behavior as the initial runtime contract.
2. Freeze the hook event names already in use and mark new ones as roadmap-controlled additions.
3. Define the canonical event envelope and schema versioning policy.
4. Define which service is allowed to persist which resource.
5. Publish a replay contract for authoritative event streams.
6. Add a service-boundary ADR for `research-core` versus Quarry.

### Dependencies

- none

### Deliverables

- ownership matrix
- canonical event envelope
- identifier glossary
- replay contract
- compatibility matrix for old callers

### Exit criteria

- every target service has a written ownership boundary
- no future phase depends on ambiguous authority
- the current hook payloads are considered stable enough to build around

## Phase 1 - session-core becomes the sole durable authority

### Goal

Make `session-core` the only authoritative writer for durable conversational state and lineage.

### Scope

- sessions
- threads
- messages
- approvals
- run lineage
- research lineage links

### Work items

1. Extend `session-core` schemas to support durable threads separate from transient runs.
2. Add append-only storage for run events and research events.
3. Introduce canonical lineage tables keyed by `session_id`, `thread_id`, `run_id`, `parent_run_id`, and `action_id`.
4. Add idempotent append semantics using event id plus idempotency key.
5. Add APIs for timeline retrieval and replay.
6. Emit authoritative NATS events for every durable mutation.
7. Remove any accidental durable-write responsibilities from `agent-core`, `ai-core`, and Convex.

### Services touched

- `apps/Control Plane/session-core`
- NATS subjects shared with MP v2 and Application Plane

### Dependencies

- Phase 0 contract freeze

### Suggested implementation order

1. Schema changes
2. write APIs
3. event publishing
4. replay API
5. caller cutover

### Parallel work allowed

- read-model design in Convex can begin once the authoritative event schemas are frozen

### Exit criteria

- all durable session and message history originates in `session-core`
- event replay into an empty database rebuilds the same lineage graph
- duplicate event delivery does not produce duplicate durable records

## Phase 2 - Normalize the execution spine around agent-core

### Goal

Make `agent-core` a clean execution runtime that consumes commands and emits execution facts.

### Scope

- run state machine
- lifecycle hooks
- tool execution
- permission checks
- subagent lifecycle
- compaction boundaries

### Work items

1. Define the canonical run state machine: created, queued, running, waiting_for_approval, waiting_for_tool, compacting, completed, failed, cancelled.
2. Extend lifecycle coverage beyond `SESSION_START` to include:
   - `SESSION_END`
   - `SUBAGENT_START`
   - `SUBAGENT_STOP`
   - run completion and failure boundaries
3. Introduce a unified immutable execution context object containing:
   - org and user identity
   - run identifiers
   - budget context
   - resolved model and tool policy
   - memory handles
   - artifact handles
4. Route every tool call through:
   - pre-tool hook
   - policy resolution
   - approval decision if required
   - execution
   - post-tool success or failure handling
5. Publish execution events back to `session-core` instead of persisting runtime truth locally.
6. Keep local runtime state ephemeral and rebuildable.
7. Harden task completion semantics so tasks cannot be double-completed or silently orphaned.
8. Formalize compaction boundary events and compaction summaries.

### Services touched

- `apps/Model Plane v2/agent-core`
- `apps/Control Plane/session-core`

### Dependencies

- Phase 1 durable authority

### Suggested implementation order

1. run state machine
2. execution context object
3. lifecycle hook expansion
4. tool path normalization
5. event emission and replay validation

### Exit criteria

- `session-core` can reconstruct a full run timeline from `agent-core` events
- every tool call is observable with policy, approval, and result metadata
- compaction produces explicit before and after boundaries

## Phase 3 - Formalize compaction and durable memory behavior

### Goal

Adopt Claude Code style context discipline without copying terminal-specific concerns.

### Scope

- startup context loading
- thread memory recall
- auto compact triggers
- compaction summaries
- post-compact cleanup
- memory tiers

### Work items

1. Define memory tiers:
   - bootstrap instructions
   - org memory
   - thread memory
   - recent execution context
   - retrieved semantic memory
2. Explicitly define when each memory tier loads.
3. Add pre-compact snapshot inputs to allow reproducible summaries.
4. Add post-compact invalidation rules for stale tool and retrieval caches.
5. Add fallback behavior when compaction or memory adapters fail.
6. Record compaction metadata into authoritative lineage.
7. Ensure memory recall references `thread_id` and `research_thread_id` instead of only session scope.

### Services touched

- `agent-core`
- memory bridge surfaces already present in MP v2
- `session-core` for storing compaction lineage metadata

### Dependencies

- Phase 2 execution spine

### Exit criteria

- compaction is replayable and observable
- memory recall is thread-aware
- failure modes are explicit instead of silent degradation

## Phase 4 - Build Convex as projection only

### Goal

Let the Application Plane provide reactive UX while remaining a projection and collaboration surface.

### Scope

- agent threads
- research threads
- live message views
- run status views
- approvals and activity feeds
- collaboration and presence

### Work items

1. Define Convex read models for:
   - `agentThreads`
   - `researchThreads`
   - `threadMessages`
   - `runStatus`
   - `approvalQueue`
   - `researchSummaries`
2. Build NATS subscribers or projection workers that consume authoritative events.
3. Add rebuild tooling that can repopulate the projection from retained event history.
4. Add parity checks between authoritative state and projection state.
5. Update frontend code paths to tolerate projection lag explicitly.
6. Remove direct business-decision logic from Convex mutations where it exists.

### Services touched

- `apps/Application Plane/convex-core`
- `session-core`
- `agent-core`

### Dependencies

- Phase 1 event authority
- Phase 2 event completeness

### Parallel work allowed

- this phase can progress in parallel with Phase 5 once event schemas are stable

### Exit criteria

- deleting and replaying the projection reproduces the same frontend state
- frontend clients do not treat Convex as the primary source of truth

## Phase 5 - capability-core becomes the only governance surface

### Goal

Remove scattered policy decisions from runtime and inference services.

### Scope

- model routing
- budget decisions
- tool eligibility
- approval requirements
- fallback routing
- decision auditing

### Work items

1. Define a unified policy decision contract for execution requests.
2. Add decision outputs for:
   - chosen model route
   - allowed tools
   - denied tools with reasons
   - approval mode required
   - budget status and fallback policy
3. Replace inline policy branching in `agent-core` with calls to `capability-core` plus bounded caching.
4. Ensure `ai-core` only receives pre-resolved routing and policy envelopes.
5. Add auditable decision logs keyed by run, thread, and org.
6. Add deterministic policy tests for allow, deny, ask, fallback, and budget exhaustion.

### Services touched

- `apps/Model Plane v2/capability-core`
- `agent-core`
- `ai-core`

### Dependencies

- Phase 1 authoritative identifiers
- ideally Phase 2 execution context normalization

### Exit criteria

- every model and tool decision can be explained by `capability-core`
- policy behavior is deterministic under test for equivalent inputs

## Phase 6 - Introduce connector and gateway normalization

### Goal

Give persistent agents an external channel layer without coupling transports to `agent-core`.

### Scope

- inbound connectors
- outbound delivery
- secrets isolation
- connector registry
- idempotent delivery
- outbox and retry semantics

### Work items

1. Define a normalized connector envelope with:
   - connector name
   - connector auth reference
   - sender identity
   - thread mapping metadata
   - normalized content blocks
   - attachments
   - delivery metadata
   - idempotency key
2. Add a connector registry for transport capabilities and auth references.
3. Add a gateway service or extend an existing gateway to own inbound webhook and socket connectivity.
4. Add an outbound delivery outbox with retry semantics.
5. Publish connector events into NATS using the same canonical identifiers.
6. Prevent connector secrets from leaking into `agent-core` and Convex.
7. Add a later-stage connector mode for trusted-user browser execution so MP v2 can support a Manus Browser Operator style flow without coupling local browser trust to Quarry's server-side sandbox model.

### Services touched

- new or extended gateway service
- `session-core`
- Application Plane projections

### Dependencies

- Phase 1 authority
- Phase 5 governance if approval policies depend on channel risk

### Exit criteria

- at least one webhook connector and one long-lived gateway connector operate through the normalized envelope
- inbound dedupe and outbound replay are proven
- the roadmap leaves room for a future trusted-browser connector mode without forcing it into the initial browser substrate

## Phase 7 - Create research-core as a standalone orchestration service

### Goal

Insert the missing layer for Manus-style wide research without teaching Quarry MP v2 semantics.

### Scope

- research jobs
- hypothesis planning
- fan-out orchestration
- evidence aggregation
- contradiction tracking
- citation packaging
- job resume and export

### Work items

1. Create `research-core` as a dedicated service with its own storage and lifecycle.
2. Add a research job model:
   - created
   - planning
   - dispatching
   - running
   - synthesizing
   - completed
   - failed
   - cancelled
3. Add hypothesis tree storage keyed by `research_thread_id`.
4. Add parallel task fan-out where each unit of work receives fresh context.
5. Add source ranking, deduplication, and contradiction tracking.
6. Add provenance packaging so each finding can be traced back to evidence.
7. Add a compatibility adapter so existing MP v2 callers can call `research-core` before direct Quarry assumptions are removed.
8. Publish progress events back into `session-core` and Convex projections.

### Services touched

- new `research-core`
- `session-core`
- `capability-core`
- Convex projections
- Quarry adapters

### Dependencies

- Phase 1 authority
- Phase 5 governance
- Phase 4 projection work is helpful but not blocking

### Exit criteria

- research workflows no longer couple `agent-core` or `ai-core` directly to Quarry internals
- each research report includes evidence lineage and provenance metadata

## Phase 8 - Upgrade Quarry into a persistent sandbox substrate

### Goal

Evolve Quarry from browser session management into resumable, thread-scoped execution environments.

### Scope

- sandbox lifecycle
- checkpointing
- artifact persistence
- action journals
- browser state resume
- recycle policy
- sleep and wake semantics
- restorable versus disposable artifact classes

### Work items

1. Expand Quarry session ownership to support durable `sandbox_id` records.
2. Key sandboxes by org, user, and `research_thread_id`.
3. Add explicit lifecycle operations:
   - create
   - resume
   - checkpoint
   - recycle
   - terminate
4. Persist artifact metadata, snapshots, journals, and checkpoint references.
5. Separate reusable artifacts from temporary execution scratch space.
6. Add recycle and wake behavior inspired by Manus sandbox lifecycle.
7. Add failure recovery that can provision a fresh sandbox while preserving restorable artifacts.
8. Keep research planning outside Quarry.
9. Add explicit sleep, wake, and stale-recycle policies so dormant sandboxes can hibernate without losing durable artifacts and checkpoints.
10. Split artifacts into at least two classes: restorable artifacts that survive sandbox recycling, and disposable scratch outputs that do not.

### Services touched

- `apps/Ingestion Plane/Quarry`
- `research-core`

### Dependencies

- Phase 7 research-core contract

### Exit criteria

- research jobs can resume with sandbox continuity after service restarts
- artifact and snapshot isolation holds by org, user, and thread
- sandbox recycling preserves the intended restorable artifact set and does not accidentally carry forward disposable scratch state

## Phase 9 - Add wide-research execution flows

### Goal

Deliver Manus-style wide research on top of `research-core` and Quarry.

### Scope

- multi-agent fan-out
- fresh context per item
- centralized orchestration
- synthesis back into durable research threads
- resume after partial completion
- operator-style browser execution for branches that need interactive workflows

### Work items

1. Add a planner that decomposes a research goal into many independent work units.
2. Assign each work unit fresh context rather than accumulating one monolithic context window.
3. Route each work unit through `capability-core` for the appropriate model and tool mix.
4. Dispatch browsing, crawling, and extraction through Quarry sandboxes.
5. Aggregate outputs into structured findings, tables, and citations.
6. Add contradiction detection and duplicate source suppression.
7. Add confidence scoring and source diversity scoring.
8. Materialize a resumable research thread in `session-core` plus Convex projection.
9. Support partial completion and resume so a large research job can continue from branch-level durable state instead of restarting all branches.
10. Add an operator path for research branches that require multi-step interactive browsing, backed by Quarry sandboxes first and a future trusted-browser connector mode second.

### Services touched

- `research-core`
- `capability-core`
- Quarry
- `session-core`
- Convex

### Dependencies

- Phase 7 and Phase 8

### Exit criteria

- large research tasks can fan out to many independent runs with consistent quality
- synthesis preserves evidence traceability instead of returning ungrounded summaries
- partially completed research jobs can resume without losing finished branch work

## Phase 10 - Close the Hermes-style learning loop

### Goal

Turn existing trajectory and skills infrastructure into a measurable self-improvement system.

### Scope

- trajectory capture alignment
- skill synthesis
- evaluation and review
- staged rollout
- rollback and retirement
- memory enrichment

### Work items

1. Align trajectory records to authoritative run and thread identifiers.
2. Add clustering over successful trajectories.
3. Generate candidate reusable skills from successful trajectories.
4. Evaluate candidate skills offline before promotion.
5. Add confidence windows and success-rate thresholds.
6. Add org-scoped rollout first; do not start with cross-org sharing.
7. Add rollback and retirement signals for underperforming learned skills.
8. Add semantic retrieval over successful trajectories and learned patterns.
9. Feed measured outcomes back into tool and hook recommendations.

### Services touched

- `agent-core`
- learning modules built around existing trajectory, cron, and skill registry code
- `capability-core` for policy gating learned skill use

### Dependencies

- Phase 2 execution facts
- Phase 1 durable lineage
- Phase 7 and 9 if research-derived skills are included

### Exit criteria

- learned skills are reviewable, versioned, measurable, and reversible
- no automatic promotion occurs without passing evaluation thresholds

## Phase 11 - Observability, replay, and failure drills

### Goal

Make the new architecture operable before full cutover.

### Scope

- traces
- cost attribution
- replay tools
- dead letter handling
- lag monitoring
- runbooks

### Work items

1. Add append-only traces for runs, hooks, tool calls, research jobs, sandbox actions, and connector deliveries.
2. Add query surfaces by session, thread, run, action, sandbox, and connector delivery.
3. Add cost attribution keyed by org, thread, run, and research job.
4. Add projection lag and rebuild dashboards.
5. Add dead-letter handling for NATS delivery failures.
6. Add failure drills for:
   - NATS outage
   - Convex projection lag
   - Quarry outage
   - capability-core degradation
   - research-core restart during fan-out
7. Publish operator runbooks.

### Dependencies

- Phases 1 through 10

### Exit criteria

- operators can explain, replay, and recover every critical workflow
- there is a documented degraded mode for each critical service outage

## Phase 12 - Cutover and deprecation cleanup

### Goal

Remove compatibility shims only after the new architecture is proven.

### Scope

- delete legacy write paths
- remove direct Quarry calls from runtime services
- lock `ai-core` to inference concerns
- publish ownership docs and long-term maintenance boundaries

### Work items

1. Remove direct durable write paths outside `session-core`.
2. Remove direct Quarry orchestration from `agent-core`.
3. Remove hidden policy branches from `ai-core` and `agent-core`.
4. Keep only compatibility adapters still required for legacy callers.
5. Publish final service ownership and escalation docs.

### Dependencies

- every prior phase complete

### Exit criteria

- runtime architecture matches the target service split
- no hidden side channels remain for durability, research orchestration, or governance

## Recommended sequencing and parallelism

### Critical path

1. Phase 0
2. Phase 1
3. Phase 2
4. Phase 5
5. Phase 7
6. Phase 8
7. Phase 9
8. Phase 10
9. Phase 11
10. Phase 12

### Parallel windows

- Phase 3 can run after Phase 2 stabilizes.
- Phase 4 can start once Phase 1 and Phase 2 event contracts are frozen.
- Phase 6 can run after Phase 1 and Phase 5 are stable enough for connector risk policies.
- learning experiments for Phase 10 can begin earlier, but promotion gates should wait for authoritative lineage.

## Service-by-service deliverables

### session-core

- canonical thread schema
- append-only run and research event storage
- replay APIs
- backfill APIs
- idempotent event publishing

### agent-core

- full run state machine
- expanded lifecycle hooks
- unified execution context
- tool path normalization
- trajectory capture bound to authoritative IDs

### capability-core

- routing and budget contracts
- tool governance decisions
- approval policy contracts
- decision audit logs

### ai-core

- inference-only service contracts
- resolved provider and model input envelopes
- no hidden persistence or policy behavior

### research-core

- research job API
- hypothesis planning
- fan-out orchestration
- evidence and citation normalization
- synthesis and export

### Quarry

- durable sandbox records
- artifact journals
- checkpoint and resume APIs
- recoverable browser state

### Application Plane / Convex

- thread projections
- run-status projections
- activity feeds
- projection rebuild tooling

### gateway and connectors

- normalized inbound envelope
- outbound outbox
- auth reference isolation
- retry and dedupe behavior

## Verification gates

### Authority gate

Only `session-core` may persist sessions, messages, and durable lineage.

### Projection gate

Convex must be deletable and rebuildable from retained authoritative events.

### Execution gate

Run timelines must be reproducible from `agent-core` execution events.

### Governance gate

Every model and tool decision must be explainable by `capability-core`.

### Research gate

Every externally sourced research finding must include provenance and evidence lineage.

### Sandbox gate

Sandbox checkpoint, resume, recycle, and failure recovery must preserve intended artifacts while isolating scratch state.

### Learning gate

Learned skills must be measurable, reviewable, reversible, and org-scoped before broader sharing.

## Suggested rollout order

1. Ship authority and replay first.
2. Ship runtime normalization next.
3. Ship projection rebuildability before UI reliance increases.
4. Ship centralized governance before wide-research orchestration.
5. Ship `research-core` behind a compatibility adapter.
6. Ship Quarry sandbox durability after the `research-core` contract is stable.
7. Ship wide research to a small org cohort.
8. Ship learning gates after authoritative lineage is trusted.
9. Remove shims only after replay and failure drills pass.

## Manus-specific parity targets

These are the practical parity targets MP v2 should aim for if the goal is to "match Manus in some way" without rebuilding Manus wholesale.

### Target 1 - Wide research parity

MP v2 can claim parity when one research request can be decomposed into many independent branches, executed in parallel with fresh context, and synthesized back into one durable research thread with citations and evidence lineage.

### Target 2 - Persistent execution parity

MP v2 can claim parity when a long-running research or execution task can sleep, resume, checkpoint, recycle, and recover without losing the intended durable artifacts.

### Target 3 - Browser operator parity

MP v2 can claim parity when it supports both:

- server-side browser execution through Quarry sandboxes
- a later optional trusted-browser connector mode for user-session tasks

### Target 4 - Durable workflow parity

MP v2 can claim parity when research and execution flows resume from durable workflow state and not from prompt reconstruction alone.

### Target 5 - Learning parity

MP v2 can claim parity when successful runs become retrievable organizational memory and candidate reusable skills, subject to evaluation and rollout controls.

## Deliberately out of scope for this roadmap

- IDE-specific UI work
- terminal UX and prompt ergonomics
- voice and mobile surfaces
- plugin marketplace UX
- frontend styling concerns unrelated to projection contracts

## Research inputs used to update this roadmap

The sequencing in this document reflects the current external product and documentation patterns visible in April 2026:

- Claude Code docs for hooks, memory, lifecycle events, compaction hooks, instruction loading, and session-aware auto memory.
- Convex agent docs for persisted threads, hybrid context retrieval, and durable workflow execution with retries and idempotent steps.
- OpenClaw and ClawHub docs for gateway-centered session routing, skill registry metadata, connector normalization, and channel abstraction.
- Manus docs and feature pages for wide-research orchestration, per-task persistent sandboxes, and browser operation across trusted sessions.
- Hermes Agent docs and repository state for learning loops, persistent memory, skill evolution, messaging gateway operation, and long-lived agent environments.

## First implementation slices after this document

The smallest pragmatic slices after the already-landed `SESSION_START` work are:

1. wire `SESSION_END`, `SUBAGENT_START`, and `SUBAGENT_STOP` into the live runtime path
2. freeze the authoritative event envelope in `agent-core` and `session-core`
3. add append-only run event storage in `session-core`
4. add a Convex replayable projection for thread and run status
5. extract the first compatibility adapter for `research-core`
6. define the Quarry sandbox lifecycle contract with sleep, wake, checkpoint, recycle, and artifact classes
7. define the first research branch contract that enforces fresh-context fan-out plus centralized synthesis

Those seven slices move the architecture toward the target shape without requiring a big-bang cutover.