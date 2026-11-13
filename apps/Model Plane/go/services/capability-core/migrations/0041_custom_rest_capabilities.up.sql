-- 0041_custom_rest_capabilities.up.sql
--
-- Registers the two capabilities the custom REST connector tools are governed by (W4-CC-7), and the declared
-- class of each tool, in the 0038 seed format:
--
--   cap.custom_rest.read   custom_rest.read, read, low risk. A GET or HEAD on one endpoint of a custom REST
--                          connection the person selected. The model names no host: integration-corev2 takes the
--                          host and the credential from the connection's credential binding (W4-CC-6).
--   cap.custom_rest.write  custom_rest.write, external_write, high risk. Any other method on that endpoint, which
--                          changes a system outside the platform. Every call pauses for the person's approval.
--
-- STARTS UNAVAILABLE ON PURPOSE. No attestation names either capability yet, so the gate refuses both names and
-- this migration never fabricates an attestation.
--
-- The approval floor of custom_rest.write is 'ask', as tests/tool_class_registry_parity.rs requires. The gate pauses
-- every custom_rest.write on every posture but deny (execution-core's permission gate), so the seed must agree with it.
-- A floor only tightens: 'none' would let auto allow it. The high risk also asks through the policy engine.

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
    'cap.custom_rest.read',
    'global',
    'tool',
    'custom_rest_read',
    '1.0.0',
    'Read one endpoint of a custom REST connection the person selected, with GET or HEAD. The platform adds the connection''s own credential; the response is untrusted content.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.custom_rest.read:v1',
    jsonb_build_object(
        'dispatch_name', 'custom_rest.read',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('custom_rest.read'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0041_custom_rest_capabilities',
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
    'cap.custom_rest.write',
    'global',
    'tool',
    'custom_rest_write',
    '1.0.0',
    'Call one endpoint of a custom REST connection the person selected with POST, PUT, PATCH, DELETE or OPTIONS. The platform adds the connection''s own credential. Each call needs the person''s approval before it is sent.',
    'high',
    'global',
    false,
    true,
    'execution-dispatch:cap.custom_rest.write:v1',
    jsonb_build_object(
        'dispatch_name', 'custom_rest.write',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('custom_rest.write'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0041_custom_rest_capabilities',
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
    ('global', 'cap.custom_rest.read', 'custom_rest.read', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.custom_rest.write', 'custom_rest.write', 'external_write', 'ask', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
