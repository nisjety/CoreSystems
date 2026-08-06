-- ============================================================
--  Data Plane v2 — Postgres Schema
--  Run automatically on first container start
-- ============================================================

CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

-- ── documents ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS documents (
    document_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id        TEXT         NOT NULL,
    source        TEXT         NOT NULL,
    type          TEXT         NOT NULL,
    title         TEXT         NOT NULL,
    content       TEXT         NOT NULL,
    status        TEXT         NOT NULL DEFAULT 'pending',
    metadata      JSONB        NOT NULL DEFAULT '{}',
    error_message TEXT,
    zdr_classification TEXT    NOT NULL DEFAULT 'internal',
    zdr_reason    TEXT,
    extraction_trace  JSONB,
    created_by    TEXT,
    deleted_by    TEXT,
    -- Per-User Data Ownership & Sharing: owner + visibility. Default 'org'
    -- (org-shared) so the platform is non-breaking; Private is an explicit
    -- opt-in. owner_id defaults to the system account for non-API writers.
    owner_id      TEXT         NOT NULL DEFAULT 'org-system-account',
    visibility    TEXT         NOT NULL DEFAULT 'org',
    idempotency_key TEXT,
    -- P2-3: the SOURCE content's own last-modified time (e.g. SharePoint's
    -- lastModifiedDateTime), distinct from updated_at (this row's own
    -- bookkeeping). NULL means unknown, not "old" — see the decay stage.
    document_date TIMESTAMPTZ,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    deleted_at    TIMESTAMPTZ,
    CONSTRAINT documents_visibility_chk CHECK (visibility IN ('private', 'org', 'shared'))
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_idempotency
    ON documents (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_documents_org_id     ON documents (org_id);
CREATE INDEX IF NOT EXISTS idx_documents_status     ON documents (status);
CREATE INDEX IF NOT EXISTS idx_documents_org_status ON documents (org_id, status);
CREATE INDEX IF NOT EXISTS idx_documents_owner      ON documents (org_id, owner_id);
CREATE INDEX IF NOT EXISTS idx_documents_type       ON documents (org_id, type);
CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_crawl_url_dedup
    ON documents (org_id, (metadata->>'url'))
    WHERE source = 'quarry';

-- full-text search on document content
CREATE INDEX IF NOT EXISTS idx_documents_content_fts
    ON documents USING GIN(to_tsvector('simple', content));

-- ── source_objects ──────────────────────────────────────────────────────────
-- Canonical source inventory for connectors such as SharePoint/OneDrive.
-- Search engines consume this as a rebuildable read model; Postgres remains
-- source of truth for object identity, hashes, ACL tags, and sync metadata.

CREATE TABLE IF NOT EXISTS source_objects (
    source_object_id TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id           TEXT         NOT NULL,
    connector        TEXT         NOT NULL,
    source           TEXT         NOT NULL,
    external_id      TEXT         NOT NULL,
    site_id          TEXT,
    drive_id         TEXT,
    item_id          TEXT,
    parent_id        TEXT,
    path             TEXT,
    name             TEXT         NOT NULL,
    mime_type        TEXT,
    size_bytes       BIGINT,
    etag             TEXT,
    ctag             TEXT,
    quickxor_hash    TEXT,
    sha1_hash        TEXT,
    content_hash     TEXT,
    acl_tags         TEXT[]       NOT NULL DEFAULT '{}',
    metadata         JSONB        NOT NULL DEFAULT '{}',
    modified_at      TIMESTAMPTZ,
    discovered_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    deleted_at       TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, connector, external_id)
);

CREATE INDEX IF NOT EXISTS idx_source_objects_org_source
    ON source_objects (org_id, source);
