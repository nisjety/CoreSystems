-- 018_space_effect_policies — Control-owned processing floors for the first
-- scoped Model effect. No default row is inserted: absence must deny issuance.

BEGIN;

CREATE TABLE IF NOT EXISTS space_effect_policies (
    org_id                           TEXT PRIMARY KEY,
    privacy_policy_ref               TEXT NOT NULL,
    purpose                          TEXT NOT NULL,
    lawful_basis                     TEXT NOT NULL,
    privacy_class                    TEXT NOT NULL,
    third_party_processing_allowed   BOOLEAN NOT NULL DEFAULT FALSE,
    retention_class                  TEXT NOT NULL,
    residency                        TEXT NOT NULL,
    deletion_scope                   TEXT NOT NULL,
    zero_data_retention              BOOLEAN NOT NULL DEFAULT FALSE,
    thread_create_entitled           BOOLEAN NOT NULL DEFAULT FALSE,
    policy_revision                  BIGINT NOT NULL DEFAULT 1,
    entitlement_revision             BIGINT NOT NULL DEFAULT 1,
    updated_at                       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_effect_policies_revisions_positive_chk
        CHECK (policy_revision > 0 AND entitlement_revision > 0),
    CONSTRAINT space_effect_policies_nonempty_chk
        CHECK (
            char_length(btrim(privacy_policy_ref)) > 0 AND
            char_length(btrim(purpose)) > 0 AND
            char_length(btrim(lawful_basis)) > 0 AND
            char_length(btrim(privacy_class)) > 0 AND
            char_length(btrim(retention_class)) > 0 AND
            char_length(btrim(residency)) > 0 AND
            char_length(btrim(deletion_scope)) > 0
        )
);

COMMIT;
