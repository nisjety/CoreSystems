const bronze = (ctx, name) => ctx.ref("dw_bronze_xero", name);

const xeroDate = (json, path) => `
  COALESCE(
    SAFE_CAST(JSON_VALUE(${json}, '${path}') AS DATE),
    DATE(TIMESTAMP_MILLIS(SAFE_CAST(
      REGEXP_EXTRACT(JSON_VALUE(${json}, '${path}'), r'/Date\\((-?\\d+)') AS INT64
    )))
  )`;

const documentedStatus = (status) =>
  `NOT source_deleted AND ${status} IN ('AUTHORISED', 'PAID')`;

publish("fact_expense_claim", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Current Xero employee expense claims from canonical raw history. Source values are retained for every lifecycle state; additive amounts require an authorised or paid non-deleted claim.",
  assertions: { nonNull: ["tenant_id", "expense_claim_id", "expense_claim_key"], uniqueKey: ["expense_claim_key"] },
}).query(ctx => `
  WITH source AS (
    SELECT *,
           COALESCE(JSON_VALUE(payload_json, '$.ExpenseClaimID'), record_id) AS expense_claim_id,
           JSON_VALUE(payload_json, '$.Status') AS status,
           SAFE_CAST(JSON_VALUE(payload_json, '$.Total') AS NUMERIC) AS source_total_amount,
           SAFE_CAST(JSON_VALUE(payload_json, '$.AmountDue') AS NUMERIC) AS source_amount_due,
           SAFE_CAST(JSON_VALUE(payload_json, '$.AmountPaid') AS NUMERIC) AS source_amount_paid
    FROM ${bronze(ctx, "xero_expense_claims")}
  )
  SELECT CONCAT(tenant_id, '|', expense_claim_id) AS expense_claim_key,
         tenant_id, expense_claim_id, record_id AS expense_claim_internal_id,
         JSON_VALUE(payload_json, '$.User.UserID') AS user_id,
         JSON_VALUE(payload_json, '$.User.FirstName') AS user_first_name,
         JSON_VALUE(payload_json, '$.User.LastName') AS user_last_name,
         JSON_VALUE(payload_json, '$.User.EmailAddress') AS user_email,
         status,
         ${xeroDate("payload_json", "$.ReportingDate")} AS reporting_date,
         ${xeroDate("payload_json", "$.PaymentDueDate")} AS payment_due_date,
         ${xeroDate("payload_json", "$.UpdatedDateUTC")} AS updated_date,
         source_total_amount, source_amount_due, source_amount_paid,
         ${documentedStatus("status")} AS is_reportable,
         IF(${documentedStatus("status")}, source_total_amount, NULL) AS total_amount,
         IF(${documentedStatus("status")}, source_amount_due, NULL) AS amount_due,
         IF(${documentedStatus("status")}, source_amount_paid, NULL) AS amount_paid,
         source_status, source_deleted, generation, version_id, run_id, receipt_id,
         observed_at, source_updated_at
  FROM source
`);

