-- Revert: recreate the (dead) document_acl table as it existed in init.sql.
-- Provided for migration reversibility only; nothing reads this table.
CREATE TABLE IF NOT EXISTS document_acl (
    acl_id           TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id           TEXT         NOT NULL,
    document_id      TEXT         NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    user_id          TEXT         NOT NULL,
    permission_level TEXT         NOT NULL DEFAULT 'read',
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_document_acl_user_org  ON document_acl (user_id, org_id);
CREATE INDEX IF NOT EXISTS idx_document_acl_document  ON document_acl (document_id);
