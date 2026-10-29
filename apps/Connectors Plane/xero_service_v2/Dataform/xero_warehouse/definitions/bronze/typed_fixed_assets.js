const project = dataform.projectConfig.defaultDatabase;
const stagingSchema = "dw_staging_xero";
const assetStatuses = ["DRAFT", "REGISTERED", "DISPOSED"];
const assetStatusList = assetStatuses.map(status => `'${status}'`).join(", ");
const assetListSnapshots = {
  database: project,
  schema: stagingSchema,
  name: "extended_list_snapshots",
};

declare(assetListSnapshots);

function appendTypedAssetHistory(name, endpoint, identity, projection, description) {
  publish(name, {
    type: "incremental",
    schema: "dw_bronze_xero",
    protected: true,
    description,
    bigquery: {
      partitionBy: "DATE(asset_observed_at)",
      clusterBy: ["tenant_id", "source_record_id"],
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
      ORDER BY asset_observed_at DESC, source_receipt_id DESC
    ) = 1
  `);
}

appendTypedAssetHistory(
  "xero_fixed_asset_observations",
  "assets_asset_details",
  ["generation", "tenant_id", "asset_id", "source_version_id"],
  `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS asset_observation_id,
    generation,
    tenant_id,
    JSON_VALUE(payload_json, '$.assetId') AS asset_id,
    JSON_VALUE(payload_json, '$.assetTypeId') AS asset_type_id,
    JSON_VALUE(payload_json, '$.assetName') AS asset_name,
    JSON_VALUE(payload_json, '$.assetNumber') AS asset_number,
    JSON_VALUE(payload_json, '$.assetStatus') AS asset_status,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.warrantyExpiryDate'), 1, 10) AS DATE)
      AS warranty_expiry_date,
    JSON_VALUE(payload_json, '$.serialNumber') AS serial_number,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.purchaseDate'), 1, 10) AS DATE) AS purchase_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.purchasePrice') AS NUMERIC) AS purchase_price,
    SAFE_CAST(JSON_VALUE(payload_json, '$.accountingBookValue') AS NUMERIC)
      AS accounting_book_value,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.disposalDate'), 1, 10) AS DATE) AS disposal_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.disposalPrice') AS NUMERIC) AS disposal_price,
    SAFE_CAST(JSON_VALUE(payload_json, '$.canRollback') AS BOOL) AS can_rollback,
    SAFE_CAST(JSON_VALUE(payload_json, '$.isDeleteEnabledForDate') AS BOOL)
      AS is_delete_enabled_for_date,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.bookDepreciationDetail.depreciationStartDate'), 1, 10) AS DATE)
      AS book_depreciation_start_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationDetail.currentAccumDepreciationAmount') AS NUMERIC)
      AS book_current_accumulated_depreciation,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationDetail.priorAccumDepreciationAmount') AS NUMERIC)
      AS book_prior_accumulated_depreciation,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationDetail.currentCapitalGain') AS NUMERIC)
      AS book_current_capital_gain,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationDetail.currentGainLoss') AS NUMERIC)
      AS book_current_gain_loss,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationDetail.costLimit') AS NUMERIC)
      AS book_cost_limit,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationDetail.residualValue') AS NUMERIC)
      AS book_residual_value,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.depreciationMethod')
      AS book_depreciation_method,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.depreciationCalculationMethod')
      AS book_depreciation_calculation_method,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationSetting.depreciationRate') AS NUMERIC)
      AS book_depreciation_rate,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationSetting.effectiveLifeYears') AS INT64)
      AS book_effective_life_years,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.bookEffectiveDateOfChangeId')
      AS book_effective_date_of_change_id,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.averagingMethod')
      AS book_depreciation_averaging_method,
    source_deleted AS is_source_deleted,
    observed_at AS asset_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `,
  "Append-only typed Xero fixed-asset detail observations. Each row remains pinned to the immutable raw receipt and response version."
);

appendTypedAssetHistory(
  "xero_fixed_asset_type_observations",
  "assets_asset_types",
  ["generation", "tenant_id", "asset_type_id", "source_version_id"],
  `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS asset_type_observation_id,
    generation,
    tenant_id,
    JSON_VALUE(payload_json, '$.assetTypeId') AS asset_type_id,
    JSON_VALUE(payload_json, '$.assetTypeName') AS asset_type_name,
    JSON_VALUE(payload_json, '$.fixedAssetAccountId') AS fixed_asset_account_id,
    JSON_VALUE(payload_json, '$.accumulatedDepreciationAccountId')
      AS accumulated_depreciation_account_id,
    JSON_VALUE(payload_json, '$.depreciationExpenseAccountId') AS depreciation_expense_account_id,
    SAFE_CAST(JSON_VALUE(payload_json, '$.lockPrivateUseAccount') AS BOOL)
      AS lock_private_use_account,
    SAFE_CAST(JSON_VALUE(payload_json, '$.locks') AS INT64) AS locked_asset_count,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.depreciationMethod')
      AS book_depreciation_method,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.depreciationCalculationMethod')
      AS book_depreciation_calculation_method,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationSetting.depreciationRate') AS NUMERIC)
      AS book_depreciation_rate,
    SAFE_CAST(JSON_VALUE(payload_json, '$.bookDepreciationSetting.effectiveLifeYears') AS INT64)
      AS book_effective_life_years,
    JSON_VALUE(payload_json, '$.bookDepreciationSetting.averagingMethod')
      AS book_depreciation_averaging_method,
    source_deleted AS is_source_deleted,
    observed_at AS asset_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `,
  "Append-only typed Xero fixed-asset-type observations, retaining native account references and source response provenance."
);

appendTypedAssetHistory(
  "xero_fixed_asset_settings_observations",
  "assets_settings",
  ["generation", "tenant_id", "source_record_id", "source_version_id"],
  `
    CONCAT(generation, '|', tenant_id, '|', endpoint, '|', record_id, '|', version_id)
      AS asset_settings_observation_id,
    generation,
    tenant_id,
    JSON_VALUE(payload_json, '$.assetNumberPrefix') AS asset_number_prefix,
    SAFE_CAST(JSON_VALUE(payload_json, '$.assetNumberSequence') AS INT64) AS asset_number_sequence,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.assetStartDate'), 1, 10) AS DATE) AS asset_start_date,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.lastDepreciationDate'), 1, 10) AS DATE) AS last_depreciation_date,
    SAFE_CAST(JSON_VALUE(payload_json, '$.optInForTax') AS BOOL) AS is_tax_opted_in,
    SAFE_CAST(SUBSTR(JSON_VALUE(payload_json, '$.taxStartDate'), 1, 10) AS DATE) AS tax_start_date,
    JSON_VALUE(payload_json, '$.defaultGainOnDisposalAccountId')
      AS default_gain_on_disposal_account_id,
    JSON_VALUE(payload_json, '$.defaultLossOnDisposalAccountId')
      AS default_loss_on_disposal_account_id,
    JSON_VALUE(payload_json, '$.defaultCapitalGainOnDisposalAccountId')
      AS default_capital_gain_on_disposal_account_id,
    source_deleted AS is_source_deleted,
    observed_at AS asset_observed_at,
    record_id AS source_record_id,
    version_id AS source_version_id,
    receipt_id AS source_receipt_id,
    run_id AS source_run_id
  `,
  "Append-only typed Xero fixed-asset settings observations. Settings are source snapshots, not inferred accounting periods."
);

publish("xero_asset_run_certification", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Per-run Xero Assets inventory completeness evidence. An Assets run is certified only when every supported asset status has one complete, reconciled list snapshot in the same successful run.",
}).query(ctx => `
  WITH source_outcomes AS (
    SELECT
      entity.tenant_id,
      entity.run_id AS source_run_id,
      MAX(run.finished_at) AS run_finished_at,
      MAX(run.status) AS run_status,
      COUNT(*) AS request_count,
      COUNTIF(entity.error_present OR entity.termination != 'ended_on_empty_page')
        AS failed_or_partial_request_count,
      SUM(entity.records) AS expected_record_count
    FROM ${ctx.ref({ database: project, schema: stagingSchema, name: "sync_run_entities" })} AS entity
    INNER JOIN ${ctx.ref({ database: project, schema: stagingSchema, name: "sync_runs" })} AS run
      USING (run_id)
    WHERE entity.endpoint = 'assets_assets'
    GROUP BY entity.tenant_id, source_run_id
  ), status_snapshots AS (
    SELECT
      tenant_id,
      run_id AS source_run_id,
      COUNT(*) AS snapshot_count,
      COUNT(DISTINCT request_key) AS status_count,
      COUNTIF(request_key IN (${assetStatusList})) AS required_status_snapshot_count,
      COUNT(DISTINCT IF(request_key IN (${assetStatusList}), request_key, NULL))
        AS distinct_required_status_count,
      SUM(record_count) AS snapshot_record_count
    FROM ${ctx.ref(assetListSnapshots)}
    WHERE source_endpoint = 'assets_assets'
    GROUP BY tenant_id, source_run_id
  )
  SELECT
    outcome.tenant_id,
    outcome.source_run_id,
    outcome.run_finished_at,
    outcome.run_status,
    outcome.request_count,
    outcome.failed_or_partial_request_count,
    outcome.expected_record_count,
    COALESCE(snapshot.snapshot_count, 0) AS snapshot_count,
    COALESCE(snapshot.status_count, 0) AS status_count,
    COALESCE(snapshot.required_status_snapshot_count, 0) AS required_status_snapshot_count,
    COALESCE(snapshot.distinct_required_status_count, 0) AS distinct_required_status_count,
    COALESCE(snapshot.snapshot_record_count, 0) AS snapshot_record_count,
    outcome.run_status = 'succeeded'
      AND outcome.request_count = ${assetStatuses.length}
      AND outcome.failed_or_partial_request_count = 0
      AND COALESCE(snapshot.snapshot_count, 0) = ${assetStatuses.length}
      AND COALESCE(snapshot.status_count, 0) = ${assetStatuses.length}
      AND COALESCE(snapshot.required_status_snapshot_count, 0) = ${assetStatuses.length}
      AND COALESCE(snapshot.distinct_required_status_count, 0) = ${assetStatuses.length}
      AND outcome.expected_record_count = COALESCE(snapshot.snapshot_record_count, 0)
      AS is_certified,
    CASE
      WHEN outcome.run_status != 'succeeded' THEN 'run_not_succeeded'
      WHEN outcome.request_count != ${assetStatuses.length} THEN 'asset_status_request_count_mismatch'
      WHEN outcome.failed_or_partial_request_count > 0 THEN 'source_request_failed_or_partial'
      WHEN COALESCE(snapshot.snapshot_count, 0) != ${assetStatuses.length}
        OR COALESCE(snapshot.status_count, 0) != ${assetStatuses.length}
        OR COALESCE(snapshot.required_status_snapshot_count, 0) != ${assetStatuses.length}
        OR COALESCE(snapshot.distinct_required_status_count, 0) != ${assetStatuses.length}
        THEN 'asset_status_snapshot_set_incomplete'
      WHEN outcome.expected_record_count != COALESCE(snapshot.snapshot_record_count, 0)
        THEN 'source_snapshot_count_mismatch'
      ELSE 'certified'
    END AS certification_state
  FROM source_outcomes AS outcome
  LEFT JOIN status_snapshots AS snapshot
    USING (tenant_id, source_run_id)
`);

for (const assetSource of [
  { name: "fixed_asset", table: "xero_fixed_asset_observations", key: "asset_id" },
  { name: "fixed_asset_type", table: "xero_fixed_asset_type_observations", key: "asset_type_id" },
  { name: "fixed_asset_settings", table: "xero_fixed_asset_settings_observations", key: "source_record_id" },
]) {
  assert(`xero_${assetSource.name}_observation_grain`)
    .schema("dw_bronze_xero_assertions")
    .query(ctx => `
      SELECT generation, tenant_id, ${assetSource.key}, source_version_id
      FROM ${ctx.ref("dw_bronze_xero", assetSource.table)}
      GROUP BY 1, 2, 3, 4
      HAVING generation IS NULL OR tenant_id IS NULL OR ${assetSource.key} IS NULL
        OR source_version_id IS NULL OR COUNT(*) != 1
    `);
}

assert("xero_asset_run_certification_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, source_run_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_asset_run_certification")}
    GROUP BY tenant_id, source_run_id
    HAVING tenant_id IS NULL OR source_run_id IS NULL OR COUNT(*) != 1
  `);
