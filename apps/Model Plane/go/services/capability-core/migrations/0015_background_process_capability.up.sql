-- 0015_background_process_capability.up.sql
--
-- Registers cap.process.background, the capability the policy engine evaluates
-- for execution-core's S4.2 `process_*` tool family (`process_start`,
-- `process_read`, `process_stdin`, `process_signal`, `process_list`).
--
-- WHY ONE ROW FOR FIVE TOOL NAMES
--
-- They are one authority over one object, not five authorities. A caller that
-- may start a background process may obviously read what it printed, feed it
-- stdin, stop it, and list its own Space's processes; splitting those into
-- separate capabilities would create rows an operator could set
-- inconsistently — "may start but may not stop" is not a posture anyone wants,
-- and it is the posture a five-row model makes reachable by accident.
-- `capability_policy::trusted_capability_id` maps all five names here, and the
-- `dispatch_name` below records the family's entry point, matching the 0008
-- convention of naming the row after the execution-core dispatch name.
--
-- WHY risk_level='low', STATED RATHER THAN ASSERTED
--
-- This row's honest comparison is not cap.command.shell (arbitrary
-- operator-allowlisted host commands with the run's own filesystem and network
-- reach, correctly `high` and correctly `ask`). It is cap.command.sandbox,
-- seeded by 0010 at `low`. A background process runs the same code body — never
-- a named host command — under the same bubblewrap argv, which means it
-- inherits every constraint 0010's rationale rests on:
--   * read-only root filesystem
--   * networking disabled - no egress, no lateral reach, no exfiltration path
--   * secret-scrubbed output
--   * no path from the code body to a named host command
--
-- The residual blast radius OVER cap.command.sandbox is exactly two things,
-- and both are bounded and operator-tunable:
--   * duration - a per-call sandbox dies with its call; this one lives until
--     its TTL (<= 1 hour, and never past the lease's own expiry), so the
--     ceiling is wall-clock time in a container that is destroyed either way.
--   * concurrency - a per-call sandbox is one process; a Space may hold
--     several, bounded by a registry-enforced count limit.
-- The workspace differs too, but in the direction of LESS exposure to the host:
-- the process writes only its own Space's hydrated workspace and a scratch
-- directory outside it, never the image.
--
-- Routing that through the high-risk `ask` branch would put an approval prompt
-- in front of every long computation, which is the approval-fatigue argument
-- 0010 already made against doing it to a calculator - and it would degrade the
-- gate cap.command.shell genuinely needs. If product wants `ask` for the first
-- slice, this is the one field to change; nothing else in the design depends on
-- the value.
--
-- THE CAPABILITY IS NOT THE ONLY GATE
--
-- Deliberately so, and worth stating because `low` reads permissive on its own.
-- A `process_start` call must ALSO hold a Space lease whose `processes_
-- permitted` column is true, which sandbox-manager sets only when Control's
-- signed capability decision carried `space:processes` - granted only to a
-- Space with `process_registry_entitled` (user-core migration 028, default
-- FALSE). So this row governs "may this org's tool loop reach the family at
-- all"; per-Space authority is a separate, deny-by-default decision that this
-- migration cannot and does not grant.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Exactly like 0008, 0010, 0013 and 0014: source presence is not runtime
-- health. execution-core's health reporter attests this id only when the
-- bubblewrap and interpreter probes pass AND the process host is enabled on
-- that instance (EXECUTION_CORE_PROCESS_HOST=enabled) - an instance that cannot
-- host a background process must not report that it can. This migration must
-- never fabricate an attestation.
--
-- kind='tool' and scope='global' mirror the live cap.command.sandbox row from
-- 0010, so kind-filtered reads and the capabilities_org_kind_name_uq index
-- behave identically for both. The in-process static seed in
-- internal/registry/registry.go declares the same capability with
-- models.KindCommand and workspace scope, mirroring how cap.command.sandbox is
-- declared there; that divergence is pre-existing and deliberate on both sides.

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
    'cap.process.background',
    'global',
    'tool',
    'process_start',
    '1.0.0',
    'Start and manage a background process in a Space''s hydrated sandbox workspace: read-only root filesystem, networking disabled, TTL-bounded lifetime (never past the lease), count-limited concurrency, and secret-scrubbed durable output with head+tail retention. Accepts a code body, never a named host command, and requires a Space lease that Control separately granted background-process authority.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.process.background:v1',
    jsonb_build_object(
        'dispatch_name', 'process_start',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array(
            'process_start',
            'process_read',
            'process_stdin',
            'process_signal',
            'process_list'
        ),
        -- The isolation blob states the DIFFERENCES from cap.command.sandbox
        -- first, then repeats the properties that are unchanged, so a reader
        -- comparing the two rows can see at a glance what this capability
        -- actually adds.
        'isolation', jsonb_build_object(
            'workspace', 'lease_hydrated_space_workspace',
            'lifetime', 'ttl_bounded_background',
            'output', 'redacted_durable_head_tail',
            'concurrency', 'registry_count_bounded',
            'network', 'disabled',
            'rootfs', 'read_only',
            'host_command_execution', false
        ),
        'requires_space_authority', 'space:processes'
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0015_background_process_capability',
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
