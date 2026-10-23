-- WS17.E3 (plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.E3").
--
-- Plaintext removal for profile snapshots, after the backfill (migration 0017).
--
--   * The UPDATE clears the legacy column only where an envelope already sits
--     beside it. The envelope layer writes snapshot NULL together with the
--     envelope, and the lazy and backfill re-seals do the same, so in a
--     consistent store the UPDATE changes no row. It never removes the only copy
--     of a snapshot: a row without an envelope is not matched.
--   * The CHECK constraint is NOT VALID: it is enforced on new writes and does
--     not scan the table. It makes a row that holds both payloads impossible
--     from the current code.
--
-- What this migration does NOT do: it does not prove that no plaintext row is
-- left. That is the backfill gate, checked by the operator with the query in
-- docs/RUNBOOKS.md (WS17.E3) before the release that enables REQUIRED. A
-- pre-0016 binary that writes a row after `on` mode has run can produce a row
-- with both payloads, and then its write fails the constraint. Such a binary
-- cannot read the envelope rows either, so it is not a supported rollback target
-- once `on` has run.
--
-- Idempotent: the UPDATE is repeatable, and the constraint is added only when
-- absent. Applied files are immutable; do not edit.

UPDATE quarry_profiles
SET snapshot = NULL
WHERE envelope IS NOT NULL
  AND snapshot IS NOT NULL;

DO $$
BEGIN
    ALTER TABLE quarry_profiles
        ADD CONSTRAINT quarry_profiles_no_plaintext
        CHECK (snapshot IS NULL OR envelope IS NULL) NOT VALID;
EXCEPTION
    WHEN duplicate_object THEN
        NULL;
END
$$;
