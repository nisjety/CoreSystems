-- Bring organization_interactive_retention_outbox in line with its sibling
-- outbox: 017 created organization_gdpr_audit_outbox WITH
-- `..._org_id_nonempty CHECK (btrim(org_id) <> '')`, and 019 created this table
-- without it. Same shape, same publisher pattern, same failure mode — the
-- missing constraint was an inconsistency, not a decision.
--
-- Why it matters here: org_id is what the publisher fans the retention event
-- out ON. `NOT NULL` alone still admits '' and '   ', and such a row is not a
-- record of anything — it is an event addressed to no organisation. It cannot
-- be published, so it is retried until it exhausts `attempts` and then sits in
-- the table looking like a real backlog.

-- Undeliverable rows must go before the constraint can be validated. This is
-- safe to delete, unlike most migration DML: this table is a transient work
-- queue, not a system of record, and a row with no org has no destination to
-- be delivered to. The org's actual retention posture lives on the
-- organizations row and is unaffected.
DELETE FROM organization_interactive_retention_outbox
WHERE btrim(org_id) = '';

-- Postgres has no ADD CONSTRAINT IF NOT EXISTS, so guard on the catalog to
-- keep this re-runnable.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'organization_interactive_retention_outbox_org_id_nonempty'
  ) THEN
    ALTER TABLE organization_interactive_retention_outbox
      ADD CONSTRAINT organization_interactive_retention_outbox_org_id_nonempty
      CHECK (btrim(org_id) <> '');
  END IF;
END
$$;
