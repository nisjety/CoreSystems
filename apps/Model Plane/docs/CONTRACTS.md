# Model Plane Contracts

## Canonical Identifiers

All IDs use ULID format (26-character Crockford Base32). Immutable after creation.

| ID Type | Owner | Format | Example |
|---------|-------|--------|---------|
| agent_id | capability-core | ULID | `01HXYZ01234567890ABCDEFGHI` |
| thread_id | session-core | ULID | `01HTHR01234567890ABCDEFGHI` |
| session_key | session-core | ULID | `01HSES01234567890ABCDEFGHI` |
| run_id | session-core | ULID | `01HRUN01234567890ABCDEFGHI` |
| step_id | execution-core | ULID | `01HSTP01234567890ABCDEFGHI` |
| checkpoint_id | session-core | ULID | `01HCHK01234567890ABCDEFGHI` |
| sandbox_lease_id | sandbox-manager | ULID | `01HSBX01234567890ABCDEFGHI` |
| browser_lease_id | browser-broker | ULID | `01HBRW01234567890ABCDEFGHI` |

## Lifecycle Event Taxonomy

All events use the `Event` proto envelope with `EventType` discriminator.

| Event | Enum Value | Producer | Description |
|-------|-----------|----------|-------------|
| SESSION_START | 1 | session-core | New user interaction session begins |
| SESSION_END | 2 | session-core | All runs resolved, session terminates |
| INSTRUCTIONS_LOADED | 10 | session-core | System instructions loaded into context |
| USER_PROMPT_SUBMIT | 11 | model-gateway | User prompt accepted and routed |
| PRE_TOOL_USE | 20 | execution-core | Before tool invocation (hook point) |
| POST_TOOL_USE | 21 | execution-core | After successful tool invocation |
| POST_TOOL_USE_FAILURE | 22 | execution-core | Tool invocation failed |
| PERMISSION_REQUEST | 30 | execution-core | Tool requires user/policy approval |
| SUBAGENT_START | 40 | execution-core | Parent delegates to subagent |
| SUBAGENT_STOP | 41 | execution-core | Subagent run completes |
| TASK_CREATED | 50 | execution-core | New task in execution graph |
| TASK_COMPLETED | 51 | execution-core | Task in execution graph completes |
| PRE_COMPACT | 60 | session-core | Before context compaction |
| POST_COMPACT | 61 | session-core | After context compaction |
| STOP | 70 | execution-core | Run terminates normally |
| STOP_FAILURE | 71 | execution-core | Run terminates with error |
| INGRESS_ACCEPTED | 80 | model-gateway | Request accepted at gateway |
| INGRESS_REJECTED | 81 | model-gateway | Request rejected at gateway |
| RUN_STARTED | 90 | session-core | Run begins execution |
| RUN_COMPLETED | 91 | session-core | Run completes successfully |
| RUN_FAILED | 92 | session-core | Run fails |
| ACTION_STARTED | 100 | execution-core | Single action begins |
| ACTION_COMPLETED | 101 | execution-core | Single action completes |
| CHECKPOINT_SAVED | 110 | session-core | Checkpoint persisted |
| CHECKPOINT_RESTORED | 111 | session-core | Run restored from checkpoint |
| PLAN_CREATED | 120 | execution-core | Plan proposed, awaiting approval |
| APPROVAL_REQUESTED | 121 | execution-core | Approval gate opened |
| PLAN_TRANSITIONED | 122 | orchestrator-core | Plan state transition (draft/proposed/approved/rejected/executing/completed/failed/superseded/archived) |
| APPROVAL_DECIDED | 123 | session-core | Approval granted, denied, or timed out |
| TODO_CREATED | 124 | session-core | Todo item created on a plan |
| TODO_TRANSITIONED | 125 | session-core | Todo state transition (pending/in_progress/blocked/completed/cancelled) |
| SUBAGENT_ATTACHED | 126 | session-core | Subagent attached to plan or step (coder/reviewer/researcher/explorer/generic) |
| SUBAGENT_STOPPED | 127 | session-core | Subagent lifecycle ended |
| RUN_PAUSED_FOR_APPROVAL | 128 | orchestrator-core | Run paused pending approval decision |
| RUN_RESUMED_AFTER_APPROVAL | 129 | orchestrator-core | Run resumed after approval resolved |
| RECOVERY_ATTEMPTED | 130 | execution-core | Recovery loop triggered |

