const project = dataform.projectConfig.defaultDatabase;
const stagingSchema = "dw_staging_xero";

const sources = {
  records: { database: project, schema: stagingSchema, name: "raw_records" },
  collections: { database: project, schema: stagingSchema, name: "raw_collections" },
  children: { database: project, schema: stagingSchema, name: "raw_children" },
  receipts: { database: project, schema: stagingSchema, name: "ingest_receipts" },
};

for (const source of Object.values(sources)) {
  declare(source);
}

const recordIdentity = ["generation", "tenant_id", "endpoint", "record_id", "version_id"];
const collectionIdentity = [...recordIdentity, "collection_name"];
const childIdentity = [...collectionIdentity, "child_key"];
const recordColumns = [
  "generation", "tenant_id", "endpoint", "record_id", "version_id", "observed_at",
  "source_updated_at", "run_id", "receipt_id", "source_status", "source_deleted",
  "payload_hash", "payload_json",
];
const receiptColumns = [
  "receipt_id", "generation", "tenant_id", "endpoint", "run_id", "observed_at",
  "source_bucket", "source_object", "meta_generation", "data_generation", "http_status",
  "expected_records", "record_count", "child_count", "metadata_json",
];
const collectionColumns = [
  "generation", "tenant_id", "endpoint", "record_id", "version_id", "observed_at",
  "source_updated_at", "collection_name", "child_count",
];
const childColumns = [
  "generation", "tenant_id", "endpoint", "record_id", "version_id", "collection_name",
  "child_key", "line_position", "native_id", "payload_hash", "payload_json",
];

function appendOnlyHistory(name, source, identity, options = {}) {
  const partitionBy = options.partitionBy;
  const clusterBy = options.clusterBy || ["tenant_id", "endpoint"];
  publish(name, {
    type: "incremental",
    schema: "dw_bronze_xero",
    protected: true,
    description: options.description,
    bigquery: {
      ...(partitionBy ? { partitionBy } : {}),
      clusterBy,
    },
  }).query(ctx => `
    WITH source AS (
      SELECT *
      FROM ${ctx.ref(source)}
      WHERE ${identity.map(field => `\`${field}\` IS NOT NULL`).join(" AND ")}
    )
    SELECT ${options.columns.map(field => `source.\`${field}\` AS \`${field}\``).join(",\n           ")}
    FROM source
    ${ctx.when(ctx.incremental(), `
      WHERE NOT EXISTS (
        SELECT 1
        FROM ${ctx.self()} AS prior
        WHERE ${identity.map(field => `prior.\`${field}\` = source.\`${field}\``).join(" AND ")}
      )
    `)}
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY ${identity.map(field => `\`${field}\``).join(", ")}
      ORDER BY ${options.orderBy || "observed_at DESC"}
    ) = 1
  `);
}

appendOnlyHistory("xero_raw_record_history", sources.records, recordIdentity, {
  partitionBy: "DATE(observed_at)",
  clusterBy: ["tenant_id", "endpoint", "record_id"],
  description: "Immutable Xero response records, including original JSON payload, source lifecycle state, receipt, run, and observation provenance.",
  orderBy: "observed_at DESC, receipt_id DESC",
  columns: recordColumns,
});

publish("xero_raw_record_source_duplicates", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Auditable duplicate physical raw-record rows. Exact historical replays are retained and marked; conflicting rows with the same canonical version fail the Bronze safety assertion.",
  bigquery: {
    clusterBy: ["tenant_id", "endpoint", "record_id"],
  },
}).query(ctx => `
  SELECT generation, tenant_id, endpoint, record_id, version_id,
         COUNT(*) AS source_duplicate_row_count,
         COUNT(DISTINCT receipt_id) AS distinct_receipt_count,
         COUNT(DISTINCT TO_JSON_STRING(STRUCT(
           observed_at, source_updated_at, run_id, receipt_id, source_status,
           source_deleted, payload_hash, payload_json
         ))) AS distinct_observation_count,
         COUNT(DISTINCT TO_JSON_STRING(STRUCT(
           observed_at, source_updated_at, run_id, receipt_id, source_status,
           source_deleted, payload_hash, payload_json
         ))) = 1 AS is_exact_replay_duplicate
  FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history")}
  GROUP BY 1, 2, 3, 4, 5
  HAVING COUNT(*) > 1
