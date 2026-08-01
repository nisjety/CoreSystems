-- Implicit dissatisfaction signals alongside explicit ratings.
--
-- A regenerate, a re-asked question or "det er feil" in the next message is
-- evidence that the previous answer missed. It is WEAK evidence — every one has
-- an innocent reading — so it is stored beside explicit ratings and weighted far
-- below them by the quality policy, never merged into them.
--
-- The primary-key change is the load-bearing part of this migration.
--
-- The old key was (org_id, run_id, user_id, skill_id), which makes a re-rating an
-- upsert — correct for a human changing their mind. But it means an implicit
-- signal about the same turn would UPSERT OVER that human's rating: a stray
-- regenerate would silently erase a deliberate thumbs-up, and the store would
-- show one `poor` row where a person had said `good`. model-gateway already
-- namespaces the two on the wire (`feedback_implicit_...` vs `feedback_...`), and
-- without this change the database would undo that at the last step.
--
-- Adding source AND signal_kind to the key keeps:
--   * one row per human per turn (source='explicit', kind='')  — re-rating still
--     replaces, as before;
--   * one row per implicit KIND per turn — a regenerate and a correction on the
--     same turn are two samples, while detecting the same kind twice stays one.

ALTER TABLE feedback_ratings
    ADD COLUMN IF NOT EXISTS source text NOT NULL DEFAULT 'explicit'
        CHECK (source IN ('explicit', 'implicit'));

-- Which detector fired, for implicit rows. Empty for explicit ones, which have
-- no kind — the person simply stated a judgement.
ALTER TABLE feedback_ratings
    ADD COLUMN IF NOT EXISTS signal_kind text NOT NULL DEFAULT '';

-- The detector's confidence, 0 < x <= 1. Always 1 for an explicit rating: a
-- stated judgement is a full sample. Scaled again by the policy's implicit
-- weight, so this is the detector's half of the discount, not the whole of it.
ALTER TABLE feedback_ratings
    ADD COLUMN IF NOT EXISTS signal_strength double precision NOT NULL DEFAULT 1
        CHECK (signal_strength > 0 AND signal_strength <= 1);

-- Repoint the primary key. Existing rows all default to
-- ('explicit', '') so the new key is a strict superset of the old one and no
-- row can collide during the swap.
ALTER TABLE feedback_ratings
    DROP CONSTRAINT IF EXISTS feedback_ratings_pkey;
ALTER TABLE feedback_ratings
    ADD CONSTRAINT feedback_ratings_pkey
    PRIMARY KEY (org_id, run_id, user_id, skill_id, source, signal_kind);

-- The demotion sweep reads skill-attached rows per tenant and needs the source
-- split to weigh them, so carry it in the index rather than re-reading the heap.
CREATE INDEX IF NOT EXISTS feedback_ratings_org_skill_source_idx
    ON feedback_ratings (org_id, skill_id, source)
    WHERE skill_id <> '';
