CREATE TABLE IF NOT EXISTS operating_maps (
    operating_map_id  TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id            TEXT         NOT NULL UNIQUE,
    map_status        TEXT         NOT NULL DEFAULT 'draft',
    current_version_id TEXT,
    generated_from    JSONB        NOT NULL DEFAULT '{}',
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS operating_map_versions (
    version_id         TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    operating_map_id   TEXT         NOT NULL REFERENCES operating_maps(operating_map_id) ON DELETE CASCADE,
    org_id             TEXT         NOT NULL,
    departments        JSONB        NOT NULL DEFAULT '[]',
    workflows          JSONB        NOT NULL DEFAULT '[]',
    agent_blueprints   JSONB        NOT NULL DEFAULT '[]',
    rollout_phases     JSONB        NOT NULL DEFAULT '[]',
    risk_overlays      JSONB        NOT NULL DEFAULT '[]',
    learning_modules   JSONB        NOT NULL DEFAULT '[]',
    roi_notes          JSONB        NOT NULL DEFAULT '[]',
    evidence_refs      JSONB        NOT NULL DEFAULT '[]',
    confidence         DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    created_by_run_id  TEXT,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS operating_map_proposals (
    proposal_id        TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    operating_map_id   TEXT         NOT NULL REFERENCES operating_maps(operating_map_id) ON DELETE CASCADE,
    org_id             TEXT         NOT NULL,
    proposed_version   JSONB        NOT NULL,
    evidence_refs      JSONB        NOT NULL DEFAULT '[]',
    generated_by_run_id TEXT,
    proposal_status    TEXT         NOT NULL DEFAULT 'pending',
    reviewed_by        TEXT,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    reviewed_at        TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS operating_map_blueprint_suggestions (
    suggestion_id      TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    operating_map_id   TEXT         NOT NULL REFERENCES operating_maps(operating_map_id) ON DELETE CASCADE,
    version_id         TEXT         NOT NULL REFERENCES operating_map_versions(version_id) ON DELETE CASCADE,
    org_id             TEXT         NOT NULL,
    blueprint_id       TEXT         NOT NULL,
    role               TEXT         NOT NULL,
    source_workflow_id TEXT,
    name               TEXT         NOT NULL,
    suggestion_status  TEXT         NOT NULL DEFAULT 'suggested',
    requested_by       TEXT,
    payload            JSONB        NOT NULL DEFAULT '{}',
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, version_id, blueprint_id)
);

CREATE INDEX IF NOT EXISTS idx_operating_map_versions_map
    ON operating_map_versions (operating_map_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operating_map_versions_org
    ON operating_map_versions (org_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operating_map_proposals_map
    ON operating_map_proposals (operating_map_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operating_map_proposals_status
    ON operating_map_proposals (org_id, proposal_status);
CREATE INDEX IF NOT EXISTS idx_operating_map_versions_departments_gin
    ON operating_map_versions USING GIN (departments);
CREATE INDEX IF NOT EXISTS idx_operating_map_versions_workflows_gin
    ON operating_map_versions USING GIN (workflows);
CREATE INDEX IF NOT EXISTS idx_operating_map_blueprint_suggestions_map
    ON operating_map_blueprint_suggestions (operating_map_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operating_map_blueprint_suggestions_status
    ON operating_map_blueprint_suggestions (org_id, suggestion_status);

DO $$ BEGIN
    CREATE TRIGGER set_operating_maps_updated_at BEFORE UPDATE ON operating_maps
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TRIGGER set_operating_map_blueprint_suggestions_updated_at BEFORE UPDATE ON operating_map_blueprint_suggestions
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

COMMENT ON TABLE operating_maps IS
    'Org-scoped Verevon AI Operating Map root owned by Data Plane wiki-store.';
COMMENT ON TABLE operating_map_versions IS
    'Accepted, evidence-referenced Operating Map versions that can be mirrored into wiki knowledge.';
COMMENT ON TABLE operating_map_proposals IS
    'Generated Operating Map proposals awaiting human review.';
COMMENT ON TABLE operating_map_blueprint_suggestions IS
    'Version-scoped agent blueprint suggestions created from approved Operating Map workflow opportunities.';
