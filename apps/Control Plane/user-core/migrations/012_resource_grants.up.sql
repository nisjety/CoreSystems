-- 012_resource_grants — generalize document_acl into the single per-user grant
-- store for the Per-User Data Ownership & Sharing phase.
--
-- This makes `resource_grants` the ONE authority for explicit per-subject grants
-- across every ownable resource type (documents first; subject_type + role are
-- present from day one so teams/edit-grants land without a schema change). The
-- legacy `document_acl` copy is backfilled and dropped in the SAME transaction,
-- so there is never a window where two grant tables disagree.
--
-- The matching DPv2 duplicate (infra/postgres/init.sql) is dropped by a separate
-- Data Plane migration (20260620120000_drop_document_acl.sql). Deployment
-- ordering: apply this Control-Plane migration BEFORE the Data-Plane drop; the
-- DPv2 copy is dead (zero code refs) so the interim window is benign.

BEGIN;

CREATE TABLE IF NOT EXISTS resource_grants (
    grant_id      TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id        TEXT NOT NULL,
    resource_type TEXT NOT NULL,                 -- 'document' (MVP); ownable types only
    resource_id   TEXT NOT NULL,
    subject_type  TEXT NOT NULL,                 -- 'user' (MVP) | 'team'
    subject_id    TEXT NOT NULL,
    role          TEXT NOT NULL,                 -- 'view' (MVP) | 'edit'
    granted_by    TEXT NOT NULL DEFAULT '',
    granted_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT resource_grants_subject_type_chk CHECK (subject_type IN ('user', 'team')),
    CONSTRAINT resource_grants_role_chk         CHECK (role IN ('view', 'edit')),
    CONSTRAINT resource_grants_unique           UNIQUE (org_id, resource_type, resource_id, subject_type, subject_id)
);

-- ListVisible / BatchCheck hot path: "which resource ids of type T is subject S
-- granted in org O?". subject_id leads (high cardinality); subject_type — a near
-- constant 'user' in MVP — trails. INCLUDE makes both queries index-only.
CREATE INDEX IF NOT EXISTS idx_resource_grants_visible
    ON resource_grants (subject_id, org_id, resource_type, subject_type)
    INCLUDE (role, resource_id);

-- ListByResource / share-dialog: "who is granted resource R?"
CREATE INDEX IF NOT EXISTS idx_resource_grants_resource
    ON resource_grants (org_id, resource_type, resource_id);

-- Backfill from the legacy document_acl table when present, then drop it so
-- resource_grants is the sole authority. Guarded so fresh databases (no legacy
-- table) and re-runs are both safe. The inner GROUP BY collapses any duplicate
-- (org, document, user) rows — the legacy table had no unique constraint — to a
-- single grant whose role is the MOST permissive (bool_or), avoiding both an
-- intra-statement ON CONFLICT error and a silent privilege downgrade.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_schema = 'public' AND table_name = 'document_acl'
    ) THEN
        INSERT INTO resource_grants
            (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by, granted_at)
        SELECT
            d.acl_id, d.org_id, 'document', d.document_id, 'user', d.user_id, d.role, '', d.created_at
        FROM (
            SELECT
                min(acl_id)     AS acl_id,
                org_id,
                document_id,
                user_id,
                CASE WHEN bool_or(lower(permission_level) IN ('write', 'edit', 'admin', 'owner'))
                     THEN 'edit' ELSE 'view' END AS role,
                max(created_at) AS created_at
            FROM document_acl
            GROUP BY org_id, document_id, user_id
        ) d
        ON CONFLICT (org_id, resource_type, resource_id, subject_type, subject_id) DO NOTHING;

        DROP TABLE document_acl;
    END IF;
END $$;

COMMIT;
