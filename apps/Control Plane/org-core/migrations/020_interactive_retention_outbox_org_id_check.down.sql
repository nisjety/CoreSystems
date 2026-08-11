-- Only the constraint is reversible. The rows 020 deleted were undeliverable
-- outbox entries addressed to no organisation; there is nothing to restore and
-- nothing that reads them.
ALTER TABLE organization_interactive_retention_outbox
  DROP CONSTRAINT IF EXISTS organization_interactive_retention_outbox_org_id_nonempty;
