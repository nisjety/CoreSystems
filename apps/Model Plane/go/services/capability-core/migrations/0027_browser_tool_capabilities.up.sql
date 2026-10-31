-- 0027_browser_tool_capabilities.up.sql
--
-- Registers the two capabilities the Do loop's browser tools are governed by (WS21-S5a), and the
-- declared class of each tool, in the 0016 seed format:
--
--   cap.browser.observe  browser.open and browser.observe, read, low risk. Both run under a
--                        read-level BrowserBroker run grant that the model never sees.
--   cap.browser.act      browser.act, external_write, medium risk. It is NOT offered by the Do loop
--                        in this slice; the row exists so that a later slice's call is governed by
--                        its own capability rather than by cap.browser.open.
--
-- WHY A NEW CAPABILITY FOR THE READ TOOLS
--
-- The legacy browser_agent keeps cap.browser.open (high risk, an autonomous loop that plans its own
-- clicks). A Do tool that only opens a page and reads it back is a different authority, and folding
-- it into the high-risk row would put an approval prompt in front of every page read. The grant the
-- read runs under is read-only and host-fenced, so `low` is the honest ceiling for this family; a
-- click or a typed value is `cap.browser.act`.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Source presence is not runtime health. execution-core attests cap.browser.observe only when the
-- BrowserBroker health check and Quarry's readiness both pass AND EXECUTION_DO_BROWSER_ENABLED is on
-- for that instance. cap.browser.act is never attested by this slice. This migration never
-- fabricates an attestation.
--
-- The rows mirror the 0015 layout, so kind-filtered reads and the capabilities_org_kind_name_uq
-- index behave the same as for the other execution-dispatch capabilities.

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
    'cap.browser.observe',
    'global',
    'tool',
    'browser_observe',
    '1.0.0',
    'Open a governed browser page and read it back under a read-only, host-fenced BrowserBroker run grant. The model names a URL and reads the page; grant, run and lease ids never reach it.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.browser.observe:v1',
    jsonb_build_object(
        'dispatch_name', 'browser.open',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array(
            'browser.open',
            'browser.observe',
            'browser.close'
        ),
        'grant_owner', 'browser-broker',
        'grant_level', 'read',
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0027_browser_tool_capabilities',
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
    'cap.browser.act',
    'global',
    'tool',
    'browser_act',
    '1.0.0',
    'Take one governed action on an open browser page (click or type on an observed ref). Reserved for the Do loop''s browser.act, which this slice does not offer; it needs its own grant level and risk gate.',
    'medium',
    'global',
    false,
    true,
    'execution-dispatch:cap.browser.act:v1',
    jsonb_build_object(
        'dispatch_name', 'browser.act',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('browser.act'),
        'grant_owner', 'browser-broker',
        'grant_level', 'act',
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0027_browser_tool_capabilities',
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

-- MOVED, NOT ADDED. 0016 seeded browser.observe and browser.act under cap.browser.open. Those two
-- tool names now resolve to the capabilities above in execution-core, so the old rows would govern
-- nothing and would leave one tool name under two capabilities. cap.browser.open keeps browser_agent.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.browser.open'
  AND tool_name IN ('browser.observe', 'browser.act')
  AND source = 'seed';

-- The declared class of each tool. browser.open and browser.observe are reads: the grant they run
-- under is read-only, and the Do loop never offers browser.act here. A row is re-asserted only while
-- it is still the seed's, so an operator override is never rewritten.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.browser.observe', 'browser.open', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.browser.observe', 'browser.observe', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.browser.observe', 'browser.close', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.browser.act', 'browser.act', 'external_write', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
