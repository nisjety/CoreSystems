-- Reverses 0135_space_attention_outbox_fixes.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only, inside
-- one transaction.
--
-- Restores the two function bodies exactly as migration 0080 wrote them: the stub that reads
-- messages.metadata, and the run payload without agent_id. Rows already in session_audit_outbox
-- keep the payload they were written with. Safe to run twice.

BEGIN;

CREATE OR REPLACE FUNCTION space_message_mentions(p_message_id TEXT)
RETURNS TEXT[]
LANGUAGE sql
STABLE
AS $$
    SELECT coalesce(
        ARRAY(
            SELECT q.subject_id
            FROM (
                SELECT DISTINCT m.item ->> 'subject_id' AS subject_id
                FROM messages AS x,
                     LATERAL jsonb_array_elements(
                         CASE WHEN jsonb_typeof(x.metadata -> 'mentions') = 'array'
                              THEN x.metadata -> 'mentions'
                              ELSE '[]'::jsonb END
                     ) AS m(item)
                WHERE x.id = p_message_id
                  AND jsonb_typeof(m.item) = 'object'
            ) AS q
            WHERE q.subject_id IS NOT NULL
              AND q.subject_id <> ''
              AND length(q.subject_id) <= 128
            ORDER BY q.subject_id
            LIMIT 10
        ),
        ARRAY[]::TEXT[]
    )
$$;

-- 0080's trigger function, as it stood before this file.
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

COMMIT;
