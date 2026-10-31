-- 0001_baseline: anbud-core owns this database from here on.
--
-- Deliberately no domain tables yet. They arrive with the raw-document store
-- and the procedure/notice/lot model (docs/ARCHITECTURE.md, "Build order"),
-- once the shape of a downloaded Doffin notice has been captured with
-- cmd/doffin-probe. Recording this version proves the migration path (pool,
-- runner, schema_migrations) works end to end on first bring-up.
SELECT 1;
