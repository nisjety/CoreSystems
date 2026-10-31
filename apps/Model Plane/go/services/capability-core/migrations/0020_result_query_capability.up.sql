-- 0020_result_query_capability.up.sql
--
-- Registers cap.result.query, the capability the policy engine evaluates for
-- execution-core's `result_query` (WS25-S5). The tool reads back a result the
-- SAME run parked under a handle because it was too large to show in full. The
-- store is in-process and scoped to that run, so the tool reads nothing from
-- another run, tenant or user and writes nothing.
--
-- WHY A READ CLASS WITH NO APPROVAL FLOOR
--
-- The parked text is the output of a read-class tool the run already made, and
-- the label it is read under keeps that tool's source and screening verdict. A
-- read of it is no more consequential than the original read, so it is `read`
-- with an `none` floor, and a second approval prompt would only teach people to
-- click through both.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- As for every row in 0008 and 0013: source presence is not runtime health. An
-- authenticated health authority must attest this capability before the gate
-- allows it, and this migration never fabricates an attestation.
--
-- kind='tool' and the dispatch-name convention follow 0008/0010/0011/0013.
-- The class columns come from 0016: they are set here directly because this row
-- is created after the 0016 ceiling pass has already run for the boot.

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
    health_checked_at,
    side_effect_class,
    approval_floor
) VALUES (
    'cap.result.query',
    'global',
    'tool',
    'result_query',
    '1.0.0',
    'Read back a result this run held under a handle because it was too large to show in full. Read-only, scoped to the run that parked it.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.result.query:v1',
    jsonb_build_object(
        'dispatch_name', 'result_query',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'reads', jsonb_build_object(
            'source', 'execution-core.run-scoped-result-store',
            'scope', 'this_run_only',
            'writes', false
        )
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0020_result_query_capability',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'bounded',
    NULL,
    'read',
    'none'
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

-- The declared class of the one tool this capability governs, in the 0016
-- seed format so the registry's seed reader can see it. It is re-asserted only
-- while the row is still the seed's, so an operator override is never rewritten.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.result.query', 'result_query', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
