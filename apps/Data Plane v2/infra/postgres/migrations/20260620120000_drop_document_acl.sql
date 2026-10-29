-- Drop the dormant, duplicated document_acl table from the Data Plane.
--
-- Per-User Data Ownership & Sharing (PR-1): the single grant authority is
-- user-core's resource_grants. This Data-Plane copy was DEAD (zero code refs)
-- and DUPLICATED the Control-Plane table. Retrieval enforces ownership via the
-- documents.owner_id/visibility columns (PR-2) + resource_grants (PR-3), so no
-- Data-Plane-local ACL table is needed.
DROP TABLE IF EXISTS document_acl;
