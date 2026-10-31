-- visma_service_v2 — schema bootstrap
-- Separate database from v1. Owner: visma_v2.

CREATE SCHEMA IF NOT EXISTS vsv2;

CREATE TABLE IF NOT EXISTS vsv2.tenant_company_map (
    tenant_id        TEXT PRIMARY KEY,
    company_id       TEXT NOT NULL,
    company_name     TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Aquatiq AS — Phase 0 fills company_id once introspected.
INSERT INTO vsv2.tenant_company_map (tenant_id, company_id, company_name)
VALUES (
    '33f319b9-4d57-11ee-960c-025417856183',
    'PENDING_PHASE_0_DISCOVERY',
    'Aquatiq AS'
)
ON CONFLICT (tenant_id) DO NOTHING;
