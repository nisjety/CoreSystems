-- 036_space_visibility_directory: room visibility, join role and the per-org
-- directory policy (WS04-S8, decision D-WS04-2).
--
--  * registered_spaces.visibility is 'private' (default, never listed, not
--    joinable) or 'org_public' (listed in the room directory and joinable by an
--    active organization member, only while the organization's directory policy
--    is enabled).
--  * registered_spaces.join_role is the role a self-join receives. It is
--    viewer or editor and never elevated (the CHECK enforces what the pure role
--    rules in internal/spaces/roles.go already enforce).
--  * space_directory_policies is the deny-by-default per-organization switch.
--    There is NO default row: absence denies. The operator path
--    (PUT /api/v1/internal/spaces/directory-policy) upserts a row and appends
--    one space_directory_policy_events row per real change.
--  * idx_registered_spaces_directory backs the directory room list.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE registered_spaces
    ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'private',
    ADD COLUMN IF NOT EXISTS join_role TEXT NOT NULL DEFAULT 'editor';

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_visibility_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_visibility_chk
    CHECK (visibility IN ('private', 'org_public'));

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_join_role_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_join_role_chk
    CHECK (join_role IN ('viewer', 'editor'));

CREATE TABLE IF NOT EXISTS space_directory_policies (
    org_id                   TEXT PRIMARY KEY,
    org_public_rooms_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS space_directory_policy_events (
    event_id                 BIGSERIAL PRIMARY KEY,
    org_id                   TEXT NOT NULL,
    actor_principal_id       TEXT NOT NULL,
    org_public_rooms_enabled BOOLEAN NOT NULL,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_registered_spaces_directory
    ON registered_spaces (org_id)
    WHERE visibility = 'org_public' AND registration_state = 'active';

COMMIT;
