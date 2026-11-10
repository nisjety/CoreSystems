-- Intentionally non-destructive, like 0004 and 0011: the price rows are operator
-- inputs that the repository cannot restore, so a rollback keeps them. The
-- previous binary never reads the table.
SELECT 1;
