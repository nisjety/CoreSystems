-- Space attention outbox (build plan WS11-S5).
--
-- Turns the persistent rows of the Space event log (0072) into one content-free event per
-- row on `mp.v1.space.<space_ref>.event`, carried by the existing session_audit_outbox
-- (so the row commits in the SAME transaction as the log row, and the drainer publishes it
-- to JetStream with the row id as Nats-Msg-Id). The consumer (Application Plane convex-core)
-- learns THAT something happened to whom; it never receives text, titles, previews, goals
-- or outputs. Ids and closed words only.
--
-- Three event classes cross: message.appended (user and assistant rows), run.state_changed
-- and thread.deleted. A zero-retention run never reaches the log, a 'zdr_waived' row is
-- excluded by the trigger WHEN, and the function re-checks the class a second time.
--
-- Off by default: space_attention_settings.enabled is written from SPACE_ATTENTION_EMIT at
-- boot (space::attention_outbox::sync_flag), which refuses to turn on while the log itself
-- is off. While the row is false the trigger function returns before touching any table.
-- Fail-open like the log: a failure inside the function is swallowed into
-- space_event_errors (a source word and a sqlstate, no ids) and never blocks a post.
--
-- The subject CHECK on session_audit_outbox is widened by exactly one family. Keeping it
-- narrow means a typo'd subject still fails here instead of reaching the bus; the drainer
-- applies its own per-route authority check (audit_publisher.rs, OutboxRoute::SpaceEvent).
--
-- The reversal is in revert/0080_space_attention_outbox.down.sql.

ALTER TABLE session_audit_outbox
    DROP CONSTRAINT IF EXISTS session_audit_outbox_subject_check;

ALTER TABLE session_audit_outbox
    ADD CONSTRAINT session_audit_outbox_subject_check
    CHECK (
        subject LIKE 'verevon.audit.v2.model.session-core.%'
        OR subject LIKE 'mp.v1.run.%.event'
        OR subject LIKE 'mp.v1.space.%.event'
    );

CREATE TABLE IF NOT EXISTS space_attention_settings (
    singleton  BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO space_attention_settings (singleton, enabled)
VALUES (TRUE, FALSE)
ON CONFLICT (singleton) DO NOTHING;

-- Subject ids a message mentions (at most 10, distinct, each at most 128 characters).
-- A stub reading messages.metadata->'mentions'[*].subject_id; the identity-bound mention
-- work (WS08) may replace the body, the contract is: ids only, no names, no text.
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

DROP TRIGGER IF EXISTS space_attention_outbox_ins ON space_event_log;
CREATE TRIGGER space_attention_outbox_ins
    AFTER INSERT ON space_event_log
    FOR EACH ROW
    WHEN (NEW.retention_class = 'persistent'
          AND NEW.kind IN ('message.appended', 'run.state_changed', 'thread.deleted'))
    EXECUTE FUNCTION space_attention_to_outbox();