## Idempotency Strategy

Hash: `blake3(producer | event_type | resource_ref | idempotency_key)`

- Command ingestion: deduplicate by idempotency hash in NATS consumer.
- Run creation: deduplicate by session_key + goal hash.
- Tool completion: deduplicate by step_id + action_id.
- Workflow transitions: Temporal handles natively.
- Event storage: unique constraint on event_id in events table.

## NATS Subject Tree

New namespace: `mp.v1.*`

| Subject Pattern | Publisher | Consumer | Status |
|----------------|-----------|----------|--------|
| `mp.v1.run.{run_id}.event` | session-core, execution-core | orchestrator-core, frontend | ✅ live |
| `mp.v1.session.{session_key}.command` | model-gateway | session-core | ✅ live |
| `mp.v1.ingress.{kind}` | model-gateway | telemetry, billing | ✅ live |
| `mp.v1.orchestration.todo` | orchestrator-core (target) | indexers, frontend | 🟡 constant defined in `mp_events::subjects::SUBJECT_TODO`; producers pending § orchestration handlers fill-in |
| `mp.v1.orchestration.plan` | orchestrator-core (target) | indexers, frontend | ❌ planned |
| `mp.v1.orchestration.approval` | orchestrator-core (target) | indexers, frontend | ❌ planned |
| `mp.v1.orchestration.subagent` | execution-core (target) | orchestrator-core | ❌ planned |
| `mp.v1.ai.{modality}.{kind}` | ai-core (future) | telemetry, billing | ❌ planned (Phase 5) |
| `mp.v1.tasks.*` | task-core (future) | coordinator, frontend | ❌ planned (Phase 4) |
| `mp.v1.cron.*` | task-core (future) | coordinator | ❌ planned (Phase 4) |
| `mp.v1.memory.*` | memory-core (future) | indexers | ❌ planned (Phase 7) |
| `mp.v1.knowledge.*` | memory-core (future) | indexers, frontend | ❌ planned (Phase 7) |

Legacy (compatibility adapter active in dev):

| Legacy Subject | New Subject |
|---------------|-------------|
| `verevon.agent.run.{run_id}.event` | `mp.v1.run.{run_id}.event` |
| `verevon.session.{id}.command` | `mp.v1.session.{id}.command` |
| `aqencia.reasoning.reasoning.started` | `mp.v1.ingress.run_started_compat` |
| `aqencia.reasoning.reasoning.completed` | `mp.v1.ingress.run_completed_compat` |
| `aqencia.reasoning.usage.recorded` | `mp.v1.ingress.usage` |
| `aqencia.reasoning.decision.made` | `mp.v1.ingress.decision` |
| `aqencia.reasoning.quota.exceeded` | `mp.v1.ingress.quota_exceeded` |

## MinIO Object Key Conventions

```
org/{org_id}/thread/{thread_id}/run/{run_id}/artifact/{artifact_id}
org/{org_id}/thread/{thread_id}/run/{run_id}/checkpoint/{checkpoint_id}
```

## Storage Split

| Concern | Store | Reason | Status |
|---------|-------|--------|--------|
| Thread/run/checkpoint metadata | Postgres | Transactional, queryable, event replay | ✅ live |
| Plan / todo / approval / subagent-lineage / run-event log | Postgres (via `session-core/orchestration_store.rs`) | Transactional, queryable, replay parity | 🟡 module scaffolded, migrations + handlers pending |
| Capability registry (tools, skills, commands, plugins, MCP, models, routing, memory adapters, safety) | Postgres (target) | Authoritative, scoped, audited | ❌ in-memory only today |
| Artifact bytes | MinIO | Large objects, streaming, versioning | ❌ bucket not provisioned |
| Hot cache (context, leases, idempotency dedup, rate-limit state, inference cache) | Redis | Low-latency reads, TTL expiry | ❌ not integrated; in-process today |
| Workflow state | Temporal | Durable execution, compensation | ✅ live |
| Memory blocks (graph + wiki + provenance) | Postgres + object store (target) | Append-only with provenance | ❌ in-memory `letta-bridge/memstore` only |

See gap-analysis.md § 13 for the canonical stub-replacement inventory.
