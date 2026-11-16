-- WS19-S1: room canvas storage in wiki-store-go. One Markdown canvas per Space, append-only versions and a
-- head compare-and-swap. The Control decision that authorises each write is verified by the HTTP layer (WS19-S3),
-- not here.
--
-- Deliberately not wiki_pages or wiki_page_versions. Every wiki publication emits
-- dataplane.wiki.version.published with full content into the org-wide embedding stream, so a canvas kept there
-- would leak room text to the whole org. These tables carry no outbox row and no event.
--
-- Phase 1 allows one active canvas per Space (uq_space_canvases_active). Dropping that index is the multi-canvas change.
--
-- Privilege: dataplane_app may INSERT and UPDATE the canvas head and may only SELECT and INSERT versions, so a
-- version is append-only by privilege. Only the unscoped purge path (Space deletion, org erasure) deletes rows.

CREATE TABLE IF NOT EXISTS space_canvases (
    canvas_id        TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id           TEXT NOT NULL,
    space_ref        TEXT NOT NULL,
    state            TEXT NOT NULL DEFAULT 'active',
    head_version_id  TEXT,
    head_version_no  INTEGER NOT NULL DEFAULT 0,
    created_by       TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT space_canvases_state_chk CHECK (state IN ('active', 'archived')),
    CONSTRAINT space_canvases_ident_chk CHECK (
        char_length(btrim(space_ref)) > 0
        AND char_length(btrim(org_id)) > 0
        AND char_length(btrim(created_by)) > 0
    ),
    CONSTRAINT space_canvases_head_chk CHECK (
        head_version_no >= 0
        AND ((head_version_id IS NULL) = (head_version_no = 0))
    ),
    -- Target of the composite foreign key from space_canvas_versions: a version can only name a canvas of its own org.
    CONSTRAINT space_canvases_canvas_org_key UNIQUE (canvas_id, org_id)
);

-- One active canvas per Space. Dropping this index is the whole multi-canvas change.
CREATE UNIQUE INDEX IF NOT EXISTS uq_space_canvases_active
    ON space_canvases (org_id, space_ref)
    WHERE state = 'active';

CREATE TABLE IF NOT EXISTS space_canvas_versions (
    version_id               TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    canvas_id                TEXT NOT NULL,
    org_id                   TEXT NOT NULL,
    version_no               INTEGER NOT NULL,
    parent_version_id        TEXT,
    content                  TEXT NOT NULL,
    content_sha256           TEXT NOT NULL,
    content_bytes            INTEGER NOT NULL,
    author_kind              TEXT NOT NULL,
    author_id                TEXT NOT NULL,
    on_behalf_of             TEXT,
    run_id                   TEXT,
    change_kind              TEXT NOT NULL,
    restored_from_version_id TEXT,
    summary                  TEXT,
    decision_ref             TEXT NOT NULL,
    authority_revision       BIGINT NOT NULL,
    audience_revision        BIGINT NOT NULL,
    idempotency_hash         TEXT NOT NULL,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT space_canvas_versions_canvas_org_fk FOREIGN KEY (canvas_id, org_id)
        REFERENCES space_canvases (canvas_id, org_id) ON DELETE CASCADE,
    CONSTRAINT scv_version_no_chk CHECK (version_no >= 1),
    CONSTRAINT scv_author_chk CHECK (author_kind IN ('user', 'agent')),
    CONSTRAINT scv_agent_chk CHECK (
        author_kind = 'user'
        OR (run_id IS NOT NULL AND on_behalf_of IS NOT NULL)
    ),
    CONSTRAINT scv_user_chk CHECK (
        author_kind = 'agent'
        OR (run_id IS NULL AND on_behalf_of IS NULL)
    ),
    CONSTRAINT scv_change_chk CHECK (change_kind IN ('create', 'edit', 'restore')),
    CONSTRAINT scv_restore_chk CHECK ((change_kind = 'restore') = (restored_from_version_id IS NOT NULL)),
    CONSTRAINT scv_size_chk CHECK (content_bytes BETWEEN 0 AND 65536),
    CONSTRAINT scv_digest_chk CHECK (content_sha256 ~ '^sha256:[0-9a-f]{64}$'),
    CONSTRAINT scv_summary_chk CHECK (summary IS NULL OR char_length(summary) <= 200),
    CONSTRAINT scv_provenance_chk CHECK (
        char_length(btrim(decision_ref)) > 0
        AND authority_revision > 0
        AND audience_revision > 0
    ),
    CONSTRAINT scv_canvas_version_key UNIQUE (canvas_id, version_no),
    CONSTRAINT scv_canvas_idempotency_key UNIQUE (canvas_id, idempotency_hash)
);

-- Rate-limit counts and history scans read one canvas's versions by time or by number.
CREATE INDEX IF NOT EXISTS idx_scv_canvas_created ON space_canvas_versions (canvas_id, created_at DESC);

-- dataplane_app changes the head and reads both tables. It never UPDATEs or DELETEs a version, and it cannot DELETE a
-- canvas: purge runs on the unscoped pool. Both tables carry org_id, so the per-table policy needs no child policy.
GRANT SELECT, INSERT, UPDATE ON public.space_canvases TO dataplane_app;
GRANT SELECT, INSERT ON public.space_canvas_versions TO dataplane_app;

ALTER TABLE public.space_canvases ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS space_canvases_rls_isolation ON public.space_canvases;
CREATE POLICY space_canvases_rls_isolation ON public.space_canvases
    USING (org_id = current_setting('app.current_org', true))
    WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE public.space_canvas_versions ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS space_canvas_versions_rls_isolation ON public.space_canvas_versions;
CREATE POLICY space_canvas_versions_rls_isolation ON public.space_canvas_versions
    USING (org_id = current_setting('app.current_org', true))
    WITH CHECK (org_id = current_setting('app.current_org', true));
