# ADR-001: Service Ownership Matrix

- **Status:** Accepted
- **Date:** 2025-01-XX
- **Deciders:** Model Plane v2 architecture working group
- **Supersedes:** none
- **Superseded by:** none

## Context

Model Plane v2 adopts the Option B two-core topology (`ai-core` + `agent-core`)
with distinct services for session durability, capability governance, research,
sandboxed execution, LLM inference, and artifact storage. Today, `agent-core` is
a 60+ module monolith that silently owns concerns that belong elsewhere
(session state, policy, permissions, skills catalog, cost accounting, voice,
Letta bridging). Without a crisp ownership contract, every downstream phase
(contract lock, eventing, state machines, auth) becomes ambiguous and blocks.

This ADR establishes the canonical ownership boundaries that all subsequent
ADRs, schemas, events, and APIs must respect.

## Decision

Each service below owns its listed concerns exclusively. If another service
needs that concern, it MUST call the owner's API or consume its events — it
MUST NOT keep a local copy of truth.

### Ownership Matrix

| Service          | Owns (authoritative)                                                                                 | Does NOT own (must call/subscribe)                                                             |
|------------------|-------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------|
| `session-core`   | session, thread, research_thread, run, parent_run, action, compaction checkpoints, run state machine | orchestration logic, tool execution, policy evaluation, model inference                        |
| `agent-core`     | orchestration DAG, planning, step dispatch, run reducer (stateless), tool-call coordination           | session persistence, policy, permissions, skills catalog, cost ledger, voice, Letta memory     |
| `capability-core`| policy bundles, permission grants, skills catalog, skill versions, approval workflows                 | session state, run state, execution, inference                                                 |
| `research-core`  | research jobs, research threads, query depth, source deduplication, citation graph                    | orchestration, skill dispatch, policy evaluation                                               |
| `execution-core` | sandbox lifecycle, tool_call execution, resource quotas, egress enforcement                           | run state, policy decisions, skill definitions                                                 |
| `llm-worker`     | model inference, streaming tokens, provider routing, prompt caching                                   | run state, policy, cost ledger (emits cost events only)                                        |
| `quarry`         | artifacts, attachments, blob lifecycle, connector deliveries                                          | session state, run state, skills                                                               |
| `ai-core`        | low-level AI primitives (embeddings, rerank, safety classifiers)                                      | orchestration, session state, policy                                                           |
| `cost-core`      | cost ledger, token accounting, budget enforcement                                                     | run state, orchestration                                                                       |
| `voice-core`     | voice sessions, STT/TTS streaming, barge-in state                                                     | run state, orchestration                                                                       |
| `memory-core`    | long-term memory (Letta bridge), semantic memory, episodic recall                                     | run state, orchestration                                                                       |

### Boundary Rules

1. **Single writer per resource.** Only the owner service writes to its
   authoritative store. All other services read via API or event subscription.
2. **No shadow state.** Non-owners may cache with TTL ≤ 60s but must never
   persist authoritative state.
3. **Events over polling.** Owners emit events on state change; consumers
   subscribe. Polling is a code smell except for health probes.
4. **Identifiers cross boundaries, not rows.** Services exchange canonical
   identifiers (see `docs/identifiers.md`), never full DB rows.
5. **Run state is single-sourced in `session-core`.** `agent-core` computes
   transitions and calls `session-core.runs.transition(run_id, to_state)`;
   it does NOT persist run status locally.

## Consequences

### Positive
- Contract lock (Phase 0) becomes tractable — each API has exactly one owner.
- Event envelope (ADR-002) can mandate `producing_service` without ambiguity.
- Run state machine (ADR-003) has one writer, enabling invariants.
- `agent-core` shrinks to orchestration only; extracting session/policy/skills
  becomes a mechanical refactor rather than a design debate.

### Negative
- Short-term migration cost: current `agent-core` modules owning session,
  policy, permissions, skills, cost, voice, and Letta must be extracted.
- Cross-service call latency replaces in-process calls; requires careful
  batching and event-driven patterns to avoid chatty RPC.

### Neutral
- Downstream ADRs (event envelope, run state, auth, rate limiting) now have
  a stable foundation to build on.

## Compliance

- All new modules MUST declare ownership in their `README.md` header.
- PRs that add persistence to a non-owning service MUST be rejected.
- CI lint: grep for `CREATE TABLE` or ORM model definitions outside owner
  service — fail build if violated.

## References

- `docs/Future roadmap.md` — Phase 0, Phase 1 (contracts)
- `docs/GAP_ANALYSIS.md` — Gap 1 (contract lock), Gap 3 (session-core missing)
- ADR-002 — Event envelope (depends on this ADR)
- ADR-003 — Run state machine (depends on this ADR)
