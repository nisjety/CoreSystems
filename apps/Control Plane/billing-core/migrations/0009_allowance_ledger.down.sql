DROP TABLE IF EXISTS billing_allowance_snapshots;
DROP TABLE IF EXISTS billing_allowance_entries;
DROP TABLE IF EXISTS billing_allowance_policies;
DROP TRIGGER IF EXISTS billing_account_allowance_pause ON billing_accounts;
DROP FUNCTION IF EXISTS billing_track_allowance_pause();
DROP TABLE IF EXISTS billing_allowance_pauses;
