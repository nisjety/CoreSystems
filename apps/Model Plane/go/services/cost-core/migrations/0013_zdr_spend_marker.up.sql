-- Marks the content-free spend row of a zero-data-retention turn (FU-30). The
-- row is ids, token counts and an amount, admitted by its ZDR_SPEND kind; this
-- column lets a reader tell it from an ordinary row. FALSE on every existing
-- row is the true historical value: until now no ZDR row was stored.
--
-- Additive and idempotent: the entrypoint re-applies every up migration on boot.
ALTER TABLE cost_entries
    ADD COLUMN IF NOT EXISTS zdr boolean NOT NULL DEFAULT false;
