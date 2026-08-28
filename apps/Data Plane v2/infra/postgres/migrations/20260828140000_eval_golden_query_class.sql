-- Segments the golden set by query CLASS, so the eval scorecard can report
-- lexical-identifier and natural-language quality separately instead of one
-- blended number.
--
-- Why this matters: `w_bm25` is worth +0.37 nDCG on mined identifier queries
-- and COSTS nDCG on natural-language questions (docs/retrieval-fusion-
-- evidence-2026-08-26.md, round 6). Averaged into one number those two effects
-- partially cancel, so a regression specific to one class can move the blended
-- metric by less than its noise floor and pass unnoticed. The retrieval-side
-- gate (scripts/eval-gate.py) already gates the two classes as separate cells
-- for exactly this reason; this migration brings the same segmentation to the
-- durable, in-service eval (data-quality-go), which scores real traffic
-- against `eval_golden_judgments` rather than a fixed offline query list.
--
-- NOT NULL with a default rather than nullable: every one of the 87 existing
-- rows genuinely IS a natural-language question (docs/eval-build-golden-set.py
-- hand-authors only that class), so the default is the correct classification
-- for existing data, not a placeholder needing a backfill pass.
ALTER TABLE eval_golden_judgments
    ADD COLUMN IF NOT EXISTS query_class TEXT NOT NULL DEFAULT 'natural_language';