publish("fact_expense_claim_receipt", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Expense-claim receipt rows nested in canonical Xero raw claim documents. Position fallback remains explicit if Xero omits a ReceiptID.",
  assertions: { nonNull: ["tenant_id", "expense_claim_id", "receipt_id", "expense_claim_receipt_key"], uniqueKey: ["expense_claim_receipt_key"] },
}).query(ctx => `
  WITH claims AS (
    SELECT * EXCEPT(receipt_id), receipt_id AS raw_receipt_id,
           COALESCE(JSON_VALUE(payload_json, '$.ExpenseClaimID'), record_id) AS expense_claim_id,
           JSON_VALUE(payload_json, '$.Status') AS expense_claim_status
    FROM ${bronze(ctx, "xero_expense_claims")}
  ), receipts AS (
    SELECT claim.*, receipt, receipt_position,
           COALESCE(JSON_VALUE(receipt, '$.ReceiptID'), CONCAT('position:', CAST(receipt_position AS STRING))) AS nested_receipt_id,
           JSON_VALUE(receipt, '$.Status') AS status,
           SAFE_CAST(JSON_VALUE(receipt, '$.Total') AS NUMERIC) AS source_total_amount,
           SAFE_CAST(JSON_VALUE(receipt, '$.SubTotal') AS NUMERIC) AS source_sub_total_amount,
           SAFE_CAST(JSON_VALUE(receipt, '$.TotalTax') AS NUMERIC) AS source_total_tax_amount
    FROM claims AS claim
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(claim.payload_json, '$.Receipts'), [])) AS receipt WITH OFFSET AS receipt_position
  )
  SELECT CONCAT(tenant_id, '|', expense_claim_id, '|', nested_receipt_id) AS expense_claim_receipt_key,
         tenant_id, expense_claim_id, nested_receipt_id AS receipt_id, receipt_position,
         JSON_VALUE(receipt, '$.Contact.ContactID') AS contact_id,
         JSON_VALUE(receipt, '$.Contact.Name') AS contact_name,
         JSON_VALUE(receipt, '$.User.UserID') AS user_id,
         status, JSON_VALUE(receipt, '$.Reference') AS reference,
         ${xeroDate("receipt", "$.Date")} AS receipt_date,
         source_sub_total_amount, source_total_tax_amount, source_total_amount,
         NOT source_deleted
           AND expense_claim_status IN ('AUTHORISED', 'PAID')
           AND COALESCE(status, 'AUTHORISED') IN ('AUTHORISED', 'PAID') AS is_reportable,
         IF(NOT source_deleted AND expense_claim_status IN ('AUTHORISED', 'PAID')
              AND COALESCE(status, 'AUTHORISED') IN ('AUTHORISED', 'PAID'), source_total_amount, NULL) AS total_amount,
         source_status, source_deleted, generation, version_id, run_id, raw_receipt_id AS parent_receipt_id,
         observed_at, source_updated_at
  FROM receipts
`);

publish("fact_expense_claim_receipt_line", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Line items nested in Xero expense-claim receipts. Line position is retained whenever Xero omits LineItemID.",
  assertions: { nonNull: ["tenant_id", "expense_claim_id", "receipt_id", "line_item_id", "expense_claim_receipt_line_key"], uniqueKey: ["expense_claim_receipt_line_key"] },
}).query(ctx => `
  WITH claims AS (
    SELECT * EXCEPT(receipt_id), receipt_id AS raw_receipt_id,
           COALESCE(JSON_VALUE(payload_json, '$.ExpenseClaimID'), record_id) AS expense_claim_id,
           JSON_VALUE(payload_json, '$.Status') AS expense_claim_status
    FROM ${bronze(ctx, "xero_expense_claims")}
  ), receipts AS (
    SELECT claim.*, receipt, receipt_position,
           COALESCE(JSON_VALUE(receipt, '$.ReceiptID'), CONCAT('position:', CAST(receipt_position AS STRING))) AS nested_receipt_id,
           JSON_VALUE(receipt, '$.Status') AS receipt_status
    FROM claims AS claim
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(claim.payload_json, '$.Receipts'), [])) AS receipt WITH OFFSET AS receipt_position
  ), lines AS (
    SELECT receipt.*, line, line_position,
           COALESCE(JSON_VALUE(line, '$.LineItemID'), CONCAT('position:', CAST(line_position AS STRING))) AS line_item_id,
           SAFE_CAST(JSON_VALUE(line, '$.LineAmount') AS NUMERIC) AS source_line_amount,
           SAFE_CAST(JSON_VALUE(line, '$.TaxAmount') AS NUMERIC) AS source_tax_amount
    FROM receipts AS receipt
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(receipt.receipt, '$.LineItems'), [])) AS line WITH OFFSET AS line_position
  )
  SELECT CONCAT(tenant_id, '|', expense_claim_id, '|', nested_receipt_id, '|', line_item_id) AS expense_claim_receipt_line_key,
         tenant_id, expense_claim_id, nested_receipt_id AS receipt_id, line_item_id, receipt_position, line_position,
         ${xeroDate("receipt", "$.Date")} AS receipt_date,
         JSON_VALUE(line, '$.AccountID') AS account_id,
         JSON_VALUE(line, '$.AccountCode') AS account_code,
         JSON_VALUE(line, '$.Description') AS description,
         SAFE_CAST(JSON_VALUE(line, '$.Quantity') AS NUMERIC) AS quantity,
         SAFE_CAST(JSON_VALUE(line, '$.UnitAmount') AS NUMERIC) AS unit_amount,
         JSON_VALUE(line, '$.TaxType') AS tax_type,
         source_line_amount, source_tax_amount,
         NOT source_deleted AND expense_claim_status IN ('AUTHORISED', 'PAID')
           AND COALESCE(receipt_status, 'AUTHORISED') IN ('AUTHORISED', 'PAID') AS is_reportable,
         IF(NOT source_deleted AND expense_claim_status IN ('AUTHORISED', 'PAID')
              AND COALESCE(receipt_status, 'AUTHORISED') IN ('AUTHORISED', 'PAID'), source_line_amount, NULL) AS line_amount,
         source_status, source_deleted, generation, version_id, run_id, raw_receipt_id AS parent_receipt_id,
         observed_at, source_updated_at
  FROM lines
`);

