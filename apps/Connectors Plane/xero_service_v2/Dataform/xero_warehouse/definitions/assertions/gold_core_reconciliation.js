const silverSchema = "dw_1_silver_xero";
const reconciliationSchema = "dw_1_silver_xero_assertions";

// Only one-to-one tenant-scoped projections belong here. Facts that intentionally
// split one Silver relation into customer/supplier outputs keep their own grain
// assertions until their filtered source contracts are declared explicitly.
const relationships = [
  { name: "common.dim_branding_theme", silver: "dim_branding_theme", goldSchema: "xero_gold_common", gold: "dim_branding_theme" },
  { name: "common.dim_company", silver: "dim_company", goldSchema: "xero_gold_common", gold: "dim_company" },
  { name: "common.dim_contact", silver: "dim_contact", goldSchema: "xero_gold_common", gold: "dim_contact" },
  { name: "common.dim_contact_address", silver: "dim_contact_address", goldSchema: "xero_gold_common", gold: "dim_contact_address" },
  { name: "common.dim_contact_group", silver: "dim_contact_group", goldSchema: "xero_gold_common", gold: "dim_contact_group" },
  { name: "common.dim_contact_phone", silver: "dim_contact_phone", goldSchema: "xero_gold_common", gold: "dim_contact_phone" },
  { name: "common.dim_currency", silver: "dim_currency", goldSchema: "xero_gold_common", gold: "dim_currency" },
  { name: "common.dim_financial_period", silver: "dim_financial_period", goldSchema: "xero_gold_common", gold: "dim_financial_period" },
  { name: "common.dim_project", silver: "dim_project", goldSchema: "xero_gold_common", gold: "dim_project" },
  { name: "common.bridge_company_organisation_action", silver: "bridge_company_organisation_action", goldSchema: "xero_gold_common", gold: "bridge_company_organisation_action" },
  { name: "common.dim_tracking_category", silver: "dim_tracking_category", goldSchema: "xero_gold_common", gold: "dim_tracking_category" },
  { name: "common.dim_tracking_option", silver: "dim_tracking_option", goldSchema: "xero_gold_common", gold: "dim_tracking_option" },
  { name: "common.dim_user", silver: "dim_user", goldSchema: "xero_gold_common", gold: "dim_user" },
  { name: "common.bridge_contact_group_membership", silver: "bridge_contact_group_membership", goldSchema: "xero_gold_common", gold: "bridge_contact_group_membership" },
  { name: "finance.dim_account", silver: "dim_account", goldSchema: "xero_gold_finance", gold: "dim_account" },
  { name: "finance.dim_account_classification_history", silver: "dim_account_classification_history", goldSchema: "xero_gold_finance", gold: "dim_account_classification_history" },
  { name: "finance.dim_budget", silver: "dim_budget", goldSchema: "xero_gold_finance", gold: "dim_budget" },
  { name: "finance.dim_fixed_asset", silver: "dim_fixed_asset", goldSchema: "xero_gold_finance", gold: "dim_fixed_asset" },
  { name: "finance.dim_fixed_asset_settings", silver: "dim_fixed_asset_settings", goldSchema: "xero_gold_finance", gold: "dim_fixed_asset_settings" },
  { name: "finance.dim_fixed_asset_type", silver: "dim_fixed_asset_type", goldSchema: "xero_gold_finance", gold: "dim_fixed_asset_type" },
  { name: "finance.dim_invoice_reminder_configuration", silver: "dim_invoice_reminder_configuration", goldSchema: "xero_gold_finance", gold: "dim_invoice_reminder_configuration" },
  { name: "finance.dim_repeating_invoice", silver: "dim_repeating_invoice", goldSchema: "xero_gold_finance", gold: "dim_repeating_invoice" },
  { name: "finance.dim_repeating_invoice_line", silver: "dim_repeating_invoice_line", goldSchema: "xero_gold_finance", gold: "dim_repeating_invoice_line" },
  { name: "finance.dim_tax_rate", silver: "dim_tax_rate", goldSchema: "xero_gold_finance", gold: "dim_tax_rate" },
  { name: "finance.fact_bank_balance_snapshot", silver: "fact_bank_balance_snapshot", goldSchema: "xero_gold_finance", gold: "fact_bank_balance_snapshot" },
  { name: "finance.fact_bank_transaction", silver: "fact_bank_transaction", goldSchema: "xero_gold_finance", gold: "fact_bank_transaction" },
  { name: "finance.fact_bank_transaction_line", silver: "fact_bank_transaction_line", goldSchema: "xero_gold_finance", gold: "fact_bank_transaction_line" },
  { name: "finance.fact_bank_transfer", silver: "fact_bank_transfer", goldSchema: "xero_gold_finance", gold: "fact_bank_transfer" },
  { name: "finance.fact_batch_payment", silver: "fact_batch_payment", goldSchema: "xero_gold_finance", gold: "fact_batch_payment" },
  { name: "finance.fact_batch_payment_line", silver: "fact_batch_payment_line", goldSchema: "xero_gold_finance", gold: "fact_batch_payment_line" },
  { name: "finance.fact_accounts_payable_open_item", silver: "fact_accounts_payable_open_item", silverFilter: "is_current_open_balance", goldSchema: "xero_gold_finance", gold: "fact_accounts_payable_open_item" },
  { name: "finance.fact_accounts_receivable_open_item", silver: "fact_accounts_receivable_open_item", silverFilter: "is_current_open_balance", goldSchema: "xero_gold_finance", gold: "fact_accounts_receivable_open_item" },
  { name: "finance.fact_budget_balance", silver: "fact_budget_balance", goldSchema: "xero_gold_finance", gold: "fact_budget_balance" },
  { name: "finance.fact_financial_report_cell", silver: "fact_financial_report_cell", goldSchema: "xero_gold_finance", gold: "fact_financial_report_cell" },
  { name: "finance.fact_financial_report_snapshot", silver: "fact_financial_report_snapshot", goldSchema: "xero_gold_finance", gold: "fact_financial_report_snapshot" },
  { name: "finance.fact_financial_statement_line_snapshot", silver: "fact_financial_statement_line_snapshot", goldSchema: "xero_gold_finance", gold: "fact_financial_statement_line_snapshot" },
  { name: "finance.fact_fixed_asset_value_observation", silver: "fact_fixed_asset_value_observation", goldSchema: "xero_gold_finance", gold: "fact_fixed_asset_value_observation" },
  { name: "finance.fact_general_ledger", silver: "fact_general_ledger", goldSchema: "xero_gold_finance", gold: "fact_general_ledger" },
  { name: "finance.fact_general_ledger_balance", silver: "fact_general_ledger_balance", goldSchema: "xero_gold_finance", gold: "fact_general_ledger_balance" },
  { name: "finance.fact_linked_transaction", silver: "fact_linked_transaction", goldSchema: "xero_gold_finance", gold: "fact_linked_transaction" },
  { name: "finance.fact_vat_activity", silver: "fact_tax_activity", goldSchema: "xero_gold_finance", gold: "fact_vat_activity" },
  { name: "sales.dim_item", silver: "dim_item", goldSchema: "xero_gold_sales", gold: "dim_item" },
  { name: "sales.fact_quote", silver: "fact_quote", goldSchema: "xero_gold_sales", gold: "fact_quote" },
  { name: "sales.fact_quote_line", silver: "fact_quote_line", goldSchema: "xero_gold_sales", gold: "fact_quote_line" },
  { name: "procurement.fact_purchase_order", silver: "fact_purchase_order", goldSchema: "xero_gold_procurement", gold: "fact_purchase_order" },
  { name: "procurement.fact_purchase_order_line", silver: "fact_purchase_order_line", goldSchema: "xero_gold_procurement", gold: "fact_purchase_order_line" },
  { name: "operations.dim_project_task", silver: "dim_project_task", goldSchema: "xero_gold_operations", gold: "dim_project_task" },
  { name: "operations.dim_project_user", silver: "dim_project_user", goldSchema: "xero_gold_operations", gold: "dim_project_user" },
  { name: "operations.fact_project_time_entry", silver: "fact_project_time_entry", goldSchema: "xero_gold_operations", gold: "fact_project_time_entry" },
  { name: "operations.fact_project_operational_snapshot", silver: "dim_project", goldSchema: "xero_gold_operations", gold: "fact_project_operational_snapshot" },
];

