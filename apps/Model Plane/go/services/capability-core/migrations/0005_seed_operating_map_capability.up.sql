-- 0005_seed_operating_map_capability.up.sql
--
-- Registers the Model Plane synthesis capability used by Verevon Knowledge's
-- AI Operating Map. Data Plane remains the durable owner of map versions,
-- proposals, wiki publication, retrieval, graph, and source traces.

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
    schema_input,
    schema_output,
    config_json,
    tags,
    enabled_for_scopes,
    rollout_state,
    created_by
) VALUES (
    'operating_map.generate',
    'global',
    'inference',
    'operating_map.generate',
    '1.0.0',
    'Synthesizes evidence-grounded AI Operating Map proposals from Data Plane retrieval, graph, wiki, and source traces, then submits proposals for human review.',
    'medium',
    'workspace',
    false,
    true,
    'operating-map:generate:v1',
    '{
        "type": "object",
        "required": ["org_id"],
        "additionalProperties": false,
        "properties": {
            "org_id": {"type": "string"},
            "generated_from": {"type": "object"},
            "include_departments": {
                "type": "array",
                "items": {"type": "string"}
            },
            "refresh_reason": {"type": "string"}
        }
    }'::jsonb,
    '{
        "type": "object",
        "required": ["proposal_id", "run_id", "status"],
        "properties": {
            "proposal_id": {"type": "string"},
            "run_id": {"type": "string"},
            "status": {"type": "string"},
            "evidence_refs": {
                "type": "array",
                "items": {"type": "object"}
            }
        }
    }'::jsonb,
    '{
        "owner_plane": "model",
        "owner_service": "capability-core",
        "runtime_state": "gateway-wiki-store-proposal-path",
        "data_owner_plane": "data",
        "durable_state_owner": "wiki-store-go",
        "action_id": "operating_map.generate",
        "review_required": true,
        "zero_data_retention_boundary": "data-plane"
    }'::jsonb,
    ARRAY['operating-map','knowledge','retrieval','strategy','human-review'],
    ARRAY['workspace'],
    'stable',
    'migration:0005_seed_operating_map_capability'
)
ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    version = EXCLUDED.version,
    description = EXCLUDED.description,
    risk_level = EXCLUDED.risk_level,
    scope = EXCLUDED.scope,
    lazy_load = EXCLUDED.lazy_load,
    enabled = EXCLUDED.enabled,
    idempotency_key = EXCLUDED.idempotency_key,
    schema_input = EXCLUDED.schema_input,
    schema_output = EXCLUDED.schema_output,
    config_json = EXCLUDED.config_json,
    tags = EXCLUDED.tags,
    enabled_for_scopes = EXCLUDED.enabled_for_scopes,
    rollout_state = EXCLUDED.rollout_state,
    updated_at = now();
