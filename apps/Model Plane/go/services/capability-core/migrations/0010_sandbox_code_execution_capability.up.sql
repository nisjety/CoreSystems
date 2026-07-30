-- 0010_sandbox_code_execution_capability.up.sql
--
-- Registers cap.command.sandbox, the capability the policy engine evaluates
-- for execution-core's hermetic `code_interpreter` tool.
--
-- WHY THIS IS A SEPARATE ROW FROM cap.command.shell
--
-- These are not two settings of one tool. cap.command.shell (seeded by
-- 0008_execution_dispatch_capabilities) stays risk_level='high' because it
-- runs an arbitrary operator-allowlisted command with the run's own
-- filesystem and network reach. internal/policy engine.EvaluateCapability
-- returns `ask` for every high-risk capability, and that is exactly right for
-- shell: arbitrary command execution must stay human-approved. Nothing in this
-- migration touches, relaxes, or re-risks that row.
--
-- WHY risk_level='low' IS THE HONEST CLASSIFICATION HERE
--
-- The sandbox executor accepts only a code body, never a host command name,
-- and runs it under constraints the caller and the model cannot select,
-- widen, or opt out of:
--   * read-only root filesystem - the code cannot mutate or persist into the
--     image, so it cannot leave an implant behind for a later call
--   * networking disabled - no egress, no lateral reach inside the plane, and
--     therefore no exfiltration path for anything it does compute
--   * wall-clock timeout - the process is killed on expiry, so worst-case
--     spend is bounded by that ceiling rather than by model behavior
--   * per-call throwaway workspace - created for the call and discarded after
--     it, so nothing carries across invocations, runs, or tenants
--   * secret-scrubbed stdout/stderr - captured output cannot ferry credentials
--     back into the transcript
-- With no egress, no writable image, no cross-call persistence, and no path
-- from the code body to a named host command, the residual blast radius is
-- bounded CPU and wall-clock time inside a container that is destroyed either
-- way. Sending that through the high-risk `ask` branch would make a
-- calculator step `awaiting_approval` and train operators to rubber-stamp
-- approval prompts, which degrades the gate cap.command.shell genuinely needs.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Exactly like every row in 0008: source presence is not runtime health. An
-- authenticated health authority must attest this capability
-- (CapabilitiesStore.AttestAvailabilityGlobal) after the sandbox dependency
-- and the execution path pass live checks. This migration must never fabricate
-- an attestation.
--
-- kind='tool' matches the live cap.command.shell row and the rest of the
-- execution-dispatch catalog seeded by 0008, so kind-filtered reads and the
-- capabilities_org_kind_name_uq index behave identically for both rows. The
-- in-process static seed in internal/registry/registry.go declares the same
-- capability with models.KindCommand, mirroring how cap.command.shell is
-- declared there; that divergence is pre-existing and deliberate on both
-- sides.
--
-- `name` carries the execution-core dispatch name (`code_interpreter`), which
-- is the 0008 convention (cap.command.shell is named `shell`, not
-- "Shell Command").

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
    'cap.command.sandbox',
    'global',
    'tool',
    'code_interpreter',
    '1.0.0',
    'Run a code body in a hermetic per-call sandbox: read-only root filesystem, networking disabled, wall-clock timeout, throwaway workspace discarded after the call, and secret-scrubbed output. Cannot execute a named host command and cannot persist between calls.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.command.sandbox:v1',
    jsonb_build_object(
        'dispatch_name', 'code_interpreter',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'isolation', jsonb_build_object(
            'network', 'disabled',
            'rootfs', 'read_only',
            'workspace', 'per_call_throwaway',
            'wall_clock_timeout', true,
            'output_secret_scrubbed', true,
            'host_command_execution', false
        )
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0010_sandbox_code_execution_capability',
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
