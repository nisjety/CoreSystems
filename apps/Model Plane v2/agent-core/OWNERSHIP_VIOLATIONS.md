# agent-core — ADR-001 Ownership Violations Registry

**Status:** Phase 0 inventory — blocks Phase 1 extraction refactor
**Source of truth:** [`docs/adr/ADR-001-service-ownership-matrix.md`](../docs/adr/ADR-001-service-ownership-matrix.md)
**Related ADRs:** ADR-002 (event envelope), ADR-003 (run state machine)

---

## Purpose

`agent-core` currently owns persistent state that per **ADR-001** belongs to other services. Every row in the migrations tree was audited against the ownership matrix (§Services, 11 entries) and the five Boundary Rules. This registry catalogs each violation so Phase 1 can extract them service-by-service without guesswork.

Boundary Rules recap:

1. **Single writer** — each table has exactly one service that writes to it.
2. **No shadow state** — no caching another service's authoritative data in local tables.
3. **Events over polling** — cross-service awareness via NATS envelopes (ADR-002).
4. **Identifiers cross boundaries** — not rows, not joins.
5. **Run state single-sourced** — `runs.state` owned by the run authority; other services subscribe.

---

## Migration-Level Violations

| # | Migration | Table / Column | Violation | Correct Owner | Phase 1 Action |
|---|-----------|----------------|-----------|---------------|----------------|
| V1 | `001_agent_runs.up.sql` | `agent_runs` (whole table) | Rule 1 + Rule 5 — run state authority | **session-core** (per ADR-001 §Services; ADR-003 scopes writer contract) | Extract to session-core; agent-core keeps orchestration scratch only, publishes `run.state.changed` events (ADR-002). |
| V2 | `004_hooks.up.sql` + `015_hook_type_expansion.up.sql` | `hook_configs` | Rule 1 — hooks are policy/permission triggers | **capability-core** | Move to capability-core; agent-core calls `POST /v1/hooks/evaluate` per tool invocation. |
| V3 | `005_mcp_servers.up.sql` | `mcp_servers` | Rule 1 — capability registry | **capability-core** | Move to capability-core; expose via `GET /v1/capabilities/mcp-servers`. |
| V4 | `006_agent_memory.up.sql` | `agent_memory` | Rule 1 — Letta/memory bridge | **memory-core** | Move to memory-core; delete local table once read path switches. |
| V5 | `007_agent_skills.up.sql` + `014_org_skills.up.sql` | `agent_skills`, `org_skills` | Rule 1 — skills catalog | **capability-core** | Move to capability-core; agent-core loads via capability-client (already stubbed). |
| V6 | `008_mcp_oauth_tokens.up.sql` | `mcp_oauth_tokens` | Rule 1 — OAuth grants are capability-scoped secrets | **capability-core** (grants) + **secrets layer** (token material) | Move grant metadata to capability-core; encrypted token material to KV/secret-core. |
| V7 | `009_run_cost_usd.up.sql` | `agent_runs.total_cost_usd` column | Rule 1 + Rule 2 — cost ledger is cost-core territory; column is a **shadow cache** of cost-core data | **cost-core** | Drop column after Phase 1; agent-core queries `GET /v1/cost/runs/{run_id}` or subscribes to `cost.run.updated`. |
| V8 | `010_org_policy_limits.up.sql` | `org_policy_limits` | Rule 1 — policy bundles | **capability-core** | Move to capability-core; agent-core calls `POST /v1/policy/check`. |
| V9 | `012_analytics_events.up.sql` | `analytics_events` | Rule 1 — general telemetry belongs to observability/cost plane | **cost-core** (billing-relevant) or **observability sink** (non-billing) | Split by `event_type`: cost-relevant → cost-core; operational → NATS + external sink. Delete local table. |
| V10 | `013_agent_trajectories.up.sql` | `agent_trajectories` | **Ambiguous** — hybrid trace + memory + skill-mining artefact | **Decision required** (candidates: agent-core orchestration-private, memory-core for Letta sync, observability sink) | ADR-004 pending: decide owner before Phase 1. Default proposal: **agent-core-private** (orchestration telemetry), with async forward to memory-core via `agent.trajectory.captured` event. |

### Compliant (stays in agent-core)

| Migration | Table | Reason |
|-----------|-------|--------|
| `002_agent_tasks` + `011_task_type` | `agent_tasks` | Orchestration plan/task graph — agent-core core domain. |
| `003_cron_tasks` | `cron_tasks` | Scheduler hand-off lives with orchestrator; no cross-service write path. |

---

## Python Module Hotspots (to re-audit after migrations extracted)

These modules presumably read/write the violating tables. They must be re-pointed at service APIs in Phase 1. Exact tagging deferred — migration-level extraction drives the refactor.

| Module | Suspected boundary crossing | Verify against |
|--------|------------------------------|----------------|
| `app/repository.py`, `app/database.py` | Raw SQL touching `agent_runs`, `agent_skills`, `agent_memory`, `org_policy_limits`, `mcp_*` | V1, V3, V4, V5, V6, V8 |
| `app/capability_client.py` | Should be the **only** path to skills/policy/hooks | V2, V3, V5, V6, V8 |
| `app/cost_tracker.py`, `app/usage_reporter.py`, `app/pricing.py` | Must write via cost-core API, not local column | V7, V9 |
| `app/nats_publisher.py`, `app/events/` | Envelope shape must match ADR-002 | — |
| `app/skills/`, `app/mcp/`, `app/plugins/` | Candidates for thin client wrappers | V3, V5, V6 |
| `app/letta/`, `app/voice/` | Memory bridge must not own storage | V4 |
| `app/permissions/`, `app/policy/`, `app/approvals/` | Decision cache only — authority is capability-core | V2, V8 |
| `app/documents_client.py` | Cross-service client — must not read other services' DB | Rule 4 |

---

## Phase 1 Extraction Order (proposed)

1. **V7 + V9** (cost) — smallest blast radius, pure read path replacement.
2. **V2 + V8** (policy/hooks) — already fronted by `capability_client.py`.
3. **V3 + V5 + V6** (capabilities: MCP, skills, OAuth grants) — bundled capability-core migration.
4. **V4** (memory) — requires memory-core Letta integration first.
5. **V10** decision via ADR-004.
6. **V1** (runs) — largest refactor; gated behind ADR-003 session-core contract and event-first read path.

---

## Compliance Gate

Per ADR-001 §Compliance, CI must fail on `CREATE TABLE` statements outside each service's owner directory. After Phase 1, this file should reach **zero rows in the violations table** and be archived.