`);

publish("xero_raw_record_history_canonical", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "One canonical raw record per immutable version. Physical replay duplicates remain visible in xero_raw_record_source_duplicates and never enter downstream Xero models.",
}).query(ctx => `
  SELECT ${recordColumns.map(field => `ranked.\`${field}\` AS \`${field}\``).join(",\n         ")},
         COALESCE(duplicate.source_duplicate_row_count, 1) AS source_duplicate_row_count,
         COALESCE(duplicate.is_exact_replay_duplicate, FALSE) AS is_exact_replay_duplicate
  FROM (
    SELECT history.*,
           ROW_NUMBER() OVER (
             PARTITION BY ${recordIdentity.map(field => `history.\`${field}\``).join(", ")}
             ORDER BY history.observed_at DESC, history.receipt_id DESC
           ) AS canonical_rank
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history")} AS history
  ) AS ranked
  LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_record_source_duplicates")} AS duplicate
    USING (generation, tenant_id, endpoint, record_id, version_id)
  WHERE ranked.canonical_rank = 1
`);

appendOnlyHistory("xero_raw_receipt_history", sources.receipts, ["receipt_id"], {
  partitionBy: "DATE(observed_at)",
  clusterBy: ["tenant_id", "endpoint", "run_id"],
  description: "Immutable Xero ingestion receipts. A receipt pins the GCS object and its observed response before any typed transformation.",
  orderBy: "observed_at DESC",
  columns: receiptColumns,
});

appendOnlyHistory("xero_raw_collection_history", sources.collections, collectionIdentity, {
  partitionBy: "DATE(observed_at)",
  clusterBy: ["tenant_id", "endpoint", "record_id"],
  description: "Immutable observed Xero child-collection cardinalities for each parent response version.",
  orderBy: "observed_at DESC",
  columns: collectionColumns,
});

publish("xero_raw_child_history", {
  type: "incremental",
  schema: "dw_bronze_xero",
  protected: true,
  description: "Immutable Xero child payload history, enriched with parent response and receipt provenance. Source position is retained when Xero has no stable child identifier.",
  bigquery: {
    partitionBy: "DATE(observed_at)",
    clusterBy: ["tenant_id", "endpoint", "record_id"],
  },
}).query(ctx => `
  WITH source AS (
    SELECT ${childColumns.map(field => `child.\`${field}\` AS \`${field}\``).join(",\n           ")},
           parent.observed_at,
           parent.source_updated_at,
           parent.run_id,
           parent.receipt_id,
           parent.source_status,
           parent.source_deleted AS parent_source_deleted
    FROM ${ctx.ref(sources.children)} AS child
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")} AS parent
      USING (generation, tenant_id, endpoint, record_id, version_id)
    WHERE ${childIdentity.map(field => `child.\`${field}\` IS NOT NULL`).join(" AND ")}
  )
  SELECT source.*
  FROM source
  ${ctx.when(ctx.incremental(), `
    WHERE NOT EXISTS (
      SELECT 1
      FROM ${ctx.self()} AS prior
      WHERE ${childIdentity.map(field => `prior.\`${field}\` = source.\`${field}\``).join(" AND ")}
    )
  `)}
  QUALIFY ROW_NUMBER() OVER (
    PARTITION BY ${childIdentity.map(field => `\`${field}\``).join(", ")}
    ORDER BY observed_at DESC, receipt_id DESC
  ) = 1
