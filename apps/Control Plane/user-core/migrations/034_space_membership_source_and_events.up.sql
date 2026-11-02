-- 034_space_membership_source_and_events: Control-native Space membership
-- writer (WS04-S4).
--
--  * space_memberships.source says which writer owns a row, so the declarative
--    Application sync can be scoped to rows it created and never clobbers a
--    row a Control actor wrote (member_ops, self_join) or the registrar row.
--  * registered_spaces.roster_mode marks the organization room, whose roster
--    is derived from the organization and refused by the Control writer.
--  * space_membership_events is the append-only membership ledger. Ids, roles
--    and counts only (no names, no content), so it is safe for Zero Data
--    Retention organizations. actor_id and subject_id are pseudonymised by the
--    GDPR erasure saga. published_* columns make the ledger usable as an
--    outbox later (WS04-S10); S4 does not read them.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE space_memberships
    ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'application_sync';

UPDATE space_memberships
SET source = 'registration'
WHERE granted_by = 'application-space-registration'
  AND source = 'application_sync';

ALTER TABLE space_memberships DROP CONSTRAINT IF EXISTS space_memberships_source_chk;
ALTER TABLE space_memberships
    ADD CONSTRAINT space_memberships_source_chk
    CHECK (source IN ('registration', 'application_sync', 'member_ops', 'self_join'));

ALTER TABLE registered_spaces
    ADD COLUMN IF NOT EXISTS roster_mode TEXT NOT NULL DEFAULT 'managed';

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_roster_mode_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_roster_mode_chk
    CHECK (roster_mode IN ('managed', 'org_derived'));

CREATE TABLE IF NOT EXISTS space_membership_events (
    seq              BIGINT GENERATED ALWAYS AS IDENTITY,
    event_id         TEXT PRIMARY KEY,
    space_ref        TEXT NOT NULL REFERENCES registered_spaces(space_ref),
    org_id           TEXT NOT NULL,
    space_seq        BIGINT NOT NULL,
    action           TEXT NOT NULL,
    actor_kind       TEXT NOT NULL,
    actor_id         TEXT NOT NULL,
    subject_type     TEXT,
    subject_id       TEXT,
    from_role        TEXT,
    to_role          TEXT,
    idempotency_key  TEXT,
    published_at     TIMESTAMPTZ,
    publish_attempts INT NOT NULL DEFAULT 0,
    next_publish_at  TIMESTAMPTZ,
    last_error       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_membership_events_action_chk CHECK (action IN (
        'member_added', 'member_joined', 'role_changed', 'member_removed', 'member_left',
        'archived', 'unarchived', 'visibility_changed', 'settings_changed')),
    CONSTRAINT space_membership_events_actor_chk CHECK (actor_kind IN ('user', 'service'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_space_membership_events_idem
    ON space_membership_events (space_ref, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_space_membership_events_space
    ON space_membership_events (space_ref, space_seq);

CREATE INDEX IF NOT EXISTS idx_space_membership_events_pending
    ON space_membership_events (next_publish_at)
    WHERE published_at IS NULL;

COMMIT;
