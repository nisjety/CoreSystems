-- Space attention outbox: two defects of migration 0080 (build plan WS11-S5; depth items
-- DEPTH-WS11-S5-1 and DEPTH-WS11-S5-2). Reversal: revert/0135_space_attention_outbox_fixes.down.sql.
--
-- 1. Mentions. space_message_mentions read messages.metadata->'mentions', a key WS08 never writes,
--    so every SPACE_MESSAGE_APPENDED carried an empty list. It now reads message_mentions (0075),
--    the rows WS08 writes inside the post's own transaction before it commits, which is when the
--    trigger below runs. The contract is the one 0080 stated: the human mentions of that message
--    (subject_kind 'user'), ids only, distinct, at most 10, each at most 128 characters, in subject
--    order. Agent mentions never cross.
-- 2. Agent. SPACE_RUN_STATE_CHANGED carried no agent id, so agent activity was never recorded.
--    runs.agent_id is the agent kind a run was started with (the column default is 'general-v1'),
--    not the Control agent subject the roster is keyed by, so it is not used. The subject is
--    runs.agent_subject_id, which only a verified wake decision sets. Any other run carries ''.
--
-- Both functions are replaced in place. The trigger, the subject CHECK, the settings row and the
-- trigger's WHEN clause are unchanged, and the trigger still fails open.

CREATE OR REPLACE FUNCTION space_message_mentions(p_message_id TEXT)
RETURNS TEXT[]
LANGUAGE sql
STABLE
AS $$
    SELECT coalesce(
        ARRAY(
            SELECT mm.subject_id
            FROM message_mentions AS mm
            WHERE mm.message_id = p_message_id
              AND mm.subject_kind = 'user'
              AND mm.subject_id <> ''
              AND length(mm.subject_id) <= 128
            ORDER BY mm.subject_id
            LIMIT 10
        ),
        ARRAY[]::TEXT[]
    )
$$;

-- AFTER INSERT ON space_event_log. Inserts one outbox row for the persistent kinds the
-- attention projection consumes. Never raises.
CREATE OR REPLACE FUNCTION space_attention_to_outbox()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_event_id   TEXT;
    v_event_type TEXT;
    v_payload    JSONB;
    v_role       TEXT;
BEGIN
    BEGIN
        IF NOT coalesce(
            (SELECT enabled FROM space_attention_settings WHERE singleton), FALSE
        ) THEN
            RETURN NULL;
        END IF;
        IF NEW.retention_class IS DISTINCT FROM 'persistent' THEN
            RETURN NULL;
        END IF;
        IF NEW.org_id IS NULL OR NEW.org_id = '' THEN
            RETURN NULL;
        END IF;

        -- The ref becomes ONE subject token under a single-token ACL wildcard. A ref that
        -- could add a token or a wildcard is skipped and counted, never published.
        IF NEW.space_ref IS NULL OR NEW.space_ref !~ '^[^.*>[:space:]]+$'
           OR length(NEW.space_ref) > 128 THEN
            INSERT INTO space_event_errors (source, sqlstate)
            VALUES ('attention_ref', '22023');
            RETURN NULL;
        END IF;

        v_event_id := 'space:' || NEW.org_id || ':' || NEW.space_ref || ':'
                      || NEW.generation::text || ':' || NEW.seq::text;
        IF length(v_event_id) > 128 THEN
            INSERT INTO space_event_errors (source, sqlstate)
            VALUES ('attention_event_id', '22001');
            RETURN NULL;
        END IF;

        IF NEW.kind = 'message.appended' THEN
            v_role := NEW.refs ->> 'role';
            IF v_role IS NULL OR v_role NOT IN ('user', 'assistant')
               OR NEW.thread_id IS NULL OR NEW.message_id IS NULL THEN
                RETURN NULL;
            END IF;
            v_event_type := 'SPACE_MESSAGE_APPENDED';
            v_payload := jsonb_build_object(
                'space_ref', NEW.space_ref,
                'space_seq', NEW.seq,
                'kind', NEW.kind,
                'thread_id', NEW.thread_id,
                'message_id', NEW.message_id,
                'role', v_role,
                'author_subject_id', coalesce(NEW.actor_subject_id, ''),
                'thread_owner_subject_id',
                    coalesce((SELECT t.user_id FROM threads t WHERE t.id = NEW.thread_id), ''),
                'mentions',
                    CASE WHEN v_role = 'user'
                         THEN to_jsonb(space_message_mentions(NEW.message_id))
                         ELSE '[]'::jsonb END,
                'audience_revision', NEW.audience_revision
            );
        ELSIF NEW.kind = 'run.state_changed' THEN
            IF NEW.run_id IS NULL OR NEW.thread_id IS NULL THEN
                RETURN NULL;
            END IF;
            v_event_type := 'SPACE_RUN_STATE_CHANGED';
            v_payload := jsonb_build_object(
                'space_ref', NEW.space_ref,
                'space_seq', NEW.seq,
                'kind', NEW.kind,
                'thread_id', NEW.thread_id,
                'run_id', NEW.run_id,
                'state', coalesce(NEW.refs ->> 'state', 'other'),
                'starter_subject_id',
                    coalesce((SELECT r.user_id FROM runs r WHERE r.id = NEW.run_id), ''),
                'agent_id',
                    coalesce((SELECT r.agent_subject_id FROM runs r WHERE r.id = NEW.run_id), ''),
                'audience_revision', NEW.audience_revision
            );
        ELSIF NEW.kind = 'thread.deleted' THEN
            IF NEW.thread_id IS NULL THEN
                RETURN NULL;
            END IF;
            v_event_type := 'SPACE_THREAD_DELETED';
            v_payload := jsonb_build_object(
                'space_ref', NEW.space_ref,
                'space_seq', NEW.seq,
                'kind', NEW.kind,
                'thread_id', NEW.thread_id,
                'audience_revision', NEW.audience_revision
            );
        ELSE
            -- Unknown or uninteresting kinds never cross.
            RETURN NULL;
        END IF;

        INSERT INTO session_audit_outbox (event_id, subject, payload)
        VALUES (
            v_event_id,
            'mp.v1.space.' || NEW.space_ref || '.event',
            jsonb_build_object(
                'event_id', v_event_id,
                'event_type', v_event_type,
                'schema_version', 1,
                'ts', to_char(NEW.created_at AT TIME ZONE 'UTC',
                              'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                'producer', 'session-core',
                'correlation_id', NEW.thread_id,
                'causation_id', coalesce(NEW.message_id, NEW.run_id, v_event_id),
                'idempotency_key', v_event_id,
                'org_id', NEW.org_id,
                'user_id', coalesce(NEW.actor_subject_id, ''),
                'resource_ref', 'space:' || NEW.space_ref,
                'payload', v_payload,
                'zdr', FALSE
            )
        )
        ON CONFLICT (event_id) DO NOTHING;
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('attention_outbox', SQLSTATE);
    END;
    RETURN NULL;
END
$$;
