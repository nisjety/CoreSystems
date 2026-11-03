-- Reverses 0104_routing_policy_candidates.sql: drops the candidate store and routing_policy.candidate_id.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER, both steps:
--   1. roll session-core back to a build without the candidate RPCs, or they fail on the missing table;
--   2. copy routing_policy_candidates first if its audit trail matters: this file destroys it.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE routing_policy
    DROP COLUMN IF EXISTS candidate_id;

DROP TABLE IF EXISTS routing_policy_candidates;

COMMIT;
