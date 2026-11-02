-- 039_approval_grants (WS10-S2): Control-held approval authority for a shared
-- Space. Three things live here and nothing else:
--
--   * approval_grant_entitled: the org-level, deny-by-default bit that lets a
--     manager record a standing approval rule. One entitlement-catalogue row
--     (entitlement_catalog.go) carries it.
--   * space_approval_policies: the per-Space ceiling (max_grant_scope), the
--     default addressee and the quarantine mode. A missing row means the most
--     restrictive defaults ('once', 'requester', 'off').
--   * space_approval_grants: STANDING RULES only (scope 'always'). A once token
--     lives in Session Core; thread/run grants are Session Core's too (Model
--     Plane plan mp-WS13-S6). A rule waives a prompt for one tool of one agent
--     in one Space; it never widens owner-plane authority, and any current
--     deny (block) still wins over it.
--   * space_approval_grant_events: append-only history of rule life-cycle and
--     use. Ids, keys and reason codes only: never tool arguments or content, so
--     it is safe next to a Zero Data Retention org.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS approval_grant_entitled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS space_approval_policies (
    space_ref               TEXT PRIMARY KEY REFERENCES registered_spaces(space_ref) ON DELETE RESTRICT,
    org_id                  TEXT NOT NULL,
    max_grant_scope         TEXT NOT NULL DEFAULT 'once'
        CHECK (max_grant_scope IN ('once', 'thread', 'always')),
    default_addressee_kind  TEXT NOT NULL DEFAULT 'requester'
        CHECK (default_addressee_kind IN ('requester', 'any_editor', 'any_manager')),
    quarantine_mode         TEXT NOT NULL DEFAULT 'off'
        CHECK (quarantine_mode IN ('off', 'shadow', 'enforce')),
    revision                BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_by              TEXT NOT NULL,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS space_approval_grants (
    rule_id                 TEXT PRIMARY KEY,
    org_id                  TEXT NOT NULL,
    space_ref               TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE RESTRICT,
    scope                   TEXT NOT NULL CHECK (scope = 'always'),
    agent_subject_id        TEXT NOT NULL CHECK (char_length(btrim(agent_subject_id)) > 0),
    tool_key                TEXT NOT NULL CHECK (char_length(btrim(tool_key)) > 0),
    action_key              TEXT NOT NULL CHECK (char_length(btrim(action_key)) > 0),
    action_schema_hash      TEXT NOT NULL CHECK (action_schema_hash ~ '^sha256:[0-9a-f]{64}$'),
    source_approval_id      TEXT NOT NULL DEFAULT '',
    granted_by_subject_id   TEXT NOT NULL CHECK (char_length(btrim(granted_by_subject_id)) > 0),
    granted_by_role         TEXT NOT NULL CHECK (granted_by_role IN ('manager', 'owner')),
    authority_revision      BIGINT NOT NULL CHECK (authority_revision > 0),
    idempotency_key         TEXT NOT NULL CHECK (char_length(idempotency_key) BETWEEN 8 AND 200),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    review_by               TIMESTAMPTZ NOT NULL,
    revoked_at              TIMESTAMPTZ,
    revoked_by              TEXT,
    revoke_reason           TEXT,
    use_count               BIGINT NOT NULL DEFAULT 0,
    last_used_at            TIMESTAMPTZ,
    UNIQUE (org_id, idempotency_key)
);

-- One live rule per agent action in a Space: a changed schema hash replaces the
-- old rule (the creator route revokes it as 'superseded' in the same
-- transaction), it never stacks.
CREATE UNIQUE INDEX IF NOT EXISTS space_approval_grants_live_idx
    ON space_approval_grants (space_ref, agent_subject_id, action_key)
    WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS space_approval_grant_events (
    event_id                BIGSERIAL PRIMARY KEY,
    rule_id                 TEXT NOT NULL,
    org_id                  TEXT NOT NULL,
    space_ref               TEXT NOT NULL,
    kind                    TEXT NOT NULL CHECK (kind IN ('created', 'used', 'revoked', 'expired', 'use_denied')),
    actor_subject_id        TEXT NOT NULL DEFAULT '',
    run_id                  TEXT NOT NULL DEFAULT '',
    reason_code             TEXT NOT NULL DEFAULT '',
    at                      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS space_approval_grant_events_rule_idx
    ON space_approval_grant_events (rule_id, event_id);
