-- Reverses 0004_audience_revision_ceiling.up.sql.
--
-- Dropping these columns removes the ceiling entirely, which makes every
-- process in a Space readable by any member holding a valid read decision.
-- That is a WIDENING, not a restoration: before 0004 no human could read the
-- registry at all, so there is no prior state this returns to. Roll this back
-- only together with whatever made the human read path unreachable again.

DROP INDEX IF EXISTS sandbox_processes_space_audience_idx;

ALTER TABLE sandbox_processes
    DROP COLUMN IF EXISTS recipient_audience_revision;

ALTER TABLE leases
    DROP COLUMN IF EXISTS recipient_audience_revision;
