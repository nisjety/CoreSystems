-- Retire the dead `memoryEnabled` AI setting (build plan WS07-S5).
--
-- Nothing ever read it. Its stored `false` was a side effect of the old
-- struct-patch PUT, which wrote the zero value of every field on any save, so
-- it never recorded a user's choice. The memory preference now lives in
-- session-core (user_memory_controls); this key must not be read as intent.
-- No backfill, by design.
--
-- Up-only and idempotent: the service re-runs every *.up.sql at boot, and
-- removing the key cannot be undone because its value carried no meaning.
UPDATE user_settings
   SET settings = settings - 'memoryEnabled'
 WHERE category = 'ai'
   AND jsonb_exists(settings, 'memoryEnabled');
