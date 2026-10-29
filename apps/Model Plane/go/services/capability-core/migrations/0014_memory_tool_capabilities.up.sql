-- 0014_memory_tool_capabilities.up.sql
--
-- Registers cap.memory.search and cap.memory.index — the capabilities
-- execution-core's policy binding maps the two memory tools to:
--
--   recall_memory -> cap.memory.search
--   save_memory   -> cap.memory.index
--
-- WHY THIS MIGRATION EXISTS AT ALL
--
-- Both ids have lived in internal/registry/registry.go's static seed since the
-- registry was written — and production resolves capabilities from POSTGRES
-- (cmd/main.go wires the store; EvaluatePolicy reads store.GetForOrg), where
-- these rows never existed. So both tools were advertised to the model,
-- described in the system prompt (SNIPPET_MEMORY_TOOLS), and dispatched by
-- name — and every call died at the fail-closed capability gate with
-- "capability policy unavailable". The static seed satisfying the
-- source-scanning contract test while production reads a table it never
-- reached is the exact registry-vs-migrations split that test now exists to
-- catch; this migration is the durable half.
--
-- risk_level='low': both tools are confined to the tenant's own memory rows
-- via the caller's session credential; save_memory writes one bounded fact,
-- ZDR runs are refused inside the tool itself before any write.
--
-- STARTS UNAVAILABLE ON PURPOSE, exactly like 0008/0013: source presence is
-- not runtime health. execution-core's health reporter attests these two per
-- heartbeat from a live session-core connect probe
-- (health_attest::memory_attestations); this migration must never fabricate
-- an attestation.

INSERT INTO capabilities (
    id, org_id, kind, name, version, description,
    risk_level, scope, lazy_load, enabled,
    idempotency_key, config_json, tags, enabled_for_scopes,
    rollout_state, created_by,
    availability_state, availability_reason_code, availability_reason,
    execution_mode, cost_class, health_checked_at
) VALUES
(
    'cap.memory.search', 'global', 'memory', 'recall_memory', '1.1.0',
    'Search long-term memory for facts saved in earlier conversations. Read-only, tenant-scoped through the calling run''s own session credential.',
    'low', 'global', false, true,
    'execution-dispatch:cap.memory.search:v1',
    jsonb_build_object(
        'dispatch_name', 'recall_memory',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'reads', jsonb_build_object('source', 'session-core.MemoryService', 'writes', false)
    ),
    ARRAY['execution-dispatch'], ARRAY['global'], 'stable',
    'migration:0014_memory_tool_capabilities',
    'unavailable', 'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable', 'bounded', NULL
),
(
    'cap.memory.index', 'global', 'memory', 'save_memory', '1.1.0',
    'Persist one durable fact to long-term memory. Refused inside the tool on Zero-Data-Retention runs before any write.',
    'low', 'global', false, true,
    'execution-dispatch:cap.memory.index:v1',
    jsonb_build_object(
        'dispatch_name', 'save_memory',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'writes', jsonb_build_object('target', 'session-core.MemoryService', 'zdr', 'refused_in_tool')
    ),
    ARRAY['execution-dispatch'], ARRAY['global'], 'stable',
    'migration:0014_memory_tool_capabilities',
    'unavailable', 'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable', 'bounded', NULL
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
