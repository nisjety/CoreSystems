const project = dataform.projectConfig.defaultDatabase;
const stagingSchema = "dw_staging_xero";

const sources = {
  endpointCatalog: { database: project, schema: stagingSchema, name: "endpoint_catalog" },
  syncRuns: { database: project, schema: stagingSchema, name: "sync_runs" },
  syncRunEntities: { database: project, schema: stagingSchema, name: "sync_run_entities" },
};

// The standard `xero sync` collection set that feeds the current Accounting
// warehouse. Reporting is a separately certified Xero run mode, so readiness
// requires one coherent successful run for each cohort rather than mixing
// endpoint attempts from unrelated runs.
const standardAccountingOperationIds = [
  "getAccounts",
  "getBankTransactions",
  "getBankTransfers",
  "getBatchPayments",
  "getBrandingThemes",
  "getBudget",
  "getBudgets",
  "getContactGroups",
  "getContacts",
  "getCreditNotes",
  "getCurrencies",
  "getExpenseClaims",
  "getInvoices",
  "getItems",
  "getJournals",
  "getLinkedTransactions",
  "getManualJournals",
  "getOrganisations",
  "getOverpayments",
  "getPayments",
  "getPrepayments",
  "getPurchaseOrders",
  "getQuotes",
  "getReceipts",
  "getRepeatingInvoices",
  "getTaxRates",
  "getTrackingCategories",
  "getUsers",
];
const currentReportingOperationIds = [
  "getReportBalanceSheet",
  "getReportBankSummary",
  "getReportBudgetSummary",
  "getReportExecutiveSummary",
  "getReportProfitAndLoss",
  "getReportTrialBalance",
];
// Aged receivables/payables are contact-fan-out reports. Their own bounded
// source mode is evaluated separately from tenant-wide report snapshots.
const currentAgedContactReportingOperationIds = [
  "getReportAgedPayablesByContact",
  "getReportAgedReceivablesByContact",
];
const currentAccountingOperationIds = [
  ...standardAccountingOperationIds,
  ...currentReportingOperationIds,
];
// Xero child-ID routes are not inferred from a latest endpoint attempt. They
// become not applicable only when their parent inventory is certified complete
// and empty in one source run; this keeps an incomplete fan-out from inflating
// warehouse coverage.
const dataDependentParentRoutes = [
  { operationId: "getProject", parentSourceEndpoint: "projects_projects" },
  { operationId: "getTasks", parentSourceEndpoint: "projects_projects" },
  { operationId: "getTask", parentSourceEndpoint: "projects_tasks" },
  { operationId: "getTimeEntries", parentSourceEndpoint: "projects_projects" },
  { operationId: "getTimeEntry", parentSourceEndpoint: "projects_time" },
];
const operationList = operationIds => operationIds
  .map(operationId => `'${operationId}'`)
  .join(", ");
const dataDependentParentRouteRows = dataDependentParentRoutes
  .map(({ operationId, parentSourceEndpoint }) =>
    `STRUCT('${operationId}' AS operation_id, '${parentSourceEndpoint}' AS parent_source_endpoint)`)
  .join(",\n      ");
const currentAccountingOperationList = operationList(currentAccountingOperationIds);
const standardAccountingOperationList = operationList(standardAccountingOperationIds);
const currentReportingOperationList = operationList(currentReportingOperationIds);
const currentAgedContactReportingOperationList = operationList(currentAgedContactReportingOperationIds);

for (const source of Object.values(sources)) {
  declare(source);
}

publish("xero_endpoint_catalog", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Pinned Xero API route inventory exposed under the warehouse's natural naming convention.",
  assertions: { nonNull: ["operation_id", "api_family", "source_endpoint", "spec_commit"], uniqueKey: ["operation_id"] },
}).query(ctx => `
  SELECT operation_id, api_family, path, extraction_class, source_endpoint, spec_commit
  FROM ${ctx.ref(sources.endpointCatalog)}
`);

