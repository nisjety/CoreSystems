-- 0032_document_pii_default_policy.up.sql
--
-- E3.6-8 (capability-core half): the catalogue key for the org default of fail-closed PII
-- redaction of parsed documents. capability-core owns the policy (safety_policies, beside
-- pii_filter / content_safety / injection_defense); Quarry's document pipeline reads the
-- effective value and applies it, and the optional `ai_language_pii` model tier is a field of the
-- same policy.
--
-- The key is the safety_policies KIND `document_pii`. Its config_json is the closed schema
-- {"mode": "off" | "redact" | "fail_closed", "model_tier": bool}, validated by the SafetyHandler
-- (internal/api/safety_document_pii.go) when DOCUMENT_PII_POLICY is on:
--   off          no redaction stage for parsed documents (today's behaviour);
--   redact       redact, and keep going if a page's redaction errors;
--   fail_closed  redact, and a page whose redaction errors is a failed page with a typed reason,
--                never empty markdown.
--
-- This migration seeds the PLATFORM default only: one global row, mode `off`, so nothing changes
-- until an operator or an org writes a stronger one. The row is inserted once and never rewritten
-- (ON CONFLICT DO NOTHING), so an operator's change survives every boot. An org's own row (same
-- kind, its org_id) beats the global one; a stored value that does not parse is read as fail_closed.
-- No other reader of safety_policies looks at this kind (model-gateway reads kind pii_filter and
-- content_safety only).

INSERT INTO safety_policies
    (id, org_id, name, description, kind, config_json, applies_to, priority, enabled, created_by)
VALUES
    ('sp_global_document_pii', 'global', 'document_pii_default',
     'Platform default for PII redaction of parsed documents (E3.6-8): off until an org or an operator sets a stronger mode.',
     'document_pii', '{"mode": "off", "model_tier": false}'::jsonb, ARRAY['document'], 0, true,
     'migration:0032_document_pii_default_policy')
ON CONFLICT (id) DO NOTHING;
