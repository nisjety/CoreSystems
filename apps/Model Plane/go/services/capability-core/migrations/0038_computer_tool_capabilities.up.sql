-- 0038_computer_tool_capabilities.up.sql
--
-- Registers the two capabilities the Do loop's computer tools are governed by (W4-C C1), and the declared
-- class of each tool, in the 0027 seed format:
--
--   cap.computer.observe  computer.observe, read, low risk. A look at the screen. The model names no
--                         position and no input reaches it.
--   cap.computer.act      computer.act, external_write, high risk. One click, keystroke or scroll on the
--                         person's computer, which reaches any application, so it has its own capability and
--                         a read can never answer for it.
--
-- STARTS UNAVAILABLE ON PURPOSE. No computer backend is built (F-t3-024), so nothing attests either capability
-- and the gate refuses both names. This migration never fabricates an attestation.
--
-- The approval floor of computer.act stays 'none'. tests/tool_class_registry_parity.rs holds the seed to the
-- name heuristics, and those allow a write under 'auto'. A pause for each act belongs to the executor when it
-- lands, as browser.act's does (WS21-S5b).

INSERT INTO capabilities (
    id,
    org_id,
    kind,
    name,
    version,
    description,
    risk_level,
    scope,
    lazy_load,
    enabled,
    idempotency_key,
    config_json,
    tags,
    enabled_for_scopes,
    rollout_state,
    created_by,
    availability_state,
    availability_reason_code,
    availability_reason,
    execution_mode,
    cost_class,
    health_checked_at
) VALUES (
    'cap.computer.observe',
    'global',
    'tool',
    'computer_observe',
    '1.0.0',
    'Look at the person''s computer screen and read back its elements by reference. The model names no position; what the screen shows is untrusted content.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.computer.observe:v1',
    jsonb_build_object(
        'dispatch_name', 'computer.observe',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('computer.observe'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0038_computer_tool_capabilities',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'bounded',
    NULL
)
ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    version = EXCLUDED.version,
    description = EXCLUDED.description,
    risk_level = EXCLUDED.risk_level,
    scope = EXCLUDED.scope,
    enabled = EXCLUDED.enabled,
    idempotency_key = EXCLUDED.idempotency_key,
    config_json = EXCLUDED.config_json,
    tags = EXCLUDED.tags,
    enabled_for_scopes = EXCLUDED.enabled_for_scopes,
    rollout_state = EXCLUDED.rollout_state,
    cost_class = EXCLUDED.cost_class,
    updated_at = now();

INSERT INTO capabilities (
    id,
    org_id,
    kind,
    name,
    version,
    description,
    risk_level,
    scope,
    lazy_load,
    enabled,
    idempotency_key,
    config_json,
    tags,
    enabled_for_scopes,
    rollout_state,
    created_by,
    availability_state,
    availability_reason_code,
    availability_reason,
    execution_mode,
    cost_class,
    health_checked_at
) VALUES (
    'cap.computer.act',
    'global',
    'tool',
    'computer_act',
    '1.0.0',
    'Take one governed action on the person''s computer: a click, a typed text, a key or a scroll, each on an observed element reference. Reserved for the Do loop''s computer.act, which no backend serves yet; each act needs its own approval before it runs.',
    'high',
    'global',
    false,
    true,
    'execution-dispatch:cap.computer.act:v1',
    jsonb_build_object(
        'dispatch_name', 'computer.act',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('computer.act'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0038_computer_tool_capabilities',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'bounded',
    NULL
)
ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    version = EXCLUDED.version,
    description = EXCLUDED.description,
    risk_level = EXCLUDED.risk_level,
    scope = EXCLUDED.scope,
    enabled = EXCLUDED.enabled,
    idempotency_key = EXCLUDED.idempotency_key,
    config_json = EXCLUDED.config_json,
    tags = EXCLUDED.tags,
    enabled_for_scopes = EXCLUDED.enabled_for_scopes,
    rollout_state = EXCLUDED.rollout_state,
    cost_class = EXCLUDED.cost_class,
    updated_at = now();

-- The declared class of each tool, re-asserted only while a row is still the seed's, so an operator override
-- is never rewritten.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.computer.observe', 'computer.observe', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.computer.act', 'computer.act', 'external_write', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
