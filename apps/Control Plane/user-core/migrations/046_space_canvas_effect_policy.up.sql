-- WS19-S2: the room canvas (one shared Markdown document per Space, stored in Data
-- Plane) gets its own deny-by-default entitlement. Pilot-only until counsel rules
-- (SD-08 and SD-19): the column defaults FALSE for every organisation, no preset
-- enables it (org_policy_presets.go), and the catalogue row (entitlement_catalog.go)
-- is the only other place the bit appears. Under Space zero data retention Control
-- refuses every canvas decision, read and write, so the bit is inert there.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS canvas_entitled BOOLEAN NOT NULL DEFAULT FALSE;
