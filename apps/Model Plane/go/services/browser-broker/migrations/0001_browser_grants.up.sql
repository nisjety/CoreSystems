-- browser-broker 0001: durable browser grants (WS1.C).
--
-- The container entrypoint re-applies every *.up.sql on each start, so every
-- statement here must be idempotent (IF NOT EXISTS). Later migrations follow
-- the R-3 allocation: 0002 WS7, 0003 WS6, 0004 WS8, each with
-- ADD COLUMN IF NOT EXISTS.
--
-- Zero Data Retention grants are never stored here (X-2): they live only in
-- the broker's memory with a "z_" id prefix. There is deliberately no zdr
-- column, and the id CHECK refuses a "z_" id, so a routing bug fails the
-- insert instead of persisting a ZDR grant.
--
-- Only browser-broker reads or writes this table.

CREATE TABLE IF NOT EXISTS browser_grants (
    id                   TEXT        PRIMARY KEY,
    kind                 TEXT        NOT NULL,
    org_id               TEXT        NOT NULL,
    owner_id             TEXT        NOT NULL,
    session_key          TEXT        NOT NULL,
    scope_url            TEXT        NOT NULL,
    allowed_domains      TEXT[]      NOT NULL,
    allowed_actions      TEXT[]      NOT NULL DEFAULT '{}',
    allowed_frame_ids    TEXT[]      NOT NULL DEFAULT '{}',
    allowed_dialog_ids   TEXT[]      NOT NULL DEFAULT '{}',
    allowed_artifact_ids TEXT[]      NOT NULL DEFAULT '{}',
    -- A run grant or a sensitive approval is bound to its parent; deleting
    -- the parent (purge) deletes the subtree.
    parent_grant_id      TEXT        REFERENCES browser_grants (id) ON DELETE CASCADE,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at           TIMESTAMPTZ NOT NULL,
    -- The hard backstop a renewal can never pass. Equal to expires_at until
    -- renewable kinds exist (WS8).
    absolute_expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at           TIMESTAMPTZ,
    -- A short caller label (at most 128 bytes), never a domain or URL.
    revoked_reason       TEXT,
    CONSTRAINT browser_grants_kind_check
        CHECK (kind IN ('session', 'run', 'sensitive_approval')),
    CONSTRAINT browser_grants_org_check CHECK (org_id <> ''),
    CONSTRAINT browser_grants_domains_check
        CHECK (cardinality(allowed_domains) BETWEEN 1 AND 32),
    CONSTRAINT browser_grants_parent_check
        CHECK ((kind = 'session') = (parent_grant_id IS NULL)),
    CONSTRAINT browser_grants_no_zdr_id_check CHECK (id NOT LIKE 'z\_%'),
    CONSTRAINT browser_grants_expiry_check CHECK (expires_at <= absolute_expires_at),
    CONSTRAINT browser_grants_reason_check
        CHECK (revoked_reason IS NULL OR octet_length(revoked_reason) <= 128)
);

-- The per-owner session cap counts live session grants.
CREATE INDEX IF NOT EXISTS browser_grants_owner_active_idx
    ON browser_grants (org_id, owner_id, expires_at)
    WHERE kind = 'session' AND revoked_at IS NULL;

-- Revocation cascade and the per-parent run cap.
CREATE INDEX IF NOT EXISTS browser_grants_parent_idx
    ON browser_grants (parent_grant_id)
    WHERE parent_grant_id IS NOT NULL;

-- Derive is idempotent on (parent, session_key).
CREATE UNIQUE INDEX IF NOT EXISTS browser_grants_run_idempotency_idx
    ON browser_grants (parent_grant_id, session_key)
    WHERE kind = 'run';

-- Purge of long-expired rows.
CREATE INDEX IF NOT EXISTS browser_grants_expiry_idx
    ON browser_grants (expires_at);
