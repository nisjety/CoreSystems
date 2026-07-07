-- ============================================================================
-- provider_leads — PII POSTURE CHANGE, SCOPED TO THIS TABLE ONLY
-- ============================================================================
-- leads-core is otherwise COMPANY-ONLY by design (lead_lists /
-- lead_list_companies carry no person/role/contact/birth-number column).
--
-- This table is the ONE deliberate exception: it stores provider lead-form
-- responses (LinkedIn Lead Gen forms via integration-corev2's actions
-- gateway), and lead-form answers are PERSON DATA (names, emails, phone
-- numbers — whatever the form asked). Rules that keep the posture contained:
--
--   1. Provider leads live ONLY here. They are NEVER mixed into the
--      company-search tables (lead_lists / lead_list_companies) and NEVER
--      flow through the metered company CSV-export path.
--   2. Every row is org-scoped; GDPR erasure is
--      DELETE /api/v1/provider-leads?organizationId=... (internal-only),
--      which removes an org's provider leads in one statement.
--   3. Sync-run audit events carry COUNTS ONLY — never form answers.
--
-- See leads-core/README.md ("PII posture") before touching this schema.
-- ============================================================================
CREATE TABLE IF NOT EXISTS provider_leads (
    id               TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL,
    connection_id    TEXT NOT NULL,
    provider_key     TEXT NOT NULL,
    provider_lead_id TEXT NOT NULL,
    form_id          TEXT NOT NULL DEFAULT '',
    form_name        TEXT NOT NULL DEFAULT '',
    submitted_at     TIMESTAMPTZ,
    -- Raw provider question/answer pairs, verbatim. PERSON DATA lives here.
    fields           JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Re-syncs are idempotent: the same provider lead upserts, never duplicates.
    UNIQUE (org_id, provider_key, provider_lead_id)
);
CREATE INDEX IF NOT EXISTS idx_provider_leads_org ON provider_leads (org_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_provider_leads_connection ON provider_leads (connection_id);
