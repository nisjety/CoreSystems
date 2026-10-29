const project = dataform.projectConfig.defaultDatabase;
const stagingSchema = "dw_staging_xero";
const projectEndpoints = [
  "projects_projects",
  "projects_users",
  "projects_tasks",
  "projects_time",
];
const projectEndpointList = projectEndpoints.map(endpoint => `'${endpoint}'`).join(", ");

function appendProjectObservation(name, endpoint, identity, projection, description) {
  publish(name, {
    type: "incremental",
    schema: "dw_bronze_xero",
    protected: true,
    // Project observations are append-only and protected. Any added or removed
    // column requires an explicit, reviewed BigQuery migration before release.
    // EXTEND emits one ALTER TABLE per added column and can exhaust BigQuery's
    // table metadata update quota during a full workflow invocation.
    onSchemaChange: "FAIL",
    description,
    bigquery: {
      partitionBy: "DATE(project_observed_at)",
      clusterBy: ["tenant_id", "source_run_id"],
    },
  }).query(ctx => `
    WITH projected AS (
      SELECT ${projection}
      FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")}
      WHERE endpoint = '${endpoint}'
    ), source AS (
      SELECT *
      FROM projected
      WHERE ${identity.map(field => `${field} IS NOT NULL`).join(" AND ")}
    )
    SELECT source.*
    FROM source
    ${ctx.when(ctx.incremental(), `
      WHERE NOT EXISTS (
        SELECT 1
        FROM ${ctx.self()} AS prior
        WHERE ${identity.map(field => `prior.${field} = source.${field}`).join(" AND ")}
      )
    `)}
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY ${identity.join(", ")}
      ORDER BY project_observed_at DESC, source_receipt_id DESC
    ) = 1
  `);
}

function projectObservationProjection(observationId) {
  return `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS ${observationId},
    generation,
    2 AS projection_schema_revision,
    tenant_id,
    JSON_VALUE(payload_json, '$.projectId') AS project_id,
    JSON_VALUE(payload_json, '$.contactId') AS contact_id,
    JSON_VALUE(payload_json, '$.taskId') AS task_id,
    JSON_VALUE(payload_json, '$.timeEntryId') AS time_entry_id,
    JSON_VALUE(payload_json, '$.userId') AS user_id,
    JSON_VALUE(payload_json, '$.name') AS project_name,
    JSON_VALUE(payload_json, '$.status') AS project_status,
    JSON_VALUE(payload_json, '$.name') AS task_name,
    JSON_VALUE(payload_json, '$.status') AS task_status,
    JSON_VALUE(payload_json, '$.chargeType') AS charge_type,
    SAFE_CAST(JSON_VALUE(payload_json, '$.rate.value') AS NUMERIC) AS rate_amount,
    COALESCE(JSON_VALUE(payload_json, '$.rate.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS rate_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalAmount.value') AS NUMERIC) AS total_amount,
    COALESCE(JSON_VALUE(payload_json, '$.totalAmount.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS total_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.amountInvoiced.value') AS NUMERIC) AS amount_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.amountInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS amount_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.amountToBeInvoiced.value') AS NUMERIC)
      AS amount_to_be_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.amountToBeInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS amount_to_be_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.fixedMinutes') AS INT64) AS fixed_minutes,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalMinutes') AS INT64) AS total_minutes,
    SAFE_CAST(JSON_VALUE(payload_json, '$.minutesInvoiced') AS INT64) AS minutes_invoiced,
    SAFE_CAST(JSON_VALUE(payload_json, '$.nonChargeableMinutes') AS INT64) AS non_chargeable_minutes,
    JSON_VALUE(payload_json, '$.status') AS time_entry_status,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.dateUtc'), 1, 10) AS DATE) AS entry_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.dateEnteredUtc') AS TIMESTAMP) AS entered_at,
    SAFE_CAST(JSON_VALUE(payload_json, '$.duration') AS INT64) AS duration_minutes,
    JSON_VALUE(payload_json, '$.currencyCode') AS currency_code,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.deadlineUtc'), 1, 10) AS DATE) AS deadline_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.estimate.value') AS NUMERIC) AS estimate_amount,
    COALESCE(JSON_VALUE(payload_json, '$.estimate.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS estimate_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalInvoiced.value') AS NUMERIC) AS total_invoiced_amount,
    COALESCE(JSON_VALUE(payload_json, '$.totalInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS total_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalToBeInvoiced.value') AS NUMERIC)
      AS total_to_be_invoiced_amount,
    COALESCE(JSON_VALUE(payload_json, '$.totalToBeInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS total_to_be_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalExpenseAmount.value') AS NUMERIC)
      AS total_expense_amount,
    COALESCE(JSON_VALUE(payload_json, '$.totalExpenseAmount.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS total_expense_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.expenseAmountInvoiced.value') AS NUMERIC)
      AS expense_amount_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.expenseAmountInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS expense_amount_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.expenseAmountToBeInvoiced.value') AS NUMERIC)
      AS expense_amount_to_be_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.expenseAmountToBeInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS expense_amount_to_be_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalTaskAmount.value') AS NUMERIC) AS total_task_amount,
    COALESCE(JSON_VALUE(payload_json, '$.totalTaskAmount.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS total_task_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.taskAmountInvoiced.value') AS NUMERIC) AS task_amount_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.taskAmountInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS task_amount_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.taskAmountToBeInvoiced.value') AS NUMERIC)
      AS task_amount_to_be_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.taskAmountToBeInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS task_amount_to_be_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.projectAmountInvoiced.value') AS NUMERIC)
      AS project_amount_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.projectAmountInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS project_amount_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.creditNoteAmount.value') AS NUMERIC) AS credit_note_amount,
    COALESCE(JSON_VALUE(payload_json, '$.creditNoteAmount.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS credit_note_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.deposit.value') AS NUMERIC) AS deposit_amount,
    COALESCE(JSON_VALUE(payload_json, '$.deposit.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS deposit_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.depositApplied.value') AS NUMERIC) AS deposit_applied_amount,
    COALESCE(JSON_VALUE(payload_json, '$.depositApplied.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS deposit_applied_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.minutesLogged') AS INT64) AS minutes_logged,
    SAFE_CAST(JSON_VALUE(payload_json, '$.minutesToBeInvoiced') AS INT64) AS minutes_to_be_invoiced,
    source_deleted AS is_source_deleted,
    observed_at AS project_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `;
}

