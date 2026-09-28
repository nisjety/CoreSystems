-- 0037_agent_memory_lineage.sql
--
-- Write-time memory grounding (extractor-grounding design doc, 2026-09).
--
-- The root cause this supports: `agent_memory`'s uniqueness is real but keyed
-- on exact-string `key`, and both the phrase matcher and the LLM extractor
-- mint `key` from free-form/model-chosen text with no cross-call
-- normalization, so semantically identical facts phrased differently
-- legitimately get distinct keys and both pass the constraint as "new" rows
-- (e.g. "User prefers dark mode." stored once as
-- `user:preference:jeg_foretrekker_mork_modus` and again as
-- `user:preference:i_prefer_dark_mode`).
--
-- The fix is a write-time grounding pass (session-core's new
-- `memory_grounding` module) that, before storing a candidate whose key does
-- not exactly match anything, asks a classifier whether it should ADD (no
-- real overlap), EXTEND (adds detail to an existing memory), SUPERSEDE
-- (replaces/corrects one) or NOOP (redundant). ADD is a normal insert through
-- the existing `upsert_agent_memory` path, unchanged. EXTEND and SUPERSEDE
-- must NEVER overwrite or delete the memory they act on -- a correction is
-- itself a fact worth keeping a history of, and an in-place UPDATE would
-- destroy the very provenance a person reviewing "what do you remember about
-- me" relies on. So both write a NEW row and mark the old one non-current.
--
-- Two columns:
--
--   * `supersedes_id` -- nullable, self-referencing FK. Points at the row
--     this one carries forward from. `ON DELETE SET NULL` rather than
--     CASCADE or the no-action default: the pointer runs from the NEW row to
--     the OLD one, so if the old row is later hard-deleted by some other path
--     (a user's `DeleteMemory` call, a GDPR erasure), the new row -- which is
--     the CURRENT fact -- must survive with its lineage pointer simply
--     cleared, not be dragged into deletion by a foreign key aimed the wrong
--     direction. `ON DELETE CASCADE` would do exactly that (delete the
--     replacement because the thing it replaced was removed), which is the
--     one behavior this column must never have.
--
--   * `is_latest` -- boolean, default true. Every row this migration finds is
--     the only version of itself that has ever existed, so `true` for all of
--     them is exactly correct, not a placeholder. A SUPERSEDE or EXTEND write
--     flips the OLD row's `is_latest` to false in the same transaction that
--     inserts the new one with `is_latest = true` -- both statements commit
--     together or neither does, so a reader can never observe two rows
--     claiming to be latest, or zero.
--
-- EXTEND and SUPERSEDE share `supersedes_id` rather than getting a second
-- `extends_id` column. The read side (0004's `search_agent_memory` /
-- `load_agent_memory_context_rows` / `list_user_memory`, all updated
-- alongside this migration to filter `is_latest = true`) only ever needs to
-- know "is this row still current," which does not depend on WHY a row
-- stopped being current -- an EXTENDed row is exactly as superseded, for
-- ranking and injection purposes, as a SUPERSEDEd one. The distinction that
-- DOES matter -- an operator or a future audit surface asking "did this
-- change because it was corrected or because more detail arrived?" -- is
-- carried the same way every other write-time distinction already is in this
-- table: as a marker in `source_links` (`lineage:extend` / `lineage:supersede`,
-- alongside the existing `extractor:llm` / `tool:save_memory` markers), not a
-- second schema column for a question the hot read path never asks.
--
-- Non-destructive and reversible: both columns are nullable/defaulted, no
-- existing row changes shape, and the ON CONFLICT targets every other
-- migration already established are untouched, so a deployment that never
-- sets `MEMORY_GROUNDING_ENABLED=true` never produces a row where either
-- column differs from its default.

ALTER TABLE agent_memory
    ADD COLUMN IF NOT EXISTS supersedes_id TEXT REFERENCES agent_memory(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS is_latest BOOLEAN NOT NULL DEFAULT true;

-- Lineage lookups ("what superseded this row", GDPR/audit walks) are rare and
-- id-keyed; a plain btree on the FK column is enough and keeps this migration
-- from guessing at a read pattern the grounding module does not have yet.
CREATE INDEX IF NOT EXISTS agent_memory_supersedes_idx
    ON agent_memory (supersedes_id)
    WHERE supersedes_id IS NOT NULL;
