-- A support-contact-specific preference for post-resolution satisfaction
-- surveys. It is intentionally separate from Control-Plane user consent and
-- defaults to false: a customer contact is not a Verevon user or marketing
-- recipient. The actor is retained as a compact audit attribution.
CREATE TABLE conversation_csat_preferences (
    org_id TEXT NOT NULL,
    contact_id TEXT NOT NULL REFERENCES conversation_contacts(id) ON DELETE CASCADE,
    opted_in BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, contact_id)
);

CREATE INDEX conversation_csat_preferences_org_opted_in_idx
    ON conversation_csat_preferences (org_id, opted_in, updated_at DESC);