publish("fact_prepayment", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Current Xero prepayment documents from canonical raw history. Lifecycle evidence remains visible even when a prepayment is not reportable.",
  assertions: { nonNull: ["tenant_id", "prepayment_id", "prepayment_key"], uniqueKey: ["prepayment_key"] },
}).query(ctx => `
  WITH source AS (
    SELECT *, COALESCE(JSON_VALUE(payload_json, '$.PrepaymentID'), record_id) AS prepayment_id,
           JSON_VALUE(payload_json, '$.Status') AS status,
           SAFE_CAST(JSON_VALUE(payload_json, '$.Total') AS NUMERIC) AS source_total_amount,
           SAFE_CAST(JSON_VALUE(payload_json, '$.RemainingCredit') AS NUMERIC) AS source_remaining_credit
    FROM ${bronze(ctx, "xero_prepayments")}
  )
  SELECT CONCAT(tenant_id, '|', prepayment_id) AS prepayment_key,
         tenant_id, prepayment_id, record_id AS prepayment_internal_id,
         JSON_VALUE(payload_json, '$.Type') AS prepayment_type, status,
         JSON_VALUE(payload_json, '$.Contact.ContactID') AS contact_id,
         JSON_VALUE(payload_json, '$.Contact.Name') AS contact_name,
         JSON_VALUE(payload_json, '$.CurrencyCode') AS currency_code,
         SAFE_CAST(JSON_VALUE(payload_json, '$.CurrencyRate') AS NUMERIC) AS currency_rate,
         ${xeroDate("payload_json", "$.Date")} AS prepayment_date,
         ${xeroDate("payload_json", "$.UpdatedDateUTC")} AS updated_date,
         SAFE_CAST(JSON_VALUE(payload_json, '$.SubTotal') AS NUMERIC) AS source_sub_total_amount,
         SAFE_CAST(JSON_VALUE(payload_json, '$.TotalTax') AS NUMERIC) AS source_total_tax_amount,
         source_total_amount, source_remaining_credit,
         ${documentedStatus("status")} AS is_reportable,
         IF(${documentedStatus("status")}, source_total_amount, NULL) AS total_amount,
         IF(${documentedStatus("status")}, source_remaining_credit, NULL) AS remaining_credit,
         source_status, source_deleted, generation, version_id, run_id, receipt_id,
         observed_at, source_updated_at
  FROM source
`);