CREATE INDEX IF NOT EXISTS idx_source_objects_drive_item
    ON source_objects (org_id, drive_id, item_id)
    WHERE drive_id IS NOT NULL AND item_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_content_hash
    ON source_objects (org_id, content_hash)
    WHERE content_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_quickxor_hash
    ON source_objects (org_id, quickxor_hash)
    WHERE quickxor_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_sha1_hash
    ON source_objects (org_id, sha1_hash)
    WHERE sha1_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_modified_at
    ON source_objects (org_id, modified_at DESC)
    WHERE modified_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_metadata_gin
    ON source_objects USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_source_objects_acl_tags_gin
    ON source_objects USING GIN (acl_tags);

-- ── knowledge_units ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS knowledge_units (
    knowledge_id      TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    document_id       TEXT         NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    org_id            TEXT         NOT NULL,
    chunk_index       INTEGER      NOT NULL,
    text              TEXT         NOT NULL,
    embedding_status  TEXT         NOT NULL DEFAULT 'pending',
    content_hash      TEXT,
    chunk_version     TEXT         NOT NULL DEFAULT '1',
    parent_chunk_id   TEXT,
    -- P2-2: wider neighboring-chunk context for retrieval-time expansion.
    -- Distinct from parent_chunk_id (re-crawl lineage) above.
    parent_window_text TEXT,
    embedding_model   TEXT,
    metadata          JSONB        NOT NULL DEFAULT '{}',
    error_message     TEXT,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    -- Phase 3 freshness seam: set when vectors are upserted (embedding_status='done').
    embedded_at       TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_ku_document_id      ON knowledge_units (document_id);
CREATE INDEX IF NOT EXISTS idx_ku_embedded_at      ON knowledge_units (embedded_at);
CREATE INDEX IF NOT EXISTS idx_ku_org_id           ON knowledge_units (org_id);
CREATE INDEX IF NOT EXISTS idx_ku_embedding_status ON knowledge_units (embedding_status);
CREATE INDEX IF NOT EXISTS idx_ku_content_hash     ON knowledge_units (content_hash);
CREATE INDEX IF NOT EXISTS idx_ku_text_fts         ON knowledge_units USING GIN(to_tsvector('simple', text));

-- ── document_acl REMOVED (Per-User Data Ownership & Sharing phase) ────────────
-- The dormant, duplicated document_acl table was consolidated into user-core's
-- resource_grants — the single grant authority for the whole platform. It is
-- dropped by migration 20260620120000_drop_document_acl.sql. Do NOT recreate it
-- here: retrieval enforces ownership via the documents.owner_id/visibility
-- columns plus user-core's resource_grants, never a Data-Plane-local ACL copy.

-- ── retrieval_runs (trace audit) ─────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS retrieval_runs (
    trace_id              TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id                TEXT         NOT NULL,
    actor_user_id         TEXT,
    query                 TEXT         NOT NULL,
    query_embedding_model TEXT,
    index_version         TEXT,
    filters_json          JSONB,
    reranker_name         TEXT,
    reranker_model        TEXT,
    zdr_mode              TEXT,
    top_k                 INTEGER,
    dense_retrieval_ms    INTEGER,
    sparse_retrieval_ms   INTEGER,
    rerank_ms             INTEGER,
    total_ms              INTEGER,
    candidate_count_dense   INTEGER,
    candidate_count_sparse  INTEGER,
    candidate_count_fused   INTEGER,
    candidate_count_reranked INTEGER,
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_retrieval_runs_org    ON retrieval_runs (org_id);
CREATE INDEX IF NOT EXISTS idx_retrieval_runs_time   ON retrieval_runs (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_retrieval_runs_zdr    ON retrieval_runs (org_id, zdr_mode);
CREATE INDEX IF NOT EXISTS idx_retrieval_runs_actor  ON retrieval_runs (org_id, actor_user_id, created_at DESC);

-- ── retrieval_candidates ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS retrieval_candidates (
    id               BIGSERIAL    PRIMARY KEY,
    trace_id         TEXT         NOT NULL REFERENCES retrieval_runs(trace_id) ON DELETE CASCADE,
    rank             INTEGER      NOT NULL,
    knowledge_id     TEXT,
    document_id      TEXT,
    dense_score      FLOAT,
    sparse_score     FLOAT,
    rerank_score     FLOAT,
    final_score      FLOAT,
    source_chunk_ref TEXT,
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rc_trace  ON retrieval_candidates (trace_id);
CREATE INDEX IF NOT EXISTS idx_rc_rank   ON retrieval_candidates (trace_id, rank);

-- ── graph_entities ───────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_entities (
    entity_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id      TEXT         NOT NULL,
    entity_type TEXT         NOT NULL,
    entity_text TEXT         NOT NULL,
    confidence  FLOAT,
    provenance  TEXT,
    source_refs JSONB,
    metadata    JSONB,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ge_org  ON graph_entities (org_id);
CREATE INDEX IF NOT EXISTS idx_ge_type ON graph_entities (org_id, entity_type);
CREATE INDEX IF NOT EXISTS idx_ge_text ON graph_entities USING GIN(to_tsvector('simple', entity_text));

-- ── graph_relationships ──────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_relationships (
    rel_id        TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id        TEXT         NOT NULL,
    entity_a_id   TEXT         NOT NULL REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
    entity_b_id   TEXT         NOT NULL REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
    relation_type TEXT         NOT NULL,
    confidence    FLOAT,
    provenance    TEXT,
    source_refs   JSONB,
    metadata      JSONB,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gr_org    ON graph_relationships (org_id);
CREATE INDEX IF NOT EXISTS idx_gr_a      ON graph_relationships (entity_a_id);
CREATE INDEX IF NOT EXISTS idx_gr_b      ON graph_relationships (entity_b_id);
CREATE INDEX IF NOT EXISTS idx_gr_type   ON graph_relationships (org_id, relation_type);

-- ── graph_claims ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_claims (
    claim_id                TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id                  TEXT         NOT NULL,
    claim_text              TEXT         NOT NULL,
    entity_ids              JSONB,
    confidence              FLOAT,
    provenance              TEXT,
    source_refs             JSONB,
    contradicted_by_claim_ids JSONB,
    claim_status            TEXT         DEFAULT 'active',
    metadata                JSONB,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gc_org    ON graph_claims (org_id);
CREATE INDEX IF NOT EXISTS idx_gc_status ON graph_claims (org_id, claim_status);
CREATE INDEX IF NOT EXISTS idx_gc_text   ON graph_claims USING GIN(to_tsvector('simple', claim_text));

-- ── graph_communities ────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_communities (
    community_id TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id       TEXT         NOT NULL,
    entity_ids   JSONB        NOT NULL,
    summary      TEXT,
    level        INTEGER      NOT NULL DEFAULT 0,
    metadata     JSONB,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gcomm_org ON graph_communities (org_id);

-- ── wiki_pages ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_pages (
    page_id            TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id             TEXT         NOT NULL,
    workspace_id       TEXT         NOT NULL,
    title              TEXT         NOT NULL,
    path               TEXT         NOT NULL,
    current_version_id TEXT,
    page_status        TEXT         DEFAULT 'draft',
    backlinks          JSONB,
    metadata           JSONB,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    deleted_at         TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wp_org       ON wiki_pages (org_id);
CREATE INDEX IF NOT EXISTS idx_wp_workspace ON wiki_pages (org_id, workspace_id);
CREATE INDEX IF NOT EXISTS idx_wp_path      ON wiki_pages (org_id, path);
CREATE INDEX IF NOT EXISTS idx_wp_status    ON wiki_pages (org_id, page_status);

-- ── wiki_page_versions ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_page_versions (
    version_id       TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    page_id          TEXT         NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
    content          TEXT,
    source_refs      JSONB,
    proposed_by_agent TEXT,
    proposed_by_user  TEXT,
    approved_by       Text,
    edit_reason       TEXT,
    version_status    TEXT         DEFAULT 'draft',
    metadata          JSONB,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    published_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wpv_page    ON wiki_page_versions (page_id);
CREATE INDEX IF NOT EXISTS idx_wpv_created ON wiki_page_versions (page_id, created_at DESC);

-- ── wiki_source_logs ─────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_source_logs (
    log_id              TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    page_id             TEXT         NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
    org_id              TEXT         NOT NULL,
    source_type         TEXT         NOT NULL,
    source_ref          TEXT         NOT NULL,
    sync_status         TEXT         NOT NULL DEFAULT 'synced',
    details             JSONB        NOT NULL DEFAULT '{}'::JSONB,
    -- Legacy synthesis provenance remains readable during contract migration.
    original_chunks     JSONB,
    processing_model    TEXT,
    synthesis_prompt_hash TEXT,
    metadata            JSONB,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_wsl_page ON wiki_source_logs (page_id);

-- ── wiki_maintenance_logs ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_maintenance_logs (
    log_id       TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    page_id      Text         NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
    org_id       TEXT         NOT NULL,
    action       TEXT         NOT NULL,
    actor        TEXT         NOT NULL,
    details      JSONB        NOT NULL DEFAULT '{}'::JSONB,
    kind         TEXT,
    detected_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    -- Legacy issue workflow remains available to existing readers.
    issue_type   TEXT,
    issue_details JSONB,
    proposed_fix  TEXT,
    issue_status  TEXT         DEFAULT 'open',
    metadata      JSONB,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    resolved_at   TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wml_page   ON wiki_maintenance_logs (page_id);
CREATE INDEX IF NOT EXISTS idx_wml_status ON wiki_maintenance_logs (page_id, issue_status);
CREATE INDEX IF NOT EXISTS idx_wsl_org_page_created ON wiki_source_logs (org_id, page_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_wml_org_page_created ON wiki_maintenance_logs (org_id, page_id, created_at DESC);

CREATE OR REPLACE FUNCTION sync_wiki_source_log_contract()
RETURNS TRIGGER AS $$
BEGIN
    SELECT page.org_id INTO NEW.org_id FROM wiki_pages AS page WHERE page.page_id = NEW.page_id;
    NEW.source_type := COALESCE(NULLIF(NEW.source_type, ''), 'legacy');
    NEW.source_ref := COALESCE(NULLIF(NEW.source_ref, ''), NULLIF(NEW.synthesis_prompt_hash, ''), 'legacy:' || NEW.log_id);
    NEW.sync_status := COALESCE(NULLIF(NEW.sync_status, ''), 'synced');
    NEW.details := COALESCE(NEW.details, NEW.metadata, '{}'::JSONB);
    NEW.synthesis_prompt_hash := COALESCE(NEW.synthesis_prompt_hash, NEW.source_ref);
    NEW.metadata := COALESCE(NEW.metadata, NEW.details);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$ BEGIN
    CREATE TRIGGER sync_wiki_source_log_contract_before_write
    BEFORE INSERT OR UPDATE ON wiki_source_logs
    FOR EACH ROW EXECUTE FUNCTION sync_wiki_source_log_contract();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION sync_wiki_maintenance_log_contract()
RETURNS TRIGGER AS $$
BEGIN
    SELECT page.org_id INTO NEW.org_id FROM wiki_pages AS page WHERE page.page_id = NEW.page_id;
    NEW.action := COALESCE(NULLIF(NEW.action, ''), NULLIF(NEW.kind, ''), NULLIF(NEW.issue_type, ''), 'legacy');
    NEW.actor := COALESCE(NULLIF(NEW.actor, ''), 'legacy');
    NEW.details := COALESCE(NEW.details, NEW.issue_details, NEW.metadata, '{}'::JSONB);
    NEW.kind := COALESCE(NEW.kind, NEW.action);
    NEW.issue_type := COALESCE(NEW.issue_type, NEW.action);
    NEW.issue_details := COALESCE(NEW.issue_details, NEW.details);
    NEW.metadata := COALESCE(NEW.metadata, NEW.details);
    NEW.detected_at := COALESCE(NEW.detected_at, NEW.created_at, NOW());
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$ BEGIN
    CREATE TRIGGER sync_wiki_maintenance_log_contract_before_write
    BEFORE INSERT OR UPDATE ON wiki_maintenance_logs
    FOR EACH ROW EXECUTE FUNCTION sync_wiki_maintenance_log_contract();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- ── wiki_proposals ───────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_proposals (
    proposal_id       TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    page_id           TEXT         NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
    org_id            TEXT         NOT NULL,
    proposed_content  TEXT         NOT NULL,
    edit_reason       TEXT,
    proposed_by_agent TEXT,
    source_refs       JSONB,
    proposal_status   TEXT         DEFAULT 'pending',
    reviewed_by       TEXT,
    metadata          JSONB,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_wprop_page ON wiki_proposals (page_id);
CREATE INDEX IF NOT EXISTS idx_wprop_status ON wiki_proposals (org_id, proposal_status);

-- ── graph_text_units (maps graph objects → knowledge units) ──────────────────

CREATE TABLE IF NOT EXISTS graph_text_units (
    id              BIGSERIAL    PRIMARY KEY,
    org_id          TEXT         NOT NULL,
    knowledge_id    TEXT         NOT NULL REFERENCES knowledge_units(knowledge_id) ON DELETE CASCADE,
    entity_id       TEXT         REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
    rel_id          TEXT         REFERENCES graph_relationships(rel_id) ON DELETE CASCADE,
    claim_id        TEXT         REFERENCES graph_claims(claim_id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_graph_text_unit_one CHECK (
        (entity_id IS NOT NULL)::int + (rel_id IS NOT NULL)::int + (claim_id IS NOT NULL)::int = 1
    )
);

CREATE INDEX IF NOT EXISTS idx_gtu_org           ON graph_text_units (org_id);
CREATE INDEX IF NOT EXISTS idx_gtu_knowledge_id  ON graph_text_units (knowledge_id);
CREATE INDEX IF NOT EXISTS idx_gtu_entity_id     ON graph_text_units (entity_id) WHERE entity_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_gtu_rel_id        ON graph_text_units (rel_id) WHERE rel_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_gtu_claim_id      ON graph_text_units (claim_id) WHERE claim_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_gtu_dedup
    ON graph_text_units (org_id, knowledge_id, COALESCE(entity_id, ''), COALESCE(rel_id, ''), COALESCE(claim_id, ''));

-- ── chunk_lineage (old → new chunk mapping on reindex) ──────────────────────

CREATE TABLE IF NOT EXISTS chunk_lineage (
    id                 BIGSERIAL    PRIMARY KEY,
    document_id        TEXT         NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    old_knowledge_id   TEXT         NOT NULL,
    new_knowledge_id   TEXT         NOT NULL,
    old_chunk_index    INTEGER,
    old_content_hash   TEXT,
    reason             TEXT         NOT NULL DEFAULT 'reindex',
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_cl_document   ON chunk_lineage (document_id);
CREATE INDEX IF NOT EXISTS idx_cl_old_kid    ON chunk_lineage (old_knowledge_id);
CREATE INDEX IF NOT EXISTS idx_cl_new_kid    ON chunk_lineage (new_knowledge_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cl_dedup
    ON chunk_lineage (document_id, old_knowledge_id, new_knowledge_id);

-- ── org_quotas ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS org_quotas (
    org_id              TEXT         PRIMARY KEY,
    plan_tier           TEXT         NOT NULL DEFAULT 'free',
    documents_limit     INTEGER      NOT NULL DEFAULT 100,
    api_calls_per_month INTEGER      NOT NULL DEFAULT 10000,
    storage_gb          DECIMAL      NOT NULL DEFAULT 1.0,
    concurrent_users    INTEGER      NOT NULL DEFAULT 1,
    custom_models       BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- ── data_plane_audit_log ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS data_plane_audit_log (
    id             BIGSERIAL    PRIMARY KEY,
    user_id        TEXT         NOT NULL,
    org_id         TEXT         NOT NULL,
    action         TEXT         NOT NULL,
    resource_type  TEXT         NOT NULL,
    resource_id    TEXT,
    ip_address     TEXT,
    user_agent     TEXT,
    details        TEXT,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_user_id    ON data_plane_audit_log (user_id);
CREATE INDEX IF NOT EXISTS idx_audit_org_id     ON data_plane_audit_log (org_id);
CREATE INDEX IF NOT EXISTS idx_audit_created_at ON data_plane_audit_log (created_at);

-- ── index_versions (for replayable retrieval) ────────────────────────────────

CREATE TABLE IF NOT EXISTS index_versions (
    version_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id       TEXT         NOT NULL,
    description  TEXT,
    document_count INTEGER,
    chunk_count    INTEGER,
    embedding_model TEXT,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iv_org ON index_versions (org_id);

-- ── auto-update triggers ─────────────────────────────────────────────────────

CREATE OR REPLACE FUNCTION trigger_set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$ BEGIN
    CREATE TRIGGER set_documents_updated_at BEFORE UPDATE ON documents
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TRIGGER set_source_objects_updated_at BEFORE UPDATE ON source_objects
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TRIGGER set_knowledge_units_updated_at BEFORE UPDATE ON knowledge_units
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TRIGGER set_wiki_pages_updated_at BEFORE UPDATE ON wiki_pages
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TRIGGER set_org_quotas_updated_at BEFORE UPDATE ON org_quotas
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- ── cost_events (consumed from NATS dataplane.cost.ledger) ──────────────────

CREATE TABLE IF NOT EXISTS cost_events (
    id                BIGSERIAL    PRIMARY KEY,
    event_type        TEXT         NOT NULL,
    model             TEXT         NOT NULL,
    org_id            TEXT         NOT NULL,
    count             INTEGER      NOT NULL DEFAULT 0,
    estimated_tokens  BIGINT       NOT NULL DEFAULT 0,
    idempotency_key   TEXT,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_cost_events_idempotency
    ON cost_events (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_cost_events_org_created
    ON cost_events (org_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cost_events_type_created
    ON cost_events (event_type, created_at DESC);

-- ── graph_exports (D4 + D5 spec §2.1) ─────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_exports (
    export_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id      TEXT         NOT NULL,
    format      TEXT         NOT NULL CHECK (format IN ('json','graphml','html','markdown')),
    uri         TEXT         NOT NULL,
    bytes       BIGINT,
    sha256      TEXT,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_graph_exports_org_created
    ON graph_exports (org_id, created_at DESC);

-- ── retrieval_runs.mode_mix (D4 + D5 spec §7) ─────────────────────────────
-- Defensive: column may not exist if init.sql was loaded by an older snapshot.
ALTER TABLE retrieval_runs ADD COLUMN IF NOT EXISTS mode_mix JSONB;

-- ── access_audit_log (Wave 3 §15-E) ────────────────────────────────────────

CREATE TABLE IF NOT EXISTS access_audit_log (
    id                BIGSERIAL    PRIMARY KEY,
    request_id        TEXT         NOT NULL,
    user_id           TEXT,
    org_id            TEXT         NOT NULL,
    endpoint          TEXT         NOT NULL,
    http_status       INTEGER      NOT NULL,
    latency_ms        INTEGER      NOT NULL DEFAULT 0,
    auth_method       TEXT         NOT NULL,
    document_ids      TEXT[]       NOT NULL DEFAULT '{}',
    cause             TEXT,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_access_audit_org_created
    ON access_audit_log (org_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_access_audit_user_created
    ON access_audit_log (user_id, created_at DESC)
    WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_access_audit_cause
    ON access_audit_log (cause)
    WHERE cause <> 'ok';

-- Wave-3.1 §15-F — user-attributed cost ledger.
ALTER TABLE cost_events ADD COLUMN IF NOT EXISTS user_id TEXT;
CREATE INDEX IF NOT EXISTS idx_cost_events_user_created
    ON cost_events (user_id, created_at DESC)
    WHERE user_id IS NOT NULL;

-- Wave-3.2 §16.3.7 — GIN indexes on existing JSONB columns
CREATE INDEX IF NOT EXISTS idx_documents_extraction_trace_gin
    ON documents USING GIN (extraction_trace);
CREATE INDEX IF NOT EXISTS idx_documents_metadata_gin
    ON documents USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_retrieval_runs_filters_gin
    ON retrieval_runs USING GIN (filters_json);
CREATE INDEX IF NOT EXISTS idx_retrieval_runs_mode_mix_gin
    ON retrieval_runs USING GIN (mode_mix);

-- Wave-3.2 §16.1.3 — zdr_actions_applied column
ALTER TABLE retrieval_runs ADD COLUMN IF NOT EXISTS zdr_actions_applied JSONB;

-- Wave-3.2 §16.5.3 — admin audit log
CREATE TABLE IF NOT EXISTS admin_audit_log (
    audit_id    BIGSERIAL    PRIMARY KEY,
    org_id      TEXT,
    actor       TEXT         NOT NULL,
    action      TEXT         NOT NULL,
    target_kind TEXT,
    target_id   TEXT,
    request_id  TEXT,
    payload     JSONB,
    outcome     TEXT         NOT NULL DEFAULT 'ok',
    error       TEXT,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_admin_audit_log_time     ON admin_audit_log (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_actor    ON admin_audit_log (actor, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_action   ON admin_audit_log (action, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_org
    ON admin_audit_log (org_id, created_at DESC)
    WHERE org_id IS NOT NULL;

-- Wave-3.3 §16.3.3 — precomputed BM25 tsvector + GIN index
ALTER TABLE knowledge_units
    ADD COLUMN IF NOT EXISTS content_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', text)) STORED;
CREATE INDEX IF NOT EXISTS idx_ku_content_tsv_gin
    ON knowledge_units USING GIN (content_tsv);

-- Wave-3.5 §16.1.1 — mode_mix_applied
ALTER TABLE retrieval_runs ADD COLUMN IF NOT EXISTS mode_mix_applied JSONB;

-- Wave-3.5 §16.1.4 — per-agent retrieval config
CREATE TABLE IF NOT EXISTS agent_retrieval_configs (
    config_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id      TEXT         NOT NULL,
    agent_id    TEXT         NOT NULL,
    weights     JSONB        NOT NULL,
    rerank      BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_retrieval_configs_org_agent
    ON agent_retrieval_configs (org_id, agent_id);

-- Wave-3.5 §16.2.2 — per-org cache versioning
CREATE TABLE IF NOT EXISTS org_versions (
    org_id      TEXT         PRIMARY KEY,
    version     BIGINT       NOT NULL DEFAULT 1,
    bumped_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Wave-3.5 §16.2.6 — transactional bulk ingest outbox
CREATE TABLE IF NOT EXISTS documents_outbox (
    outbox_id    BIGSERIAL    PRIMARY KEY,
    org_id       TEXT         NOT NULL,
    event_type   TEXT         NOT NULL,
    payload      JSONB        NOT NULL,
    published    BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_documents_outbox_unpublished
    ON documents_outbox (created_at)
    WHERE published = FALSE;

-- CAG — pinned permanent-memory context. Org-scoped facts preloaded into
-- context packs (and served via /v1/context/preload) WITHOUT a retrieval loop.
CREATE TABLE IF NOT EXISTS context_pins (
    pin_id      TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id      TEXT         NOT NULL,
    title       TEXT         NOT NULL DEFAULT '',
    content     TEXT         NOT NULL,
    priority    INTEGER      NOT NULL DEFAULT 100,
    pinned_by   TEXT,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_context_pins_org_priority
    ON context_pins (org_id, priority, created_at);

-- Eval golden sets — human/agent-judged relevant ids per query, so retrieval
-- evals compute REAL recall@10/nDCG@10/MRR instead of candidate-count proxies.
CREATE TABLE IF NOT EXISTS eval_golden_judgments (
    org_id       TEXT         NOT NULL,
    query_norm   TEXT         NOT NULL,
    relevant_ids JSONB        NOT NULL,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, query_norm)
);
