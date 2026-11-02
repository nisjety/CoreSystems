-- Thread activity summary (build plan WS15-S5; pulled forward by rule R7 because the
-- attention read WS11-S2 and the room timeline WS15-S6 both read "latest activity per
-- thread" and must not scan messages).
--
-- Six summary columns on threads, kept current by a trigger on messages that runs inside the
-- appender's transaction on the thread row the appender already holds FOR UPDATE, so it adds
-- no lock wait, covers every present and future writer, and rolls back with the message.
-- threads.last_activity_at already exists (migration 0049, written by the append paths) and is
-- the activity time of this summary: it is deliberately not re-added or touched here.
--
-- The summary holds ids, counts and a role word only. No message content, title or preview is
-- copied, so nothing here needs a retention decision; the summary is erased with its thread.
--
-- Existing threads are NOT filled by this migration (sqlx runs it in one transaction at boot,
-- and a full-table pass would block startup). A bounded background task fills them
-- (space/thread_activity.rs) until no row with last_message_seq IS NULL still has messages;
-- readers refuse with "summary_backfill_pending" until then. A thread whose summary is not yet
-- filled is rebuilt whole by the trigger on its next message, never half-counted.
-- The reversal is in revert/0078_thread_activity_summary.down.sql.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS last_message_seq BIGINT,
    ADD COLUMN IF NOT EXISTS turn_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_role TEXT,
    ADD COLUMN IF NOT EXISTS last_author_subject_id TEXT,
    ADD COLUMN IF NOT EXISTS recent_author_subject_ids TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS activity_max_audience_revision BIGINT;

-- Rebuilds one thread's summary from its messages. Idempotent. Takes the thread row lock first
-- so the rebuild reads after any in-flight appender has committed (READ COMMITTED: the UPDATE
-- below takes its snapshot after the lock is granted). Any path that deletes or rewrites part
-- of a thread's messages (author erasure, tombstones) must call this afterwards.
CREATE OR REPLACE FUNCTION thread_activity_recompute(p_thread_id TEXT)
RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM 1 FROM threads WHERE id = p_thread_id FOR UPDATE;
    IF NOT FOUND THEN
        RETURN;
    END IF;

    UPDATE threads t SET
        last_message_seq = s.max_seq,
        turn_count = s.turns,
        last_role = s.last_role,
        last_author_subject_id = s.last_author,
        recent_author_subject_ids = s.recent,
        activity_max_audience_revision = s.max_rev
    FROM (
        SELECT
            (SELECT max(m.sequence) FROM messages m WHERE m.thread_id = p_thread_id) AS max_seq,
            (SELECT count(*)::integer FROM messages m
              WHERE m.thread_id = p_thread_id AND m.role IN ('user', 'assistant')) AS turns,
            (SELECT m.role FROM messages m WHERE m.thread_id = p_thread_id
              ORDER BY m.sequence DESC LIMIT 1) AS last_role,
            (SELECT m.author_subject_id FROM messages m
              WHERE m.thread_id = p_thread_id AND m.role = 'user' AND m.author_subject_id IS NOT NULL
              ORDER BY m.sequence DESC LIMIT 1) AS last_author,
            COALESCE((SELECT array_agg(a.author_subject_id ORDER BY a.newest DESC) FROM (
                SELECT m.author_subject_id, max(m.sequence) AS newest FROM messages m
                 WHERE m.thread_id = p_thread_id AND m.role = 'user' AND m.author_subject_id IS NOT NULL
                 GROUP BY m.author_subject_id
                 ORDER BY max(m.sequence) DESC
                 LIMIT 5) a), '{}') AS recent,
            (SELECT max(m.recipient_audience_revision) FROM messages m
              WHERE m.thread_id = p_thread_id) AS max_rev
    ) s
    WHERE t.id = p_thread_id;
END
$$;

CREATE OR REPLACE FUNCTION thread_activity_apply()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_seq BIGINT;
BEGIN
    SELECT last_message_seq INTO v_seq FROM threads WHERE id = NEW.thread_id;
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    -- Summary not filled yet (a thread that predates this migration, or its first message):
    -- rebuild from the table, which already contains NEW, instead of counting one row.
    IF v_seq IS NULL THEN
        PERFORM thread_activity_recompute(NEW.thread_id);
        RETURN NULL;
    END IF;

    UPDATE threads t SET
        last_message_seq = GREATEST(t.last_message_seq, NEW.sequence),
        turn_count = t.turn_count + CASE WHEN NEW.role IN ('user', 'assistant') THEN 1 ELSE 0 END,
        last_role = CASE WHEN NEW.sequence >= COALESCE(t.last_message_seq, 0) THEN NEW.role ELSE t.last_role END,
        last_author_subject_id = CASE
            WHEN NEW.role = 'user' AND NEW.author_subject_id IS NOT NULL
                 AND NEW.sequence >= COALESCE(t.last_message_seq, 0)
            THEN NEW.author_subject_id ELSE t.last_author_subject_id END,
        recent_author_subject_ids = CASE
            WHEN NEW.role = 'user' AND NEW.author_subject_id IS NOT NULL
            THEN (array_prepend(NEW.author_subject_id,
                                array_remove(t.recent_author_subject_ids, NEW.author_subject_id)))[1:5]
            ELSE t.recent_author_subject_ids END,
        activity_max_audience_revision = GREATEST(t.activity_max_audience_revision, NEW.recipient_audience_revision)
    WHERE t.id = NEW.thread_id;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS messages_thread_activity ON messages;
CREATE TRIGGER messages_thread_activity
    AFTER INSERT ON messages
    FOR EACH ROW
    EXECUTE FUNCTION thread_activity_apply();

-- The room window: newest-first by last_message_seq inside one Space.
CREATE INDEX IF NOT EXISTS idx_threads_space_activity
    ON threads (org_id, space_id, last_message_seq DESC)
    WHERE space_id IS NOT NULL AND archived_at IS NULL AND last_message_seq IS NOT NULL;

-- Threads still waiting for the backfill (also lists empty threads, which never leave it).
CREATE INDEX IF NOT EXISTS idx_threads_activity_pending
    ON threads (id)
    WHERE last_message_seq IS NULL;
