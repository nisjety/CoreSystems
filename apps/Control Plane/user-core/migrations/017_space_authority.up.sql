-- 017_space_authority — Control's registered-Space authority foundation.
--
-- Application creates immutable Space references and lifecycle events; Control
-- is the only owner of membership, privacy/audience/entitlement revisions, and
-- later signed access decisions. These tables intentionally do not duplicate
-- the Application lifecycle projection or store arbitrary content.

BEGIN;

CREATE TABLE IF NOT EXISTS registered_spaces (
    space_ref                       TEXT PRIMARY KEY,
    org_id                          TEXT NOT NULL,
    space_kind                      TEXT NOT NULL,
    owner_principal_id              TEXT NOT NULL,
    application_lifecycle_revision  BIGINT NOT NULL,
    registration_state              TEXT NOT NULL DEFAULT 'active',
    registered_at                   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT registered_spaces_kind_chk
        CHECK (space_kind IN ('personal', 'room', 'project', 'case')),
    CONSTRAINT registered_spaces_lifecycle_revision_chk
        CHECK (application_lifecycle_revision > 0),
    CONSTRAINT registered_spaces_owner_principal_chk
        CHECK (char_length(btrim(owner_principal_id)) > 0),
    CONSTRAINT registered_spaces_state_chk
        CHECK (registration_state IN ('active', 'suspended', 'deleting', 'deleted'))
);

CREATE INDEX IF NOT EXISTS idx_registered_spaces_org
    ON registered_spaces (org_id, space_kind, registration_state);

-- An aggregate revision changes whenever any effective Space authority claim
-- changes. Component revisions make stale cache/resume revalidation exact.
CREATE TABLE IF NOT EXISTS space_authority_revisions (
    space_ref                   TEXT PRIMARY KEY REFERENCES registered_spaces(space_ref) ON DELETE CASCADE,
    authority_revision          BIGINT NOT NULL DEFAULT 1,
    membership_revision         BIGINT NOT NULL DEFAULT 1,
    privacy_revision            BIGINT NOT NULL DEFAULT 1,
    recipient_audience_revision BIGINT NOT NULL DEFAULT 1,
    entitlement_revision        BIGINT NOT NULL DEFAULT 1,
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_authority_revisions_positive_chk CHECK (
        authority_revision > 0 AND membership_revision > 0 AND privacy_revision > 0
        AND recipient_audience_revision > 0 AND entitlement_revision > 0
    )
);

-- Membership roles are authority facts, not UI labels. A future decision
-- issuer resolves these with the registered Space and current org membership;
-- no browser-supplied actor, role, or audience is trusted.
CREATE TABLE IF NOT EXISTS space_memberships (
    space_ref       TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE CASCADE,
    subject_type    TEXT NOT NULL DEFAULT 'user',
    subject_id      TEXT NOT NULL,
    role            TEXT NOT NULL,
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    revision        BIGINT NOT NULL DEFAULT 1,
    granted_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, subject_type, subject_id),
    CONSTRAINT space_memberships_subject_chk CHECK (subject_type IN ('user', 'service')),
    CONSTRAINT space_memberships_role_chk CHECK (role IN ('viewer', 'editor', 'manager', 'owner')),
    CONSTRAINT space_memberships_revision_chk CHECK (revision > 0)
);

CREATE INDEX IF NOT EXISTS idx_space_memberships_subject
    ON space_memberships (subject_type, subject_id, active);

COMMIT;