publish("fact_prepayment_line", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Line items nested in canonical Xero prepayment documents.",
  assertions: { nonNull: ["tenant_id", "prepayment_id", "line_item_id", "prepayment_line_key"], uniqueKey: ["prepayment_line_key"] },
}).query(ctx => `
  WITH documents AS (
    SELECT *, COALESCE(JSON_VALUE(payload_json, '$.PrepaymentID'), record_id) AS prepayment_id,
           JSON_VALUE(payload_json, '$.Status') AS status
    FROM ${bronze(ctx, "xero_prepayments")}
  ), lines AS (
    SELECT document.*, line, line_position,
           COALESCE(JSON_VALUE(line, '$.LineItemID'), CONCAT('position:', CAST(line_position AS STRING))) AS line_item_id,
           SAFE_CAST(JSON_VALUE(line, '$.LineAmount') AS NUMERIC) AS source_line_amount
    FROM documents AS document
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(document.payload_json, '$.LineItems'), [])) AS line WITH OFFSET AS line_position
  )
  SELECT CONCAT(tenant_id, '|', prepayment_id, '|', line_item_id) AS prepayment_line_key,
         tenant_id, prepayment_id, line_item_id, line_position,
         ${xeroDate("payload_json", "$.Date")} AS prepayment_date,
         JSON_VALUE(line, '$.AccountID') AS account_id,
         JSON_VALUE(line, '$.AccountCode') AS account_code,
         JSON_VALUE(line, '$.Description') AS description,
         SAFE_CAST(JSON_VALUE(line, '$.Quantity') AS NUMERIC) AS quantity,
         SAFE_CAST(JSON_VALUE(line, '$.UnitAmount') AS NUMERIC) AS unit_amount,
         SAFE_CAST(JSON_VALUE(line, '$.TaxAmount') AS NUMERIC) AS source_tax_amount,
         JSON_VALUE(line, '$.TaxType') AS tax_type,
         source_line_amount, ${documentedStatus("status")} AS is_reportable,
         IF(${documentedStatus("status")}, source_line_amount, NULL) AS line_amount,
         source_status, source_deleted, generation, version_id, run_id, receipt_id,
         observed_at, source_updated_at
  FROM lines
`);

publish("fact_prepayment_allocation", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Invoice allocations nested in canonical Xero prepayment documents. Allocation position is retained if Xero omits AllocationID.",
  assertions: { nonNull: ["tenant_id", "prepayment_id", "allocation_id", "prepayment_allocation_key"], uniqueKey: ["prepayment_allocation_key"] },
}).query(ctx => `
  WITH documents AS (
    SELECT *, COALESCE(JSON_VALUE(payload_json, '$.PrepaymentID'), record_id) AS prepayment_id,
           JSON_VALUE(payload_json, '$.Status') AS status
    FROM ${bronze(ctx, "xero_prepayments")}
  ), allocations AS (
    SELECT document.*, allocation, allocation_position,
           COALESCE(JSON_VALUE(allocation, '$.AllocationID'), CONCAT('position:', CAST(allocation_position AS STRING))) AS allocation_id,
           SAFE_CAST(JSON_VALUE(allocation, '$.Amount') AS NUMERIC) AS source_allocation_amount
    FROM documents AS document
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(document.payload_json, '$.Allocations'), [])) AS allocation WITH OFFSET AS allocation_position
  )
  SELECT CONCAT(tenant_id, '|', prepayment_id, '|', allocation_id) AS prepayment_allocation_key,
         tenant_id, prepayment_id, allocation_id, allocation_position,
         JSON_VALUE(allocation, '$.Invoice.InvoiceID') AS invoice_id,
         ${xeroDate("allocation", "$.Date")} AS allocation_date,
         source_allocation_amount, ${documentedStatus("status")} AS is_reportable,
         IF(${documentedStatus("status")}, source_allocation_amount, NULL) AS allocation_amount,
         source_status, source_deleted, generation, version_id, run_id, receipt_id,
         observed_at, source_updated_at
  FROM allocations
`);

