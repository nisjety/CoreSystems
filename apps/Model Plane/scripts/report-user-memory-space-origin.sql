BEGIN READ ONLY;

-- report-user-memory-space-origin.sql
--
-- Depth register D3-266 (EP-8-S4). Read-only: counts, per org_id, the user-scope agent_memory rows whose
-- origin Space cannot be proven from source_links. The tests are those of session-core's legacy room-origin
-- filter (apps/Model Plane/rust/services/session-core/src/space/memory_scope.rs, legacy_room_origin_sql):
-- a 'thread:<id>' link joins threads on ct.id = substr(link, 8).
--
--   (a) in_space_withheld         at least one 'thread:' link whose thread sits in a non-personal Space
--                                 (space_id IS NOT NULL AND space_kind IS DISTINCT FROM 'personal'). The
--                                 recall filter already withholds these rows.
--   (b) thread_absent_unprovable  at least one 'thread:' link whose thread id is absent from threads. The
--                                 filter sees no Space for these rows, so it neither withholds nor attributes
--                                 them, and they are recalled in personal chats. Must be 0, or reviewed by the
--                                 owner, before the containment switch goes on.
--   (c) no_thread_link            a non-empty source_links with no 'thread:' link at all.
--   (d) total_rows                every user-scope row with a non-empty source_links.
--
-- Only org_id and counts leave this statement. It selects no content, key, topic, source_links value, thread id
-- or user id. It writes nothing: no temp tables, no DDL, and the transaction ends with ROLLBACK.

WITH user_rows AS (
    SELECT
        m.org_id,
        EXISTS (
            SELECT 1
            FROM unnest(m.source_links) AS sl(link)
            WHERE sl.link LIKE 'thread:%'
        ) AS has_thread_link,
        EXISTS (
            SELECT 1
            FROM unnest(m.source_links) AS sl(link)
            JOIN threads ct ON ct.id = substr(sl.link, 8)
            WHERE sl.link LIKE 'thread:%'
              AND ct.space_id IS NOT NULL
              AND ct.space_kind IS DISTINCT FROM 'personal'
        ) AS in_space_withheld,
        EXISTS (
            SELECT 1
            FROM unnest(m.source_links) AS sl(link)
            WHERE sl.link LIKE 'thread:%'
              AND NOT EXISTS (
                  SELECT 1
                  FROM threads ct
                  WHERE ct.id = substr(sl.link, 8)
              )
        ) AS thread_absent_unprovable
    FROM agent_memory m
    WHERE m.scope = 'user'
      AND cardinality(m.source_links) > 0
)
SELECT
    org_id,
    count(*) FILTER (WHERE in_space_withheld)        AS in_space_withheld,
    count(*) FILTER (WHERE thread_absent_unprovable) AS thread_absent_unprovable,
    count(*) FILTER (WHERE NOT has_thread_link)      AS no_thread_link,
    count(*)                                         AS total_rows
FROM user_rows
GROUP BY org_id
ORDER BY org_id;

ROLLBACK;
