-- Migration 005: drop the agent-run scaffold tables (G36-cutover Step D).
--
-- Wave 9 (§8.24 + §8.26 in `apps/Frontend Plane/verevon/verevon-gap.md`) moved
-- agent-run state — plan / todo / lineage / approval — to Rust session-core
-- in the Model Plane. The corresponding repositories, service methods, and
-- HTTP handlers were removed from CP session-core in the same wave. This
-- migration removes the now-orphaned tables.
--
-- Verified zero external callers via:
--   grep -rn "/v1/(plans|todos|lineage|approvals)" apps/ --include="*.go"
--                                                       --include="*.ts"
--                                                       --include="*.rs"
-- → only matches were inside the CP source we just deleted.
--
-- Tables dropped here were created by 004_add_orchestration_tables.up.sql.
-- The drop is unconditional because we own the migration ledger; rolling
-- back would replay 004 and recreate empty tables.

BEGIN;

DROP TABLE IF EXISTS subagent_lineage_edges CASCADE;
DROP TABLE IF EXISTS approvals              CASCADE;
DROP TABLE IF EXISTS todos                  CASCADE;
DROP TABLE IF EXISTS plan_steps             CASCADE;
DROP TABLE IF EXISTS plans                  CASCADE;

COMMIT;
