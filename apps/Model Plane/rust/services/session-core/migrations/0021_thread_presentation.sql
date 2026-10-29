-- Durable user-facing presentation for Model-Plane conversation threads.
--
-- This is deliberately additive and reversible at the application layer:
-- archiving hides a thread from ordinary history but never erases messages,
-- runs, or audit evidence. Subject erasure remains a separately authorized
-- Control-Plane workflow.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS presentation_title TEXT,
    ADD COLUMN IF NOT EXISTS presentation_preview TEXT,
    ADD COLUMN IF NOT EXISTS pinned_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;
