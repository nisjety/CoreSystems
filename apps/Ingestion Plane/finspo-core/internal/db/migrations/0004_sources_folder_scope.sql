-- Folder-scoped sources: a source may cover a single folder subtree inside a
-- document library instead of the whole drive. folder_id is the Graph
-- DriveItem id of the scoped folder; folder_path is its drive-root-relative
-- path ("/Contracts/2026") — the delta sync uses the path to filter items,
-- the id to re-anchor the picker UI. Both NULL means "whole library", which
-- keeps every pre-existing row's behavior unchanged.
ALTER TABLE sources ADD COLUMN IF NOT EXISTS folder_id   TEXT;
ALTER TABLE sources ADD COLUMN IF NOT EXISTS folder_path TEXT;

-- The plain (organization_id, drive_id) uniqueness made a drive registrable
-- exactly once per org. With folder scoping an org may register several
-- disjoint folders of the SAME drive, so the key widens to include the scoped
-- folder. COALESCE folds NULL to '' so "whole library" still collides with
-- itself (NULLs would otherwise never conflict and duplicate rows could pile
-- up on re-register).
ALTER TABLE sources DROP CONSTRAINT IF EXISTS sources_organization_id_drive_id_key;
CREATE UNIQUE INDEX IF NOT EXISTS uq_sources_drive_scope
    ON sources (organization_id, drive_id, COALESCE(folder_id, ''));
