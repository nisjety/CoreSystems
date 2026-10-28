-- d16-legacy-profile-count.sql: how many saved browser profiles have no owner, and what the
-- owner check would do to them under `enforce` (decision D16 Q2, WS17.P2).
--
-- READ ONLY. It selects counts and tenant ids from `quarry_profiles`. It never selects a
-- snapshot, an envelope, a name or an owner id, and it changes nothing (the transaction is
-- opened READ ONLY, so the database refuses a write).
--
-- Written for the stack session; it was syntax-checked offline and NOT run against any stack.
--
-- Run (psql 12 or later):
--   psql "$DSN" -X -v ON_ERROR_STOP=1 -v since="'2026-10-29T00:00:00Z'" \
--        -f scripts/evidence/d16-legacy-profile-count.sql
-- or through scripts/evidence/d16-legacy-profile-count.sh, which reads the DSN from a file.
--
-- `since` is optional: the time the run-start owner stamp (commit "stamp the profile owner at
-- run start") reached the live edge. It splits the ownerless rows into "written before the
-- stamp" (the legacy set the decision is about) and "written after it" (a person's run should
-- never leave one; a model-driven run does, by design, until an acting-user claim exists).
--
-- How to read it, against what the edge does (profile_guard.rs, `decide`):
--   * owner IS NOT NULL                       owned: the owner check works as designed.
--   * owner IS NULL and scope = org_shared    legacy shared: stays open to the org, never hidden.
--   * owner IS NULL and scope <> org_shared   legacy private: HIDDEN from everyone under
--                                             `enforce` (not deleted). This is the number that
--                                             decides whether to rescue or backfill first.
-- Rescue (a person confirms the owner, then UPDATE ... SET owner_user_id) and the choice of a
-- backfill from run records are the owner's decisions; this script does neither.

\if :{?since}
\else
  \set since '''infinity'''
\endif

BEGIN READ ONLY;

\echo '== 1. totals'
SELECT count(*)                                              AS rows_total,
       count(*) FILTER (WHERE owner_user_id IS NOT NULL)     AS owned,
       count(*) FILTER (WHERE owner_user_id IS NULL)         AS ownerless
  FROM quarry_profiles;

\echo '== 2. ownerless rows by scope (what enforce does to them)'
SELECT scope,
       CASE WHEN scope = 'org_shared' THEN 'stays open to the org'
            ELSE 'hidden from everyone under enforce' END    AS under_enforce,
       count(*)                                              AS rows
  FROM quarry_profiles
 WHERE owner_user_id IS NULL
 GROUP BY scope
 ORDER BY scope;

\echo '== 3. legacy private rows: before and after the stamp reached the edge'
SELECT count(*) FILTER (WHERE updated_at <  :since::timestamptz) AS before_stamp,
       count(*) FILTER (WHERE updated_at >= :since::timestamptz) AS after_stamp,
       count(*)                                                  AS hidden_total
  FROM quarry_profiles
 WHERE owner_user_id IS NULL
   AND scope <> 'org_shared';

\echo '== 4. legacy private rows by how much they hold'
-- No payload is read: `rev` counts stored saves, and a row that was only ever created
-- (placeholder snapshot, saved_at NULL, rev 0) holds no browsing state at all.
SELECT CASE WHEN saved_at IS NULL AND rev = 0 THEN 'placeholder: never saved a capture'
            ELSE 'holds a saved capture' END                 AS kind,
       count(*)                                              AS rows
  FROM quarry_profiles
 WHERE owner_user_id IS NULL
   AND scope <> 'org_shared'
 GROUP BY 1
 ORDER BY 1;

\echo '== 5. legacy private rows by age of the last write'
SELECT CASE WHEN updated_at >= now() - interval '7 days'  THEN 'a. last 7 days'
            WHEN updated_at >= now() - interval '30 days' THEN 'b. 8 to 30 days'
            WHEN updated_at >= now() - interval '90 days' THEN 'c. 31 to 90 days'
            ELSE 'd. older than 90 days' END                 AS last_write,
       count(*)                                              AS rows
  FROM quarry_profiles
 WHERE owner_user_id IS NULL
   AND scope <> 'org_shared'
 GROUP BY 1
 ORDER BY 1;

\echo '== 6. organizations with legacy private rows (top 20; tenant ids, counts only)'
SELECT org_id,
       count(*)                                              AS hidden_rows,
       max(updated_at)                                       AS last_write
  FROM quarry_profiles
 WHERE owner_user_id IS NULL
   AND scope <> 'org_shared'
 GROUP BY org_id
 ORDER BY hidden_rows DESC, org_id
 LIMIT 20;

\echo '== 7. sanity: a service principal must never be an owner (expect 0)'
SELECT count(*)                                              AS service_owned_rows
  FROM quarry_profiles
 WHERE owner_user_id LIKE 'service:%';

COMMIT;
