-- Reverses 0032_document_pii_default_policy.up.sql: removes only the seeded global row, by id and
-- only while it still carries the migration's created_by. Org rows of the same kind are untouched.
DELETE FROM safety_policies
WHERE id = 'sp_global_document_pii'
  AND org_id = 'global'
  AND created_by = 'migration:0032_document_pii_default_policy';
