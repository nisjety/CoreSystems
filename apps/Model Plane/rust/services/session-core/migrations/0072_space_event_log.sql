-- Space event log (build plan WS02-S1).
--
-- A per-Space, strictly sequenced, content-free log of what changed in a shared room:
-- a thread appeared, a message was appended, a run changed state. It carries ids and
-- closed words only (never message text, titles, previews, goals or outputs), so a
-- subscriber learns THAT something changed and refetches through the authorized read
-- path. The stream that serves it (WS02-S3) re-checks the viewer's authority on every
-- delivery; nothing here is an authorization input.
--
-- Rows are written by deferred constraint triggers on messages, threads and runs, not
-- by Rust call sites: they cover every present and future writer, and they fire at
-- COMMIT, so the per-Space head row is the LAST lock a transaction takes and seq order
-- equals commit order. RULE: no code path may lock space_event_heads earlier than the
-- commit-time trigger does.
--
-- Fail-open: every trigger body swallows its own error into space_event_errors (a
-- sqlstate and a source word, no ids), and the swallowed error rolls back that row's
-- head increment with it, so seq stays contiguous. A realtime defect never blocks a
-- post. PL/pgSQL cannot catch a query cancellation: that one case still aborts the
-- commit.
--
-- Off by default: space_event_settings.enabled is written from SPACE_REALTIME_EMIT at
-- boot (space::events::sync_flag). While it is false space_event_emit returns before
-- touching any table. A zero-retention run (runs.retention_class = 'zdr') never
-- produces a row: the trigger WHEN excludes it, space_event_emit refuses the class, and
-- the log's CHECK refuses it a third time.
--
-- Seq is monotonic per (org, space) and is never reset by a generation bump; a bump
-- (off to on) moves min_seq above the old head so older cursors read as too old.
--
-- The reversal is in revert/0072_space_event_log.down.sql.

CREATE TABLE IF NOT EXISTS space_event_settings (
    singleton  BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO space_event_settings (singleton, enabled)
VALUES (TRUE, FALSE)
ON CONFLICT (singleton) DO NOTHING;

CREATE TABLE IF NOT EXISTS space_event_heads (
    org_id        TEXT NOT NULL,
    space_ref     TEXT NOT NULL,
    generation    BIGINT NOT NULL DEFAULT 1,
    head_seq      BIGINT NOT NULL DEFAULT 0,
    min_seq       BIGINT NOT NULL DEFAULT 1,
    last_event_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, space_ref)
);

CREATE TABLE IF NOT EXISTS space_event_log (
    org_id           TEXT NOT NULL,
    space_ref        TEXT NOT NULL,
    seq              BIGINT NOT NULL,
    generation       BIGINT NOT NULL,
    kind             TEXT NOT NULL CHECK (kind ~ '^[a-z_]+(\.[a-z_]+)?$'),
    -- Widened by later migrations (reactions, bindings); an unknown scope is stubbed
    -- 'hidden' by the stream's gate.
    scope            TEXT NOT NULL DEFAULT 'space' CHECK (scope IN ('space')),
    actor_type       TEXT NOT NULL CHECK (actor_type IN ('user', 'agent', 'system')),
    actor_subject_id TEXT,
    thread_id        TEXT,
    run_id           TEXT,
    message_id       TEXT,
    audience_revision BIGINT,
    retention_class  TEXT NOT NULL DEFAULT 'persistent'
        CHECK (retention_class IN ('persistent', 'zdr_waived')),
    refs             JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (pg_column_size(refs) <= 512),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, space_ref, seq)
);

CREATE INDEX IF NOT EXISTS space_event_log_thread_idx
    ON space_event_log (org_id, space_ref, thread_id)
    WHERE thread_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS space_event_log_actor_idx
    ON space_event_log (org_id, actor_subject_id)
    WHERE actor_subject_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS space_event_log_age_idx
    ON space_event_log (created_at);

