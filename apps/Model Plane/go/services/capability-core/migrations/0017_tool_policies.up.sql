--
-- Per-tool allow / ask / never policies, at user and org scope (WS05-S5).
--
-- A person can tighten any tool for themselves (ask, never), an org admin can
-- tighten it for the whole org, and either may relax an MCP tool to `allow` when
-- the tool's class permits it. What a policy may do is decided by the policy
-- engine (internal/policy), not here: this migration only stores them. Nothing
-- reads the table unless TOOL_POLICY_ENFORCED is "true" on capability-core, so
-- applying it changes no decision anywhere.
--
-- tool_policies keeps one ACTIVE row per (org, scope, capability): the partial
-- unique index below. Setting a policy again updates that row in place, so its id
-- survives; revoking sets revoked_at and the row stays as history. Nothing is
-- ever deleted, and every write also appends a registry_audit_log row.
--
--   scope_kind 'org'   scope_value is the org id (CHECK below), set by an admin
--   scope_kind 'user'  scope_value is the person it applies to, set by themselves
--
-- An `allow` is bound to the exact tool definition a person reviewed:
-- pinned_digest must equal the tool's pinned digest when the policy is
-- evaluated, so a re-approved (changed) definition stops being allowed and is
-- asked about again. Only a tightening policy (ask, never) has no digest.
-- `reason` is the person's own free text; it is bounded here and is never copied
-- into the audit row.
--
-- capabilities.policy_relaxable says whether a capability's tools may ever be
-- relaxed to `allow`. It is generated from kind, so it is true exactly for
-- `mcp_tool` rows and no writer can set it otherwise; a built-in tool's allow is
-- never honoured. It is stored, not computed at read time, so an operator can
-- see which rows are relaxable with one query.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so
-- everything here is idempotent.

CREATE TABLE IF NOT EXISTS tool_policies (
    id            TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    scope_kind    TEXT NOT NULL,
    scope_value   TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    decision      TEXT NOT NULL,
    pinned_digest TEXT,
    reason        TEXT NOT NULL DEFAULT '',
    set_by        TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at    TIMESTAMPTZ,
    CONSTRAINT tool_policies_scope_kind_check CHECK (scope_kind IN ('org', 'user')),
    CONSTRAINT tool_policies_decision_check CHECK (decision IN ('allow', 'ask', 'never')),
    CONSTRAINT tool_policies_org_scope_value_check CHECK (scope_kind <> 'org' OR scope_value = org_id),
    CONSTRAINT tool_policies_allow_digest_check
        CHECK (decision <> 'allow' OR pinned_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT tool_policies_reason_length_check CHECK (char_length(reason) <= 500)
);

CREATE UNIQUE INDEX IF NOT EXISTS tool_policies_active_key
    ON tool_policies (org_id, scope_kind, scope_value, capability_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS tool_policies_capability_idx
    ON tool_policies (org_id, capability_id)
    WHERE revoked_at IS NULL;

ALTER TABLE capabilities
    ADD COLUMN IF NOT EXISTS policy_relaxable BOOLEAN GENERATED ALWAYS AS (kind = 'mcp_tool') STORED;