const relationshipGroupSize = 16;
const relationshipGroups = Array.from(
  { length: Math.ceil(relationships.length / relationshipGroupSize) },
  (_, index) => relationships.slice(index * relationshipGroupSize, (index + 1) * relationshipGroupSize),
);

function relationQuery(ctx, relation) {
  const silverFilter = relation.silverFilter ? `WHERE ${relation.silverFilter}` : "";

  return `
    WITH known_tenants AS (
      SELECT DISTINCT tenant_id
      FROM ${ctx.ref(silverSchema, "dim_company")}
    ), silver_counts AS (
      SELECT tenant_id, COUNT(*) AS silver_row_count
      FROM ${ctx.ref(silverSchema, relation.silver)}
      ${silverFilter}
      GROUP BY tenant_id
    ), gold_counts AS (
      SELECT tenant_id, COUNT(*) AS gold_row_count
      FROM ${ctx.ref(relation.goldSchema, relation.gold)}
      GROUP BY tenant_id
    ), tenant_universe AS (
      SELECT tenant_id FROM known_tenants
      UNION DISTINCT
      SELECT tenant_id FROM silver_counts
      UNION DISTINCT
      SELECT tenant_id FROM gold_counts
    )
    SELECT
      '${relation.name}' AS relationship_name,
      '${silverSchema}.${relation.silver}' AS silver_relation,
      '${relation.goldSchema}.${relation.gold}' AS gold_relation,
      tenant_id,
      COALESCE(silver_counts.silver_row_count, 0) AS silver_row_count,
      COALESCE(gold_counts.gold_row_count, 0) AS gold_row_count,
      COALESCE(silver_counts.silver_row_count, 0) = COALESCE(gold_counts.gold_row_count, 0)
        AS is_row_count_reconciled
    FROM tenant_universe
    LEFT JOIN silver_counts USING (tenant_id)
    LEFT JOIN gold_counts USING (tenant_id)
  `;
}

