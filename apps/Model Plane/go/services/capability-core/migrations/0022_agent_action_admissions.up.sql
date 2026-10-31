-- 0022_agent_action_admissions.up.sql
--
-- The agent action admission table (WS24-S6). One row per owner action the Model may be
-- offered: how it is classified, which capability governs it and the schema hash the owner
-- contract binds. A row alone admits nothing. Execution Core also pins the action's name,
-- schema hash, parameters and adapter in code, and the owner verifies every call.
--
-- status: enabled rows can be offered (only when MODEL_ACTION_ADMISSION=table); declared rows
-- name an action with no owner executor yet and are never offered; retired rows are kept for
-- history. Seeds never overwrite a row an operator has changed, so re-applying this file at boot
-- is safe.
--
-- The tickets.create row carries the same parameters, description and schema hash as the
-- constants in internal/api/model_action_view.go. A test pins that equality.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is
-- idempotent.

CREATE TABLE IF NOT EXISTS agent_action_admissions (
    action_id           TEXT PRIMARY KEY,
    tool_name           TEXT NOT NULL,
    capability_id       TEXT NOT NULL,
    model_access        TEXT NOT NULL,
    side_effect_class   TEXT NOT NULL,
    approval_floor      TEXT NOT NULL,
    authority           TEXT NOT NULL,
    hash_basis          TEXT NOT NULL,
    action_schema_hash  TEXT NOT NULL DEFAULT '',
    parameters_json     TEXT NOT NULL DEFAULT '{}',
    description         TEXT NOT NULL DEFAULT '',
    requires_approval   BOOLEAN NOT NULL DEFAULT FALSE,
    status              TEXT NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT agent_action_admissions_status_check
        CHECK (status IN ('enabled', 'declared', 'retired')),
    CONSTRAINT agent_action_admissions_hash_basis_check
        CHECK (hash_basis IN ('owner_contract', 'catalog_entry')),
    CONSTRAINT agent_action_admissions_enabled_check
        CHECK (status <> 'enabled' OR (tool_name <> '' AND action_schema_hash ~ '^sha256:[0-9a-f]{64}$'))
);

INSERT INTO agent_action_admissions
    (action_id, tool_name, capability_id, model_access, side_effect_class, approval_floor,
     authority, hash_basis, action_schema_hash, parameters_json, description, requires_approval, status)
VALUES
    ('tickets.create', 'tickets.create', 'cap.tool.ticket.create', 'write_approval', 'owner_effect',
     'durable_owner', 'control_run_decision', 'owner_contract',
     'sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df',
     '{"type":"object","additionalProperties":false,"properties":{"conversation_id":{"type":"string","maxLength":200},"work_type":{"type":"string","enum":["","customer_case","internal_work","incident"]},"priority":{"type":"string","enum":["","low","normal","high","urgent"]},"severity":{"type":"string","enum":["","low","medium","high","critical"]},"category":{"type":"string","maxLength":80},"intent":{"type":"string","maxLength":240}},"required":["conversation_id"]}',
     'Create one governed ticket in a Conversation Core conversation. This always pauses for durable human approval and the owner independently rechecks current authority and its target resource grant before writing.',
     TRUE, 'enabled'),
    ('brreg.lookup_organization', 'brreg.lookup_organization', 'cap.tool.information.read', 'read', 'read',
     'none', 'delegated_user_bearer', 'catalog_entry', '', '{}',
     'Look up a company in the Brreg company register.', FALSE, 'declared'),
    ('shipping.get_quotes', 'shipping.get_quotes', 'cap.tool.shipping.read', 'read', 'read',
     'none', 'delegated_user_bearer', 'catalog_entry', '', '{}',
     'Get shipping quotes for a shipment.', FALSE, 'declared')
ON CONFLICT (action_id) DO NOTHING;
