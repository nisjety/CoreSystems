-- Reverses 0012_run_watch_subscriptions.up.sql.
DROP INDEX IF EXISTS run_watch_subscriptions_pending_idx;
DROP INDEX IF EXISTS run_watch_subscriptions_run_user_uq;
DROP TABLE IF EXISTS run_watch_subscriptions;
