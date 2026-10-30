const contracts = [
  { operationId: "getAccount", bronze: ["xero_account_detail_observations"], silver: ["audit_account_direct_detail_reconciliation", "dim_account"], gold: ["xero_gold_finance.dim_account"] },
  { operationId: "getAccounts", bronze: ["xero_accounts"], silver: ["dim_account"], gold: ["xero_gold_finance.dim_account"] },
  { operationId: "getBankTransactions", bronze: ["xero_bank_transactions", "xero_bank_transaction_lines"], silver: ["fact_bank_transaction", "fact_bank_transaction_line"], gold: ["xero_gold_finance.fact_bank_transaction", "xero_gold_finance.fact_bank_transaction_line"] },
  { operationId: "getBankTransfers", bronze: ["xero_bank_transfers"], silver: ["fact_bank_transfer"], gold: ["xero_gold_finance.fact_bank_transfer"] },
  { operationId: "getBatchPayments", bronze: ["xero_batch_payments", "xero_batch_payment_lines"], silver: ["fact_batch_payment", "fact_batch_payment_line"], gold: ["xero_gold_finance.fact_batch_payment", "xero_gold_finance.fact_batch_payment_line"] },
  { operationId: "getBrandingThemes", bronze: ["xero_branding_themes"], silver: ["dim_branding_theme"], gold: ["xero_gold_common.dim_branding_theme"] },
  { operationId: "getBudget", bronze: ["xero_budget_detail_observations", "xero_budget_balance_observations"], silver: ["fact_budget_balance"], gold: ["xero_gold_finance.fact_budget_balance"] },
  { operationId: "getBudgets", bronze: ["xero_budgets"], silver: ["dim_budget"], gold: ["xero_gold_finance.dim_budget"] },
  { operationId: "getContactGroup", bronze: ["xero_contact_group_detail_observations"], silver: ["audit_contact_group_direct_detail_reconciliation"], gold: ["xero_gold_common.dim_contact_group"] },
  { operationId: "getContactGroups", bronze: ["xero_contact_groups", "xero_contact_group_members"], silver: ["dim_contact_group", "bridge_contact_group_membership"], gold: ["xero_gold_common.dim_contact_group", "xero_gold_common.bridge_contact_group_membership"] },
  { operationId: "getContacts", bronze: ["xero_contacts", "xero_contact_addresses", "xero_contact_phones"], silver: ["dim_contact", "dim_contact_address", "dim_contact_phone"], gold: ["xero_gold_common.dim_contact", "xero_gold_common.dim_contact_address", "xero_gold_common.dim_contact_phone", "xero_gold_finance.dim_customer", "xero_gold_finance.dim_supplier"] },
  { operationId: "getCreditNotes", bronze: ["xero_credit_notes", "xero_credit_note_lines"], silver: ["fact_credit_note", "fact_credit_note_line", "fact_credit_note_allocation"], gold: ["xero_gold_finance.fact_customer_credit_note", "xero_gold_finance.fact_supplier_credit_note", "xero_gold_finance.fact_customer_credit_note_line", "xero_gold_finance.fact_supplier_credit_note_line", "xero_gold_finance.fact_customer_credit_note_allocation", "xero_gold_finance.fact_supplier_credit_note_allocation"] },
  { operationId: "getCurrencies", bronze: ["xero_currencies"], silver: ["dim_currency"], gold: ["xero_gold_common.dim_currency"] },
  { operationId: "getExpenseClaims", bronze: ["xero_expense_claims"], silver: ["fact_expense_claim", "fact_expense_claim_receipt", "fact_expense_claim_receipt_line"], gold: ["xero_gold_finance.fact_expense_claim", "xero_gold_finance.fact_expense_claim_receipt", "xero_gold_finance.fact_expense_claim_receipt_line"] },
  { operationId: "getInvoices", bronze: ["xero_invoices", "xero_invoice_lines"], silver: ["fact_invoice", "fact_invoice_line"], gold: ["xero_gold_finance.fact_customer_invoice", "xero_gold_finance.fact_supplier_invoice", "xero_gold_finance.fact_customer_invoice_line", "xero_gold_finance.fact_supplier_invoice_line"] },
  { operationId: "getInvoice", bronze: ["xero_invoice_attachment_lists", "xero_invoice_attachment_observations"], silver: ["dim_invoice_attachment"], gold: ["xero_gold_finance.dim_invoice_attachment"] },
  { operationId: "getItems", bronze: ["xero_items"], silver: ["dim_item"], gold: ["xero_gold_sales.dim_item"] },
  { operationId: "getJournals", bronze: ["xero_journals", "xero_journal_lines"], silver: ["fact_general_ledger"], gold: ["xero_gold_finance.fact_general_ledger"] },
  { operationId: "getLinkedTransactions", bronze: ["xero_linked_transactions"], silver: ["fact_linked_transaction"], gold: ["xero_gold_finance.fact_linked_transaction"] },
  { operationId: "getManualJournals", bronze: ["xero_manual_journals", "xero_manual_journal_lines"], silver: ["fact_general_ledger"], gold: ["xero_gold_finance.fact_general_ledger"] },
  { operationId: "getOrganisations", bronze: ["xero_organisations"], silver: ["dim_company"], gold: ["xero_gold_common.dim_company"] },
  { operationId: "getOverpayments", bronze: ["xero_overpayments", "xero_overpayment_lines"], silver: ["fact_overpayment", "fact_overpayment_line"], gold: ["xero_gold_finance.fact_customer_overpayment", "xero_gold_finance.fact_supplier_overpayment", "xero_gold_finance.fact_customer_overpayment_line", "xero_gold_finance.fact_supplier_overpayment_line"] },
  { operationId: "getPayments", bronze: ["xero_payments", "xero_payment_invoice_currency"], silver: ["fact_payment", "fact_customer_payment", "fact_non_invoice_payment"], gold: ["xero_gold_finance.fact_customer_payment", "xero_gold_finance.fact_supplier_payment", "xero_gold_finance.fact_non_invoice_payment"] },
  { operationId: "getPrepayments", bronze: ["xero_prepayments"], silver: ["fact_prepayment", "fact_prepayment_line", "fact_prepayment_allocation"], gold: ["xero_gold_finance.fact_prepayment", "xero_gold_finance.fact_prepayment_line", "xero_gold_finance.fact_prepayment_allocation"] },
  { operationId: "getPurchaseOrders", bronze: ["xero_purchase_orders", "xero_purchase_order_lines"], silver: ["fact_purchase_order", "fact_purchase_order_line"], gold: ["xero_gold_procurement.fact_purchase_order", "xero_gold_procurement.fact_purchase_order_line"] },
  { operationId: "getQuotes", bronze: ["xero_quotes", "xero_quote_lines"], silver: ["fact_quote", "fact_quote_line"], gold: ["xero_gold_sales.fact_quote", "xero_gold_sales.fact_quote_line"] },
  { operationId: "getReceipts", bronze: ["xero_receipts"], silver: ["fact_receipt", "fact_receipt_line"], gold: ["xero_gold_finance.fact_receipt", "xero_gold_finance.fact_receipt_line"] },
  { operationId: "getRepeatingInvoices", bronze: ["xero_repeating_invoices", "xero_repeating_invoice_lines"], silver: ["dim_repeating_invoice", "dim_repeating_invoice_line"], gold: ["xero_gold_finance.dim_repeating_invoice", "xero_gold_finance.dim_repeating_invoice_line"] },
  { operationId: "getTaxRates", bronze: ["xero_tax_rates"], silver: ["dim_tax_rate"], gold: ["xero_gold_finance.dim_tax_rate"] },
  { operationId: "getTrackingCategories", bronze: ["xero_tracking_categories", "xero_tracking_options"], silver: ["dim_tracking_category", "dim_tracking_option"], gold: ["xero_gold_common.dim_tracking_category", "xero_gold_common.dim_tracking_option"] },
  { operationId: "getUsers", bronze: ["xero_users"], silver: ["dim_user"], gold: ["xero_gold_common.dim_user"] },
  { operationId: "getAssets", bronze: ["xero_fixed_asset_observations"], silver: ["dim_fixed_asset", "fact_fixed_asset_value_observation"], gold: ["xero_gold_finance.dim_fixed_asset", "xero_gold_finance.fact_fixed_asset_value_observation"] },
  { operationId: "getAssetById", bronze: ["xero_fixed_asset_observations"], silver: ["dim_fixed_asset", "fact_fixed_asset_value_observation"], gold: ["xero_gold_finance.dim_fixed_asset", "xero_gold_finance.fact_fixed_asset_value_observation"] },
  { operationId: "getAssetTypes", bronze: ["xero_fixed_asset_type_observations"], silver: ["dim_fixed_asset_type"], gold: ["xero_gold_finance.dim_fixed_asset_type"] },
  { operationId: "getAssetSettings", bronze: ["xero_fixed_asset_settings_observations"], silver: ["dim_fixed_asset_settings"], gold: ["xero_gold_finance.dim_fixed_asset_settings"] },
  { operationId: "getInvoiceReminders", bronze: ["xero_invoice_reminder_observations"], silver: ["dim_invoice_reminder_configuration"], gold: ["xero_gold_finance.dim_invoice_reminder_configuration"] },
  { operationId: "getOrganisationActions", bronze: ["xero_organisation_action_observations"], silver: ["bridge_company_organisation_action"], gold: ["xero_gold_common.bridge_company_organisation_action"] },
  { operationId: "getProjects", bronze: ["xero_project_observations"], silver: ["dim_project"], gold: ["xero_gold_common.dim_project", "xero_gold_operations.fact_project_operational_snapshot"] },
  { operationId: "getProject", bronze: ["xero_project_detail_observations"], silver: ["audit_project_direct_detail_reconciliation", "dim_project"], gold: ["xero_gold_common.dim_project", "xero_gold_operations.fact_project_operational_snapshot"] },
  { operationId: "getTask", bronze: ["xero_project_task_detail_observations"], silver: ["audit_project_task_direct_detail_reconciliation"], gold: ["xero_gold_operations.dim_project_task"] },
  { operationId: "getTasks", bronze: ["xero_project_task_observations"], silver: ["dim_project_task"], gold: ["xero_gold_operations.dim_project_task"] },
  { operationId: "getProjectUsers", bronze: ["xero_project_user_observations"], silver: ["dim_project_user"], gold: ["xero_gold_operations.dim_project_user"] },
  { operationId: "getTimeEntry", bronze: ["xero_project_time_entry_detail_observations"], silver: ["audit_project_time_entry_direct_detail_reconciliation"], gold: ["xero_gold_operations.fact_project_time_entry"] },
  { operationId: "getTimeEntries", bronze: ["xero_project_time_entry_observations"], silver: ["fact_project_time_entry"], gold: ["xero_gold_operations.fact_project_time_entry"] },
  { operationId: "getReportBalanceSheet", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell", "fact_financial_statement_line_snapshot"], gold: ["xero_gold_finance.fact_financial_report_snapshot", "xero_gold_finance.fact_financial_report_cell", "xero_gold_finance.fact_financial_statement_line_snapshot"] },
  { operationId: "getReportAgedReceivablesByContact", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell", "fact_contact_aging_report_run", "fact_contact_aging_report_line", "fact_contact_aging_report_amount"], gold: ["xero_gold_finance.fact_accounts_receivable_aging_report_amount", "xero_datamart.accounts_receivable_aging_by_contact"] },
  { operationId: "getReportAgedPayablesByContact", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell", "fact_contact_aging_report_run", "fact_contact_aging_report_line", "fact_contact_aging_report_amount"], gold: ["xero_gold_finance.fact_accounts_payable_aging_report_amount", "xero_datamart.accounts_payable_aging_by_contact"] },
  { operationId: "getReportBankSummary", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell", "fact_bank_balance_snapshot"], gold: ["xero_gold_finance.fact_financial_report_snapshot", "xero_gold_finance.fact_financial_report_cell", "xero_gold_finance.fact_bank_balance_snapshot"] },
  { operationId: "getReportBudgetSummary", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell"], gold: ["xero_gold_finance.fact_financial_report_snapshot", "xero_gold_finance.fact_financial_report_cell"] },
  { operationId: "getReportExecutiveSummary", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell"], gold: ["xero_gold_finance.fact_financial_report_snapshot", "xero_gold_finance.fact_financial_report_cell"] },
  { operationId: "getReportProfitAndLoss", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell", "fact_financial_statement_line_snapshot"], gold: ["xero_gold_finance.fact_financial_report_snapshot", "xero_gold_finance.fact_financial_report_cell", "xero_gold_finance.fact_financial_statement_line_snapshot"] },
  { operationId: "getReportTrialBalance", bronze: ["xero_report_snapshots", "xero_report_rows"], silver: ["fact_financial_report_snapshot", "fact_financial_report_cell", "fact_general_ledger_balance"], gold: ["xero_gold_finance.fact_financial_report_snapshot", "xero_gold_finance.fact_financial_report_cell", "xero_gold_finance.fact_general_ledger_balance"] },
];

const quote = value => `'${value.replace(/'/g, "''")}'`;
const stringArray = values => values.length
  ? `[${values.map(quote).join(", ")}]`
  : "CAST([] AS ARRAY<STRING>)";
const contractRows = contracts.map(contract => `STRUCT(
  ${quote(contract.operationId)} AS operation_id,
  ${stringArray(contract.bronze)} AS bronze_models,
  ${stringArray(contract.silver)} AS silver_models,
  ${stringArray(contract.gold)} AS gold_models
)`).join(",\n");
const uniqueContractModels = Array.from(new Map(
  contracts.flatMap(contract => [
    ...contract.bronze.map(name => ({ schema: "dw_bronze_xero", name })),
    ...contract.silver.map(name => ({ schema: "dw_1_silver_xero", name })),
    ...contract.gold.map(target => {
      const [schema, name] = target.split(".");
      return { schema, name };
    }),
  ]).map(model => [`${model.schema}.${model.name}`, model]),
).values());
const dependencyGroupSize = 45;
const contractDependencyGroups = Array.from(
  { length: Math.ceil(uniqueContractModels.length / dependencyGroupSize) },
  (_, index) => uniqueContractModels.slice(
    index * dependencyGroupSize,
    (index + 1) * dependencyGroupSize,
  ),
);

for (const [index, models] of contractDependencyGroups.entries()) {
  publish(`xero_warehouse_contract_dependency_group_${index + 1}`, {
    type: "view",
    schema: "dw_bronze_xero",
    description: "Bounded compiled dependency group for declared Xero warehouse contract models. Every referenced relation is read with a zero-row predicate so stale contract names fail compilation without scanning source data.",
  }).query(ctx => {
    const dependencyQueries = models.map(model => `
      SELECT '${model.schema}.${model.name}' AS model_name
      FROM ${ctx.ref(model.schema, model.name)}
      WHERE FALSE
    `).join("\nUNION ALL\n");
    return `
      SELECT COUNT(*) + ${models.length} AS declared_contract_model_count
      FROM (${dependencyQueries})
    `;
  });
}

publish("xero_endpoint_warehouse_coverage", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "One row per tenant and documented Xero operation showing raw extraction status separately from the declared Bronze, Silver and Gold warehouse contract. Every declared model is a compiled zero-row dependency, so a stale contract name cannot be published as coverage.",
  assertions: { nonNull: ["tenant_id", "operation_id", "warehouse_contract_state"], uniqueKey: ["tenant_id", "operation_id"] },
}).query(ctx => {
  const dependencyGroupCounts = contractDependencyGroups.map((_, index) => `
    SELECT declared_contract_model_count
    FROM ${ctx.ref("dw_bronze_xero", `xero_warehouse_contract_dependency_group_${index + 1}`)}
  `).join("\nUNION ALL\n");
  return `
  WITH contracts AS (
    SELECT * FROM UNNEST([
      ${contractRows}
    ])
  ), declared_contract_model_count AS (
    SELECT SUM(declared_contract_model_count) AS declared_contract_model_count
    FROM (${dependencyGroupCounts})
  )
  SELECT
    coverage.*,
    dependency.declared_contract_model_count,
    COALESCE(contract.bronze_models, CAST([] AS ARRAY<STRING>)) AS bronze_models,
    COALESCE(contract.silver_models, CAST([] AS ARRAY<STRING>)) AS silver_models,
    COALESCE(contract.gold_models, CAST([] AS ARRAY<STRING>)) AS gold_models,
    ARRAY_LENGTH(COALESCE(contract.bronze_models, CAST([] AS ARRAY<STRING>))) > 0 AS has_bronze_contract,
    ARRAY_LENGTH(COALESCE(contract.silver_models, CAST([] AS ARRAY<STRING>))) > 0 AS has_silver_contract,
    ARRAY_LENGTH(COALESCE(contract.gold_models, CAST([] AS ARRAY<STRING>))) > 0 AS has_gold_contract,
    CASE
      WHEN coverage.is_out_of_scope THEN 'out_of_scope'
      WHEN contract.operation_id IS NULL THEN 'not_modelled'
      WHEN ARRAY_LENGTH(contract.gold_models) > 0 THEN 'gold'
      WHEN ARRAY_LENGTH(contract.silver_models) > 0 THEN 'silver'
      WHEN ARRAY_LENGTH(contract.bronze_models) > 0 THEN 'bronze'
      ELSE 'raw_only'
    END AS warehouse_contract_state,
    coverage.is_authoritative AND ARRAY_LENGTH(COALESCE(contract.gold_models, CAST([] AS ARRAY<STRING>))) > 0
      AS is_authoritative_with_declared_gold_contract
  FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_coverage")} AS coverage
  CROSS JOIN declared_contract_model_count AS dependency
  LEFT JOIN contracts AS contract USING (operation_id)
`;
});

publish("xero_endpoint_warehouse_coverage_summary", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Tenant-level source-to-declared-warehouse-contract summary. Contract counts describe expected model coverage; workflow execution and reconciliation assertions remain the evidence of materialized data.",
  assertions: { nonNull: ["tenant_id"] },
}).query(ctx => `
  SELECT
    tenant_id,
    COUNT(*) AS documented_operation_count,
    COUNTIF(is_out_of_scope) AS out_of_scope_operation_count,
    COUNTIF(is_authoritative) AS authoritative_raw_operation_count,
    COUNTIF(has_bronze_contract) AS bronze_contract_operation_count,
    COUNTIF(has_silver_contract) AS silver_contract_operation_count,
    COUNTIF(has_gold_contract) AS gold_contract_operation_count,
    COUNTIF(is_authoritative_with_declared_gold_contract)
      AS authoritative_raw_with_declared_gold_contract_operation_count,
    COUNTIF(warehouse_contract_state = 'raw_only') AS raw_only_operation_count,
    COUNTIF(warehouse_contract_state = 'not_modelled') AS not_modelled_operation_count,
    COUNTIF(warehouse_contract_state = 'gold') AS declared_gold_operation_count
  FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_warehouse_coverage")}
  GROUP BY tenant_id
`);

publish("xero_current_accounting_warehouse_release_readiness", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Tenant-level release evidence for the current Xero Accounting warehouse. Standard Accounting and reporting remain separate coherent source cohorts; every selected endpoint must also have a complete immutable raw delivery reconciliation and a declared Gold contract before the tenant is released for Accounting consumption. It exposes source freshness as observed evidence and evaluates contact-scoped AR/AP aging as a separate optional portfolio release, so neither is silently inferred from the base Accounting result.",
  assertions: { nonNull: ["tenant_id", "release_state", "is_ready_for_release"], uniqueKey: ["tenant_id"] },
}).query(ctx => `
  WITH current_coverage AS (
    SELECT *
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_warehouse_coverage")}
    WHERE is_current_accounting_warehouse_required
  ), reconciliation AS (
    SELECT coverage.tenant_id,
           COUNT(*) AS required_operation_count,
           COUNTIF(coverage.is_authoritative) AS authoritative_operation_count,
           COUNTIF(coverage.warehouse_contract_state = 'gold') AS gold_contract_operation_count,
           COUNTIF(raw.is_complete) AS raw_reconciled_operation_count,
           MIN(IF(coverage.is_authoritative, coverage.last_observed, NULL))
             AS oldest_required_source_observed_at,
           MAX(IF(coverage.is_authoritative, coverage.last_observed, NULL))
             AS newest_required_source_observed_at,
           ARRAY_AGG(
             IF(
               raw.is_complete IS NOT TRUE,
               STRUCT(
                 coverage.operation_id AS operation_id,
                 coverage.run_id AS run_id,
                 COALESCE(raw.reconciliation_state, 'missing_reconciliation') AS reconciliation_state
               ),
               NULL
             )
             IGNORE NULLS
           ) AS unreconciled_operations
    FROM current_coverage AS coverage
    LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_run_reconciliation")} AS raw
      ON coverage.tenant_id = raw.tenant_id
     AND coverage.run_id = raw.run_id
     AND coverage.source_endpoint = raw.endpoint
    GROUP BY coverage.tenant_id
  ), aged_contact_reporting_reconciliation AS (
    SELECT coverage.tenant_id,
           COUNT(*) AS required_operation_count,
           COUNTIF(coverage.is_authoritative) AS authoritative_operation_count,
           COUNTIF(coverage.warehouse_contract_state = 'gold') AS gold_contract_operation_count,
           COUNTIF(raw.is_complete) AS raw_reconciled_operation_count,
           ARRAY_AGG(
             IF(
               raw.is_complete IS NOT TRUE,
               STRUCT(
                 coverage.operation_id AS operation_id,
                 coverage.run_id AS run_id,
                 COALESCE(raw.reconciliation_state, 'missing_reconciliation') AS reconciliation_state
               ),
               NULL
             )
             IGNORE NULLS
           ) AS unreconciled_operations
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_warehouse_coverage")} AS coverage
    LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_run_reconciliation")} AS raw
      ON coverage.tenant_id = raw.tenant_id
     AND coverage.run_id = raw.run_id
     AND coverage.source_endpoint = raw.endpoint
    WHERE coverage.is_current_aged_contact_reporting_required
    GROUP BY coverage.tenant_id
  )
  SELECT
    summary.tenant_id,
    summary.current_accounting_operation_count,
    summary.current_accounting_authoritative_operation_count,
    summary.standard_accounting_operation_count,
    summary.standard_accounting_authoritative_operation_count,
    summary.standard_accounting_distinct_run_count,
    summary.current_reporting_operation_count,
    summary.current_reporting_authoritative_operation_count,
    summary.current_reporting_distinct_run_count,
    summary.is_ready_for_standard_accounting_warehouse,
    summary.is_ready_for_current_reporting_warehouse,
    reconciliation.required_operation_count,
    reconciliation.authoritative_operation_count,
    reconciliation.gold_contract_operation_count,
    reconciliation.raw_reconciled_operation_count,
    reconciliation.unreconciled_operations,
    reconciliation.oldest_required_source_observed_at,
    reconciliation.newest_required_source_observed_at,
    CURRENT_TIMESTAMP() AS release_evaluated_at,
    CASE
      WHEN reconciliation.oldest_required_source_observed_at IS NULL
        THEN 'missing_authoritative_source_observation'
      WHEN TIMESTAMP_DIFF(
        CURRENT_TIMESTAMP(), reconciliation.oldest_required_source_observed_at, HOUR
      ) <= 24 THEN 'fresh_within_24_hours'
      WHEN TIMESTAMP_DIFF(
        CURRENT_TIMESTAMP(), reconciliation.oldest_required_source_observed_at, HOUR
      ) <= 48 THEN 'aging_within_48_hours'
      ELSE 'stale_over_48_hours'
    END AS source_freshness_state,
    reconciliation.oldest_required_source_observed_at IS NOT NULL
      AND TIMESTAMP_DIFF(
        CURRENT_TIMESTAMP(), reconciliation.oldest_required_source_observed_at, HOUR
      ) <= 48 AS is_fresh_within_default_48_hours,
    CASE
      WHEN NOT summary.is_ready_for_standard_accounting_warehouse THEN 'standard_accounting_cohort_not_ready'
      WHEN NOT summary.is_ready_for_current_reporting_warehouse THEN 'reporting_cohort_not_ready'
      WHEN reconciliation.required_operation_count != summary.current_accounting_operation_count
        OR reconciliation.authoritative_operation_count != reconciliation.required_operation_count
        THEN 'current_endpoint_coverage_not_authoritative'
      WHEN reconciliation.gold_contract_operation_count != reconciliation.required_operation_count
        THEN 'gold_contract_incomplete'
      WHEN reconciliation.raw_reconciled_operation_count != reconciliation.required_operation_count
        THEN 'raw_delivery_not_reconciled'
      ELSE 'ready'
    END AS release_state,
    summary.is_ready_for_standard_accounting_warehouse
      AND summary.is_ready_for_current_reporting_warehouse
      AND reconciliation.required_operation_count = summary.current_accounting_operation_count
      AND reconciliation.authoritative_operation_count = reconciliation.required_operation_count
      AND reconciliation.gold_contract_operation_count = reconciliation.required_operation_count
      AND reconciliation.raw_reconciled_operation_count = reconciliation.required_operation_count
      AS is_ready_for_release,
    summary.current_accounting_latest_observed_at,
    summary.current_aged_contact_reporting_operation_count,
    summary.current_aged_contact_reporting_authoritative_operation_count,
    summary.current_aged_contact_reporting_distinct_run_count,
    summary.is_ready_for_current_aged_contact_reporting,
    aged.required_operation_count AS aging_portfolio_required_operation_count,
    aged.authoritative_operation_count AS aging_portfolio_authoritative_operation_count,
    aged.gold_contract_operation_count AS aging_portfolio_gold_contract_operation_count,
    aged.raw_reconciled_operation_count AS aging_portfolio_raw_reconciled_operation_count,
    aged.unreconciled_operations AS aging_portfolio_unreconciled_operations,
    CASE
      WHEN COALESCE(aged.required_operation_count, 0)
        != summary.current_aged_contact_reporting_operation_count
        THEN 'aged_contact_reporting_definition_incomplete'
      WHEN NOT summary.is_ready_for_current_aged_contact_reporting
        THEN 'aged_contact_reporting_cohort_not_ready'
      WHEN aged.authoritative_operation_count != aged.required_operation_count
        THEN 'aged_contact_reporting_coverage_not_authoritative'
      WHEN aged.gold_contract_operation_count != aged.required_operation_count
        THEN 'aged_contact_reporting_gold_contract_incomplete'
      WHEN aged.raw_reconciled_operation_count != aged.required_operation_count
        THEN 'aged_contact_reporting_raw_delivery_not_reconciled'
      ELSE 'ready'
    END AS aging_portfolio_release_state,
    summary.is_ready_for_current_aged_contact_reporting
      AND COALESCE(aged.required_operation_count, 0)
        = summary.current_aged_contact_reporting_operation_count
      AND COALESCE(aged.authoritative_operation_count, 0)
        = COALESCE(aged.required_operation_count, 0)
      AND COALESCE(aged.gold_contract_operation_count, 0)
        = COALESCE(aged.required_operation_count, 0)
      AND COALESCE(aged.raw_reconciled_operation_count, 0)
        = COALESCE(aged.required_operation_count, 0)
      AS is_ready_for_aging_portfolio_release
  FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_coverage_summary")} AS summary
  INNER JOIN reconciliation USING (tenant_id)
  LEFT JOIN aged_contact_reporting_reconciliation AS aged USING (tenant_id)
`);

assert("xero_endpoint_warehouse_contract_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, operation_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_warehouse_coverage")}
    GROUP BY tenant_id, operation_id
    HAVING COUNT(*) != 1
  `);

assert("xero_current_accounting_warehouse_contract_completeness")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, operation_id, warehouse_contract_state
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_warehouse_coverage")}
    WHERE is_current_accounting_warehouse_required
      AND warehouse_contract_state != 'gold'
  `);

assert("xero_current_accounting_release_state_consistency")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, release_state, is_ready_for_release,
           aging_portfolio_release_state, is_ready_for_aging_portfolio_release,
           required_operation_count, authoritative_operation_count,
           gold_contract_operation_count, raw_reconciled_operation_count,
           aging_portfolio_required_operation_count,
           aging_portfolio_authoritative_operation_count,
           aging_portfolio_gold_contract_operation_count,
           aging_portfolio_raw_reconciled_operation_count
    FROM ${ctx.ref("dw_bronze_xero", "xero_current_accounting_warehouse_release_readiness")}
    WHERE (release_state = 'ready') != is_ready_for_release
       OR (aging_portfolio_release_state = 'ready') != is_ready_for_aging_portfolio_release
       OR (
         is_ready_for_release
         AND (
           required_operation_count != current_accounting_operation_count
           OR authoritative_operation_count != required_operation_count
           OR gold_contract_operation_count != required_operation_count
           OR raw_reconciled_operation_count != required_operation_count
         )
       )
       OR (
         is_ready_for_aging_portfolio_release
         AND (
           aging_portfolio_required_operation_count
             != current_aged_contact_reporting_operation_count
           OR aging_portfolio_authoritative_operation_count
             != aging_portfolio_required_operation_count
           OR aging_portfolio_gold_contract_operation_count
             != aging_portfolio_required_operation_count
           OR aging_portfolio_raw_reconciled_operation_count
             != aging_portfolio_required_operation_count
         )
       )
  `);