`);

publish("xero_raw_record_current", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Latest observed raw Xero response per tenant, endpoint, and source record. source_deleted reflects Xero's documented lifecycle field; absence from a later response is not inferred as deletion.",
}).query(ctx => `
  SELECT ${recordColumns.map(field => `history.\`${field}\` AS \`${field}\``).join(",\n         ")},
         TRUE AS is_latest_observation
  FROM (
    SELECT history.*,
           ROW_NUMBER() OVER (
             PARTITION BY tenant_id, endpoint, record_id
             ORDER BY observed_at DESC, receipt_id DESC, version_id DESC
           ) AS observation_rank
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")} AS history
  ) AS history
  WHERE observation_rank = 1
`);

publish("xero_raw_direct_response_history", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Immutable Xero Accounting GET response history. The canonical parent identity is derived from the pinned raw-receipt metadata, preserving early loader rows that used a run-scoped record ID without rewriting raw history.",
}).query(ctx => `
  WITH direct_records AS (
    SELECT record.* EXCEPT(record_id),
           record.record_id AS loader_record_id,
           JSON_VALUE(receipt.metadata_json, '$."x-operation-id"') AS operation_id,
           JSON_VALUE(receipt.metadata_json, '$."x-parent-record-id"') AS parent_record_id,
           JSON_VALUE(receipt.metadata_json, '$."x-source-endpoint"') AS source_endpoint
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")} AS record
    INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_receipt_history")} AS receipt
      ON record.receipt_id = receipt.receipt_id
    WHERE STARTS_WITH(record.endpoint, 'accounting_get_')
  )
  SELECT *,
         operation_id IS NOT NULL AND parent_record_id IS NOT NULL AS is_parent_scoped,
         COALESCE(
           IF(operation_id IS NOT NULL AND parent_record_id IS NOT NULL,
              CONCAT(operation_id, ':', parent_record_id), NULL),
           loader_record_id
         ) AS canonical_record_id
  FROM direct_records
`);

publish("xero_raw_direct_response_current", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Latest parent-scoped direct Accounting GET response per tenant and documented Xero operation. Unscoped historical direct requests remain visible only in direct-response history.",
}).query(ctx => `
  SELECT * EXCEPT(observation_rank),
         TRUE AS is_latest_observation
  FROM (
    SELECT history.*,
           ROW_NUMBER() OVER (
             PARTITION BY tenant_id, operation_id, parent_record_id
             ORDER BY observed_at DESC, receipt_id DESC, version_id DESC
           ) AS observation_rank
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_history")} AS history
    WHERE is_parent_scoped
  )
  WHERE observation_rank = 1
`);

publish("xero_raw_collection_current", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Child collections observed on the latest raw parent version. An omitted collection is not treated as an empty or deleted collection.",
}).query(ctx => `
  SELECT ${collectionColumns.map(field => `collection.\`${field}\` AS \`${field}\``).join(",\n         ")},
         COALESCE(parent.source_deleted, FALSE) AS parent_source_deleted,
         NOT COALESCE(parent.source_deleted, FALSE) AS is_current,
         FALSE AS is_superseded,
         COALESCE(parent.source_deleted, FALSE) AS is_deleted,
         CASE
           WHEN parent.source_deleted THEN 'source_deleted'
           ELSE 'current'
         END AS record_state
  FROM ${ctx.ref("dw_bronze_xero", "xero_raw_collection_history")} AS collection
  INNER JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_record_current")} AS parent
    USING (generation, tenant_id, endpoint, record_id, version_id)
