-- 0004_audience_revision_ceiling.up.sql
--
-- Records the recipient-audience revision a lease was granted under, and
-- carries it onto every process that lease admits, so a human reading a
-- Space's processes is subject to the same audience ceiling Session Core
-- already applies to its runs.
--
-- WHAT THE CEILING IS FOR
--
-- A Space's recipient audience is who its records may be disclosed to, and it
-- has a revision that bumps whenever that set changes. Session Core filters
-- `t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <=
-- $n`, where $n is the revision carried by the reader's own Control-signed
-- read decision (run_service_grpc.rs). The effect: a record produced under an
-- audience the current reader's decision predates is not returned. Processes
-- need the identical rule, because process output is content in exactly the
-- sense a conversation turn is.
--
-- WHY THE LEASE AND NOT THE PROCESS IS THE SOURCE
--
-- A process is registered on execution-core's own service token, which carries
-- no Control decision at all — by the time a process exists, the decision that
-- authorized its Space is long gone. The lease is the last point at which one
-- was verified, so the revision is captured there and the process inherits it
-- through the same join that already gives it its Space. There is nowhere else
-- it could honestly come from.
--
-- This was the S4.2 design's "storing the lease's decision revisions on the
-- lease" open question. The answer taken here is the narrow one: this single
-- revision, because it is the only one §7's read path actually consumes.
-- Persisting the whole Permissions list and all five revisions was the wider
-- option; it is not needed yet, and a column nothing reads is a column that
-- drifts out of meaning. `processes_permitted` (migration 0003) remains the
-- derived boolean it always was.
--
-- WHY DEFAULT 0 RATHER THAN NULL
--
-- Session Core's column is nullable and its filter spells the NULL case out.
-- Here 0 does the same job without the three-valued logic: a read decision
-- whose own revision is 0 is refused by the verifier before any row is
-- compared, so no real reader can ever have a ceiling of 0, and `stored <=
-- reader` therefore admits every 0 row exactly as `IS NULL OR` would. Rows
-- that predate this migration get 0 and stay readable, which is the correct
-- outcome: they were created before the ceiling existed, and retroactively
-- hiding them would be a silent data loss dressed up as a security control.

ALTER TABLE leases
    ADD COLUMN IF NOT EXISTS recipient_audience_revision BIGINT NOT NULL DEFAULT 0;

ALTER TABLE sandbox_processes
    ADD COLUMN IF NOT EXISTS recipient_audience_revision BIGINT NOT NULL DEFAULT 0;

-- The human read path lists a Space's processes newest-first under a ceiling,
-- so the ceiling belongs in the same index as the Space. The existing
-- (org_id, space_id, id DESC) listing index cannot serve the added predicate.
CREATE INDEX IF NOT EXISTS sandbox_processes_space_audience_idx
    ON sandbox_processes (org_id, space_id, recipient_audience_revision, id DESC);
