-- WS22-S1 per-thread workspace (DEC-23): the files one user's thread keeps
-- across runs. Deliberately separate from workspace_files, which is keyed by
-- Space and run: thread rows and Space rows never share a table or an RPC.
--
-- Keyed by the owning user as well as the thread, so a second user in the same
-- org never reads or writes another user's thread files even with a guessed
-- thread id. Retention (WS22-S5) sweeps by last_used_at.
CREATE TABLE IF NOT EXISTS thread_workspaces (
    org_id        TEXT NOT NULL,
    owner_id      TEXT NOT NULL,
    thread_id     TEXT NOT NULL,
    revision      BIGINT NOT NULL DEFAULT 0,
    file_count    BIGINT NOT NULL DEFAULT 0,
    total_bytes   BIGINT NOT NULL DEFAULT 0,
    last_used_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, owner_id, thread_id)
);

CREATE INDEX IF NOT EXISTS thread_workspaces_last_used_idx
    ON thread_workspaces (last_used_at);

CREATE TABLE IF NOT EXISTS thread_workspace_files (
    org_id        TEXT NOT NULL,
    owner_id      TEXT NOT NULL,
    thread_id     TEXT NOT NULL,
    path          TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    size_bytes    BIGINT NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, owner_id, thread_id, path)
);
