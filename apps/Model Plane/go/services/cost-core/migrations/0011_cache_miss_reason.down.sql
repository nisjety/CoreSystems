-- Intentionally non-destructive, like 0004: the miss reason is retained on
-- rollback and the previous binary safely ignores the additive column.
SELECT 1;
