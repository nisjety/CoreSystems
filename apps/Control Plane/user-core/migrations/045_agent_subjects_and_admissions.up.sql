-- 045_agent_subjects_and_admissions — WS17-S4: Control's registry of agent
-- subjects, their access list, and per-Space admission (ADR 4, SD-04 option A).
--
-- Control owns who may add an agent to a room: the agent-to-owner link is a
-- Control fact, each admission is a Control row, and the room roster skips an
-- agent Control has not admitted (SPACE_AGENT_ADMISSION). Ids, states and
-- revisions only: no agent name, instructions or content.
--
-- Only `agent-*` service subjects are gated. Existing agents are backfilled as
-- org-visible, add_policy 'anyone', no owner, admitted in every Space they are
-- already a member of: no behaviour change until an owner claims one (D-6).
-- Number 045 (plan 3.E said 044; 044 is taken on main by two migrations). One migration per
-- PR (plan 4.4.1 row 15). It also carries the two entitlement floors that plan 4.6 reserved
-- for WS17-S7 (045 slot): AdmitDenial reads personal_agent_admit_entitled, so it lands here.

BEGIN;

CREATE TABLE IF NOT EXISTS agent_subjects (
    org_id              TEXT NOT NULL,
    agent_subject_id    TEXT NOT NULL,
    owner_user_id       TEXT,
    visibility          TEXT NOT NULL DEFAULT 'org',
    add_policy          TEXT NOT NULL DEFAULT 'anyone',
    lifecycle           TEXT NOT NULL DEFAULT 'active',
    suspended_reason    TEXT NOT NULL DEFAULT '',
    revision            BIGINT NOT NULL DEFAULT 1,
    registered_by       TEXT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, agent_subject_id),
    CONSTRAINT agent_subjects_id_chk CHECK (agent_subject_id ~ '^agent-[A-Za-z0-9_-]{6,64}$'),
    CONSTRAINT agent_subjects_owner_chk CHECK (owner_user_id IS NULL OR char_length(btrim(owner_user_id)) > 0),
    CONSTRAINT agent_subjects_visibility_chk CHECK (visibility IN ('owner', 'org')),
    CONSTRAINT agent_subjects_owner_required_chk CHECK (visibility <> 'owner' OR owner_user_id IS NOT NULL),
    CONSTRAINT agent_subjects_add_policy_chk CHECK (add_policy IN ('owner_only', 'nobody', 'anyone')),
    CONSTRAINT agent_subjects_policy_chk CHECK (add_policy <> 'anyone' OR visibility = 'org'),
    CONSTRAINT agent_subjects_lifecycle_chk CHECK (lifecycle IN ('active', 'suspended', 'retired')),
    CONSTRAINT agent_subjects_revision_chk CHECK (revision > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS agent_subjects_subject_uq ON agent_subjects (agent_subject_id);
CREATE INDEX IF NOT EXISTS agent_subjects_owner_idx ON agent_subjects (org_id, owner_user_id) WHERE owner_user_id IS NOT NULL;

-- Access list of an agent definition. Principals are org users; a manager may
-- edit the agent's access and policy, a collaborator may add it to rooms they
-- manage and use it.
CREATE TABLE IF NOT EXISTS agent_subject_access (
    org_id              TEXT NOT NULL,
    agent_subject_id    TEXT NOT NULL,
    principal_type      TEXT NOT NULL,
    principal_id        TEXT NOT NULL,
    role                TEXT NOT NULL,
    granted_by          TEXT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, agent_subject_id, principal_type, principal_id),
    CONSTRAINT agent_subject_access_principal_chk CHECK (principal_type = 'user'),
    CONSTRAINT agent_subject_access_role_chk CHECK (role IN ('manager', 'collaborator')),
    CONSTRAINT agent_subject_access_subject_fk FOREIGN KEY (org_id, agent_subject_id)
        REFERENCES agent_subjects (org_id, agent_subject_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS agent_subject_access_principal_idx ON agent_subject_access (org_id, principal_id);

-- One admission per (room, agent). `admitted` is the only state in which the
-- agent may sit on the roster; `withdrawn` and `suspended` are not.
CREATE TABLE IF NOT EXISTS space_agent_admissions (
    space_ref           TEXT NOT NULL REFERENCES registered_spaces (space_ref) ON DELETE CASCADE,
    org_id              TEXT NOT NULL,
    agent_subject_id    TEXT NOT NULL,
    state               TEXT NOT NULL,
    state_reason        TEXT NOT NULL DEFAULT '',
    admitted_by         TEXT NOT NULL,
    revision            BIGINT NOT NULL DEFAULT 1,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, agent_subject_id),
    CONSTRAINT space_agent_admissions_state_chk CHECK (state IN ('admitted', 'withdrawn', 'suspended')),
    CONSTRAINT space_agent_admissions_revision_chk CHECK (revision > 0),
    CONSTRAINT space_agent_admissions_subject_fk FOREIGN KEY (org_id, agent_subject_id)
        REFERENCES agent_subjects (org_id, agent_subject_id)
);

CREATE INDEX IF NOT EXISTS space_agent_admissions_subject_idx ON space_agent_admissions (org_id, agent_subject_id);

-- Append-only ledger of admission facts. Ids, states and the idempotency key
-- only. `would_skip` is written in observe mode for an agent enforce would skip.
CREATE TABLE IF NOT EXISTS space_agent_admission_events (
    id                  BIGSERIAL PRIMARY KEY,
    space_ref           TEXT NOT NULL,
    agent_subject_id    TEXT NOT NULL,
    event               TEXT NOT NULL,
    actor_subject_id    TEXT NOT NULL,
    idempotency_key     TEXT NOT NULL DEFAULT '',
    details             JSONB NOT NULL DEFAULT '{}'::jsonb,
    at                  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_agent_admission_events_event_chk
        CHECK (event IN ('admitted', 'withdrawn', 'suspended', 'would_skip'))
);

CREATE INDEX IF NOT EXISTS space_agent_admission_events_space_idx
    ON space_agent_admission_events (space_ref, agent_subject_id, id);
CREATE UNIQUE INDEX IF NOT EXISTS space_agent_admission_events_key_uq
    ON space_agent_admission_events (space_ref, idempotency_key) WHERE idempotency_key <> '';

CREATE OR REPLACE FUNCTION space_agent_admission_events_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'space_agent_admission_events is append-only (% rejected)', TG_OP
        USING ERRCODE = 'restrict_violation';
END;
$$;

DROP TRIGGER IF EXISTS space_agent_admission_events_immutable_trg ON space_agent_admission_events;
CREATE TRIGGER space_agent_admission_events_immutable_trg
    BEFORE UPDATE OR DELETE ON space_agent_admission_events
    FOR EACH ROW EXECUTE FUNCTION space_agent_admission_events_immutable();

-- Two standing deny-by-default floors on the org's Space effect policy. Each has one
-- catalogue row (entitlement_catalog.go). Neither is an authorization input on its own.
--   personal_agent_admit_entitled  a room may admit an owner-visible (personal) agent.
--   memory_declassify_entitled     declassifying a personal memory into a room (WS17-S7).
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS personal_agent_admit_entitled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS memory_declassify_entitled BOOLEAN NOT NULL DEFAULT FALSE;

-- Legacy backfill: every active agent-* service membership becomes an org-visible,
-- ownerless definition plus an admitted row in its Space. Idempotent. Ids that do
-- not match the agent pattern are not backfilled (they stay unadmitted; observe
-- mode reports them before enforce is turned on).
INSERT INTO agent_subjects (org_id, agent_subject_id, visibility, add_policy, lifecycle, revision, registered_by)
SELECT DISTINCT s.org_id, m.subject_id, 'org', 'anyone', 'active', 1, 'backfill:pre-admission'
  FROM space_memberships m
  JOIN registered_spaces s ON s.space_ref = m.space_ref
 WHERE m.subject_type = 'service'
   AND m.active = TRUE
   AND m.subject_id ~ '^agent-[A-Za-z0-9_-]{6,64}$'
ON CONFLICT DO NOTHING;

INSERT INTO space_agent_admissions (space_ref, org_id, agent_subject_id, state, state_reason, admitted_by, revision)
SELECT m.space_ref, a.org_id, a.agent_subject_id, 'admitted', '', 'backfill:pre-admission', 1
  FROM space_memberships m
  JOIN registered_spaces s ON s.space_ref = m.space_ref
  JOIN agent_subjects a ON a.org_id = s.org_id AND a.agent_subject_id = m.subject_id
 WHERE m.subject_type = 'service'
   AND m.active = TRUE
ON CONFLICT (space_ref, agent_subject_id) DO NOTHING;

COMMIT;
