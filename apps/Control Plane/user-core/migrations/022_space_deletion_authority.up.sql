-- 022_space_deletion_authority — explicit, deny-by-default authorization for
-- Application-owned Space deletion requests. A request is an auditable intent,
-- not proof that any plane has purged its own data.

BEGIN;

CREATE TABLE IF NOT EXISTS space_deletion_policies (
    org_id                              TEXT PRIMARY KEY,
    deletion_entitled                   BOOLEAN NOT NULL DEFAULT FALSE,
    personal_rollout_enabled             BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at                          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS space_legal_holds (
    space_ref           TEXT PRIMARY KEY REFERENCES registered_spaces(space_ref) ON DELETE CASCADE,
    hold_ref            TEXT NOT NULL,
    applied_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_legal_holds_ref_chk CHECK (char_length(btrim(hold_ref)) > 0)
);

CREATE TABLE IF NOT EXISTS space_deletion_requests (
    request_id          TEXT PRIMARY KEY,
    space_ref           TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE RESTRICT,
    org_id              TEXT NOT NULL,
    owner_principal_id  TEXT NOT NULL,
    idempotency_key     TEXT NOT NULL,
    status              TEXT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (space_ref, idempotency_key),
    CONSTRAINT space_deletion_requests_identity_chk CHECK (
        char_length(btrim(request_id)) > 0 AND char_length(btrim(owner_principal_id)) > 0
        AND char_length(btrim(idempotency_key)) > 0
    ),
    CONSTRAINT space_deletion_requests_status_chk
        CHECK (status IN ('authorized', 'blocked_legal_hold', 'rejected'))
);

COMMIT;
