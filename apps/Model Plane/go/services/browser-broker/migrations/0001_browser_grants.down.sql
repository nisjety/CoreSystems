-- Reverts 0001_browser_grants.up.sql. Dropping the table drops its indexes.
DROP TABLE IF EXISTS browser_grants;
