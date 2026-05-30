-- v2.3 wave 1 — spec D4/D5 closure
--
-- 1. mode_mix on retrieval_runs: record the actual blend weights used for a
--    query so the audit endpoint can return per-query mode-mix (spec §7).
-- 2. graph_exports: tracks JSON/GraphML/HTML/markdown exports of the knowledge
--    graph (spec §2.1).
-- 3. wiki_maintenance_logs: ensure the columns the new sweep endpoint writes
--    are present (action, actor, details, kind).

-- ── retrieval_runs.mode_mix ────────────────────────────────────────────────
ALTER TABLE retrieval_runs ADD COLUMN IF NOT EXISTS mode_mix JSONB;

COMMENT ON COLUMN retrieval_runs.mode_mix IS
    'Per-query blend weights actually used: {"w_dense":0.5,"w_bm25":0.2,"w_graph":0.2,"w_wiki":0.1,"rerank":true}';

-- ── graph_exports ──────────────────────────────────────────────────────────
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

-- ── wiki_maintenance_logs: ensure `kind` exists (spec uses it; v2 originally
--    shipped only `action`). Both columns coexist for backward-compat.
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS kind TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMPTZ;
