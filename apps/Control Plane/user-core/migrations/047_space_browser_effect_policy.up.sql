-- WS20-S1: the room-scoped browser run (model.space.browser_run) gets its own
-- deny-by-default entitlement. A run starts a browser session for a Space and
-- writes a result turn, so Control refuses every browser run decision under
-- Space zero data retention (the bit is ZDR inert). The column defaults FALSE for
-- every organisation. Only the agent_computer preset turns it on
-- (org_policy_presets.go); the catalogue row (entitlement_catalog.go) is the only
-- other place the bit appears. The column is browser_entitled, not
-- browser_run_entitled (DEC-19).
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS browser_entitled BOOLEAN NOT NULL DEFAULT FALSE;
