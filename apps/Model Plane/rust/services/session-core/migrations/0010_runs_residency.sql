-- 0010_runs_residency.sql
--
-- P0.4 data residency — stamp the data-residency region on every run record so
-- a run's processing region is auditable after the fact (GDPR Art. 30 record of
-- processing). Set at StartRun from the configured Model-Plane region, which
-- defaults to the EU region (Sweden Central) unless an operator explicitly
-- overrides MODEL_PLANE_RESIDENCY.
--
-- Defaults to 'swedencentral' so existing rows (pre-migration) read as the EU
-- region rather than NULL/unknown. Idempotent ADD COLUMN IF NOT EXISTS.

ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS residency TEXT NOT NULL DEFAULT 'swedencentral';
