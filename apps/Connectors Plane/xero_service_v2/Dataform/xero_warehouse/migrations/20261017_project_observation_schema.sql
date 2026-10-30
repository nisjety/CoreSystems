-- Append-only Project observation schema migration.
--
-- This is intentionally an explicit, idempotent BigQuery migration rather
-- than Dataform onSchemaChange: "EXTEND". It was applied to
-- aquatiq-datavarehus on 2026-10-17 before the Project models were changed to
-- onSchemaChange: "FAIL". Re-running it never rewrites or removes history.

ALTER TABLE `aquatiq-datavarehus.dw_bronze_xero.xero_project_observations`
  ADD COLUMN IF NOT EXISTS duration_minutes INT64,
  ADD COLUMN IF NOT EXISTS entered_at TIMESTAMP,
  ADD COLUMN IF NOT EXISTS entry_date DATE,
  ADD COLUMN IF NOT EXISTS non_chargeable_minutes INT64,
  ADD COLUMN IF NOT EXISTS time_entry_status STRING;

ALTER TABLE `aquatiq-datavarehus.dw_bronze_xero.xero_project_detail_observations`
  ADD COLUMN IF NOT EXISTS amount_invoiced_currency_code STRING,
  ADD COLUMN IF NOT EXISTS amount_to_be_invoiced NUMERIC,
  ADD COLUMN IF NOT EXISTS amount_to_be_invoiced_currency_code STRING,
  ADD COLUMN IF NOT EXISTS duration_minutes INT64,
  ADD COLUMN IF NOT EXISTS entered_at TIMESTAMP,
  ADD COLUMN IF NOT EXISTS entry_date DATE,
  ADD COLUMN IF NOT EXISTS fixed_minutes INT64,
  ADD COLUMN IF NOT EXISTS minutes_invoiced INT64,
  ADD COLUMN IF NOT EXISTS non_chargeable_minutes INT64,
  ADD COLUMN IF NOT EXISTS time_entry_status STRING,
  ADD COLUMN IF NOT EXISTS total_minutes INT64;
