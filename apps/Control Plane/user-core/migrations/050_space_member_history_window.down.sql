-- Reverts 050_space_member_history_window: removes the per-member history
-- windows. The room-level history_mode (037) and the membership ledger are not
-- touched; without the rows every member reads under the room's own mode again.
DROP TABLE IF EXISTS space_member_history_windows;
