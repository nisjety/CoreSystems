-- Record which agent drove an agent-initiated browser action.
--
-- A typed, identifier-only JSON object (`basis`, and where known `principalId`,
-- `aiRunId`, `label`; see quarry_runtime::step_receipts::AgentIdentity). Never a
-- URL, a title or free text. Only `principalId` of a `service_principal` basis is
-- verified; `aiRunId` and `label` are advisory.
--
-- Nullable and not backfilled: a receipt written before this migration, and every
-- receipt a person drove, has no agent. Old binaries keep working against the new
-- column (they neither read nor write it).
ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS initiated_agent JSONB
        CHECK (initiated_agent IS NULL OR jsonb_typeof(initiated_agent) = 'object');