appendProjectObservation(
  "xero_project_observations",
  "projects_projects",
  ["generation", "tenant_id", "project_id", "source_version_id"],
  projectObservationProjection("project_observation_id"),
  "Append-only Xero Project master observations, including source-native operational and billing summary fields."
);

appendProjectObservation(
  "xero_project_detail_observations",
  "projects_project_details",
  ["generation", "tenant_id", "project_id", "source_version_id"],
  projectObservationProjection("project_detail_observation_id"),
  "Append-only direct Xero Project detail observations. They support same-run list/detail reconciliation and never determine project lifecycle."
);

appendProjectObservation(
  "xero_project_user_observations",
  "projects_users",
  ["generation", "tenant_id", "user_id", "source_version_id"],
  `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS project_user_observation_id,
    generation,
    tenant_id,
    JSON_VALUE(payload_json, '$.userId') AS user_id,
    JSON_VALUE(payload_json, '$.name') AS user_name,
    JSON_VALUE(payload_json, '$.email') AS user_email,
    observed_at AS project_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `,
  "Append-only Xero Projects user observations. Xero exposes a tenant user directory rather than a project-role bridge."
);

appendProjectObservation(
  "xero_project_task_observations",
  "projects_tasks",
  ["generation", "tenant_id", "project_id", "task_id", "source_version_id", "projection_schema_revision"],
  `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS project_task_observation_id,
    generation,
    2 AS projection_schema_revision,
    tenant_id,
    JSON_VALUE(payload_json, '$.projectId') AS project_id,
    JSON_VALUE(payload_json, '$.taskId') AS task_id,
    JSON_VALUE(payload_json, '$.name') AS task_name,
    JSON_VALUE(payload_json, '$.status') AS task_status,
    JSON_VALUE(payload_json, '$.chargeType') AS charge_type,
    SAFE_CAST(JSON_VALUE(payload_json, '$.rate.value') AS NUMERIC) AS rate_amount,
    COALESCE(JSON_VALUE(payload_json, '$.rate.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS rate_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalAmount.value') AS NUMERIC) AS total_amount,
    COALESCE(JSON_VALUE(payload_json, '$.totalAmount.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS total_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.amountInvoiced.value') AS NUMERIC) AS amount_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.amountInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS amount_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.amountToBeInvoiced.value') AS NUMERIC)
      AS amount_to_be_invoiced,
    COALESCE(JSON_VALUE(payload_json, '$.amountToBeInvoiced.currency'), JSON_VALUE(payload_json, '$.currencyCode'))
      AS amount_to_be_invoiced_currency_code,
    SAFE_CAST(JSON_VALUE(payload_json, '$.fixedMinutes') AS INT64) AS fixed_minutes,
    SAFE_CAST(JSON_VALUE(payload_json, '$.totalMinutes') AS INT64) AS total_minutes,
    SAFE_CAST(JSON_VALUE(payload_json, '$.minutesInvoiced') AS INT64) AS minutes_invoiced,
    SAFE_CAST(JSON_VALUE(payload_json, '$.minutesToBeInvoiced') AS INT64) AS minutes_to_be_invoiced,
    SAFE_CAST(JSON_VALUE(payload_json, '$.nonChargeableMinutes') AS INT64) AS non_chargeable_minutes,
    observed_at AS project_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `,
  "Append-only Xero Project task observations. Parent project identity remains source-native and is certified against the parent-scoped membership snapshot."
);

