-- Reverses 0102_approval_descriptor_revision.sql: drops the edit revision columns of
-- approval_continuation_descriptors.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without the approve-with-edits path first (or keep
-- APPROVAL_EDIT_ENABLED off), or a decide with an edit fails on the missing columns. A descriptor
-- already revised keeps its edited input and its new fingerprint in the row; only the record that it was
-- edited (revision, the original fingerprint, the time) is dropped. A granted, revised approval still
-- waiting to be consumed by an execution-core that matches on the original fingerprint (EP-10 S5) falls
-- back as it would for any changed call, so drain or expire such approvals before reverting.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE approval_continuation_descriptors
    DROP COLUMN IF EXISTS revised_at,
    DROP COLUMN IF EXISTS original_action_fingerprint,
    DROP COLUMN IF EXISTS revision;

COMMIT;
