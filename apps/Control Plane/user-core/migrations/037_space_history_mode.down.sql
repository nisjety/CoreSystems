ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_history_mode_chk;
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS history_mode;