appendProjectObservation(
  "xero_project_task_detail_observations",
  "projects_task_details",
  ["generation", "tenant_id", "project_id", "task_id", "source_version_id", "projection_schema_revision"],
  projectObservationProjection("project_task_detail_observation_id"),
  "Append-only direct Xero Project task detail observations. They prove delivered task fields but never determine task lifecycle or list membership."
);

appendProjectObservation(
  "xero_project_time_entry_observations",
  "projects_time",
  ["generation", "tenant_id", "project_id", "time_entry_id", "source_version_id", "projection_schema_revision"],
  `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS project_time_entry_observation_id,
    generation,
    2 AS projection_schema_revision,
    tenant_id,
    JSON_VALUE(payload_json, '$.projectId') AS project_id,
    JSON_VALUE(payload_json, '$.taskId') AS task_id,
    JSON_VALUE(payload_json, '$.userId') AS user_id,
    JSON_VALUE(payload_json, '$.timeEntryId') AS time_entry_id,
    JSON_VALUE(payload_json, '$.status') AS time_entry_status,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.dateUtc'), 1, 10) AS DATE) AS entry_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.dateEnteredUtc') AS TIMESTAMP) AS entered_at,
    SAFE_CAST(JSON_VALUE(payload_json, '$.duration') AS INT64) AS duration_minutes,
    observed_at AS project_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `,
  "Append-only Xero Project time-entry observations. Duration is retained in native minutes and is not converted into a financial amount."
);

appendProjectObservation(
  "xero_project_time_entry_detail_observations",
  "projects_time_details",
  ["generation", "tenant_id", "project_id", "time_entry_id", "source_version_id", "projection_schema_revision"],
  projectObservationProjection("project_time_entry_detail_observation_id"),
  "Append-only direct Xero Project time-entry detail observations. They prove delivered fields but never determine time-entry lifecycle or list membership."
);

