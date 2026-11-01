-- "Always" as a reviewed revocable rule (build plan WS13 S7).
--
-- An "Always" choice on a pinned MCP tool's approval records a thread grant, exactly as "this
-- thread" does (migration 0062), and also proposes a standing rule for the person to confirm in
-- settings. The proposal is a row here, and its grant names it. A proposal is not a rule: nothing
-- takes effect until the person confirms it, and confirming writes a digest-pinned user-scope
-- `allow` in capability-core (its expiry is migration 0030).
--
--   tool_name, tool_digest   the tool the proposal is about and the sha256 of the definition the
--                            person reviewed. A confirmed rule applies only while the tool keeps
--                            that digest: capability-core checks it on every call.
--   state, resolved_at       proposed until the person confirms or dismisses it. Resolution is
--                            written once and sets its time.
--   created_from_approval_id the decision that proposed it, erased with it.
--
-- One open proposal per owner and tool definition: a later "Always" for the same tool reuses it.
-- A zero-retention or waived run never proposes one (DEC-32), because its grant is refused first.
-- Erasing a grant does not erase a proposal, and erasing a proposal clears its grants' link only.
-- The reversal is in revert/0070_approval_grant_rule_link.down.sql.

CREATE TABLE IF NOT EXISTS approval_rule_proposals (
    id TEXT PRIMARY KEY
        CHECK (id LIKE 'arp\_%'),
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    created_from_approval_id TEXT NOT NULL
        REFERENCES approvals(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL
        CHECK (length(tool_name) BETWEEN 1 AND 256),
    tool_digest TEXT NOT NULL
        CHECK (tool_digest ~ '^[0-9a-f]{64}$'),
    state TEXT NOT NULL DEFAULT 'proposed'
        CHECK (state IN ('proposed', 'confirmed', 'dismissed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    -- A proposal is resolved once, and only resolution sets its time.
    CONSTRAINT approval_rule_proposals_resolve_shape
        CHECK ((state = 'proposed') = (resolved_at IS NULL))
);

-- One open proposal per owner and tool definition.
CREATE UNIQUE INDEX IF NOT EXISTS approval_rule_proposals_open_idx
    ON approval_rule_proposals (org_id, user_id, tool_name, tool_digest)
    WHERE state = 'proposed';

-- The thread grant an "Always" decision created names the proposal it came with.
ALTER TABLE approval_scope_grants
    ADD COLUMN IF NOT EXISTS rule_id TEXT
        REFERENCES approval_rule_proposals(id) ON DELETE SET NULL;
