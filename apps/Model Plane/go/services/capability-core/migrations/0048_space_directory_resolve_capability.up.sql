-- 0048_space_directory_resolve_capability.up.sql
--
-- A-REC-03-TOOL (F-t1-049 residual, build plan X19): the agent directory resolver, space.directory.resolve. A room agent
-- turns a name, a unique name prefix or an exact email into one person (or agent) of its own organization. The answer is
-- ok, ambiguous (at most five candidates) or none, each with a reason. It is presence, never authority: a match lets the
-- agent add, mention, message or act for nobody, and no answer names another room.
--
-- execution-core runs the tool (space_directory_tool.rs) as a pure function over Control's run-bound people read
-- (POST /api/v1/internal/spaces/run-directory/people, scope spaces:directory:run-read). Control takes the organization,
-- the subject and the room from the run, never from the model, and never calls the gateway's human resolver.
--
-- The capability row starts unavailable with reason health_not_attested, as the 0045 and 0046 Space rows do. Its attester
-- on the owner-action lane is execution-core (availability.go), which attests it only after its Session Core probe and
-- only while SPACE_DIRECTORY_AGENT_TOOL is on with a complete configuration.
--
-- The admission row carries the golden registry's schema hash, parameters bytes and the model-facing description, in the
-- shape 0045 used for the thread rows (control_run_decision, owner_contract hash basis, no approval). It is enabled, but a
-- row alone admits nothing: Control's view lists it only when CONTROL_SPACE_SELF_API_ACTIONS names it (empty by default),
-- capability-core must run with MODEL_ACTION_ADMISSION=table, the capability must be attested, and execution-core offers
-- it only under SPACE_DIRECTORY_AGENT_TOOL (off by default) and the self-API profile marker. Control issues no view for a
-- zero-retention room, so a ZDR run is never offered the tool; the tool itself answers ids only for such a run.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot. The capability seed keeps the availability
-- columns and the enabled flag an operator may have changed, and the admission seed never overwrites a row (an operator
-- retires the row to switch it off).

INSERT INTO capabilities (
    id, org_id, kind, name, version, description, risk_level, scope,
    lazy_load, enabled, idempotency_key, config_json, tags,
    enabled_for_scopes, rollout_state, created_by, availability_state,
    availability_reason_code, availability_reason, execution_mode, cost_class,
    health_checked_at
) VALUES
    (
        'cap.tool.space.directory.resolve',
        'global',
        'tool',
        'space.directory.resolve',
        '1.0.0',
        'Look up a person or an agent in this run''s organization by name, unique name prefix or exact email. Presence only: a match grants nothing, and no other room is ever named.',
        'low',
        'global',
        false,
        true,
        'space-self-api:cap.tool.space.directory.resolve:v1',
        jsonb_build_object(
            'dispatch_name', 'space.directory.resolve',
            'owner_plane', 'control',
            'owner_service', 'user-core',
            'decision_authority', 'control-plane-user-core',
            'target_authorization', 'control-plane-run-directory',
            'model_selectable_fields', jsonb_build_array('query', 'kind'),
            'model_forbidden_fields', jsonb_build_array('space', 'room', 'actor', 'org', 'user', 'run', 'digest', 'idempotency_key')
        ),
        ARRAY['space-self-api', 'owner-action'],
        ARRAY['global'],
        'stable',
        'migration:0048_space_directory_resolve_capability',
        'unavailable',
        'health_not_attested',
        'Space directory resolver runtime health has not been attested by its owner.',
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
    ('space.directory.resolve', 'space.directory.resolve', 'cap.tool.space.directory.resolve', 'read_only', 'read',
     'none', 'control_run_decision', 'owner_contract',
     'sha256:b0d9f062f7def9dd1299885f26df5bdfd1503015df71369f45d11904b209931f',
     '{"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":100,"description":"A full name, a unique name prefix, or an exact email."},"kind":{"type":"string","enum":["person","agent","any"],"description":"What to look for. Defaults to person."}},"required":["query"],"additionalProperties":false}',
     'Look up a person or an agent in your organization by full name, a unique name prefix or an exact email. Answers status ok with one match, ambiguous with up to 5 candidates to ask the user about, or none with a reason (no_match, or directory_unavailable when the directory could not be read). A match is presence only: it does not let you add, mention, message or act for anyone. in_this_room says whether the match is in the room you run in. Other rooms are never named. Under zero data retention only ids are returned.',
     FALSE, 'enabled')
ON CONFLICT (action_id) DO NOTHING;