publish("xero_project_list_snapshot_observations", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  onSchemaChange: "FAIL",
  description: "Append-only list-membership observations emitted for Xero Projects list responses, including empty child lists that prove a parent collection was observed.",
  bigquery: {
    partitionBy: "DATE(snapshot_observed_at)",
    clusterBy: ["tenant_id", "source_run_id", "source_endpoint"],
  },
}).query(ctx => `
  WITH source AS (
    SELECT
      CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
        AS project_list_snapshot_observation_id,
      generation,
      tenant_id,
      run_id AS source_run_id,
      JSON_VALUE(payload_json, '$.source_endpoint') AS source_endpoint,
      JSON_VALUE(payload_json, '$.request_key') AS request_key,
      SAFE_CAST(JSON_VALUE(payload_json, '$.record_count') AS INT64) AS declared_record_count,
      ARRAY_LENGTH(JSON_QUERY_ARRAY(payload_json, '$.record_ids')) AS observed_member_count,
      observed_at AS snapshot_observed_at,
      record_id AS source_record_id,
      version_id AS source_version_id,
      receipt_id AS source_receipt_id,
      payload_json AS source_payload_json
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")}
    WHERE endpoint = 'projects_list_snapshots'
      AND JSON_VALUE(payload_json, '$.source_endpoint') IN (${projectEndpointList})
  )
  SELECT source.*
  FROM source
  WHERE source_run_id IS NOT NULL
    AND source_endpoint IS NOT NULL
    AND request_key IS NOT NULL
    AND source_version_id IS NOT NULL
  ${ctx.when(ctx.incremental(), `
    AND NOT EXISTS (
      SELECT 1
      FROM ${ctx.self()} AS prior
      WHERE prior.generation = source.generation
        AND prior.tenant_id = source.tenant_id
        AND prior.source_version_id = source.source_version_id
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY generation, tenant_id, source_version_id
    ORDER BY snapshot_observed_at DESC, source_receipt_id DESC
  ) = 1
`);

publish("xero_project_list_membership_observations", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  onSchemaChange: "FAIL",
  description: "Append-only native record memberships from Xero Projects list snapshots. Empty lists are represented by the companion snapshot table rather than fabricated member rows.",
  bigquery: {
    partitionBy: "DATE(snapshot_observed_at)",
    clusterBy: ["tenant_id", "source_run_id", "source_endpoint"],
  },
}).query(ctx => `
  WITH source AS (
    SELECT
      snapshot.generation,
      snapshot.tenant_id,
      snapshot.source_run_id,
      snapshot.source_endpoint,
      snapshot.request_key,
      JSON_VALUE(member) AS member_native_id,
      snapshot.snapshot_observed_at,
      snapshot.source_record_id AS snapshot_source_record_id,
      snapshot.source_version_id AS snapshot_source_version_id,
      snapshot.source_receipt_id AS snapshot_source_receipt_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_snapshot_observations")} AS snapshot,
         UNNEST(JSON_QUERY_ARRAY(snapshot.source_payload_json, '$.record_ids')) AS member
  )
  SELECT source.*
  FROM source
  WHERE member_native_id IS NOT NULL
  ${ctx.when(ctx.incremental(), `
    AND NOT EXISTS (
      SELECT 1
      FROM ${ctx.self()} AS prior
      WHERE prior.generation = source.generation
        AND prior.tenant_id = source.tenant_id
        AND prior.snapshot_source_version_id = source.snapshot_source_version_id
        AND prior.member_native_id = source.member_native_id
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY generation, tenant_id, snapshot_source_version_id, member_native_id
    ORDER BY snapshot_observed_at DESC, snapshot_source_receipt_id DESC
  ) = 1
`);

publish("xero_project_run_certification", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Per-run Projects completeness evidence. A collection is certified only when the run succeeded, every parent request has one complete snapshot, every membership record has a typed observation, and all counts reconcile exactly.",
}).query(ctx => `
  WITH source_outcomes AS (
    SELECT
      e.tenant_id,
      e.run_id AS source_run_id,
      e.endpoint AS source_endpoint,
      r.finished_at AS run_finished_at,
      r.status AS run_status,
      COUNT(*) AS request_count,
      COUNTIF(e.error_present OR e.termination != 'ended_on_empty_page') AS failed_or_partial_request_count,
      SUM(e.records) AS expected_record_count,
      FALSE AS is_synthetic_empty_child_endpoint
    FROM ${ctx.ref({ database: project, schema: stagingSchema, name: "sync_run_entities" })} AS e
    INNER JOIN ${ctx.ref({ database: project, schema: stagingSchema, name: "sync_runs" })} AS r
      USING (run_id)
    WHERE e.endpoint IN (${projectEndpointList})
    GROUP BY e.tenant_id, source_run_id, source_endpoint, run_finished_at, run_status
  ), snapshot_requests AS (
    SELECT
      tenant_id,
      source_run_id,
      source_endpoint,
      request_key,
      COUNT(*) AS snapshot_count,
      SUM(declared_record_count) AS declared_record_count,
      COUNTIF(declared_record_count IS NULL
              OR observed_member_count IS NULL
              OR declared_record_count != observed_member_count) AS invalid_snapshot_count
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_snapshot_observations")}
    GROUP BY tenant_id, source_run_id, source_endpoint, request_key
  ), snapshots AS (
    SELECT
      tenant_id,
      source_run_id,
      source_endpoint,
      SUM(snapshot_count) AS snapshot_count,
      SUM(declared_record_count) AS declared_record_count,
      SUM(invalid_snapshot_count) AS invalid_snapshot_count
    FROM snapshot_requests
    GROUP BY tenant_id, source_run_id, source_endpoint
  ), project_parent_receipts AS (
    SELECT
      tenant_id,
      run_id AS source_run_id,
      COUNT(*) AS projects_collection_receipt_count,
      SUM(record_count) AS projects_collection_record_count
    FROM ${ctx.ref({ database: project, schema: stagingSchema, name: "ingest_receipts" })}
    WHERE endpoint = 'projects_projects'
    GROUP BY tenant_id, source_run_id
  ), observation_members AS (
    SELECT DISTINCT generation, tenant_id, source_run_id,
           'projects_projects' AS source_endpoint, '' AS request_key, project_id AS member_native_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_observations")}
    UNION ALL
    SELECT DISTINCT generation, tenant_id, source_run_id,
           'projects_users' AS source_endpoint, '' AS request_key, user_id AS member_native_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_user_observations")}
    UNION ALL
    SELECT DISTINCT generation, tenant_id, source_run_id,
           'projects_tasks' AS source_endpoint, project_id AS request_key, task_id AS member_native_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_task_observations")}
    UNION ALL
    SELECT DISTINCT generation, tenant_id, source_run_id,
           'projects_time' AS source_endpoint, project_id AS request_key, time_entry_id AS member_native_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_time_entry_observations")}
  ), members AS (
    SELECT
      membership.tenant_id,
      membership.source_run_id,
      membership.source_endpoint,
      COUNT(*) AS member_count,
      COUNT(DISTINCT CONCAT(membership.request_key, '|', membership.member_native_id))
        AS distinct_member_count,
      COUNTIF(observed.member_native_id IS NOT NULL) AS materialized_member_count
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_membership_observations")} AS membership
    LEFT JOIN observation_members AS observed
      ON membership.generation = observed.generation
     AND membership.tenant_id = observed.tenant_id
     AND membership.source_run_id = observed.source_run_id
     AND membership.source_endpoint = observed.source_endpoint
     AND membership.request_key = observed.request_key
     AND membership.member_native_id = observed.member_native_id
    GROUP BY membership.tenant_id, membership.source_run_id, membership.source_endpoint
  ), project_parents AS (
    SELECT tenant_id, source_run_id, member_native_id AS project_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_membership_observations")}
    WHERE source_endpoint = 'projects_projects'
  ), parent_snapshots AS (
    SELECT
      parent.tenant_id,
      parent.source_run_id,
      endpoint AS source_endpoint,
      COUNT(*) AS parent_project_count,
      COUNTIF(snapshot.snapshot_count = 1
              AND snapshot.invalid_snapshot_count = 0) AS complete_parent_snapshot_count
    FROM project_parents AS parent
    CROSS JOIN UNNEST(['projects_tasks', 'projects_time']) AS endpoint
    LEFT JOIN snapshot_requests AS snapshot
      ON parent.tenant_id = snapshot.tenant_id
     AND parent.source_run_id = snapshot.source_run_id
     AND endpoint = snapshot.source_endpoint
     AND parent.project_id = snapshot.request_key
    GROUP BY parent.tenant_id, parent.source_run_id, endpoint
  ), child_request_sets AS (
    SELECT
      snapshot.tenant_id,
      snapshot.source_run_id,
      snapshot.source_endpoint,
      COUNT(*) AS child_snapshot_request_key_count,
      COUNTIF(parent.project_id IS NULL) AS unexpected_child_request_key_count
    FROM snapshot_requests AS snapshot
    LEFT JOIN project_parents AS parent
      ON snapshot.tenant_id = parent.tenant_id
     AND snapshot.source_run_id = parent.source_run_id
     AND snapshot.request_key = parent.project_id
    WHERE snapshot.source_endpoint IN ('projects_tasks', 'projects_time')
    GROUP BY snapshot.tenant_id, snapshot.source_run_id, snapshot.source_endpoint
  ), empty_project_child_endpoints AS (
    SELECT
      parent.tenant_id,
      parent.source_run_id,
      endpoint AS source_endpoint,
      parent.run_finished_at,
      parent.run_status,
      0 AS request_count,
      0 AS failed_or_partial_request_count,
      0 AS expected_record_count,
      TRUE AS is_synthetic_empty_child_endpoint
    FROM source_outcomes AS parent
    CROSS JOIN UNNEST(['projects_tasks', 'projects_time']) AS endpoint
    LEFT JOIN snapshots AS parent_snapshot
      ON parent.tenant_id = parent_snapshot.tenant_id
     AND parent.source_run_id = parent_snapshot.source_run_id
     AND parent_snapshot.source_endpoint = 'projects_projects'
    LEFT JOIN members AS parent_member
      ON parent.tenant_id = parent_member.tenant_id
     AND parent.source_run_id = parent_member.source_run_id
     AND parent_member.source_endpoint = 'projects_projects'
    LEFT JOIN project_parent_receipts AS parent_receipt
      ON parent.tenant_id = parent_receipt.tenant_id
     AND parent.source_run_id = parent_receipt.source_run_id
    WHERE parent.source_endpoint = 'projects_projects'
      AND parent.run_status = 'succeeded'
      AND parent.failed_or_partial_request_count = 0
      AND parent.expected_record_count = 0
      AND COALESCE(parent_snapshot.snapshot_count, 0) > 0
      AND COALESCE(parent_snapshot.declared_record_count, 0) = 0
      AND COALESCE(parent_snapshot.invalid_snapshot_count, 0) = 0
      AND COALESCE(parent_member.member_count, 0) = 0
      AND COALESCE(parent_member.distinct_member_count, 0) = 0
      AND COALESCE(parent_member.materialized_member_count, 0) = 0
      AND parent.request_count = 1
      AND COALESCE(parent_receipt.projects_collection_receipt_count, 0) = 1
      AND COALESCE(parent_receipt.projects_collection_record_count, 0) = 0
      AND NOT EXISTS (
        SELECT 1
        FROM source_outcomes AS child
        WHERE child.tenant_id = parent.tenant_id
          AND child.source_run_id = parent.source_run_id
          AND child.source_endpoint = endpoint
      )
  ), outcomes AS (
    SELECT *
    FROM source_outcomes
    UNION ALL
    SELECT *
    FROM empty_project_child_endpoints
  )
  SELECT
    outcome.tenant_id,
    outcome.source_run_id,
    outcome.source_endpoint,
    outcome.run_finished_at,
    outcome.run_status,
    outcome.request_count,
    outcome.failed_or_partial_request_count,
    outcome.expected_record_count,
    outcome.is_synthetic_empty_child_endpoint,
    COALESCE(snapshot.snapshot_count, 0) AS snapshot_count,
    COALESCE(snapshot.declared_record_count, 0) AS declared_record_count,
    COALESCE(snapshot.invalid_snapshot_count, 0) AS invalid_snapshot_count,
    COALESCE(member.member_count, 0) AS member_count,
    COALESCE(member.distinct_member_count, 0) AS distinct_member_count,
    COALESCE(member.materialized_member_count, 0) AS materialized_member_count,
    COALESCE(parent.parent_project_count, 0) AS parent_project_count,
    COALESCE(parent.complete_parent_snapshot_count, 0) AS complete_parent_snapshot_count,
    COALESCE(child.child_snapshot_request_key_count, 0) AS child_snapshot_request_key_count,
    COALESCE(child.unexpected_child_request_key_count, 0) AS unexpected_child_request_key_count,
    outcome.expected_record_count = COALESCE(snapshot.declared_record_count, 0)
      AND outcome.expected_record_count = COALESCE(member.member_count, 0)
      AND outcome.expected_record_count = COALESCE(member.distinct_member_count, 0)
      AND COALESCE(snapshot.invalid_snapshot_count, 0) = 0 AS is_member_count_reconciled,
    outcome.expected_record_count = COALESCE(member.materialized_member_count, 0)
      AS is_source_observation_reconciled,
    CASE
      WHEN outcome.source_endpoint IN ('projects_tasks', 'projects_time') THEN
        COALESCE(parent.parent_project_count, 0) = COALESCE(parent.complete_parent_snapshot_count, 0)
      ELSE TRUE
    END AS is_parent_snapshot_complete,
    CASE
      WHEN outcome.source_endpoint IN ('projects_tasks', 'projects_time') THEN
        outcome.request_count = COALESCE(parent.parent_project_count, 0)
          AND COALESCE(child.child_snapshot_request_key_count, 0)
            = COALESCE(parent.parent_project_count, 0)
          AND COALESCE(child.unexpected_child_request_key_count, 0) = 0
      ELSE TRUE
    END AS is_child_request_set_reconciled,
    outcome.run_status = 'succeeded'
      AND outcome.failed_or_partial_request_count = 0
      AND (
        COALESCE(snapshot.snapshot_count, 0) > 0
        OR outcome.is_synthetic_empty_child_endpoint
      )
      AND outcome.expected_record_count = COALESCE(snapshot.declared_record_count, 0)
      AND outcome.expected_record_count = COALESCE(member.member_count, 0)
      AND outcome.expected_record_count = COALESCE(member.distinct_member_count, 0)
      AND outcome.expected_record_count = COALESCE(member.materialized_member_count, 0)
      AND COALESCE(snapshot.invalid_snapshot_count, 0) = 0
      AND (
        outcome.source_endpoint NOT IN ('projects_tasks', 'projects_time')
        OR (
          COALESCE(parent.parent_project_count, 0) = COALESCE(parent.complete_parent_snapshot_count, 0)
          AND outcome.request_count = COALESCE(parent.parent_project_count, 0)
          AND COALESCE(child.child_snapshot_request_key_count, 0)
            = COALESCE(parent.parent_project_count, 0)
          AND COALESCE(child.unexpected_child_request_key_count, 0) = 0
        )
      ) AS is_certified,
    CASE
      WHEN outcome.run_status != 'succeeded' THEN 'run_not_succeeded'
      WHEN outcome.failed_or_partial_request_count > 0 THEN 'source_request_failed_or_partial'
      WHEN NOT outcome.is_synthetic_empty_child_endpoint
        AND COALESCE(snapshot.snapshot_count, 0) = 0 THEN 'missing_list_snapshot'
      WHEN COALESCE(snapshot.invalid_snapshot_count, 0) > 0 THEN 'invalid_list_snapshot'
      WHEN outcome.source_endpoint IN ('projects_tasks', 'projects_time')
        AND COALESCE(parent.parent_project_count, 0) != COALESCE(parent.complete_parent_snapshot_count, 0)
        THEN 'missing_or_duplicate_parent_snapshot'
      WHEN outcome.source_endpoint IN ('projects_tasks', 'projects_time')
        AND (
          outcome.request_count != COALESCE(parent.parent_project_count, 0)
          OR COALESCE(child.child_snapshot_request_key_count, 0)
            != COALESCE(parent.parent_project_count, 0)
          OR COALESCE(child.unexpected_child_request_key_count, 0) > 0
        )
        THEN 'child_request_set_mismatch'
      WHEN outcome.expected_record_count != COALESCE(snapshot.declared_record_count, 0)
        OR outcome.expected_record_count != COALESCE(member.member_count, 0)
        OR outcome.expected_record_count != COALESCE(member.distinct_member_count, 0)
        THEN 'membership_count_mismatch'
      WHEN outcome.expected_record_count != COALESCE(member.materialized_member_count, 0)
        THEN 'raw_observation_missing'
      WHEN outcome.is_synthetic_empty_child_endpoint
        THEN 'certified_empty_parent_collection'
      ELSE 'certified'
    END AS certification_state
  FROM outcomes AS outcome
  LEFT JOIN snapshots AS snapshot
    USING (tenant_id, source_run_id, source_endpoint)
  LEFT JOIN members AS member
    USING (tenant_id, source_run_id, source_endpoint)
  LEFT JOIN parent_snapshots AS parent
    USING (tenant_id, source_run_id, source_endpoint)
  LEFT JOIN child_request_sets AS child
    USING (tenant_id, source_run_id, source_endpoint)
`);

publish("xero_project_complete_runs", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Every tenant Xero Projects run whose four required collection endpoints are independently certified. This is lifecycle evidence, not an overwrite of raw history.",
}).query(ctx => `
  WITH required_endpoints AS (
    SELECT endpoint AS source_endpoint
    FROM UNNEST([${projectEndpointList}]) AS endpoint
  )
  SELECT certification.tenant_id,
         certification.source_run_id,
         MAX(certification.run_finished_at) AS run_finished_at,
         COUNT(DISTINCT certification.source_endpoint) AS certified_endpoint_count
  FROM ${ctx.ref("dw_bronze_xero", "xero_project_run_certification")} AS certification
  INNER JOIN required_endpoints
    USING (source_endpoint)
  WHERE certification.is_certified
  GROUP BY certification.tenant_id, certification.source_run_id
  HAVING certified_endpoint_count = (SELECT COUNT(*) FROM required_endpoints)
`);

