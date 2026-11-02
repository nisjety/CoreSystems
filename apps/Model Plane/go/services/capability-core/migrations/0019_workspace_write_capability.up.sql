-- 0019_workspace_write_capability.up.sql
--
-- Registers cap.workspace.write, the capability the policy engine evaluates for
-- the two Space workspace write tools (WS24-S4): workspace_edit and
-- workspace_put.
--
-- WHY A SEPARATE ROW FROM THE READS
--
-- Reading a Space's tree and changing it are different authorities, and an org
-- may want writes switched off while the reads stay on. 0018 keeps the four read
-- names on cap.workspace.read; these two names are bound only to this row, so an
-- org can disable it alone.
--
-- WHY context_write AND NO APPROVAL FLOOR
--
-- A write lands only in the calling run's own overlay. No caller here promotes it
-- to the Space, other runs do not see it, and every write result says so
-- (durability run_overlay). That is the class save_memory already holds: a
-- durable change confined to the run's own context. Plan mode and delegated
-- subagents refuse it (permission::is_restricted_context_write), and a normal run
-- is not stopped for a person, so the floor is none (DEC-24, D2a).
--
-- WHY risk_level='low', STATED RATHER THAN ASSERTED
--
-- The write is bounded to one file of at most 1 MiB, refuses text that matches a
-- secret pattern, is redacted before the overlay is stored, and never reaches the
-- network or runs code. The bytes it writes are the bytes code_interpreter already
-- writes to the same hydrated tree (cap.command.sandbox, seeded low by 0010).
--
-- WHAT THE ROW DOES NOT GRANT
--
-- The capability is not the whole gate. A write needs the same Space lease as a
-- read, which sandbox-manager issues only for the user's own delegated bearer and
-- only for a Space Control has entitled. The tools are offered only when the
-- deployment switches WORKSPACE_TOOLS to all (execution-core, default off), never
-- to a zero-retention run, never in plan mode and never to a delegated subagent.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Exactly like 0008, 0010, 0013, 0014, 0015 and 0018: source presence is not
-- runtime health. execution-core attests this id only while the workspace store
-- and the sandbox-manager are configured AND WORKSPACE_TOOLS is all, so an
-- instance that does not offer the writes cannot report that it does. This
-- migration must never fabricate an attestation.
--
-- The row carries its tools' class and floor (context_write, none) from its first
-- insert, so the ceiling is right on the first boot and not only after the next
-- seed pass in 0016.

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
    'cap.workspace.write',
    'global',
    'tool',
    'workspace_edit',
    '1.0.0',
    'Change a Space''s hydrated workspace as a per-run overlay: replace exact text in one existing file against its version token, or create or overwrite one file. Other runs of the Space do not see the change, and nothing promotes it to the Space. No network and no code.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.workspace.write:v1',
    jsonb_build_object(
        'dispatch_name', 'workspace_edit',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array(
            'workspace_edit',
            'workspace_put'
        ),
        'isolation', jsonb_build_object(
            'workspace', 'lease_hydrated_space_workspace',
            'writes', 'run_overlay_only',
            'promotion', 'none',
            'network', 'disabled',
            'symlinks', 'refused',
            'host_command_execution', false
        )
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0019_workspace_write_capability',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'bounded',
    NULL,
    'context_write',
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
    ('global', 'cap.workspace.write', 'workspace_edit', 'context_write', 'none', NULL, 'seed'),
    ('global', 'cap.workspace.write', 'workspace_put', 'context_write', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