publish("xero_endpoint_coverage", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "One row per observed Xero tenant and documented operation. A successful attempt is not a claim of field-level or historical completeness.",
}).query(ctx => `
  WITH catalog AS (
    SELECT *,
           operation_id IN ('getPaymentServices', 'getBrandingThemePaymentServices') AS is_out_of_scope,
           operation_id IN (${standardAccountingOperationList}) AS is_standard_accounting_warehouse_required,
           operation_id IN (${currentReportingOperationList}) AS is_current_reporting_warehouse_required,
           operation_id IN (${currentAgedContactReportingOperationList})
             AS is_current_aged_contact_reporting_required,
           operation_id IN (${currentAccountingOperationList}) AS is_current_accounting_warehouse_required
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_catalog")}
  ), known_tenants AS (
    SELECT DISTINCT tenant_id
    FROM ${ctx.ref(sources.syncRunEntities)}
    WHERE tenant_id IS NOT NULL AND TRIM(tenant_id) != ''
  ), deduplicated_runs AS (
    SELECT * EXCEPT(run_rank)
    FROM (
      SELECT r.*,
             ROW_NUMBER() OVER (
               PARTITION BY r.run_id
               ORDER BY r.finished_at DESC, r.started_at DESC
             ) AS run_rank
      FROM ${ctx.ref(sources.syncRuns)} AS r
    )
    WHERE run_rank = 1
  ), deduplicated_entities AS (
    SELECT * EXCEPT(entity_rank)
    FROM (
      SELECT e.*,
             ROW_NUMBER() OVER (
               PARTITION BY e.run_id, e.tenant_id, e.endpoint, e.outcome_index
               ORDER BY e.synced_at DESC
             ) AS entity_rank
      FROM ${ctx.ref(sources.syncRunEntities)} AS e
    )
    WHERE entity_rank = 1
  ), attempt_summaries AS (
    SELECT e.tenant_id, e.endpoint, e.run_id,
           r.mode AS run_mode, COALESCE(r.status, 'missing') AS run_status, r.finished_at,
           COUNT(*) AS request_count,
           SUM(COALESCE(e.pages, 0)) AS pages,
           SUM(COALESCE(e.records, 0)) AS records,
           LOGICAL_OR(COALESCE(e.error_present, FALSE)) AS error_present,
           MAX(e.synced_at) AS last_observed,
           CASE
             WHEN COUNTIF(e.termination = 'not_applicable_no_parent_id') = COUNT(*)
               THEN 'not_applicable_no_parent_id'
             WHEN COUNTIF(e.termination = 'single_response') = COUNT(*) THEN 'single_response'
             WHEN COUNTIF(e.termination = 'report-snapshot') = COUNT(*) THEN 'report-snapshot'
             WHEN COUNTIF(e.termination IN ('ended_on_empty_page', 'offset_not_advancing')) = COUNT(*)
                  AND COUNTIF(e.termination = 'offset_not_advancing') > 0
               THEN 'offset_not_advancing'
             WHEN COUNTIF(e.termination = 'ended_on_empty_page') = COUNT(*)
               THEN 'ended_on_empty_page'
             ELSE 'mixed'
           END AS termination
    FROM deduplicated_entities AS e
    LEFT JOIN deduplicated_runs AS r USING (run_id)
    GROUP BY e.tenant_id, e.endpoint, e.run_id, r.mode, r.status, r.finished_at
  ), all_latest_attempts AS (
    SELECT * FROM attempt_summaries
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY tenant_id, endpoint
      ORDER BY last_observed DESC, finished_at DESC, run_id DESC
    ) = 1
  ), run_cohort_counts AS (
    SELECT a.tenant_id, a.run_id,
           MAX(a.last_observed) AS last_observed,
           MAX(a.finished_at) AS finished_at,
           COUNT(DISTINCT IF(
             c.is_standard_accounting_warehouse_required
               AND a.run_mode != 'aged-contact-reports', c.operation_id, NULL
           )) AS standard_operation_count,
           COUNT(DISTINCT IF(
             c.is_current_reporting_warehouse_required, c.operation_id, NULL
           )) AS reporting_operation_count,
           COUNT(DISTINCT IF(
             c.is_current_aged_contact_reporting_required, c.operation_id, NULL
           )) AS aged_operation_count
    FROM attempt_summaries AS a
    INNER JOIN catalog AS c ON a.endpoint = c.source_endpoint
    GROUP BY a.tenant_id, a.run_id
  ), latest_standard_cohort AS (
    SELECT tenant_id, run_id
    FROM run_cohort_counts
    WHERE standard_operation_count = ${standardAccountingOperationIds.length}
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY tenant_id ORDER BY last_observed DESC, finished_at DESC, run_id DESC
    ) = 1
  ), latest_reporting_cohort AS (
    SELECT tenant_id, run_id
    FROM run_cohort_counts
    WHERE reporting_operation_count = ${currentReportingOperationIds.length}
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY tenant_id ORDER BY last_observed DESC, finished_at DESC, run_id DESC
    ) = 1
  ), latest_aged_cohort AS (
    SELECT tenant_id, run_id
    FROM run_cohort_counts
    WHERE aged_operation_count = ${currentAgedContactReportingOperationIds.length}
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY tenant_id ORDER BY last_observed DESC, finished_at DESC, run_id DESC
    ) = 1
  ), coverage_requirements AS (
    SELECT source_endpoint,
           LOGICAL_OR(is_standard_accounting_warehouse_required) AS is_standard,
           LOGICAL_OR(is_current_reporting_warehouse_required) AS is_reporting,
           LOGICAL_OR(is_current_aged_contact_reporting_required) AS is_aged
    FROM catalog
    GROUP BY source_endpoint
  ), latest_attempts AS (
    SELECT a.*
    FROM attempt_summaries AS a
    INNER JOIN coverage_requirements AS c ON a.endpoint = c.source_endpoint
    LEFT JOIN latest_standard_cohort AS standard ON a.tenant_id = standard.tenant_id
    LEFT JOIN latest_reporting_cohort AS reporting ON a.tenant_id = reporting.tenant_id
    LEFT JOIN latest_aged_cohort AS aged ON a.tenant_id = aged.tenant_id
    WHERE (NOT c.is_standard OR standard.run_id IS NULL OR a.run_id = standard.run_id)
      AND (NOT c.is_reporting OR reporting.run_id IS NULL OR a.run_id = reporting.run_id)
      AND (NOT c.is_aged OR aged.run_id IS NULL OR a.run_id = aged.run_id)
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY a.tenant_id, a.endpoint
      ORDER BY a.last_observed DESC, a.finished_at DESC, a.run_id DESC
    ) = 1
  ), data_dependent_parent_routes AS (
    SELECT *
    FROM UNNEST([
      ${dataDependentParentRouteRows}
    ])
  ), certified_empty_parent_evidence AS (
    SELECT certification.tenant_id,
           certification.source_run_id,
           certification.run_finished_at,
           dependency.operation_id,
           dependency.parent_source_endpoint,
           'projects_certified_empty_parent' AS evidence_type
    FROM ${ctx.ref("dw_bronze_xero", "xero_project_run_certification")} AS certification
    INNER JOIN data_dependent_parent_routes AS dependency
      ON certification.source_endpoint = dependency.parent_source_endpoint
    WHERE certification.is_certified
      AND certification.expected_record_count = 0

    UNION ALL

    SELECT certification.tenant_id,
           certification.source_run_id,
           certification.run_finished_at,
           'getAssetById' AS operation_id,
           'assets_assets' AS parent_source_endpoint,
           'assets_certified_empty_status_inventory' AS evidence_type
    FROM ${ctx.ref("dw_bronze_xero", "xero_asset_run_certification")} AS certification
    WHERE certification.is_certified
      AND certification.expected_record_count = 0
  ), ranked_empty_parent_evidence AS (
    SELECT *,
           ROW_NUMBER() OVER (
             PARTITION BY tenant_id, operation_id
             ORDER BY run_finished_at DESC, source_run_id DESC
           ) AS evidence_rank
    FROM certified_empty_parent_evidence
  ), latest_empty_parent_evidence AS (
    SELECT * FROM ranked_empty_parent_evidence WHERE evidence_rank = 1
  ), classified AS (
    SELECT t.tenant_id, c.api_family, c.operation_id, c.path, c.extraction_class,
           c.source_endpoint, c.spec_commit, c.is_out_of_scope,
           c.is_standard_accounting_warehouse_required,
           c.is_current_reporting_warehouse_required,
           c.is_current_aged_contact_reporting_required,
           c.is_current_accounting_warehouse_required,
           a.run_id, a.run_mode, a.run_status, a.request_count, a.pages, a.records,
           a.termination, a.error_present, a.last_observed, a.finished_at,
           latest.run_id AS latest_attempt_run_id,
           latest.run_mode AS latest_attempt_run_mode,
           latest.last_observed AS latest_attempt_observed_at,
           latest.termination AS latest_attempt_termination,
           COALESCE(latest.error_present, FALSE)
             OR (latest.run_id IS NOT NULL AND latest.run_status != 'succeeded')
             AS latest_attempt_failed,
           parent.source_run_id AS not_applicable_evidence_run_id,
           parent.run_finished_at AS not_applicable_evidence_finished_at,
           parent.parent_source_endpoint AS not_applicable_parent_endpoint,
           parent.evidence_type AS not_applicable_evidence_type,
           CASE
             WHEN c.is_out_of_scope THEN 'out_of_scope'
             WHEN parent.source_run_id IS NOT NULL
               AND (a.finished_at IS NULL OR parent.run_finished_at >= a.finished_at)
               THEN 'not_applicable'
             WHEN a.run_id IS NULL THEN 'not_attempted'
             WHEN a.error_present OR a.run_status != 'succeeded' THEN 'failed'
            WHEN c.operation_id = 'getBudget'
              AND a.termination = 'not_applicable_no_parent_id' THEN 'partial'
            WHEN c.source_endpoint LIKE 'accounting_get_%'
               AND a.termination = 'not_applicable_no_parent_id' THEN 'not_applicable'
             WHEN c.source_endpoint LIKE 'accounting_get_%'
               AND a.termination != 'single_response' THEN 'partial'
             WHEN c.extraction_class = 'report'
               AND a.termination != 'report-snapshot' THEN 'partial'
             WHEN c.operation_id = 'getJournals'
              AND a.termination != 'ended_on_empty_page' THEN 'partial'
             WHEN c.source_endpoint NOT LIKE 'accounting_get_%'
               AND c.extraction_class != 'report'
               AND a.termination != 'ended_on_empty_page' THEN 'partial'
             ELSE 'succeeded'
           END AS operation_status
    FROM known_tenants AS t
    CROSS JOIN catalog AS c
    LEFT JOIN latest_attempts AS a
      ON t.tenant_id = a.tenant_id AND c.source_endpoint = a.endpoint
    LEFT JOIN all_latest_attempts AS latest
      ON t.tenant_id = latest.tenant_id AND c.source_endpoint = latest.endpoint
    LEFT JOIN latest_empty_parent_evidence AS parent
      ON t.tenant_id = parent.tenant_id AND c.operation_id = parent.operation_id
  )
  SELECT *, operation_status IN ('succeeded', 'not_applicable') AS is_authoritative
  FROM classified
`);

