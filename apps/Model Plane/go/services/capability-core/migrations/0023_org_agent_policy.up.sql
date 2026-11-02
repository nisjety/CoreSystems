-- The org agent policy registry (WS25-S7, DEC-14).
--
-- One versioned document per org: org_agent_policy holds the revision in force and
-- org_agent_policy_history holds every revision, so the chain is readable. The document
-- is the closed key schema of mp-contracts/src/org_agent_policy.rs written as JSON: key
-- names and values only, never content. Nothing reads these tables to decide anything
-- yet; enforcement comes in WS25-S9, behind its own switch.
--
-- Every row is keyed by org_id, and every query in the registry filters on it.
-- RLS is not applied here (the neighbouring capability-core tables do not use it).
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything
-- here is idempotent.

CREATE TABLE IF NOT EXISTS org_agent_policy (
    org_id      TEXT PRIMARY KEY,
    revision    BIGINT NOT NULL,
    doc         JSONB NOT NULL,
    digest      TEXT NOT NULL,
    updated_by  TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT org_agent_policy_revision_check CHECK (revision >= 1),
    CONSTRAINT org_agent_policy_doc_check CHECK (jsonb_typeof(doc) = 'object'),
    CONSTRAINT org_agent_policy_digest_check CHECK (digest ~ '^[0-9a-f]{64}$')
);

CREATE TABLE IF NOT EXISTS org_agent_policy_history (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    revision    BIGINT NOT NULL,
    doc         JSONB NOT NULL,
    digest      TEXT NOT NULL,
    actor       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT org_agent_policy_history_revision_check CHECK (revision >= 1),
    CONSTRAINT org_agent_policy_history_doc_check CHECK (jsonb_typeof(doc) = 'object'),
    CONSTRAINT org_agent_policy_history_digest_check CHECK (digest ~ '^[0-9a-f]{64}$')
);

CREATE UNIQUE INDEX IF NOT EXISTS org_agent_policy_history_revision_uq
    ON org_agent_policy_history (org_id, revision);