publish("xero_project_current_run", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Latest fully certified Xero Projects run per tenant. Incomplete or quota-truncated source runs never become the current Projects business state.",
}).query(ctx => `
  SELECT tenant_id, source_run_id, run_finished_at,
         TRUE AS is_current_certified_run
  FROM ${ctx.ref("dw_bronze_xero", "xero_project_complete_runs")}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY tenant_id
    ORDER BY run_finished_at DESC, source_run_id DESC
  ) = 1
`);

publish("xero_project_entity_lifecycle", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Current and removed lifecycle state for Projects entities, derived only from complete certified membership snapshots. Historical entities are retained and marked removed rather than deleted.",
}).query(ctx => `
  WITH history AS (
    SELECT membership.tenant_id,
           membership.source_run_id,
           complete.run_finished_at,
           'project' AS entity_type,
           membership.member_native_id AS project_id,
           membership.member_native_id AS entity_id,
           membership.snapshot_observed_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_membership_observations")} AS membership
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_project_complete_runs")} AS complete
      ON membership.tenant_id = complete.tenant_id
     AND membership.source_run_id = complete.source_run_id
    WHERE membership.source_endpoint = 'projects_projects'
    UNION ALL
    SELECT membership.tenant_id,
           membership.source_run_id,
           complete.run_finished_at,
           'user' AS entity_type,
           CAST(NULL AS STRING) AS project_id,
           membership.member_native_id AS entity_id,
           membership.snapshot_observed_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_membership_observations")} AS membership
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_project_complete_runs")} AS complete
      ON membership.tenant_id = complete.tenant_id
     AND membership.source_run_id = complete.source_run_id
    WHERE membership.source_endpoint = 'projects_users'
    UNION ALL
    SELECT membership.tenant_id,
           membership.source_run_id,
           complete.run_finished_at,
           'task' AS entity_type,
           membership.request_key AS project_id,
           membership.member_native_id AS entity_id,
           membership.snapshot_observed_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_membership_observations")} AS membership
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_project_complete_runs")} AS complete
      ON membership.tenant_id = complete.tenant_id
     AND membership.source_run_id = complete.source_run_id
    WHERE membership.source_endpoint = 'projects_tasks'
    UNION ALL
    SELECT membership.tenant_id,
           membership.source_run_id,
           complete.run_finished_at,
           'time_entry' AS entity_type,
           membership.request_key AS project_id,
           membership.member_native_id AS entity_id,
           membership.snapshot_observed_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_list_membership_observations")} AS membership
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_project_complete_runs")} AS complete
      ON membership.tenant_id = complete.tenant_id
     AND membership.source_run_id = complete.source_run_id
    WHERE membership.source_endpoint = 'projects_time'
  ), ranked AS (
    SELECT history.*,
           MIN(snapshot_observed_at) OVER (
             PARTITION BY tenant_id, entity_type, project_id, entity_id
           ) AS first_seen_at,
           ROW_NUMBER() OVER (
             PARTITION BY tenant_id, entity_type, project_id, entity_id
             ORDER BY run_finished_at DESC, source_run_id DESC, snapshot_observed_at DESC
           ) AS latest_rank
    FROM history
  ), latest AS (
    SELECT * FROM ranked WHERE latest_rank = 1
  ), current_members AS (
    SELECT history.tenant_id, history.entity_type, history.project_id, history.entity_id
    FROM history
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_project_current_run")} AS current_run
      ON history.tenant_id = current_run.tenant_id
     AND history.source_run_id = current_run.source_run_id
  )
  SELECT latest.tenant_id,
         latest.entity_type,
         latest.project_id,
         latest.entity_id,
         latest.first_seen_at,
         latest.snapshot_observed_at AS last_seen_at,
         latest.source_run_id AS last_seen_source_run_id,
         current_run.source_run_id AS current_certified_run_id,
         current_run.run_finished_at AS current_run_finished_at,
         current_members.entity_id IS NOT NULL AS is_current,
         current_run.source_run_id IS NOT NULL AND current_members.entity_id IS NULL AS is_removed,
         IF(current_run.source_run_id IS NOT NULL AND current_members.entity_id IS NULL,
            current_run.source_run_id, NULL) AS removed_in_run_id,
         IF(current_run.source_run_id IS NOT NULL AND current_members.entity_id IS NULL,
            current_run.run_finished_at, NULL) AS removed_observed_at,
         CASE
           WHEN current_members.entity_id IS NOT NULL THEN 'current'
           WHEN current_run.source_run_id IS NOT NULL THEN 'removed'
           ELSE 'unverified'
         END AS record_state
  FROM latest
  LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_project_current_run")} AS current_run
    ON latest.tenant_id = current_run.tenant_id
  LEFT JOIN current_members
    ON latest.tenant_id = current_members.tenant_id
   AND latest.entity_type = current_members.entity_type
   AND latest.project_id IS NOT DISTINCT FROM current_members.project_id
   AND latest.entity_id = current_members.entity_id
`);

assert("xero_project_run_certification_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, source_run_id, source_endpoint
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_run_certification")}
    GROUP BY 1, 2, 3
    HAVING tenant_id IS NULL OR source_run_id IS NULL OR source_endpoint IS NULL OR COUNT(*) != 1
  `);

assert("xero_project_current_run_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_current_run")}
    GROUP BY tenant_id
    HAVING tenant_id IS NULL OR COUNT(*) != 1
  `);

assert("xero_project_membership_reconciliation")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, source_run_id, source_endpoint, certification_state
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_run_certification")}
    WHERE is_certified
      AND (NOT is_member_count_reconciled
           OR NOT is_source_observation_reconciled
           OR NOT is_parent_snapshot_complete
           OR NOT is_child_request_set_reconciled)
  `);