publish("xero_endpoint_coverage_summary", {
  type: "table",
  schema: "dw_bronze_xero",
  description: "Tenant-level endpoint readiness. Standard Accounting and reporting each require one coherent authoritative run; combined accounting readiness requires both certified cohorts, without falsely requiring distinct Xero sync modes to share a run id.",
  assertions: { nonNull: ["tenant_id"] },
}).query(ctx => `
  WITH summary AS (
    SELECT tenant_id,
         COUNT(*) AS documented_operation_count,
         COUNTIF(is_out_of_scope) AS out_of_scope_operation_count,
         COUNTIF(NOT is_out_of_scope) AS in_scope_operation_count,
         COUNTIF(is_authoritative) AS authoritative_operation_count,
         COUNTIF(operation_status = 'not_applicable') AS not_applicable_operation_count,
         COUNTIF(is_current_accounting_warehouse_required) AS current_accounting_operation_count,
         COUNTIF(is_current_accounting_warehouse_required AND is_authoritative)
           AS current_accounting_authoritative_operation_count,
         COUNTIF(is_standard_accounting_warehouse_required)
           AS standard_accounting_operation_count,
         COUNTIF(is_standard_accounting_warehouse_required AND is_authoritative)
           AS standard_accounting_authoritative_operation_count,
         COUNT(DISTINCT IF(is_standard_accounting_warehouse_required, run_id, NULL))
           AS standard_accounting_distinct_run_count,
         COUNTIF(is_current_reporting_warehouse_required)
           AS current_reporting_operation_count,
         COUNTIF(is_current_reporting_warehouse_required AND is_authoritative)
           AS current_reporting_authoritative_operation_count,
         COUNT(DISTINCT IF(is_current_reporting_warehouse_required, run_id, NULL))
           AS current_reporting_distinct_run_count,
         COUNTIF(is_current_aged_contact_reporting_required)
           AS current_aged_contact_reporting_operation_count,
         COUNTIF(is_current_aged_contact_reporting_required AND is_authoritative)
           AS current_aged_contact_reporting_authoritative_operation_count,
         COUNT(DISTINCT IF(is_current_aged_contact_reporting_required, run_id, NULL))
           AS current_aged_contact_reporting_distinct_run_count,
         COUNTIF(operation_status = 'not_attempted') AS not_attempted_operation_count,
         COUNTIF(operation_status = 'partial') AS partial_operation_count,
         COUNTIF(operation_status = 'failed') AS failed_operation_count,
         COUNTIF(latest_attempt_failed) AS latest_attempt_failed_operation_count,
         COUNTIF(NOT is_out_of_scope AND NOT is_authoritative) = 0
           AS is_ready_for_full_catalog,
         MAX(last_observed) AS latest_observed_at,
         MAX(IF(is_current_accounting_warehouse_required, last_observed, NULL))
           AS current_accounting_latest_observed_at
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_coverage")}
    GROUP BY tenant_id
  ), readiness AS (
    SELECT *,
      standard_accounting_operation_count = ${standardAccountingOperationIds.length}
        AND standard_accounting_authoritative_operation_count = ${standardAccountingOperationIds.length}
        AND standard_accounting_distinct_run_count = 1 AS standard_accounting_ready,
      current_reporting_operation_count = ${currentReportingOperationIds.length}
        AND current_reporting_authoritative_operation_count = ${currentReportingOperationIds.length}
        AND current_reporting_distinct_run_count = 1 AS current_reporting_ready,
      current_aged_contact_reporting_operation_count = ${currentAgedContactReportingOperationIds.length}
        AND current_aged_contact_reporting_authoritative_operation_count = ${currentAgedContactReportingOperationIds.length}
        AND current_aged_contact_reporting_distinct_run_count = 1 AS current_aged_contact_reporting_ready
    FROM summary
  )
  SELECT * EXCEPT(
      standard_accounting_ready,
      current_reporting_ready,
      current_aged_contact_reporting_ready
    ),
    standard_accounting_ready AS is_ready_for_standard_accounting_warehouse,
    current_reporting_ready AS is_ready_for_current_reporting_warehouse,
    current_aged_contact_reporting_ready AS is_ready_for_current_aged_contact_reporting,
    standard_accounting_ready AND current_reporting_ready
      AS is_ready_for_current_accounting_warehouse
  FROM readiness
`);

assert("xero_endpoint_catalog_integrity")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT 'duplicate_operation' AS issue, operation_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_catalog")}
    GROUP BY operation_id HAVING COUNT(*) != 1
    UNION ALL
    SELECT 'catalog_count', CAST(NULL AS STRING)
    FROM (SELECT COUNT(*) AS catalog_count FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_catalog")})
    WHERE catalog_count != 150
  `);

assert("xero_endpoint_coverage_grain")
  .schema("dw_bronze_xero_assertions")
  .query(ctx => `
    SELECT tenant_id, operation_id
    FROM ${ctx.ref("dw_bronze_xero", "xero_endpoint_coverage")}
    GROUP BY tenant_id, operation_id
    HAVING tenant_id IS NULL OR operation_id IS NULL OR COUNT(*) != 1
  `);
