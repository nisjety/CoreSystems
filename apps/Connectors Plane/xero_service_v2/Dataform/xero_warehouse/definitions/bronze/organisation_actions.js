publish("xero_organisation_action_observations", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  description: "Append-only Xero organisation-action observations for the identity behind this integration. These are connection capabilities, not organisation-wide user entitlements. Explicitly empty action arrays remain observable in Bronze.",
  bigquery: {
    partitionBy: "DATE(action_observed_at)",
    clusterBy: ["tenant_id", "action_name"],
  },
}).query(ctx => `
  WITH source AS (
    SELECT
      CONCAT(
        response.tenant_id, '|', response.receipt_id, '|',
        COALESCE(CAST(action_position AS STRING), 'empty')
      ) AS organisation_action_observation_key,
      response.generation,
      response.tenant_id,
      action_position,
      JSON_VALUE(action_json, '$.Name') AS action_name,
      JSON_VALUE(action_json, '$.Status') AS action_status,
      JSON_QUERY(response.payload_json, '$.Actions') IS NOT NULL
        AND ARRAY_LENGTH(IFNULL(JSON_QUERY_ARRAY(response.payload_json, '$.Actions'), [])) = 0
        AS is_explicitly_empty,
      JSON_QUERY(response.payload_json, '$.Actions') IS NULL
        AS is_source_action_set_missing,
      response.observed_at AS action_observed_at,
      response.canonical_record_id AS source_record_id,
      response.version_id AS source_version_id,
      response.receipt_id AS source_receipt_id,
      response.run_id AS source_run_id,
      response.payload_hash AS source_payload_hash
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_history")} AS response
    LEFT JOIN UNNEST(IFNULL(JSON_QUERY_ARRAY(response.payload_json, '$.Actions'), []))
      AS action_json WITH OFFSET AS action_position ON TRUE
    WHERE response.operation_id = 'getOrganisationActions'
  )
  SELECT source.*
  FROM source
  WHERE organisation_action_observation_key IS NOT NULL
    AND tenant_id IS NOT NULL
    AND source_receipt_id IS NOT NULL
  ${ctx.when(ctx.incremental(), `
    AND NOT EXISTS (
      SELECT 1
      FROM ${ctx.self()} AS prior
      WHERE prior.organisation_action_observation_key = source.organisation_action_observation_key
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY organisation_action_observation_key
    ORDER BY action_observed_at DESC, source_version_id DESC
  ) = 1
`);

assert("xero_organisation_action_observation_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT organisation_action_observation_key
    FROM ${ctx.ref("dw_bronze_xero", "xero_organisation_action_observations")}
    GROUP BY 1
    HAVING organisation_action_observation_key IS NULL OR COUNT(*) != 1
  `);
