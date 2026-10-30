const project = dataform.projectConfig.defaultDatabase;
const stagingSchema = "dw_staging_xero_main";
const bronzeSchema = "dw_bronze_xero";
const endpoints = [
  "accounts",
  "bank_transactions",
  "bank_transfers",
  "batch_payments",
  "branding_themes",
  "budgets",
  "contact_groups",
  "contacts",
  "credit_notes",
  "currencies",
  "expense_claims",
  "invoices",
  "items",
  "journals",
  "linked_transactions",
  "manual_journals",
  "organisations",
  "overpayments",
  "payments",
  "prepayments",
  "purchase_orders",
  "quotes",
  "receipts",
  "repeating_invoices",
  "tax_rates",
  "tracking_categories",
  "users",
  "report_profit_and_loss",
  "report_balance_sheet",
  "report_trial_balance",
  "report_aged_receivables_by_contact",
  "report_aged_payables_by_contact",
  "report_bank_summary",
  "report_budget_summary",
  "report_executive_summary",
];

for (const endpoint of endpoints) {
  const source = { database: project, schema: stagingSchema, name: endpoint };
  declare(source);
  publish(`xero_${endpoint}`, {
    type: "incremental",
    schema: bronzeSchema,
    protected: true,
    description: `Immutable raw observations from the Xero ${endpoint} endpoint.`,
    bigquery: {
      partitionBy: "DATE(observed_at)",
      clusterBy: ["tenant_id", "record_id"],
    },
  }).query(ctx => `
    SELECT
      source.generation,
      source.tenant_id,
      source.endpoint,
      source.record_id,
      source.version_id,
      source.observed_at,
      source.source_updated_at,
      source.run_id,
      source.receipt_id,
      source.source_status,
      source.source_deleted,
      source.payload_hash,
      source.payload_json
    FROM ${ctx.ref(source)} AS source
    WHERE source.endpoint = '${endpoint}'
    ${ctx.when(ctx.incremental(), `
      AND NOT EXISTS (
        SELECT 1 FROM ${ctx.self()} AS existing
        WHERE existing.generation = source.generation
          AND existing.tenant_id = source.tenant_id
          AND existing.record_id = source.record_id
          AND existing.version_id = source.version_id
      )
    `)}
  `);

  assert(`xero_bronze_${endpoint}_grain`)
    .database(project)
    .schema("dw_bronze_xero_assertions")
    .query(ctx => `
      SELECT generation, tenant_id, record_id, version_id
      FROM ${ctx.ref({ database: project, schema: bronzeSchema, name: `xero_${endpoint}` })}
      QUALIFY generation IS NULL OR tenant_id IS NULL OR record_id IS NULL
        OR version_id IS NULL
        OR COUNT(*) OVER (
          PARTITION BY generation, tenant_id, record_id, version_id
        ) > 1
    `);
}
