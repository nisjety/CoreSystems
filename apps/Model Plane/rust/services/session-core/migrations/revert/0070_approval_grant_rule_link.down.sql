-- Reverses 0070_approval_grant_rule_link.sql: drops the grant's rule link and the proposal table.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the
-- top level of migrations/. Run it by hand, against the database only.
--
-- A proposal is not a rule, and nothing reads one as authority. A rule the person confirmed lives
-- in capability-core's tool_policies and is not touched by this file. Look first, read-only:
--
--   SELECT state, count(*) FROM approval_rule_proposals GROUP BY state;
--
-- ORDER, both steps:
--   1. turn APPROVAL_RULE_PROPOSALS_ENABLED off on model-gateway, or roll it back to a build
--      without the proposal writes, so no decision proposes one while the table goes;
--   2. run this file.

ALTER TABLE approval_scope_grants DROP COLUMN IF EXISTS rule_id;
DROP INDEX IF EXISTS approval_rule_proposals_open_idx;
DROP TABLE IF EXISTS approval_rule_proposals;
