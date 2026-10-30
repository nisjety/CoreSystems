publish("xero_contact_group_detail_observations", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  description: "Append-only Xero getContactGroup direct-detail observations. Direct membership evidence is retained for reconciliation only and never changes the list-controlled membership lifecycle.",
  bigquery: {
    partitionBy: "DATE(contact_group_detail_observed_at)",
    clusterBy: ["tenant_id", "contact_group_id", "source_run_id"],
  },
}).query(ctx => `
  WITH direct_responses AS (
    SELECT
      CONCAT(generation, '|', tenant_id, '|', operation_id, '|', parent_record_id,
             '|', version_id) AS contact_group_detail_observation_id,
      generation,
      tenant_id,
      parent_record_id AS contact_group_id,
      JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.ContactGroupID')
        AS payload_contact_group_id,
      ARRAY_LENGTH(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')) AS payload_contact_group_count,
      JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.Name')
        AS contact_group_name,
      JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.Status')
        AS contact_group_status,
      ARRAY_LENGTH(JSON_QUERY_ARRAY(
        JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.Contacts'
      )) AS direct_member_count,
      TO_HEX(SHA256(TO_JSON_STRING(STRUCT(
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.ContactGroupID')
          AS contact_group_id,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.Name')
          AS contact_group_name,
        JSON_VALUE(JSON_QUERY_ARRAY(payload_json, '$.ContactGroups')[SAFE_OFFSET(0)], '$.Status')
          AS contact_group_status
      )))) AS shared_contact_group_fingerprint,
      observed_at AS contact_group_detail_observed_at,
      version_id AS source_version_id,
      receipt_id AS source_receipt_id,
      run_id AS source_run_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_history")}
    WHERE operation_id = 'getContactGroup'
      AND is_parent_scoped
  ), source AS (
    SELECT *
    FROM direct_responses
    WHERE tenant_id IS NOT NULL
      AND contact_group_id IS NOT NULL
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
        AND prior.contact_group_id = source.contact_group_id
        AND prior.source_version_id = source.source_version_id
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY generation, tenant_id, contact_group_id, source_version_id
    ORDER BY contact_group_detail_observed_at DESC, source_receipt_id DESC
  ) = 1
`);

assert("xero_contact_group_detail_observation_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, contact_group_id, source_version_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_contact_group_detail_observations")}
    GROUP BY 1, 2, 3
    HAVING tenant_id IS NULL OR contact_group_id IS NULL OR source_version_id IS NULL OR COUNT(*) != 1
  `);
