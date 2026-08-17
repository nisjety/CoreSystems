-- 0011_conversation_ticket_action_capability.up.sql
--
-- Registers the execution-core -> Control -> Conversation Core owner-action
-- adapter. This is NOT a generic Application Plane network capability: the
-- adapter can create only `tickets.create`, derives its retry key, obtains a
-- short-lived Control decision from the source run, and Conversation Core
-- verifies the decision plus the current target conversation before writing.
--
-- The effect is medium risk. It cannot affect a third-party provider, assign a
-- ticket, change SLA/timestamps, or select an actor/tenant; Control's current
-- agent-action entitlement and the owning Conversation Core form the two
-- separate authorization gates. It nevertheless starts unavailable: source
-- code or an environment variable never proves that the Control and owner
-- paths, decision key, and dedicated service credentials are live.

INSERT INTO capabilities (
    id, org_id, kind, name, version, description, risk_level, scope,
    lazy_load, enabled, idempotency_key, config_json, tags,
    enabled_for_scopes, rollout_state, created_by, availability_state,
    availability_reason_code, availability_reason, execution_mode, cost_class,
    health_checked_at
) VALUES (
    'cap.tool.ticket.create',
    'global',
    'tool',
    'tickets.create',
    '1.0.0',
    'Create a Conversation Core ticket only through a fresh Control run-action decision and the owner''s current target authorization.',
    'medium',
    'global',
    false,
    true,
    'execution-dispatch:cap.tool.ticket.create:v1',
    jsonb_build_object(
        'dispatch_name', 'tickets.create',
        'owner_plane', 'application',
        'owner_service', 'conversation-core',
        'decision_authority', 'control-plane-user-core',
        'target_authorization', 'conversation-core-current-resource-check',
        'model_selectable_fields', jsonb_build_array('conversation_id', 'work_type', 'priority', 'severity', 'category', 'intent'),
        'model_forbidden_fields', jsonb_build_array('actor', 'org', 'idempotency_key', 'assignment', 'timestamps', 'source', 'sla', 'labels')
    ),
    ARRAY['execution-dispatch', 'owner-action'],
    ARRAY['global'],
    'stable',
    'migration:0011_conversation_ticket_action_capability',
    'unavailable',
    'health_not_attested',
    'Ticket action runtime health has not been attested.',
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
