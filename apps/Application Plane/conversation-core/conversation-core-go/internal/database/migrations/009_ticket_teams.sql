-- Canonical Ticketing routing teams. Provider Inbox groups remain integration
-- metadata and must never be treated as the authority for ticket ownership.
CREATE TABLE IF NOT EXISTS conversation_ticket_teams (
    org_id TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, id),
    UNIQUE (org_id, name)
);

CREATE INDEX IF NOT EXISTS conversation_ticket_teams_org_active_name_idx
    ON conversation_ticket_teams (org_id, active DESC, name ASC);

-- Existing ticket routing remains available after the directory migration.
-- A team is only backfilled when it already has a stable ID; arbitrary team
-- names without IDs are intentionally not elevated into canonical authority.
INSERT INTO conversation_ticket_teams (id, org_id, name, active, created_at, updated_at)
SELECT team_id, org_id,
       CASE WHEN team_name <> '' THEN team_name ELSE team_id END,
       TRUE, NOW(), NOW()
FROM conversation_tickets
WHERE team_id <> ''
ON CONFLICT (org_id, id) DO NOTHING;
