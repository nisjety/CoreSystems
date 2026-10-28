-- 0018_workspace_read_capability.up.sql
--
-- Registers cap.workspace.read, the capability the policy engine evaluates for
-- the four read-only Space workspace tools (WS24-S3): workspace_read,
-- workspace_grep, workspace_glob and workspace_list.
--
-- WHY ONE ROW FOR FOUR TOOL NAMES
--
-- They are one authority over one object, the Space's hydrated workspace. A
-- caller that may read a file may also search, list and glob the same tree, and
-- a row per name would make "may list but not read" reachable by operator
-- error. capability_policy::trusted_capability_id maps all four names here, and
-- capability_tools carries one seed row per name, below.
--
-- WHY risk_level='low', STATED RATHER THAN ASSERTED
--
-- The tools never write, never reach the network and never run code. The bytes
-- they return are the same bytes the Space's code_interpreter already reads from
-- the same hydrated tree (cap.command.sandbox, seeded low by 0010), so a read
-- tool adds no reach beyond a capability an org already grants at low risk. The
-- files were redacted at upload, so the tree holds no more secret text than the
-- code_interpreter sandbox does, and output is bounded by the workspace reader's
-- line, byte and result caps.
--
-- WHAT THE ROW DOES NOT GRANT
--
-- The capability is not the whole gate. A read needs a Space lease, which
-- sandbox-manager issues only for the user's own delegated bearer and only for a
-- Space Control has entitled. The tools are also offered only when the
-- deployment switches WORKSPACE_TOOLS on (execution-core, default off), and never
-- to a zero-retention run.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Exactly like 0010, 0013, 0014 and 0015: source presence is not runtime health.
-- execution-core attests this id only while the workspace store and the
-- sandbox-manager are configured AND WORKSPACE_TOOLS is switched on, so an
-- instance that does not offer the tools cannot report that it does. This
-- migration must never fabricate an attestation.
--
-- The row carries its tools' class and floor (read, none) from its first insert,
-- so the ceiling is right on the first boot and not only after the next seed
-- pass in 0016. Nothing here names a write tool: writes are a separate
-- capability, added with WS24-S4, so an org can disable writes and keep reads.

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
    'cap.workspace.read',
    'global',
    'tool',
    'workspace_read',
    '1.0.0',
    'Read a Space''s hydrated workspace: a line window of one file with its version token, a regular-expression search, a directory listing and a glob. Read-only, with no network and bounded output. A run sees the Space tree as it stood when the run began, plus its own edits.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.workspace.read:v1',
    jsonb_build_object(
        'dispatch_name', 'workspace_read',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array(
            'workspace_read',
            'workspace_grep',
            'workspace_glob',
            'workspace_list'
        ),
        'isolation', jsonb_build_object(
            'workspace', 'lease_hydrated_space_workspace',
            'writes', 'none',
            'network', 'disabled',
            'symlinks', 'refused',
            'host_command_execution', false
        )
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0018_workspace_read_capability',
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

-- One row per tool, in the 0016 seed format, one tuple per line so the seed
-- tests read the declarations as text. Re-asserted only while the row is still
-- the seed's, so an operator override is never rewritten.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.workspace.read', 'workspace_glob', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.workspace.read', 'workspace_grep', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.workspace.read', 'workspace_list', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.workspace.read', 'workspace_read', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
