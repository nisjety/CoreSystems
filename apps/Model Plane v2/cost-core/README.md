# cost-core v2

Owns cost and analytics for Model Plane v2 (V7 + V9).

- Port: **8006** (external **8106**)
- DB: `cost_core_v2_db` on `reasoning-v2-postgres`
- NATS stream: `VELION_COST`, subjects `velion.cost.>`

## Responsibilities

- **V7 run cost ledger** — `run_costs` table (per-turn cost rows); aggregates to `total_cost_usd` per run.
- **V9 analytics events** — `analytics_events` append-only table.
- **Pricing** — canonical model pricing table (Anthropic / OpenAI / Google).
- **Turn cost tracker** — `CostTracker` + `parse_usage_from_response` (moved verbatim from agent-core).

## Endpoints

- `GET /health`
- `POST /v1/costs/turns` — record a turn usage row
- `GET /v1/costs/runs/{run_id}` — aggregate cost for a run
- `POST /v1/analytics/events` — ingest analytics event
- `GET /v1/analytics/events` — query (by org_id + time range)

## NATS

Subscribes to:
- `velion.cost.turn.recorded` (ADR-002 envelope) — persist + aggregate

Publishes:
- `velion.cost.run.updated`

## Migrations

- `001_run_costs` — standalone cost ledger
- `002_analytics_events` — ported verbatim from legacy `012_analytics_events`

Runs automatically on startup via `schema_migrations` tracker.

## Relationship to agent-core

agent-core now calls cost-core via HTTP (`app/cost_client.py`, `http://cost-core-v2:8006`).
Migrations `016` / `017` in agent-core drop the old `total_cost_usd` column and `analytics_events` table.
