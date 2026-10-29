publish("xero_budget_detail_observations", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Latest authoritative Xero GET /Budgets/{BudgetID} responses. Every row retains the raw direct-response provenance and preserves a zero BudgetLines array as an explicit source state.",
  assertions: {
    nonNull: ["tenant_id", "budget_id", "budget_detail_observation_key"],
    uniqueKey: ["budget_detail_observation_key"],
  },
}).query(ctx => `
  WITH responses AS (
    SELECT response.*,
           budget_json
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_current")} AS response
    CROSS JOIN UNNEST(
      IFNULL(
        JSON_QUERY_ARRAY(response.payload_json, '$.Budgets'),
        [JSON_QUERY(response.payload_json, '$.Budgets')]
      )
    ) AS budget_json
    WHERE response.operation_id = 'getBudget'
      AND budget_json IS NOT NULL
  )
  SELECT
    CONCAT(tenant_id, '|', JSON_VALUE(budget_json, '$.BudgetID'), '|', version_id)
      AS budget_detail_observation_key,
    tenant_id,
    JSON_VALUE(budget_json, '$.BudgetID') AS budget_id,
    JSON_VALUE(budget_json, '$.Type') AS budget_type,
    JSON_VALUE(budget_json, '$.Description') AS description,
    JSON_VALUE(budget_json, '$.Status') AS status,
    JSON_QUERY(budget_json, '$.Tracking') AS tracking_json,
    JSON_QUERY(budget_json, '$.BudgetLines') AS budget_lines_json,
    COALESCE(ARRAY_LENGTH(JSON_QUERY_ARRAY(budget_json, '$.BudgetLines')), 0) AS budget_line_count,
    generation,
    endpoint,
    operation_id,
    canonical_record_id,
    parent_record_id,
    source_endpoint,
    version_id,
    observed_at,
    source_updated_at,
    run_id,
    receipt_id,
    source_status,
    source_deleted,
    payload_hash,
    payload_json
  FROM responses
`);

publish("xero_budget_balance_observations", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Native Xero budget account-period balances extracted from the latest authoritative budget-detail response. Source position stays in the key because Xero does not provide a BudgetBalance identifier.",
  assertions: {
    nonNull: ["tenant_id", "budget_id", "budget_line_position", "budget_balance_position", "budget_balance_observation_key"],
    uniqueKey: ["budget_balance_observation_key"],
  },
}).query(ctx => `
  WITH budget_lines AS (
    SELECT
      detail.*,
      budget_line_json,
      budget_line_position
    FROM ${ctx.ref("dw_bronze_xero", "xero_budget_detail_observations")} AS detail
    CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(detail.budget_lines_json, '$'), []))
      AS budget_line_json WITH OFFSET AS budget_line_position
  )
  SELECT
    CONCAT(
      tenant_id, '|', budget_id, '|', version_id, '|',
      CAST(budget_line_position AS STRING), '|', CAST(budget_balance_position AS STRING)
    ) AS budget_balance_observation_key,
    tenant_id,
    budget_id,
    budget_type,
    description AS budget_description,
    status AS budget_status,
    JSON_VALUE(budget_line_json, '$.AccountID') AS account_id,
    JSON_VALUE(budget_line_json, '$.AccountCode') AS account_code,
    budget_line_position,
    budget_balance_position,
    JSON_VALUE(budget_balance_json, '$.Period') AS budget_period_raw,
    SAFE_CAST(JSON_VALUE(budget_balance_json, '$.Amount') AS NUMERIC) AS source_amount,
    SAFE_CAST(JSON_VALUE(budget_balance_json, '$.UnitAmount') AS NUMERIC) AS source_unit_amount,
    JSON_VALUE(budget_balance_json, '$.Notes') AS notes,
    generation,
    endpoint,
    operation_id,
    canonical_record_id,
    parent_record_id,
    source_endpoint,
    version_id,
    observed_at,
    source_updated_at,
    run_id,
    receipt_id,
    source_status,
    source_deleted,
    payload_hash
  FROM budget_lines
  CROSS JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(budget_line_json, '$.BudgetBalances'), []))
    AS budget_balance_json WITH OFFSET AS budget_balance_position
`);
