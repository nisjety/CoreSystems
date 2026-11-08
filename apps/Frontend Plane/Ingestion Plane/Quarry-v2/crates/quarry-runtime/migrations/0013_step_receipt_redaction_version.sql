-- Record which secret-redaction rule set a step receipt was masked under (WS12.S3).
--
-- `redaction_rule_version` is the `RULE_VERSION` the action, the observation and the
-- failure text were masked with when the receipt was written (for example
-- `qr-redact/3`).
--
-- Nullable and not backfilled: a receipt written before this migration, or while
-- masking was not enforcing, has no stamp, and the edge masks it again on every
-- read. The stored evidence is never rewritten (receipts are append-only), and
-- guessing a version for an old row would turn an unknown into a claim. Old
-- binaries keep working against the new column (they neither read nor write it).
ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS redaction_rule_version TEXT;
