-- Intentionally non-destructive, like 0004 and 0011: the marker is retained on
-- rollback and the previous binary safely ignores the additive column.
SELECT 1;
