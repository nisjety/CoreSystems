-- Wave-3.5 batch:
--   1. §16.1.1  retrieval_runs.mode_mix_applied — distinct from mode_mix.
--              `mode_mix` records the WEIGHTS THE CLIENT REQUESTED.
--              `mode_mix_applied` records WHAT THE SCORING ACTUALLY USED.
--              Until the 4-way fold-in ships, mode_mix_applied stays
--              `{"w_dense":<x>,"w_bm25":<y>,"w_graph":0,"w_wiki":0}` and
--              the audit endpoint surfaces both so consumers can reconcile.
--   2. §16.1.4  agent_retrieval_configs table — per-(org, agent) blend
--              weights so Model Plane can attach an `X-Agent-Retrieval-Config`
--              header or just `agent_id` and the engine looks up the row.
--   3. §16.2.2  org_versions table — bumped on any mutation; included in
--              cache key so in-flight retrievals don't read stale cache.

-- ── §16.1.1  mode_mix_applied ───────────────────────────────────────────────
ALTER TABLE retrieval_runs
    ADD COLUMN IF NOT EXISTS mode_mix_applied JSONB;

COMMENT ON COLUMN retrieval_runs.mode_mix_applied IS
    'Blend weights that scoring actually used (vs `mode_mix` = requested). '
    'When the 4-way fold-in lands, this and `mode_mix` should match.';

-- ── §16.1.4  agent_retrieval_configs ────────────────────────────────────────
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

COMMENT ON TABLE agent_retrieval_configs IS
    'Per-(org, agent) retrieval blend weights. Engine looks up by '
    '(org_id, agent_id) when the request carries an agent_id, falling '
    'back to the org default when absent.';

-- ── §16.2.2  org_versions ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS org_versions (
    org_id      TEXT         PRIMARY KEY,
    version     BIGINT       NOT NULL DEFAULT 1,
    bumped_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE org_versions IS
    'Monotonic counter per org_id. Bumped on any document mutation by the '
    'documents-api outbox; included in retrieval cache key so a mutation '
    'invalidates the entire org cache without waiting for TTL.';

-- ── §16.2.6  documents_outbox (transactional bulk-ingest) ───────────────────
CREATE TABLE IF NOT EXISTS documents_outbox (
    outbox_id   BIGSERIAL    PRIMARY KEY,
    org_id      TEXT         NOT NULL,
    event_type  TEXT         NOT NULL,
    payload     JSONB        NOT NULL,
    published   BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_documents_outbox_unpublished
    ON documents_outbox (created_at)
    WHERE published = FALSE;

COMMENT ON TABLE documents_outbox IS
    'Outbox for §16.2.6 — BulkIngest inserts the document row AND a row '
    'here in the same tx. A background publisher loop reads unpublished '
    'rows, emits the NATS event, then marks published=true. Crash between '
    'commit and publish is safe: the next loop tick re-emits.';
