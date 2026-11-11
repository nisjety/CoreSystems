-- Reverses 0122_promote_use_counts.sql: drops the durable promote-on-use counter table.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set PROMOTE_ON_USE_DURABLE to anything but true in execution-core (or roll session-core back to a build
-- without the PromoteOnUseService) first, or the count RPC fails on the missing table; execution-core then
-- fails closed and promotes nothing. The rows are counts and URL hashes only, so nothing is copied first.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;

DROP TABLE IF EXISTS promote_use_counts;

COMMIT;
