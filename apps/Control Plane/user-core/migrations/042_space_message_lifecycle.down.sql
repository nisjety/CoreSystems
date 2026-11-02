DROP INDEX IF EXISTS idx_space_message_policy_events_org;
DROP TABLE IF EXISTS space_message_policy_events;
DROP TABLE IF EXISTS space_message_policies;
ALTER TABLE space_effect_policies DROP COLUMN IF EXISTS message_moderate_entitled;
ALTER TABLE space_effect_policies DROP COLUMN IF EXISTS message_revise_entitled;
