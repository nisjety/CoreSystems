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
