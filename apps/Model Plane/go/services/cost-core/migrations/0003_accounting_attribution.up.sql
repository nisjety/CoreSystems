-- Additive, rollback-compatible producer attribution for accounting events.
-- Existing binaries ignore this column; new binaries derive it from verified
-- HTTP identity or the allowlisted NATS producer. Existing rows remain intact.
ALTER TABLE cost_entries
    ADD COLUMN IF NOT EXISTS producer_id text NOT NULL DEFAULT '';