`);

publish("xero_raw_child_current", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Raw Xero child lifecycle. Rows are retained historically and marked removed only when a later complete parent collection proves a stable child ID is absent.",
}).query(ctx => `
  WITH current_parent AS (
    SELECT *
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_current")}
  ), current_collection AS (
    SELECT *
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_collection_current")}
    WHERE is_current
  ), current_membership AS (
    SELECT DISTINCT child.generation, child.tenant_id, child.endpoint,
           child.record_id, child.version_id, child.collection_name, child.child_key
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_child_history")} AS child
    INNER JOIN current_collection AS collection
      USING (generation, tenant_id, endpoint, record_id, version_id, collection_name)
  ), classified AS (
    SELECT child.*,
           COALESCE(parent.source_deleted, FALSE) AS current_parent_source_deleted,
           collection.version_id AS current_collection_version_id,
           membership.child_key AS current_membership_key,
           child.native_id IS NOT NULL
             AND NOT STARTS_WITH(child.child_key, 'position:') AS is_membership_identity_stable
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_child_history")} AS child
    LEFT JOIN current_parent AS parent
      ON child.generation = parent.generation
     AND child.tenant_id = parent.tenant_id
     AND child.endpoint = parent.endpoint
     AND child.record_id = parent.record_id
    LEFT JOIN current_collection AS collection
      ON child.generation = collection.generation
     AND child.tenant_id = collection.tenant_id
     AND child.endpoint = collection.endpoint
     AND child.record_id = collection.record_id
     AND child.collection_name = collection.collection_name
    LEFT JOIN current_membership AS membership
      ON child.generation = membership.generation
     AND child.tenant_id = membership.tenant_id
     AND child.endpoint = membership.endpoint
     AND child.record_id = membership.record_id
     AND child.collection_name = membership.collection_name
     AND child.child_key = membership.child_key
  )
  SELECT ${childColumns.map(field => `child.\`${field}\` AS \`${field}\``).join(",\n         ")},
         child.current_parent_source_deleted AS parent_source_deleted,
         child.is_membership_identity_stable,
         child.current_collection_version_id IS NOT NULL
           AND child.current_membership_key IS NULL AS is_missing_from_current_collection,
         NOT child.current_parent_source_deleted
           AND child.is_membership_identity_stable
           AND child.current_collection_version_id IS NOT NULL
           AND child.current_membership_key IS NULL AS is_removed,
         NOT child.current_parent_source_deleted
           AND child.version_id = child.current_collection_version_id
           AND child.current_membership_key IS NOT NULL AS is_current,
         child.current_collection_version_id IS NOT NULL
           AND child.version_id != child.current_collection_version_id AS is_superseded,
         child.current_parent_source_deleted
           OR (
             NOT child.current_parent_source_deleted
             AND child.is_membership_identity_stable
             AND child.current_collection_version_id IS NOT NULL
             AND child.current_membership_key IS NULL
           ) AS is_deleted,
         CASE
           WHEN child.current_parent_source_deleted THEN 'source_deleted'
           WHEN child.is_membership_identity_stable
             AND child.current_collection_version_id IS NOT NULL
             AND child.current_membership_key IS NULL THEN 'removed'
           WHEN child.version_id = child.current_collection_version_id
             AND child.current_membership_key IS NOT NULL THEN 'current'
           WHEN child.current_collection_version_id IS NULL THEN 'unverified'
           ELSE 'superseded'
         END AS record_state,
         CASE
           WHEN child.current_parent_source_deleted THEN 'source_deleted'
           WHEN child.is_membership_identity_stable
             AND child.current_collection_version_id IS NOT NULL
             AND child.current_membership_key IS NULL THEN 'missing_from_complete_parent'
           WHEN child.current_collection_version_id IS NOT NULL
             AND child.current_membership_key IS NULL THEN 'unstable_child_identity'
           ELSE NULL
         END AS deletion_reason
  FROM classified AS child