-- A trigger failure that was swallowed. Source word and sqlstate only: no ids.
CREATE TABLE IF NOT EXISTS space_event_errors (
    at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    source   TEXT NOT NULL,
    sqlstate TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS space_event_errors_at_idx ON space_event_errors (at);

-- Run state vocabulary: the ONE place the stored word is derived from runs.status. The
-- Rust mirror is space::events::run_state; the two must agree (db test owed).
CREATE OR REPLACE FUNCTION space_event_run_state(p_status TEXT)
RETURNS TEXT
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT CASE p_status
        WHEN 'queued' THEN 'queued'
        WHEN 'running' THEN 'running'
        WHEN 'awaiting_approval' THEN 'awaiting_approval'
        WHEN 'completed' THEN 'completed'
        WHEN 'failed' THEN 'failed'
        WHEN 'cancelled' THEN 'cancelled'
        ELSE 'other'
    END
$$;

-- The single write chokepoint. Returns the assigned seq, or NULL when nothing was
-- written (suppressed, disabled, or a zero-retention class).
CREATE OR REPLACE FUNCTION space_event_emit(
    p_org_id            TEXT,
    p_space_ref         TEXT,
    p_kind              TEXT,
    p_actor_type        TEXT,
    p_actor_subject_id  TEXT,
    p_thread_id         TEXT,
    p_run_id            TEXT,
    p_message_id        TEXT,
    p_audience_revision BIGINT,
    p_retention_class   TEXT,
    p_refs              JSONB
)
RETURNS BIGINT
LANGUAGE plpgsql
AS $$
DECLARE
    v_seq BIGINT;
    v_gen BIGINT;
BEGIN
    -- Erasure and purge paths set this (transaction-local) so the deferred delete
    -- triggers cannot re-create rows after the log was purged.
    IF coalesce(current_setting('space_event.suppress', true), '') = 'on' THEN
        RETURN NULL;
    END IF;
    IF NOT coalesce((SELECT enabled FROM space_event_settings WHERE singleton), FALSE) THEN
        RETURN NULL;
    END IF;
    IF p_retention_class IS NULL OR p_retention_class NOT IN ('persistent', 'zdr_waived') THEN
        RETURN NULL;
    END IF;
    IF p_org_id IS NULL OR p_space_ref IS NULL OR p_org_id = '' OR p_space_ref = '' THEN
        RETURN NULL;
    END IF;
    -- Allowlist: closed words and ids only. Anything else is a defect in the caller and
    -- is swallowed and counted by the trigger that called this.
    IF p_refs IS NULL OR jsonb_typeof(p_refs) <> 'object' THEN
        RAISE EXCEPTION 'space_event_emit: refs must be an object';
    END IF;
    IF EXISTS (
        SELECT 1 FROM jsonb_object_keys(p_refs) AS k(key)
        WHERE k.key NOT IN ('role', 'message_sequence', 'origin', 'fields', 'state',
                            'prev', 'agent_id', 'parent_run_id')
    ) THEN
        RAISE EXCEPTION 'space_event_emit: refs key outside the allowlist';
    END IF;

    INSERT INTO space_event_heads (org_id, space_ref, head_seq)
    VALUES (p_org_id, p_space_ref, 1)
    ON CONFLICT (org_id, space_ref)
    DO UPDATE SET head_seq = space_event_heads.head_seq + 1, last_event_at = now()
    RETURNING head_seq, generation INTO v_seq, v_gen;

    INSERT INTO space_event_log
        (org_id, space_ref, seq, generation, kind, actor_type, actor_subject_id,
         thread_id, run_id, message_id, audience_revision, retention_class, refs)
    VALUES
        (p_org_id, p_space_ref, v_seq, v_gen, p_kind, p_actor_type, p_actor_subject_id,
         p_thread_id, p_run_id, p_message_id, p_audience_revision, p_retention_class,
         p_refs);

    -- Delivered after COMMIT. Ids only; the table is the source of truth, a lost NOTIFY
    -- costs latency (the reader polls on a floor interval), never correctness.
    PERFORM pg_notify(
        'space_events',
        json_build_object('o', p_org_id, 's', p_space_ref, 'q', v_seq, 'g', v_gen)::text
    );
    RETURN v_seq;
END
$$;

-- messages AFTER INSERT (Space rows; user, assistant and system roles).
CREATE OR REPLACE FUNCTION space_event_from_messages()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_org TEXT;
BEGIN
    BEGIN
        SELECT t.org_id INTO v_org FROM threads t WHERE t.id = NEW.thread_id;
        IF v_org IS NULL THEN
            RETURN NULL;
        END IF;
        PERFORM space_event_emit(
            v_org, NEW.space_id, 'message.appended',
            CASE
                WHEN NEW.author_subject_id IS NOT NULL THEN 'user'
                WHEN NEW.role = 'assistant' THEN 'agent'
                ELSE 'system'
            END,
            NEW.author_subject_id,
            NEW.thread_id, NULL, NEW.id, NEW.recipient_audience_revision, 'persistent',
            jsonb_build_object('role', NEW.role, 'message_sequence', NEW.sequence)
        );
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('messages_insert', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

-- threads AFTER INSERT (Space threads).
CREATE OR REPLACE FUNCTION space_event_from_threads_insert()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    BEGIN
        PERFORM space_event_emit(
            NEW.org_id, NEW.space_id, 'thread.created',
            CASE WHEN NEW.origin IN ('agent_run', 'system') THEN 'system' ELSE 'user' END,
            CASE WHEN NEW.origin IN ('agent_run', 'system') THEN NULL ELSE NEW.user_id END,
            NEW.id, NULL, NULL, NEW.recipient_audience_revision, 'persistent',
            jsonb_build_object('origin', NEW.origin)
        );
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('threads_insert', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

-- threads AFTER UPDATE of the presentation columns, pin and persona toggle.
CREATE OR REPLACE FUNCTION space_event_from_threads_update()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_fields JSONB := '[]'::jsonb;
BEGIN
    BEGIN
        IF OLD.presentation_title IS DISTINCT FROM NEW.presentation_title THEN
            v_fields := v_fields || '"title"'::jsonb;
        END IF;
        IF OLD.presentation_preview IS DISTINCT FROM NEW.presentation_preview THEN
            v_fields := v_fields || '"preview"'::jsonb;
        END IF;
        IF (OLD.pinned_at IS NULL) IS DISTINCT FROM (NEW.pinned_at IS NULL) THEN
            v_fields := v_fields || '"pinned"'::jsonb;
        END IF;
        IF OLD.persona_enabled IS DISTINCT FROM NEW.persona_enabled THEN
            v_fields := v_fields || '"persona"'::jsonb;
        END IF;
        IF jsonb_array_length(v_fields) > 0 THEN
            PERFORM space_event_emit(
                NEW.org_id, NEW.space_id, 'thread.updated', 'system', NULL,
                NEW.id, NULL, NULL, NEW.recipient_audience_revision, 'persistent',
                jsonb_build_object('fields', v_fields)
            );
        END IF;
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('threads_update', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

-- threads AFTER UPDATE OF archived_at, NULL to NOT NULL.
CREATE OR REPLACE FUNCTION space_event_from_threads_archive()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    BEGIN
        PERFORM space_event_emit(
            NEW.org_id, NEW.space_id, 'thread.archived', 'system', NULL,
            NEW.id, NULL, NULL, NEW.recipient_audience_revision, 'persistent',
            '{}'::jsonb
        );
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('threads_archive', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

-- threads AFTER DELETE. Earlier rows of the thread are rewritten to 'redacted' (every id
-- nulled) BEFORE the tombstone is emitted, so the lock order is log rows then head, the
-- same as the retention sweeper. The tombstone keeps the thread id for the replay window
-- so a client that holds the thread can drop it.
CREATE OR REPLACE FUNCTION space_event_from_threads_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    BEGIN
        IF coalesce(current_setting('space_event.suppress', true), '') <> 'on' THEN
            UPDATE space_event_log
               SET kind = 'redacted', actor_type = 'system', actor_subject_id = NULL,
                   thread_id = NULL, run_id = NULL, message_id = NULL, refs = '{}'::jsonb
             WHERE org_id = OLD.org_id AND space_ref = OLD.space_id
               AND thread_id = OLD.id;
        END IF;
        PERFORM space_event_emit(
            OLD.org_id, OLD.space_id, 'thread.deleted', 'system', NULL,
            OLD.id, NULL, NULL, OLD.recipient_audience_revision, 'persistent',
            '{}'::jsonb
        );
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('threads_delete', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

-- runs AFTER INSERT (Space runs that are not zero-retention).
CREATE OR REPLACE FUNCTION space_event_from_runs_insert()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    BEGIN
        PERFORM space_event_emit(
            NEW.org_id, NEW.space_id, 'run.state_changed', 'agent', NEW.agent_id,
            NEW.thread_id, NEW.id, NULL, NEW.recipient_audience_revision, NEW.retention_class,
            jsonb_strip_nulls(jsonb_build_object(
                'state', space_event_run_state(NEW.status),
                'agent_id', NEW.agent_id,
                'parent_run_id', NEW.parent_run_id
            ))
        );
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('runs_insert', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

-- runs AFTER UPDATE OF status, only when the stored state word moves.
CREATE OR REPLACE FUNCTION space_event_from_runs_update()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    BEGIN
        IF space_event_run_state(OLD.status) IS DISTINCT FROM space_event_run_state(NEW.status) THEN
            PERFORM space_event_emit(
                NEW.org_id, NEW.space_id, 'run.state_changed', 'agent', NEW.agent_id,
                NEW.thread_id, NEW.id, NULL, NEW.recipient_audience_revision, NEW.retention_class,
                jsonb_strip_nulls(jsonb_build_object(
                    'state', space_event_run_state(NEW.status),
                    'prev', space_event_run_state(OLD.status),
                    'agent_id', NEW.agent_id,
                    'parent_run_id', NEW.parent_run_id
                ))
            );
        END IF;
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('runs_update', SQLSTATE);
    END;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS space_event_messages_ins ON messages;
CREATE CONSTRAINT TRIGGER space_event_messages_ins
    AFTER INSERT ON messages
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.space_id IS NOT NULL AND NEW.role IN ('user', 'assistant', 'system'))
    EXECUTE FUNCTION space_event_from_messages();

DROP TRIGGER IF EXISTS space_event_threads_ins ON threads;
CREATE CONSTRAINT TRIGGER space_event_threads_ins
    AFTER INSERT ON threads
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.space_id IS NOT NULL)
    EXECUTE FUNCTION space_event_from_threads_insert();

DROP TRIGGER IF EXISTS space_event_threads_upd ON threads;
CREATE CONSTRAINT TRIGGER space_event_threads_upd
    AFTER UPDATE OF presentation_title, presentation_preview, pinned_at, persona_enabled ON threads
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.space_id IS NOT NULL)
    EXECUTE FUNCTION space_event_from_threads_update();

DROP TRIGGER IF EXISTS space_event_threads_arch ON threads;
CREATE CONSTRAINT TRIGGER space_event_threads_arch
    AFTER UPDATE OF archived_at ON threads
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.space_id IS NOT NULL AND OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL)
    EXECUTE FUNCTION space_event_from_threads_archive();

DROP TRIGGER IF EXISTS space_event_threads_del ON threads;
CREATE CONSTRAINT TRIGGER space_event_threads_del
    AFTER DELETE ON threads
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (OLD.space_id IS NOT NULL)
    EXECUTE FUNCTION space_event_from_threads_delete();

DROP TRIGGER IF EXISTS space_event_runs_ins ON runs;
CREATE CONSTRAINT TRIGGER space_event_runs_ins
    AFTER INSERT ON runs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.space_id IS NOT NULL AND NEW.retention_class <> 'zdr')
    EXECUTE FUNCTION space_event_from_runs_insert();

DROP TRIGGER IF EXISTS space_event_runs_upd ON runs;
CREATE CONSTRAINT TRIGGER space_event_runs_upd
    AFTER UPDATE OF status ON runs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.space_id IS NOT NULL AND NEW.retention_class <> 'zdr'
          AND OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION space_event_from_runs_update();

-- Subject erasure (shipped now, called by the future user-erasure path): the actor id is
-- dropped from every row that names the subject; the row itself keeps its kind and refs.
CREATE OR REPLACE FUNCTION space_event_forget_subject(p_org_id TEXT, p_subject_id TEXT)
RETURNS BIGINT
LANGUAGE plpgsql
AS $$
DECLARE
    v_rows BIGINT;
BEGIN
    UPDATE space_event_log
       SET actor_subject_id = NULL
     WHERE org_id = p_org_id AND actor_subject_id = p_subject_id;
    GET DIAGNOSTICS v_rows = ROW_COUNT;
    RETURN v_rows;
END
$$;
