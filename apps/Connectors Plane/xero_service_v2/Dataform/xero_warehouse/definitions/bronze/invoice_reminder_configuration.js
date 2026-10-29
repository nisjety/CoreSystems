publish("xero_invoice_reminder_observations", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  description: "Append-only Xero invoice-reminder configuration observations. An empty InvoiceReminders array is retained as an explicit source state; no absent configuration is inferred as deletion.",
  bigquery: {
    partitionBy: "DATE(reminder_observed_at)",
    clusterBy: ["tenant_id", "source_run_id"],
  },
}).query(ctx => `
  WITH source AS (
    SELECT
      CONCAT(
        response.tenant_id, '|', response.receipt_id, '|',
        COALESCE(CAST(reminder_position AS STRING), 'empty')
      ) AS reminder_observation_key,
      response.generation,
      response.tenant_id,
      reminder_position,
      SAFE_CAST(JSON_VALUE(reminder_json, '$.Enabled') AS BOOL) AS is_enabled,
      JSON_QUERY(response.payload_json, '$.InvoiceReminders') IS NOT NULL
        AND ARRAY_LENGTH(IFNULL(JSON_QUERY_ARRAY(response.payload_json, '$.InvoiceReminders'), [])) = 0
        AS is_explicitly_empty,
      JSON_QUERY(response.payload_json, '$.InvoiceReminders') IS NULL
        AS is_source_configuration_missing,
      response.observed_at AS reminder_observed_at,
      response.canonical_record_id AS source_record_id,
      response.version_id AS source_version_id,
      response.receipt_id AS source_receipt_id,
      response.run_id AS source_run_id,
      response.payload_hash AS source_payload_hash
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_history")} AS response
    LEFT JOIN UNNEST(
      IFNULL(JSON_QUERY_ARRAY(response.payload_json, '$.InvoiceReminders'), [])
    ) AS reminder_json WITH OFFSET AS reminder_position ON TRUE
    WHERE response.operation_id = 'getInvoiceReminders'
  )
  SELECT source.*
  FROM source
  WHERE reminder_observation_key IS NOT NULL
    AND tenant_id IS NOT NULL
    AND source_receipt_id IS NOT NULL
  ${ctx.when(ctx.incremental(), `
    AND NOT EXISTS (
      SELECT 1
      FROM ${ctx.self()} AS prior
      WHERE prior.reminder_observation_key = source.reminder_observation_key
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY reminder_observation_key
    ORDER BY reminder_observed_at DESC, source_version_id DESC
  ) = 1
`);

assert("xero_invoice_reminder_observation_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT reminder_observation_key
    FROM ${ctx.ref("dw_bronze_xero", "xero_invoice_reminder_observations")}
    GROUP BY 1
    HAVING reminder_observation_key IS NULL OR COUNT(*) != 1
  `);