`);

publish("xero_raw_ingestion_health", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Tenant and endpoint health for immutable raw Xero observations. A raw receipt is evidence of delivery, not evidence that all historical Xero data has been extracted.",
}).query(ctx => `
  WITH receipts AS (
    SELECT tenant_id, endpoint,
           COUNT(*) AS receipt_count,
           COUNTIF(http_status = 304) AS not_modified_receipt_count,
           MAX(observed_at) AS latest_receipt_at,
           MAX(IF(http_status = 200, observed_at, NULL)) AS latest_successful_receipt_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_receipt_history")}
    WHERE tenant_id != 'run' AND endpoint != 'sync_manifest'
    GROUP BY tenant_id, endpoint
  ), records AS (
    SELECT tenant_id, endpoint,
           COUNT(*) AS record_version_count,
           COUNT(DISTINCT record_id) AS distinct_record_count,
           COUNTIF(source_deleted) AS deleted_record_version_count,
           MAX(observed_at) AS latest_record_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")}
    GROUP BY tenant_id, endpoint
  )
  SELECT receipts.tenant_id,
         receipts.endpoint,
         receipts.receipt_count,
         receipts.not_modified_receipt_count,
         receipts.latest_receipt_at,
         receipts.latest_successful_receipt_at,
         COALESCE(records.record_version_count, 0) AS record_version_count,
         COALESCE(records.distinct_record_count, 0) AS distinct_record_count,
         COALESCE(records.deleted_record_version_count, 0) AS deleted_record_version_count,
         records.latest_record_at,
         receipts.receipt_count > 0 AS is_raw_history_active
  FROM receipts
  LEFT JOIN records USING (tenant_id, endpoint)
`);

publish("xero_raw_run_reconciliation", {
  type: "view",
  schema: "dw_bronze_xero",
  description: "Per-run Xero delivery reconciliation. A successful manifest is not complete until its raw page receipts and raw record counts match every tenant/endpoint outcome.",
}).query(ctx => `
  WITH outcome_rollup AS (
    SELECT run_id, tenant_id, endpoint,
           COUNT(*) AS outcome_count,
           SUM(pages) AS expected_page_count,
           SUM(records) AS expected_record_count,
           IF(COUNT(DISTINCT termination) = 1, ANY_VALUE(termination), 'mixed') AS termination,
           LOGICAL_OR(error_present) AS error_present,
           MAX(synced_at) AS synced_at
    FROM ${ctx.ref("dw_staging_xero", "sync_run_entities")}
    GROUP BY run_id, tenant_id, endpoint
  ), receipts AS (
    SELECT run_id, tenant_id, endpoint,
           COUNT(*) AS received_page_count,
           SUM(record_count) AS received_record_count,
           SUM(expected_records) AS declared_record_count,
           MAX(observed_at) AS latest_receipt_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_receipt_history")}
    WHERE tenant_id != 'run' AND endpoint != 'sync_manifest'
    GROUP BY run_id, tenant_id, endpoint
  ), records AS (
    SELECT run_id, tenant_id, endpoint, COUNT(*) AS raw_record_count,
           MAX(observed_at) AS latest_record_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")}
    GROUP BY run_id, tenant_id, endpoint
  )
  SELECT outcome.run_id, outcome.tenant_id, outcome.endpoint,
         outcome.outcome_count,
         outcome.expected_page_count, outcome.expected_record_count,
         outcome.termination, outcome.error_present, outcome.synced_at,
         COALESCE(receipt.received_page_count, 0) AS received_page_count,
         COALESCE(receipt.received_record_count, 0) AS received_record_count,
         COALESCE(receipt.declared_record_count, 0) AS declared_record_count,
         COALESCE(record.raw_record_count, 0) AS raw_record_count,
         receipt.latest_receipt_at, record.latest_record_at,
         NOT outcome.error_present
           AND outcome.expected_page_count = COALESCE(receipt.received_page_count, 0)
           AND outcome.expected_record_count = COALESCE(receipt.received_record_count, 0)
           AND outcome.expected_record_count = COALESCE(receipt.declared_record_count, 0)
           AND outcome.expected_record_count = COALESCE(record.raw_record_count, 0)
           AS is_complete,
         CASE
           WHEN outcome.error_present THEN 'source_error'
           WHEN COALESCE(receipt.received_page_count, 0) < outcome.expected_page_count
             OR COALESCE(receipt.received_record_count, 0) < outcome.expected_record_count
             OR COALESCE(record.raw_record_count, 0) < outcome.expected_record_count
             THEN 'awaiting_raw_delivery'
           WHEN outcome.expected_page_count = COALESCE(receipt.received_page_count, 0)
             AND outcome.expected_record_count = COALESCE(receipt.received_record_count, 0)
             AND outcome.expected_record_count = COALESCE(receipt.declared_record_count, 0)
             AND outcome.expected_record_count = COALESCE(record.raw_record_count, 0)
             THEN 'complete'
           ELSE 'reconciliation_failed'
         END AS reconciliation_state
  FROM outcome_rollup AS outcome
  LEFT JOIN receipts AS receipt
    USING (run_id, tenant_id, endpoint)
  LEFT JOIN records AS record
    USING (run_id, tenant_id, endpoint)
