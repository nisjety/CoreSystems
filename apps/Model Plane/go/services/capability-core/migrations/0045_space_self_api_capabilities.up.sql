-- 0045_space_self_api_capabilities.up.sql
--
-- WS18-S2: the five Space run-action capabilities an agent may be offered on its own
-- thread (Phase 1 rename, pin and post_card; Phase 2 pause_self; Phase 3 follow_up). Each
-- row starts unavailable with reason health_not_attested. Only the owner of the action
-- attests its health, and the owner-action lane names that attester per capability in
-- internal/api/availability.go. A row alone admits nothing: Control must sign it for a run,
-- the golden registry must agree with it, and its admission row must be enabled.
--
-- The matching admission rows (agent_action_admissions, migration 0022) carry the same
-- schema hash and parameters bytes as the golden registry. Phase 1 rename and pin are
-- enabled. post_card is declared until WS15-S9 lands, and pause_self and follow_up are
-- declared until their owners exist. A test pins these rows against the golden.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot. The capability
-- seed keeps the availability columns and enabled flag an operator may have changed, and
-- the admission seed never overwrites a row.

INSERT INTO capabilities (
    id, org_id, kind, name, version, description, risk_level, scope,
    lazy_load, enabled, idempotency_key, config_json, tags,
    enabled_for_scopes, rollout_state, created_by, availability_state,
    availability_reason_code, availability_reason, execution_mode, cost_class,
    health_checked_at
) VALUES
    (
        'cap.tool.space.thread.rename',
        'global',
        'tool',
        'space.thread.rename',
        '1.0.0',
        'Rename this Space thread to a short title, only through a fresh Control run-action decision. A title a person set by hand is never replaced.',
        'low',
        'global',
        false,
        true,
        'space-self-api:cap.tool.space.thread.rename:v1',
        jsonb_build_object(
            'dispatch_name', 'space.thread.rename',
            'owner_plane', 'model',
            'owner_service', 'session-core',
            'decision_authority', 'control-plane-user-core',
            'target_authorization', 'session-core-run-thread-owner',
            'model_selectable_fields', jsonb_build_array('title'),
            'model_forbidden_fields', jsonb_build_array('thread_id', 'space', 'actor', 'org', 'run', 'digest', 'idempotency_key')
        ),
        ARRAY['space-self-api', 'owner-action'],
        ARRAY['global'],
        'stable',
        'migration:0045_space_self_api_capabilities',
        'unavailable',
        'health_not_attested',
        'Space thread rename runtime health has not been attested by its owner.',
        'unavailable',
        'bounded',
        NULL
    ),
    (
        'cap.tool.space.thread.pin',
        'global',
        'tool',
        'space.thread.pin',
        '1.0.0',
        'Pin or unpin this Space thread, only through a fresh Control run-action decision. A pin a person set by hand is never changed.',
        'low',
        'global',
        false,
        true,
        'space-self-api:cap.tool.space.thread.pin:v1',
        jsonb_build_object(
            'dispatch_name', 'space.thread.pin',
            'owner_plane', 'model',
            'owner_service', 'session-core',
            'decision_authority', 'control-plane-user-core',
            'target_authorization', 'session-core-run-thread-owner',
            'model_selectable_fields', jsonb_build_array('pinned'),
            'model_forbidden_fields', jsonb_build_array('thread_id', 'space', 'actor', 'org', 'run', 'digest', 'idempotency_key')
        ),
        ARRAY['space-self-api', 'owner-action'],
        ARRAY['global'],
        'stable',
        'migration:0045_space_self_api_capabilities',
        'unavailable',
        'health_not_attested',
        'Space thread pin runtime health has not been attested by its owner.',
        'unavailable',
        'bounded',
        NULL
    ),
    (
        'cap.tool.space.thread.post_card',
        'global',
        'tool',
        'space.thread.post_card',
        '1.0.0',
        'Post one plain-text status card in this Space thread: a title, a body and up to eight labelled fields. Links and markup are refused.',
        'low',
        'global',
        false,
        true,
        'space-self-api:cap.tool.space.thread.post_card:v1',
        jsonb_build_object(
            'dispatch_name', 'space.thread.post_card',
            'owner_plane', 'model',
            'owner_service', 'session-core',
            'decision_authority', 'control-plane-user-core',
            'target_authorization', 'session-core-run-thread-owner',
            'model_selectable_fields', jsonb_build_array('title', 'body', 'tone', 'fields'),
            'model_forbidden_fields', jsonb_build_array('thread_id', 'space', 'actor', 'org', 'run', 'digest', 'idempotency_key')
        ),
        ARRAY['space-self-api', 'owner-action'],
        ARRAY['global'],
        'stable',
        'migration:0045_space_self_api_capabilities',
        'unavailable',
        'health_not_attested',
        'Space thread card runtime health has not been attested by its owner.',
        'unavailable',
        'bounded',
        NULL
    ),
    (
        'cap.tool.space.agent.pause_self',
        'global',
        'tool',
        'space.agent.pause_self',
        '1.0.0',
        'Pause this agent in its Space, only through a fresh Control run-action decision. Its owner can resume it.',
        'medium',
        'global',
        false,
        true,
        'space-self-api:cap.tool.space.agent.pause_self:v1',
        jsonb_build_object(
            'dispatch_name', 'space.agent.pause_self',
            'owner_plane', 'application',
            'owner_service', 'convex-core',
            'decision_authority', 'control-plane-user-core',
            'target_authorization', 'convex-core-binding-owner',
            'model_selectable_fields', jsonb_build_array('reason'),
            'model_forbidden_fields', jsonb_build_array('binding', 'space', 'actor', 'org', 'run', 'digest', 'idempotency_key')
        ),
        ARRAY['space-self-api', 'owner-action'],
        ARRAY['global'],
        'stable',
        'migration:0045_space_self_api_capabilities',
        'unavailable',
        'health_not_attested',
        'Space agent pause runtime health has not been attested by its owner.',
        'unavailable',
        'bounded',
        NULL
    ),
    (
        'cap.tool.space.schedule.follow_up',
        'global',
        'tool',
        'space.schedule.follow_up',
        '1.0.0',
        'Schedule one follow-up run for this thread after a delay, only after a person approves it through the schedule-create decision.',
        'high',
        'global',
        false,
        true,
        'space-self-api:cap.tool.space.schedule.follow_up:v1',
        jsonb_build_object(
            'dispatch_name', 'space.schedule.follow_up',
            'owner_plane', 'model',
            'owner_service', 'capability-core',
            'decision_authority', 'control-plane-user-core',
            'target_authorization', 'capability-core-schedule-owner',
            'model_selectable_fields', jsonb_build_array('delay_minutes', 'instruction'),
            'model_forbidden_fields', jsonb_build_array('thread_id', 'space', 'actor', 'org', 'run', 'digest', 'idempotency_key')
        ),
        ARRAY['space-self-api', 'owner-action'],
        ARRAY['global'],
        'stable',
        'migration:0045_space_self_api_capabilities',
        'unavailable',
        'health_not_attested',
        'Space follow-up schedule runtime health has not been attested by its owner.',
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
    idempotency_key = EXCLUDED.idempotency_key,
    config_json = EXCLUDED.config_json,
    tags = EXCLUDED.tags,
    enabled_for_scopes = EXCLUDED.enabled_for_scopes,
    rollout_state = EXCLUDED.rollout_state,
    cost_class = EXCLUDED.cost_class,
    updated_at = now();

INSERT INTO agent_action_admissions
    (action_id, tool_name, capability_id, model_access, side_effect_class, approval_floor,
     authority, hash_basis, action_schema_hash, parameters_json, description, requires_approval, status)
VALUES
    ('space.thread.rename', 'space.thread.rename', 'cap.tool.space.thread.rename', 'workspace_write', 'owner_effect',
     'none', 'control_run_decision', 'owner_contract',
     'sha256:817e5676063235350559652663e961ba67ac2e2c260b43b544446a3b73c98543',
     '{"type":"object","additionalProperties":false,"properties":{"title":{"type":"string","minLength":1,"maxLength":96}},"required":["title"]}',
     'Rename this Space thread to a short title of 1 to 96 characters. A title a person set by hand is never replaced.',
     FALSE, 'enabled'),
    ('space.thread.pin', 'space.thread.pin', 'cap.tool.space.thread.pin', 'workspace_write', 'owner_effect',
     'none', 'control_run_decision', 'owner_contract',
     'sha256:bbd2580ffd5c0ba20dcf692314d797a89897603052202b8d00a98c1a94638294',
     '{"type":"object","additionalProperties":false,"properties":{"pinned":{"type":"boolean"}},"required":["pinned"]}',
     'Pin or unpin this Space thread. A pin a person set by hand is never changed.',
     FALSE, 'enabled'),
    ('space.thread.post_card', 'space.thread.post_card', 'cap.tool.space.thread.post_card', 'workspace_write', 'owner_effect',
     'none', 'control_run_decision', 'owner_contract',
     'sha256:0c1cd8f2e7cb368e7ea5a7369ca19fa2723754c1ea38ad173ef98c8ba72f42e3',
     '{"type":"object","additionalProperties":false,"properties":{"title":{"type":"string","minLength":1,"maxLength":120},"body":{"type":"string","minLength":1,"maxLength":2000},"tone":{"type":"string","enum":["info","progress","done","blocked"]},"fields":{"type":"array","maxItems":8,"items":{"type":"object","additionalProperties":false,"properties":{"label":{"type":"string","minLength":1,"maxLength":40},"value":{"type":"string","minLength":1,"maxLength":200}},"required":["label","value"]}}},"required":["title","body","tone"]}',
     'Post one plain-text status card in this Space thread: a title, a body and up to eight labelled fields. Links and markup are refused.',
     FALSE, 'declared'),
    ('space.agent.pause_self', 'space.agent.pause_self', 'cap.tool.space.agent.pause_self', 'workspace_write', 'owner_effect',
     'none', 'control_run_decision', 'owner_contract',
     'sha256:8c2d8043ec52d70a1bc7c90615f64d8a501f1635d6a445417a3a8b672a42ce39',
     '{"type":"object","additionalProperties":false,"properties":{"reason":{"type":"string","minLength":1,"maxLength":200}},"required":["reason"]}',
     'Pause this agent in its Space. Its owner can resume it.',
     FALSE, 'declared'),
    ('space.schedule.follow_up', 'space.schedule.follow_up', 'cap.tool.space.schedule.follow_up', 'write_approval', 'owner_effect',
     'owner_approval', 'control_schedule_decision', 'owner_contract',
     'sha256:5e33241382f8fd3a93fcc22bb8d47f87beb794e65a4a2bf0d3d9f46ab967b3c2',
     '{"type":"object","additionalProperties":false,"properties":{"delay_minutes":{"type":"integer","minimum":15,"maximum":10080},"instruction":{"type":"string","minLength":1,"maxLength":500}},"required":["delay_minutes","instruction"]}',
     'Schedule one follow-up run for this thread after a delay of 15 to 10080 minutes. A person must approve it first.',
     TRUE, 'declared')
ON CONFLICT (action_id) DO NOTHING;