publish("fact_receipt", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Current standalone Xero receipts from canonical raw history. These are distinct from expense-claim receipt rows.",
  assertions: { nonNull: ["tenant_id", "receipt_id", "receipt_key"], uniqueKey: ["receipt_key"] },
}).query(ctx => `
  WITH source AS (
    SELECT * EXCEPT(receipt_id), receipt_id AS raw_receipt_id,
           COALESCE(JSON_VALUE(payload_json, '$.ReceiptID'), record_id) AS xero_receipt_id,
           JSON_VALUE(payload_json, '$.Status') AS status,
           SAFE_CAST(JSON_VALUE(payload_json, '$.Total') AS NUMERIC) AS source_total_amount
    FROM ${bronze(ctx, "xero_receipts")}
  )
  SELECT CONCAT(tenant_id, '|', xero_receipt_id) AS receipt_key,
         tenant_id, xero_receipt_id AS receipt_id, record_id AS receipt_internal_id,
         status, JSON_VALUE(payload_json, '$.Reference') AS reference,
         JSON_VALUE(payload_json, '$.Contact.ContactID') AS contact_id,
         JSON_VALUE(payload_json, '$.Contact.Name') AS contact_name,
         JSON_VALUE(payload_json, '$.User.UserID') AS user_id,
         ${xeroDate("payload_json", "$.Date")} AS receipt_date,
         ${xeroDate("payload_json", "$.UpdatedDateUTC")} AS updated_date,
         SAFE_CAST(JSON_VALUE(payload_json, '$.SubTotal') AS NUMERIC) AS source_sub_total_amount,
         SAFE_CAST(JSON_VALUE(payload_json, '$.TotalTax') AS NUMERIC) AS source_total_tax_amount,
         source_total_amount, ${documentedStatus("status")} AS is_reportable,
         IF(${documentedStatus("status")}, source_total_amount, NULL) AS total_amount,
         source_status, source_deleted, generation, version_id, run_id, raw_receipt_id AS parent_receipt_id,
         observed_at, source_updated_at
  FROM source
`);

publish("fact_receipt_line", {
  type: "table",
  schema: "dw_1_silver_xero",
  description: "Line items nested in standalone canonical Xero receipt documents.",
  assertions: { nonNull: ["tenant_id", "receipt_id", "line_item_id", "receipt_line_key"], uniqueKey: ["receipt_line_key"] },
}).query(ctx => `
  WITH documents AS (
    SELECT * EXCEPT(receipt_id), receipt_id AS raw_receipt_id,
           COALESCE(JSON_VALUE(payload_json, '$.ReceiptID'), record_id) AS xero_receipt_id,
           JSON_VALUE(payload_json, '$.Status') AS status
    FROM ${bronze(ctx, "xero_receipts")}
  ), lines AS (
    SELECT document.*, line, line_position,
           COALESCE(JSON_VALUE(line, '$.LineItemID'), CONCAT('position:', CAST(line_position AS STRING))) AS line_item_id,
           SAFE_CAST(JSON_VALUE(line, '$.LineAmount') AS NUMERIC) AS source_line_amount
    FROM documents AS document
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(document.payload_json, '$.LineItems'), [])) AS line WITH OFFSET AS line_position
  )
  SELECT CONCAT(tenant_id, '|', xero_receipt_id, '|', line_item_id) AS receipt_line_key,
         tenant_id, xero_receipt_id AS receipt_id, line_item_id, line_position,
         ${xeroDate("payload_json", "$.Date")} AS receipt_date,
         JSON_VALUE(line, '$.AccountID') AS account_id,
         JSON_VALUE(line, '$.AccountCode') AS account_code,
         JSON_VALUE(line, '$.Description') AS description,
         SAFE_CAST(JSON_VALUE(line, '$.Quantity') AS NUMERIC) AS quantity,
         SAFE_CAST(JSON_VALUE(line, '$.UnitAmount') AS NUMERIC) AS unit_amount,
         SAFE_CAST(JSON_VALUE(line, '$.TaxAmount') AS NUMERIC) AS source_tax_amount,
         JSON_VALUE(line, '$.TaxType') AS tax_type,
         source_line_amount, ${documentedStatus("status")} AS is_reportable,
         IF(${documentedStatus("status")}, source_line_amount, NULL) AS line_amount,
         source_status, source_deleted, generation, version_id, run_id, raw_receipt_id AS parent_receipt_id,
         observed_at, source_updated_at
  FROM lines
`);
