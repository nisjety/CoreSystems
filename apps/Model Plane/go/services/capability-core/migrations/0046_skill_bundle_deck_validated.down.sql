-- Reverses 0046_skill_bundle_deck_validated.up.sql.
--
-- Drops the deck validation stamp. A build from before this migration lists a deck template by its theme file alone,
-- as it did before the stamp existed. Turn SKILL_DECK_TEMPLATES_ENABLED off before rolling capability-core back.

ALTER TABLE skill_bundles DROP COLUMN IF EXISTS deck_validated_at;
