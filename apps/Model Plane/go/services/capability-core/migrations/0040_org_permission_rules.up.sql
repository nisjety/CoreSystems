-- The org permission rules (build plan W5-074-b, F-t3-074, DEC-15).
--
-- One row per deny or ask rule an org admin writes. The EvaluatePolicy gate reads an org's rules only while
-- CAPABILITY_PERMISSION_RULES_ENABLED is the literal true on capability-core; until then nothing reads this table.
-- A rule narrows a call and never widens one: there is no allow, so the decision check admits only deny and ask.
--
-- A rule matches a named tool call on two things:
--   tool_pattern    "*" (any tool, including a call that names none), a "prefix*" glob or an exact tool name.
--   target_kind     what the call acts on, derived by execution-core from the tool input: "domain" (the host of a web
--                   address), "recipient" (the domain of a mail recipient, never the full address), "command_prefix"
--                   (the first token of a command), or "any" (every call of the tool, whatever its target).
--   target_pattern  "*" (any value of the kind), an exact value, or "*.suffix" for a domain (the suffix itself and its
--                   subdomains). A target_kind of "any" carries "*", so no row reads as a value match it does not make.
--
-- revision is the org's policy revision the rule was written under. The admin write arrives with the admin API
-- (W5-074-c); this migration only creates the table.
--
-- Every row is keyed by org_id, and every query in the reader filters on it. RLS is not applied here (the neighbouring
-- capability-core tables do not use it).
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is idempotent.

CREATE TABLE IF NOT EXISTS org_permission_rules (
    rule_id        UUID PRIMARY KEY,
    org_id         TEXT NOT NULL,
    tool_pattern   TEXT NOT NULL,
    target_kind    TEXT NOT NULL,
    target_pattern TEXT NOT NULL,
    decision       TEXT NOT NULL,
    created_by     TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    revision       BIGINT NOT NULL DEFAULT 1,
    CONSTRAINT org_permission_rules_tool_pattern_check
        CHECK (char_length(tool_pattern) BETWEEN 1 AND 200),
    CONSTRAINT org_permission_rules_target_kind_check
        CHECK (target_kind IN ('domain', 'recipient', 'command_prefix', 'any')),
    CONSTRAINT org_permission_rules_target_pattern_check
        CHECK (char_length(target_pattern) BETWEEN 1 AND 200),
    CONSTRAINT org_permission_rules_any_target_check
        CHECK (target_kind <> 'any' OR target_pattern = '*'),
    CONSTRAINT org_permission_rules_decision_check
        CHECK (decision IN ('deny', 'ask'))
);

CREATE INDEX IF NOT EXISTS org_permission_rules_org_idx
    ON org_permission_rules (org_id);