`);

assert("xero_raw_record_history_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT generation, tenant_id, endpoint, record_id, version_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")}
    GROUP BY 1, 2, 3, 4, 5
    HAVING COUNT(*) > 1
       OR COUNTIF(generation IS NULL OR tenant_id IS NULL OR endpoint IS NULL
                  OR record_id IS NULL OR version_id IS NULL) > 0
  `);

assert("xero_raw_record_receipt_provenance")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT record.tenant_id, record.endpoint, record.record_id, record.version_id, record.receipt_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")} AS record
    LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_receipt_history")} AS receipt
      ON record.receipt_id = receipt.receipt_id
    WHERE receipt.receipt_id IS NULL
       OR record.generation IS DISTINCT FROM receipt.generation
       OR record.tenant_id IS DISTINCT FROM receipt.tenant_id
       OR record.endpoint IS DISTINCT FROM receipt.endpoint
       OR record.run_id IS DISTINCT FROM receipt.run_id
       OR record.observed_at IS DISTINCT FROM receipt.observed_at
  `);

assert("xero_raw_direct_response_parent_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, operation_id, parent_record_id, canonical_record_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_direct_response_current")}
    WHERE canonical_record_id IS NULL
       OR canonical_record_id != CONCAT(operation_id, ':', parent_record_id)
    GROUP BY 1, 2, 3, 4
  `);

assert("xero_raw_receipt_record_count")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT receipt.receipt_id, receipt.tenant_id, receipt.endpoint,
           receipt.record_count AS expected_record_count,
           COUNT(record.receipt_id) AS actual_record_count
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_receipt_history")} AS receipt
    LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_record_history_canonical")} AS record
      ON receipt.receipt_id = record.receipt_id
    GROUP BY 1, 2, 3, 4
    HAVING expected_record_count != actual_record_count
  `);

assert("xero_raw_record_duplicate_conflict")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT generation, tenant_id, endpoint, record_id, version_id,
           source_duplicate_row_count, distinct_receipt_count, distinct_observation_count
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_source_duplicates")}
    WHERE NOT is_exact_replay_duplicate
  `);

assert("xero_raw_child_collection_count")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT collection.generation, collection.tenant_id, collection.endpoint,
           collection.record_id, collection.version_id, collection.collection_name,
           collection.child_count AS expected_child_count,
           COUNT(child.child_key) AS actual_child_count
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_collection_history")} AS collection
    LEFT JOIN ${ctx.ref("dw_bronze_xero", "xero_raw_child_history")} AS child
      USING (generation, tenant_id, endpoint, record_id, version_id, collection_name)
    GROUP BY 1, 2, 3, 4, 5, 6, 7
    HAVING expected_child_count != actual_child_count
  `);

assert("xero_raw_child_lifecycle_safety")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT generation, tenant_id, endpoint, record_id, version_id, collection_name, child_key
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_child_current")}
    WHERE (is_current AND (is_deleted OR is_removed))
       OR (is_removed AND NOT is_membership_identity_stable)
       OR (is_removed AND deletion_reason IS DISTINCT FROM 'missing_from_complete_parent')
       OR (parent_source_deleted AND deletion_reason IS DISTINCT FROM 'source_deleted')
  `);
