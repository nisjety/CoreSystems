-- Manual memory changes supersede extraction from earlier conversation text.
-- These rows retain control metadata only, never the forgotten content.
CREATE TABLE user_memory_controls (
    org_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    extract_after TIMESTAMPTZ NOT NULL DEFAULT '-infinity',
    PRIMARY KEY (org_id, user_id)
);

CREATE TABLE forgotten_memory_ids (
    org_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    PRIMARY KEY (org_id, user_id, memory_id),
    FOREIGN KEY (org_id, user_id) REFERENCES user_memory_controls(org_id, user_id) ON DELETE CASCADE
);
