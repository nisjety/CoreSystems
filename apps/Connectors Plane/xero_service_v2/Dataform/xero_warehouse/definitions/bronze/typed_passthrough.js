const project = dataform.projectConfig.defaultDatabase;
const currentSnapshots = [
  "bank_transaction_lines", "bank_transactions", "bank_transfers",
  "batch_payment_line_snapshots", "batch_payment_lines", "batch_payments",
  "branding_themes", "budgets", "contact_addresses", "contact_group_members",
  "contact_group_memberships", "contact_groups", "contact_phones",
  "credit_note_allocations", "currencies", "invoice_payments", "items",
  "linked_transactions", "manual_journal_lines", "manual_journals",
  "overpayment_lines", "overpayments", "purchase_order_lines", "purchase_orders",
  "quote_lines", "quotes", "repeating_invoice_line_snapshots",
  "repeating_invoice_lines", "repeating_invoices", "report_rows", "report_snapshots",
  "tax_rates", "tracking_categories", "tracking_options", "users",
];

for (const entity of currentSnapshots) {
  const source = { database: project, schema: "dw_staging_xero", name: entity };
  declare(source);
  publish(`xero_${entity}`, {
    type: "view",
    schema: "dw_bronze_xero",
    description: `Current typed snapshot of dw_staging_xero.${entity}; not an original Xero API payload or version history.`,
  }).query(ctx => `SELECT * FROM ${ctx.ref(source)}`);
}
