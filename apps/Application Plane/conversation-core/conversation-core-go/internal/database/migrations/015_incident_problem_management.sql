-- Incidents and problems are operational records, not a ticket work type.
-- Tickets may be explicitly linked to an incident, but neither record's
-- lifecycle implicitly changes the other.

CREATE TABLE IF NOT EXISTS conversation_problems (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    problem_key TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'investigating',
    owner_user_id TEXT NOT NULL DEFAULT '',
    owner_name TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    root_cause TEXT NOT NULL DEFAULT '',
    created_by_user_id TEXT NOT NULL DEFAULT '',
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, problem_key),
    CHECK (char_length(title) BETWEEN 1 AND 300),
    CHECK (status IN ('investigating', 'known_error', 'resolved'))
);

CREATE INDEX IF NOT EXISTS conversation_problems_org_status_updated_idx
    ON conversation_problems (org_id, status, updated_at DESC);

CREATE TABLE IF NOT EXISTS conversation_incidents (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    incident_key TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'declared',
    severity TEXT NOT NULL DEFAULT 'medium',
    owner_user_id TEXT NOT NULL DEFAULT '',
    owner_name TEXT NOT NULL DEFAULT '',
    customer_impact TEXT NOT NULL DEFAULT '',
    problem_id TEXT REFERENCES conversation_problems(id) ON DELETE SET NULL,
    declared_by_user_id TEXT NOT NULL DEFAULT '',
    declared_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, incident_key),
    CHECK (char_length(title) BETWEEN 1 AND 300),
    CHECK (status IN ('declared', 'investigating', 'monitoring', 'resolved')),
    CHECK (severity IN ('low', 'medium', 'high', 'critical'))
);

CREATE INDEX IF NOT EXISTS conversation_incidents_org_status_updated_idx
    ON conversation_incidents (org_id, status, updated_at DESC);

CREATE TABLE IF NOT EXISTS conversation_incident_ticket_links (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    incident_id TEXT NOT NULL REFERENCES conversation_incidents(id) ON DELETE CASCADE,
    ticket_id TEXT NOT NULL REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    relationship TEXT NOT NULL DEFAULT 'affected',
    created_by_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, incident_id, ticket_id),
    CHECK (relationship IN ('affected', 'root_cause', 'related'))
);

CREATE INDEX IF NOT EXISTS conversation_incident_ticket_links_ticket_idx
    ON conversation_incident_ticket_links (org_id, ticket_id, created_at DESC);

-- Operational audit records must exist even before an incident is linked to a
-- customer ticket, so ticket/conversation audit streams cannot be misused.
CREATE TABLE IF NOT EXISTS conversation_incident_audits (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    incident_id TEXT NOT NULL REFERENCES conversation_incidents(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    actor_user_id TEXT NOT NULL DEFAULT '',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX IF NOT EXISTS conversation_incident_audits_incident_idx
    ON conversation_incident_audits (org_id, incident_id, created_at DESC);

CREATE TABLE IF NOT EXISTS conversation_problem_audits (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    problem_id TEXT NOT NULL REFERENCES conversation_problems(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    actor_user_id TEXT NOT NULL DEFAULT '',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX IF NOT EXISTS conversation_problem_audits_problem_idx
    ON conversation_problem_audits (org_id, problem_id, created_at DESC);
