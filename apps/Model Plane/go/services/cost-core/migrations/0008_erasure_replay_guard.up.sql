-- Opaque irreversible tombstones prevent retained usage replay from restoring
-- an erased organization's ledger. No prompts, users or organization names.
CREATE TABLE IF NOT EXISTS cost_erased_organizations (
  org_hash TEXT PRIMARY KEY,
  erased_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
