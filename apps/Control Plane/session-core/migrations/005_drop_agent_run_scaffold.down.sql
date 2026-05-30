-- Migration 005 (down): no-op.
--
-- The agent-run tables now live in Rust session-core's `session_core`
-- database (Model Plane). Replaying 004 would create empty CP tables that
-- nothing reads or writes. If a real rollback is needed, restore from a
-- pre-wave-9 backup instead.

SELECT 1;
