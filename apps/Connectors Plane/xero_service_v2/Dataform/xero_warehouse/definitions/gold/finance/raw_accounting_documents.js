const project = dataform.projectConfig.defaultDatabase;

const documentFacts = [
  { name: "fact_expense_claim", key: "expense_claim_key", date: "reporting_date" },
  { name: "fact_expense_claim_receipt", key: "expense_claim_receipt_key", date: "receipt_date", foreign: "contact" },
  { name: "fact_expense_claim_receipt_line", key: "expense_claim_receipt_line_key", date: "receipt_date", foreign: "account" },
  { name: "fact_prepayment", key: "prepayment_key", date: "prepayment_date", foreign: "contact" },
  { name: "fact_prepayment_line", key: "prepayment_line_key", date: "prepayment_date", foreign: "account" },
  { name: "fact_prepayment_allocation", key: "prepayment_allocation_key", date: "allocation_date", foreign: "invoice" },
  { name: "fact_receipt", key: "receipt_key", date: "receipt_date", foreign: "contact" },
  { name: "fact_receipt_line", key: "receipt_line_key", date: "receipt_date", foreign: "account" },
];

function foreignKey(foreign) {
  if (foreign === "account") {
    return "IF(source.account_id IS NULL, NULL, FARM_FINGERPRINT(CONCAT(source.tenant_id, '|', source.account_id))) AS account_key,";
  }
  if (foreign === "contact") {
    return "IF(source.contact_id IS NULL, NULL, FARM_FINGERPRINT(CONCAT(source.tenant_id, '|', source.contact_id))) AS contact_key,";
  }
  if (foreign === "invoice") {
    return "IF(source.invoice_id IS NULL, NULL, FARM_FINGERPRINT(CONCAT(source.tenant_id, '|', source.invoice_id))) AS invoice_key,";
  }
  return "";
}

for (const fact of documentFacts) {
  publish(fact.name, {
    type: "table",
    schema: "xero_gold_finance",
    description: `Conformed Xero ${fact.name.replaceAll("_", " ")} from canonical raw-backed Silver data. Source amounts remain visible for every lifecycle state; reportable measures obey the Silver status rule.`,
    assertions: {
      nonNull: ["tenant_id", fact.key],
      uniqueKey: [fact.key],
    },
  }).query(ctx => `
    SELECT
      FARM_FINGERPRINT(source.tenant_id) AS company_key,
      IF(source.${fact.date} IS NULL, NULL,
         FARM_FINGERPRINT(CAST(source.${fact.date} AS STRING))) AS date_key,
      period.financial_period_key,
      period.financial_period_basis,
      period.is_derived AS is_financial_period_derived,
      ${foreignKey(fact.foreign)}
      company.base_currency AS functional_currency,
      source.* EXCEPT (tenant_id, is_reportable),
      source.tenant_id,
      source.is_reportable,
      'xero' AS source_system
    FROM ${ctx.ref("dw_1_silver_xero", fact.name)} AS source
    LEFT JOIN ${ctx.ref("xero_gold_common", "dim_company")} AS company
      USING (tenant_id)
    LEFT JOIN ${ctx.ref("dw_1_silver_xero", "bridge_company_date_fiscal_period")} AS fiscal_date
      ON source.tenant_id = fiscal_date.tenant_id
     AND source.${fact.date} = fiscal_date.calendar_date
    LEFT JOIN ${ctx.ref("xero_gold_common", "dim_financial_period")} AS period
      ON fiscal_date.tenant_id = period.tenant_id
     AND fiscal_date.fiscal_configuration_state_key = period.fiscal_configuration_state_key
     AND fiscal_date.financial_period_id = period.financial_period_id
  `);
}
