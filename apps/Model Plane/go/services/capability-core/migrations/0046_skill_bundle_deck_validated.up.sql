-- 0046_skill_bundle_deck_validated.up.sql
--
-- The deck template validation stamp (F-f09-08). A bundle that holds the deck theme is listed as a deck template
-- only once it has passed ValidateDeckTemplate while SKILL_DECK_TEMPLATES_ENABLED is on, and this column records when
-- it did. Before it, a template was whatever a bundle held, so a bundle uploaded while the switch was off and never
-- checked was listed as a template the moment the switch went on.
--
-- Additive and nullable: every existing row is NULL, which lists no deck template. With the switch off nothing
-- writes the column. It is re-applied at every boot, so IF NOT EXISTS keeps the re-run a no-op.

ALTER TABLE skill_bundles ADD COLUMN IF NOT EXISTS deck_validated_at TIMESTAMPTZ NULL;
