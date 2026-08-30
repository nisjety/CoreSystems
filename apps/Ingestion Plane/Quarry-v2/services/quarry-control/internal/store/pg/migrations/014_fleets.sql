-- W5 — Fleet durable envelope for budget-coordinated agent batches.
--
-- The edge's `fleet_routes` keeps an in-process `FleetRegistry` for dev/single-node.
-- Production wires this table so fleet creation, member fan-out, and budget
-- aggregation survive edge restarts. The orchestrator's `FleetOrchestrator`
-- Temporal workflow is the writer; the edge is the reader for `GET /v1/fleets/*`
-- and `GET /v1/fleets/:id/budget` (which re-derives spend from the receipt
-- cost_micro_usd sum if this table's spend drifts).
--
-- No new event subject is needed — the existing `quarry.fleet.<fleet_id>.`
-- wildcard is a direct `events` row with type='fleet.*' using the same
-- idempotency_key contract as every other producer.

CREATE TABLE IF NOT EXISTS quarry_fleets (
    fleet_id              TEXT PRIMARY KEY,
    org_id                TEXT NOT NULL,
    budget_micro_usd      BIGINT,
    max_parallel_runs     INT NOT NULL CHECK (max_parallel_runs >= 1),
    shared_profile_id     TEXT,
    shared_domain_intel_id TEXT,
    status                TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','running','completed','failed','cancelled')),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at           TIMESTAMPTZ,
    member_run_ids        JSONB NOT NULL DEFAULT '[]'::jsonb,
    spent_micro_usd       BIGINT NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS quarry_fleets_org_id_idx
    ON quarry_fleets (org_id, created_at DESC);

-- Member-run membership is queryable via the JSONB array, but the
-- orchestrator's in-memory `Registry.AddMember` is the hot path; the
-- index below lets the edge's `GET /v1/fleets/:id` verify ownership
-- without scanning every fleet for the tenant.
CREATE INDEX IF NOT EXISTS quarry_fleets_member_gin
    ON quarry_fleets USING GIN (member_run_ids);