for (const [index, group] of relationshipGroups.entries()) {
  publish(`xero_gold_core_reconciliation_group_${index + 1}`, {
    type: "view",
    schema: reconciliationSchema,
    description: "Bounded dependency group for the tenant-scoped Xero Silver-to-Gold reconciliation manifest.",
  }).query(ctx => group.map(relation => `
    SELECT * FROM (${relationQuery(ctx, relation)})
  `).join("\nUNION ALL\n"));
}

publish("xero_gold_core_reconciliation", {
  type: "table",
  schema: silverSchema,
  description: "Tenant-scoped Silver-to-Gold row-count reconciliation manifest for Xero-native one-to-one business relations. Lifecycle states stay in the row counts; no source record is suppressed to make this audit pass.",
  assertions: {
    nonNull: ["relationship_name", "silver_relation", "gold_relation", "tenant_id"],
    uniqueKey: ["relationship_name", "tenant_id"],
  },
}).query(ctx => relationshipGroups.map((_, index) => `
  SELECT relationship_name, silver_relation, gold_relation, tenant_id,
         silver_row_count, gold_row_count, is_row_count_reconciled
  FROM ${ctx.ref(reconciliationSchema, `xero_gold_core_reconciliation_group_${index + 1}`)}
`).join("\nUNION ALL\n"));

assert("xero_gold_core_reconciliation_safety")
  .schema(reconciliationSchema)
  .query(ctx => `
    SELECT relationship_name, tenant_id, silver_relation, gold_relation,
           silver_row_count, gold_row_count
    FROM ${ctx.ref(silverSchema, "xero_gold_core_reconciliation")}
    WHERE silver_row_count != gold_row_count
  `);
