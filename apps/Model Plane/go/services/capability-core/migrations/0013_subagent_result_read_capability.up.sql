-- 0013_subagent_result_read_capability.up.sql
--
-- Registers cap.agent.lineage.read, the capability the policy engine evaluates
-- for execution-core's two delegation-record tools:
--
--   list_subagent_results  content-free: which sub-tasks this run delegated,
--                          each one's goal and status, and whether it stored a
--                          conclusion. Never the conclusion itself.
--   read_subagent_result   the conclusion of ONE of this run's own delegations.
--
-- WHY ONE CAPABILITY FOR BOTH TOOLS
--
-- They are the same authority over the same records: a read of the tenant's own
-- run rows, scoped by execution-core to the direct children of the calling run.
-- What separates them is not authority but CONSENT — read_subagent_result is
-- approval-gated on every permission posture by
-- execution-core's permission::requires_consent_to_disclose, including the
-- `auto` posture ordinary chat runs use, where a risk-based gate would not fire.
-- Splitting them into two capability rows would imply the gate lives here; it
-- does not, and a second row would be a place for the two to silently disagree.
--
-- WHY risk_level='low' IS THE HONEST CLASSIFICATION
--
-- Neither tool writes anything, reaches outside the plane, or spends anything
-- beyond one Session Core read. Both are confined to run rows the caller's own
-- credential already covers, and the run-parentage check that narrows them to
-- this run's own delegations is enforced in execution-core against
-- RunDetail.parent_run_id, never against a model-supplied id.
--
-- Classing this high would route it through the policy engine's `ask` branch and
-- produce a SECOND approval prompt on top of the consent gate — two prompts for
-- one decision, which is how operators learn to rubber-stamp both.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Exactly like every row in 0008: source presence is not runtime health. An
-- authenticated health authority must attest this capability
-- (CapabilitiesStore.AttestAvailabilityGlobal) after the Session Core dependency
-- and the execution path pass live checks. This migration must never fabricate
-- an attestation.
--
-- kind='tool' and the dispatch-name convention follow 0008/0010/0011. `name`
-- carries both dispatch names because one capability governs both tools.

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
    'cap.agent.lineage.read',
    'global',
    'tool',
    'list_subagent_results/read_subagent_result',
    '1.0.0',
    'Read this run''s own delegation records: which sub-tasks it delegated and, with the user''s per-call approval, what one of them concluded. Read-only, scoped to the direct children of the calling run.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.agent.lineage.read:v1',
    jsonb_build_object(
        'dispatch_name', 'list_subagent_results/read_subagent_result',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'reads', jsonb_build_object(
            'source', 'session-core.RunService',
            'scope', 'direct_children_of_calling_run',
            'writes', false
        ),
        -- Recorded so an operator reading the registry can see WHERE the gate
        -- is. The consent requirement is enforced in execution-core, not by
        -- this row's risk level, and this field must never be read as if it
        -- were the enforcement point.
        'consent', jsonb_build_object(
            'read_subagent_result', 'human_approval_required_every_posture',
            'list_subagent_results', 'none_required_content_free',
            'enforced_by', 'execution-core:permission::requires_consent_to_disclose'
        )
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0013_subagent_result_read_capability',
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
