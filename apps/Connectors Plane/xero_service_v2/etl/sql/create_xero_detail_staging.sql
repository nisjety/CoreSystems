-- Run in prj-dw-dev before backfilling the new Xero detail parsers.
-- Explicit types avoid BigQuery autodetecting numeric-looking IDs/codes as numbers.

CREATE TABLE IF NOT EXISTS staging_xero.budgets (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  first_seen_at TIMESTAMP,
  budget_id STRING,
  description STRING,
  budget_type STRING,
  status STRING,
  updated_at TIMESTAMP,
  tracking_json STRING,
  budget_line_count INT64
);

CREATE TABLE IF NOT EXISTS staging_xero.budget_balance_snapshots (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  budget_id STRING,
  account_key STRING,
  account_id STRING,
  account_code STRING,
  period STRING,
  period_date_local DATE,
  amount STRING,
  unit_amount STRING,
  notes STRING
);

CREATE TABLE IF NOT EXISTS staging_xero.contact_groups (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  first_seen_at TIMESTAMP,
  contact_group_id STRING,
  name STRING,
  status STRING,
  member_count INT64,
  contacts_json STRING
);

CREATE TABLE IF NOT EXISTS staging_xero.contact_group_member_snapshots (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  contact_group_id STRING,
  contact_id STRING,
  contact_name STRING
);

CREATE TABLE IF NOT EXISTS staging_xero.batch_payments (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  first_seen_at TIMESTAMP,
  batch_payment_id STRING,
  bank_account_id STRING,
  account_currency_code STRING,
  batch_payment_type STRING,
  status STRING,
  payment_date TIMESTAMP,
  payment_date_local DATE,
  updated_at TIMESTAMP,
  reference STRING,
  particulars STRING,
  code STRING,
  details STRING,
  narrative STRING,
  reconciled_statement_line_id STRING,
  total_amount FLOAT64,
  payment_count INT64,
  is_reconciled BOOL
);

CREATE TABLE IF NOT EXISTS staging_xero.batch_payment_line_snapshots (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  batch_payment_id STRING,
  line_index INT64,
  payment_id STRING,
  invoice_id STRING,
  invoice_currency_code STRING,
  account_currency_code STRING,
  amount FLOAT64,
  bank_amount FLOAT64,
  details STRING,
  invoice_has_errors BOOL,
  invoice_is_discounted BOOL
);

CREATE TABLE IF NOT EXISTS staging_xero.linked_transactions (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  first_seen_at TIMESTAMP,
  linked_transaction_id STRING,
  source_transaction_id STRING,
  source_line_item_id STRING,
  source_transaction_type STRING,
  contact_id STRING,
  target_transaction_id STRING,
  target_line_item_id STRING,
  linked_transaction_type STRING,
  status STRING,
  updated_at TIMESTAMP
);

CREATE TABLE IF NOT EXISTS staging_xero.repeating_invoices (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  first_seen_at TIMESTAMP,
  repeating_invoice_id STRING,
  invoice_type STRING,
  status STRING,
  reference STRING,
  line_amount_types STRING,
  currency_code STRING,
  branding_theme_id STRING,
  contact_id STRING,
  contact_name STRING,
  sub_total FLOAT64,
  total_tax FLOAT64,
  total FLOAT64,
  schedule_unit STRING,
  schedule_period INT64,
  schedule_due_date_type STRING,
  schedule_due_date_offset INT64,
  schedule_start_date TIMESTAMP,
  schedule_end_date TIMESTAMP,
  next_scheduled_date TIMESTAMP,
  next_scheduled_date_local DATE,
  updated_at TIMESTAMP,
  has_attachments BOOL,
  approved_for_sending BOOL,
  include_pdf BOOL,
  send_copy BOOL,
  mark_as_sent BOOL,
  line_item_count INT64
);

CREATE TABLE IF NOT EXISTS staging_xero.repeating_invoice_line_snapshots (
  tenant_id STRING,
  record_id STRING,
  synced_at TIMESTAMP,
  repeating_invoice_id STRING,
  line_position INT64,
  line_item_id STRING,
  account_id STRING,
  account_code STRING,
  item_code STRING,
  description STRING,
  quantity FLOAT64,
  unit_amount FLOAT64,
  line_amount FLOAT64,
  tax_amount FLOAT64,
  tax_type STRING,
  discount_rate FLOAT64,
  discount_amount FLOAT64,
  tracking_category_1_id STRING,
  tracking_category_1_name STRING,
  tracking_option_1_id STRING,
  tracking_option_1_name STRING,
  tracking_category_2_id STRING,
  tracking_category_2_name STRING,
  tracking_option_2_id STRING,
  tracking_option_2_name STRING
);
