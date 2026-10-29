-- A compact, operator-audited measurement ledger for customer feedback on a
-- resolved support ticket. It intentionally stores a score only: no customer
-- free text, contact address, delivery route, or raw survey token is retained.
CREATE TABLE ticket_csat_outcomes (
    org_id TEXT NOT NULL,
    ticket_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    score SMALLINT NOT NULL CHECK (score BETWEEN 1 AND 5),
    recorded_by TEXT NOT NULL DEFAULT '',
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, ticket_id),
    FOREIGN KEY (ticket_id) REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX ticket_csat_outcomes_org_recorded_at_idx
    ON ticket_csat_outcomes (org_id, recorded_at DESC);
