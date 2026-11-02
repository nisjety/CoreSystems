-- 043_resource_grants_space_subject: a Space as a grant subject (WS14-S2,
-- F-t1-013 room files / F-t1-057 artifact sharing grants).
--
-- A room file is owned by its uploader; the owner's attach act adds ONE extra
-- resource_grants row (resource, 'space', S, 'view'). The row is inert on its
-- own: it is honoured only inside a Space read that carries a current Space
-- decision AND current membership/audience (Control's space-visible join), so
-- a Space subject can only narrow or co-require, never widen an owner ACL.
-- Space rows are 'view' only: a Space is never an editor of a document.
--
-- No new index: idx_resource_grants_visible already leads with subject_id.
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE resource_grants DROP CONSTRAINT IF EXISTS resource_grants_subject_type_chk;
ALTER TABLE resource_grants
    ADD CONSTRAINT resource_grants_subject_type_chk
    CHECK (subject_type IN ('user', 'team', 'space'));

ALTER TABLE resource_grants DROP CONSTRAINT IF EXISTS resource_grants_space_role_chk;
ALTER TABLE resource_grants
    ADD CONSTRAINT resource_grants_space_role_chk
    CHECK (subject_type <> 'space' OR role = 'view');

COMMIT;
