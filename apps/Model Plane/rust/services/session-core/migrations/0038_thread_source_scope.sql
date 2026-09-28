-- First-message source scope is immutable through normal chat ingress.
-- Existing threads retain their previous workspace-grounded behavior.
ALTER TABLE threads ADD COLUMN source_scope TEXT NOT NULL DEFAULT 'workspace'
    CHECK (source_scope IN ('workspace', 'conversation'));
