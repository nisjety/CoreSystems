-- leads-core W1: saved, org-scoped company lists from filtered Brreg search.
-- COMPANY DATA ONLY — there is deliberately no person/role/contact/birth-number
-- column anywhere in this schema.
CREATE TABLE IF NOT EXISTS lead_lists (
    id         TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_lead_lists_org ON lead_lists (org_id, created_at DESC);

CREATE TABLE IF NOT EXISTS lead_list_companies (
    id                  TEXT PRIMARY KEY,
    list_id             TEXT NOT NULL REFERENCES lead_lists(id) ON DELETE CASCADE,
    org_id              TEXT NOT NULL,
    organisasjonsnummer TEXT NOT NULL,
    navn                TEXT NOT NULL,
    organisasjonsform   TEXT NOT NULL DEFAULT '',
    naeringskode        TEXT NOT NULL DEFAULT '',
    naering_beskrivelse TEXT NOT NULL DEFAULT '',
    kommunenummer       TEXT NOT NULL DEFAULT '',
    poststed            TEXT NOT NULL DEFAULT '',
    antall_ansatte      INTEGER,
    registreringsdato   TEXT NOT NULL DEFAULT '',
    hjemmeside          TEXT NOT NULL DEFAULT '',
    konkurs             BOOLEAN NOT NULL DEFAULT false,
    under_avvikling     BOOLEAN NOT NULL DEFAULT false,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (list_id, organisasjonsnummer)
);
CREATE INDEX IF NOT EXISTS idx_lead_list_companies_list ON lead_list_companies (list_id);
