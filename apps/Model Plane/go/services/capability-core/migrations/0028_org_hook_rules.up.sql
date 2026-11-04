-- The durable org hook rules (WS25-S8, DEC-14).
--
-- One row per deny or ask rule an org admin writes. The gateway forwards an org's enabled rules to
-- execution-core on every Do run once HOOKS_DURABLE is on; until then nothing reads this table. A
-- rule narrows a run and never widens it: there is no allow, and a post_tool rule may only deny,
-- because a post-tool ask would reach a person after the tool had already acted (WS25-D7).
--
-- tool_matcher is "*" (any tool), a "prefix*" glob or an exact tool name: execution-core's grammar.
-- The handler stores "*" for an empty matcher, so the partial unique index below sees one spelling
-- of "any".
--
-- Every write also bumps the org's revision in org_agent_policy, in the same statement, so the
-- gateway caches the rules and the policy document under one revision. Rules are never deleted:
-- revoking sets revoked_at and the row stays as history.
--
-- Every row is keyed by org_id, and every query in the handler filters on it. RLS is not applied
-- here (the neighbouring capability-core tables do not use it).
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is
-- idempotent.

CREATE TABLE IF NOT EXISTS org_hook_rules (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL,
    event        TEXT NOT NULL,
    tool_matcher TEXT NOT NULL,
    decision     TEXT NOT NULL,
    reason       TEXT NOT NULL,
    priority     INTEGER NOT NULL DEFAULT 0,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_by   TEXT NOT NULL,
    updated_by   TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at   TIMESTAMPTZ,
    CONSTRAINT org_hook_rules_event_check CHECK (event IN ('pre_tool', 'post_tool')),
    CONSTRAINT org_hook_rules_decision_check CHECK (decision IN ('deny', 'ask')),
    CONSTRAINT org_hook_rules_post_tool_deny_check CHECK (event <> 'post_tool' OR decision = 'deny'),
    CONSTRAINT org_hook_rules_matcher_check CHECK (char_length(tool_matcher) BETWEEN 1 AND 201),
    CONSTRAINT org_hook_rules_reason_check CHECK (char_length(reason) BETWEEN 1 AND 500)
);

-- One active rule per (event, matcher, decision) in an org. A revoked rule frees its key.
CREATE UNIQUE INDEX IF NOT EXISTS org_hook_rules_active_key
    ON org_hook_rules (org_id, event, tool_matcher, decision)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS org_hook_rules_org_active_idx
    ON org_hook_rules (org_id)
    WHERE revoked_at IS NULL;
