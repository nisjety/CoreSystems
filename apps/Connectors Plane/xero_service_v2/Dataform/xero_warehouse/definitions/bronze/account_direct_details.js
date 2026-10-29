publish("xero_account_detail_observations", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  description: "Append-only Xero getAccount direct-detail observations. Direct lookup proves field delivery but never determines account lifecycle.",
  bigquery: {
    partitionBy: "DATE(account_detail_observed_at)",
    clusterBy: ["tenant_id", "account_id", "source_run_id"],
  },
}).query(ctx => `
  WITH direct_responses AS (
    SELECT
      CONCAT(generation, '|', tenant_id, '|', operation_id, '|', parent_record_id,
             '|', version_id) AS account_detail_observation_id,
      generation,
      tenant_id,
      parent_record_id AS account_id,
      JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.AccountID')
        AS payload_account_id,
      ARRAY_LENGTH(JSON_QUERY_ARRAY(payload_json, '$.Accounts')) AS payload_account_count,
      TO_HEX(SHA256(TO_JSON_STRING(STRUCT(
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.AccountID')
          AS account_id,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.Code')
          AS account_code,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.Name')
          AS account_name,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.Type')
          AS account_type,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.Class')
          AS account_class,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.Status')
          AS account_status,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.TaxType')
          AS tax_type,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.Accounts')[SAFE_OFFSET(0)], '$.UpdatedDateUTC')
          AS updated_date_utc
      )))) AS shared_account_fingerprint,
      observed_at AS account_detail_observed_at,
      version_id AS source_version_id,
      receipt_id AS source_receipt_id,
      run_id AS source_run_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_history")}
    WHERE operation_id = 'getAccount'
      AND is_parent_scoped
  ), source AS (
    SELECT *
    FROM direct_responses
    WHERE tenant_id IS NOT NULL
      AND account_id IS NOT NULL
      AND source_version_id IS NOT NULL
  )
  SELECT source.*
  FROM source
  ${ctx.when(ctx.incremental(), `
    WHERE NOT EXISTS (
      SELECT 1
      FROM ${ctx.self()} AS prior
      WHERE prior.generation = source.generation
        AND prior.tenant_id = source.tenant_id
        AND prior.account_id = source.account_id
        AND prior.source_version_id = source.source_version_id
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY generation, tenant_id, account_id, source_version_id
    ORDER BY account_detail_observed_at DESC, source_receipt_id DESC
  ) = 1
`);

assert("xero_account_detail_observation_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, account_id, source_version_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_account_detail_observations")}
    GROUP BY 1, 2, 3
    HAVING tenant_id IS NULL OR account_id IS NULL OR source_version_id IS NULL OR COUNT(*) != 1
  `);
