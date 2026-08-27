-- Venice-style privacy tiering (PROVIDER_AND_PRIVACY_STRATEGY.md §4, §4.5):
-- the models registry gains the two columns that make its
-- ListCapabilities/GetCapability projection an honest tier disclosure
-- surface instead of decorative metadata. Labels mirror the wire enum
-- (model_plane.v1.PrivacyTier) lowercased to snake_case, matching the
-- pinned wire contract and model-gateway's /v1/models disclosure exactly:
-- unspecified|global|eu_resident|zdr_contractual|sovereign.
--
-- 'unspecified' is the safe default: this table has no per-deployment ZDR/
-- residency attestation data (that lives in inference-core's per-provider
-- ProviderCapabilities, derived from env-configured attestations), so a
-- stronger claim here would be fabricated. See the ADR at
-- docs/architecture/adr-provider-side-orchestration-privacy-tiers.md.
ALTER TABLE models
    ADD COLUMN IF NOT EXISTS privacy_tier TEXT NOT NULL DEFAULT 'unspecified',
    ADD COLUMN IF NOT EXISTS residency TEXT NOT NULL DEFAULT '';

ALTER TABLE models
    DROP CONSTRAINT IF EXISTS models_privacy_tier_check,
    ADD CONSTRAINT models_privacy_tier_check
        CHECK (privacy_tier IN ('unspecified', 'global', 'eu_resident', 'zdr_contractual', 'sovereign'));

-- The seed row for google/gemini-1.5-pro (0002_seed_models.up.sql) advertises
-- a model no adapter in the plane can serve — inference-core registers only
-- openai and anthropic providers; there is no Google adapter. Now that this
-- table's rows carry a privacy_tier disclosure, leaving it enabled would
-- fabricate a tier claim for a model that cannot serve traffic. Soft-delete
-- it (matching this table's existing deleted_at pattern) rather than hard
-- DELETE so the audit trail of "this row existed and was decorative"
-- survives.
UPDATE models
SET deleted_at = now(), updated_at = now()
WHERE provider = 'google'
  AND name = 'gemini-1.5-pro'
  AND org_id IS NULL
  AND deleted_at IS NULL;
