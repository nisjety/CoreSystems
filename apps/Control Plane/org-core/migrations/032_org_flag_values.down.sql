-- Reverses 032. Dropping org_flag_values loses every organisation's flag choices, which
-- fall back to the defaults, and dropping org_flag_audit_outbox loses undelivered audit
-- intents. Roll back only before any organisation has written a flag, or after that
-- loss is accepted.
DROP TABLE IF EXISTS org_flag_audit_outbox;
DROP TABLE IF EXISTS org_flag_values;
