-- Reverts 0047_run_loop_checkpoints.sql. Dropping the table discards every stored
-- fold, so a rollback strands any run that is paused on one: roll back only with
-- `continue_disabled` set and no run in `paused` or `parked` state.

DROP INDEX IF EXISTS run_loop_checkpoints_state_idx;
DROP TABLE IF EXISTS run_loop_checkpoints;
