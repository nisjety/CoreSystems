-- Compact, append-only browser owner timeline.
--
-- The JSONB payload is a typed, privacy-bounded control/tab/devtools/lifecycle
-- event. It must never contain DOM contents, screenshots, credentials, raw
-- DevTools/CDP data, or browser-storage values; action proof remains in the
-- separately governed step-receipt table.
CREATE TABLE IF NOT EXISTS quarry_browser_timeline_events (
    event_id    TEXT        PRIMARY KEY,
    org_id      TEXT        NOT NULL CHECK (org_id <> ''),
    actor_id    TEXT        NOT NULL CHECK (actor_id <> ''),
    run_id      TEXT        NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    event       JSONB       NOT NULL
);

CREATE INDEX IF NOT EXISTS quarry_browser_timeline_events_owner_run_idx
    ON quarry_browser_timeline_events (org_id, actor_id, run_id, occurred_at ASC, event_id ASC);
