use crate::{
    catalog::{self, Params},
    jobs::{Destination, ExtractionPlan},
    writes::{self, WriteCommand},
};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use chrono::{NaiveDate, Utc};
use gcloud_auth::{project::Config as GoogleAuthConfig, token::DefaultTokenSourceProvider};
use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, sync::Arc};
use token_source::TokenSourceProvider;
use uuid::Uuid;
use xero_auth::TokenProvider;
use xero_client::XeroApiClient;
use xero_common::{GcsConfig, SyncConfig};
use xero_gcs::{GcsRawSink, LocalDirSink, RawSink, RunManifest};
use xero_store::ConnectionStore;

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}
impl Failure {
    pub fn new(code: &'static str, message: &str, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
    pub fn invalid(message: &str) -> Self {
        Self::new("invalid_arguments", message, false)
    }
    pub fn upstream(error: xero_common::Error) -> Self {
        match error {
            xero_common::Error::Auth(_) | xero_common::Error::StateStore(_) => Self::new("reconnect_required", "Xero authentication is unavailable; reconnect this tenant through the onboarding service.", false),
            xero_common::Error::XeroApi(ref text) if text.contains("exceeds the configured byte limit") => Self::new("result_too_large", "Xero's response exceeds the interactive limit; narrow the query or use extraction.", false),
            xero_common::Error::XeroApi(ref text) if text.contains("401") || text.contains("403") => Self::new("xero_access_denied", "Xero denied this operation. Check the tenant's granted scopes and feature access.", false),
            xero_common::Error::XeroApi(ref text) if text.contains("404") => Self::new("xero_resource_unavailable", "Xero could not find this resource or the tenant does not support this feature.", false),
            xero_common::Error::XeroApi(ref text) if text.contains("409") => Self::new("xero_conflict", "Xero rejected the request because the source record changed or conflicts with the current state. Refresh the record before retrying.", false),
            xero_common::Error::XeroApi(ref text) if text.contains("429") => Self::new("xero_rate_limited", "Xero rate-limited this operation. Retry after a short delay with the same parameters.", true),
            xero_common::Error::XeroApi(ref text) if text.contains("400") || text.contains("422") => Self::new("xero_request_rejected", "Xero rejected the request parameters or the requested state transition. Inspect the catalogued schema and source record before retrying.", false),
            xero_common::Error::XeroApi(ref text) if text.contains("500") || text.contains("502") || text.contains("503") || text.contains("504") => Self::new("xero_service_unavailable", "Xero is temporarily unavailable. Retry this read with the same parameters.", true),
            xero_common::Error::Config(_) => Self::invalid("Invalid catalogued Xero request."),
            _ => Self::new("xero_request_failed", "Xero did not return a usable response. Retry with the same read parameters.", true),
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub tenant_id: String,
    pub tenant_name: String,
    pub scopes: Vec<String>,
    pub auth_mode: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub tenant_id: String,
    pub family: String,
    pub operation_id: String,
    #[serde(default)]
    pub path: Params,
    #[serde(default)]
    pub query: Params,
    #[serde(default)]
    pub header: Params,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    50
}
impl Query {
    pub fn validate(&self) -> Result<(), Failure> {
        if !(1..=100).contains(&self.limit) {
            return Err(Failure::invalid("limit must be between 1 and 100."));
        }
        catalog::operation(&self.family, &self.operation_id)?.prepare(
            &self.path,
            &self.query,
            &self.header,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Response {
    pub body: Bytes,
    pub content_type: String,
    pub http_status: u16,
    pub fetched_at: chrono::DateTime<Utc>,
}

const CLOUD_PLATFORM_SCOPE: &[&str] = &["https://www.googleapis.com/auth/cloud-platform"];
const BIGQUERY_API: &str = "https://bigquery.googleapis.com/bigquery/v2";
const DEFAULT_WAREHOUSE_MAX_BYTES_BILLED: u64 = 5 * 1024 * 1024 * 1024;
const MAX_WAREHOUSE_TABLES: usize = 200;

// The MCP overview exposes only business-facing typed staging tables. A fixed
// allowlist avoids dataset-wide metadata permission and excludes control and
// legacy compatibility tables from the client-visible inventory.
const WAREHOUSE_OVERVIEW_TABLES: &[&str] = &[
    "accounts",
    "bank_transaction_lines",
    "bank_transactions",
    "bank_transfers",
    "batch_payment_lines",
    "batch_payments",
    "branding_themes",
    "budgets",
    "contact_addresses",
    "contact_group_members",
    "contact_group_memberships",
    "contact_groups",
    "contact_phones",
    "contacts",
    "credit_note_allocations",
    "credit_note_lines",
    "credit_notes",
    "currencies",
    "invoice_lines",
    "invoice_payments",
    "invoices",
    "items",
    "journal_lines",
    "journals",
    "linked_transactions",
    "manual_journal_lines",
    "manual_journals",
    "organisations",
    "overpayment_lines",
    "overpayments",
    "payments",
    "purchase_order_lines",
    "purchase_orders",
    "quote_lines",
    "quotes",
    "repeating_invoice_lines",
    "repeating_invoices",
    "report_rows",
    "report_snapshots",
    "tax_rates",
    "tracking_categories",
    "tracking_options",
    "users",
];

#[derive(Clone, Debug, Serialize)]
pub struct WarehouseTableCount {
    pub table_name: String,
    pub row_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct WarehouseOverview {
    pub project_id: String,
    pub dataset_id: String,
    pub location: String,
    pub tables: Vec<WarehouseTableCount>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarehouseMetric {
    ProductVolumeByMonth,
}

impl WarehouseMetric {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProductVolumeByMonth => "product_volume_by_month",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarehouseMetricRequest {
    pub metric: WarehouseMetric,
    pub from: NaiveDate,
    pub to: NaiveDate,
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default = "default_warehouse_metric_limit")]
    pub limit: usize,
}

fn default_warehouse_metric_limit() -> usize {
    100
}

impl WarehouseMetricRequest {
    pub fn validate(&self) -> Result<(), Failure> {
        if self.from > self.to || (self.to - self.from).num_days() > 366 {
            return Err(Failure::invalid(
                "from and to must be ordered dates within a 366-day range.",
            ));
        }
        if !(1..=1_000).contains(&self.limit) {
            return Err(Failure::invalid("limit must be between 1 and 1000."));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct WarehouseMetricResult {
    pub project_id: String,
    pub location: String,
    pub metric: String,
    pub rows: Vec<Value>,
}

#[derive(Clone)]
struct WarehouseClient {
    http: reqwest::Client,
    token_provider: Arc<DefaultTokenSourceProvider>,
    project_id: String,
    dataset_id: String,
    location: String,
    max_bytes_billed: u64,
}

#[derive(Deserialize)]
struct BigQueryQueryResponse {
    #[serde(rename = "jobComplete")]
    job_complete: bool,
    #[serde(default)]
    rows: Vec<BigQueryRow>,
    #[serde(rename = "errorResult")]
    error_result: Option<Value>,
    #[serde(default)]
    errors: Vec<Value>,
}

#[derive(Deserialize)]
struct BigQueryRow {
    f: Vec<BigQueryCell>,
}

#[derive(Deserialize)]
struct BigQueryCell {
    v: Value,
}

impl WarehouseClient {
    async fn from_env() -> anyhow::Result<Option<Self>> {
        let configured = [
            "XERO_MCP_WAREHOUSE_PROJECT",
            "XERO_MCP_WAREHOUSE_DATASET",
            "XERO_MCP_WAREHOUSE_LOCATION",
        ]
        .into_iter()
        .map(|name| {
            (
                name,
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty()),
            )
        })
        .collect::<Vec<_>>();
        if configured.iter().all(|(_, value)| value.is_none()) {
            return Ok(None);
        }
        anyhow::ensure!(
            configured.iter().all(|(_, value)| value.is_some()),
            "XERO_MCP_WAREHOUSE_PROJECT, XERO_MCP_WAREHOUSE_DATASET and XERO_MCP_WAREHOUSE_LOCATION must be set together"
        );
        let project_id = configured[0].1.clone().expect("checked configured project");
        let dataset_id = configured[1].1.clone().expect("checked configured dataset");
        let location = configured[2]
            .1
            .clone()
            .expect("checked configured location");
        validate_project_id(&project_id)?;
        validate_dataset_id(&dataset_id)?;
        validate_location(&location)?;
        let max_bytes_billed = std::env::var("XERO_MCP_WAREHOUSE_MAX_BYTES_BILLED")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()?
            .unwrap_or(DEFAULT_WAREHOUSE_MAX_BYTES_BILLED);
        anyhow::ensure!(
            max_bytes_billed > 0,
            "XERO_MCP_WAREHOUSE_MAX_BYTES_BILLED must be positive"
        );
        let token_provider = DefaultTokenSourceProvider::new(
            GoogleAuthConfig::default().with_scopes(CLOUD_PLATFORM_SCOPE),
        )
        .await?;
        Ok(Some(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(65))
                .build()?,
            token_provider: Arc::new(token_provider),
            project_id,
            dataset_id,
            location,
            max_bytes_billed,
        }))
    }

    async fn overview(&self, tenant_ids: &[String]) -> Result<WarehouseOverview, Failure> {
        if tenant_ids.is_empty() {
            return Err(Failure::new(
                "warehouse_access_required",
                "Warehouse overview requires an authorized Xero tenant.",
                false,
            ));
        }
        let table_names = self.tenant_scoped_tables()?;
        if table_names.is_empty() {
            return Err(Failure::new(
                "warehouse_unavailable",
                "The configured Xero warehouse has no tenant-scoped tables.",
                false,
            ));
        }
        let rows = self.count_rows(&table_names, tenant_ids).await?;
        let tables = rows
            .into_iter()
            .map(|(table_name, row_count)| WarehouseTableCount {
                table_name,
                row_count,
            })
            .collect();
        Ok(WarehouseOverview {
            project_id: self.project_id.clone(),
            dataset_id: self.dataset_id.clone(),
            location: self.location.clone(),
            tables,
        })
    }

    async fn metric(
        &self,
        tenant_ids: &[String],
        request: &WarehouseMetricRequest,
    ) -> Result<WarehouseMetricResult, Failure> {
        if tenant_ids.is_empty() {
            return Err(Failure::new(
                "warehouse_access_required",
                "Warehouse metrics require an authorized Xero tenant.",
                false,
            ));
        }
        request.validate()?;
        let query = self.metric_query(request);
        let rows = self.query(&query, Some(tenant_ids)).await?;
        let rows = rows
            .iter()
            .map(product_volume_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(WarehouseMetricResult {
            project_id: self.project_id.clone(),
            location: self.location.clone(),
            metric: request.metric.as_str().into(),
            rows,
        })
    }

    fn metric_query(&self, request: &WarehouseMetricRequest) -> String {
        let from = request.from.format("%F");
        let to = request.to.format("%F");
        match request.metric {
            WarehouseMetric::ProductVolumeByMonth => format!(
                "WITH document_lines AS (\
                   SELECT l.tenant_id, i.document_date, l.item_code, l.description AS product_description, \
                          l.currency_code, c.category_name AS department_category, o.option_name AS department_name, \
                          CAST(l.quantity AS NUMERIC) AS signed_quantity, \
                          CAST(l.amount_in_currency AS NUMERIC) AS signed_net_amount, \
                          1 AS document_line_count, TRUE AS is_product_attributed \
                     FROM `{}.xero_gold_finance.fact_customer_invoice_line` AS l \
                     JOIN `{}.xero_gold_finance.fact_customer_invoice` AS i \
                       ON l.tenant_id = i.tenant_id AND l.invoice_id = i.invoice_id \
                     LEFT JOIN `{}.xero_gold_common.dim_tracking_option` AS o \
                       ON l.tracking_option_1_key = o.tracking_option_key \
                     LEFT JOIN `{}.xero_gold_common.dim_tracking_category` AS c \
                       ON l.tracking_category_1_key = c.tracking_category_key \
                    WHERE l.tenant_id IN UNNEST(@tenant_ids) \
                      AND l.is_reportable AND i.is_reportable \
                      AND i.document_date BETWEEN DATE '{from}' AND DATE '{to}' \
                   UNION ALL \
                   SELECT l.tenant_id, cnote.document_date, NULL AS item_code, l.description AS product_description, \
                          l.currency_code, c.category_name AS department_category, o.option_name AS department_name, \
                          -CAST(l.quantity AS NUMERIC) AS signed_quantity, \
                          -CAST(l.amount_in_currency AS NUMERIC) AS signed_net_amount, \
                          1 AS document_line_count, FALSE AS is_product_attributed \
                     FROM `{}.xero_gold_finance.fact_customer_credit_note_line` AS l \
                     JOIN `{}.xero_gold_finance.fact_customer_credit_note` AS cnote \
                       ON l.tenant_id = cnote.tenant_id AND l.credit_note_id = cnote.credit_note_id \
                     LEFT JOIN `{}.xero_gold_common.dim_tracking_option` AS o \
                       ON l.tracking_option_1_key = o.tracking_option_key \
                     LEFT JOIN `{}.xero_gold_common.dim_tracking_category` AS c \
                       ON l.tracking_category_1_key = c.tracking_category_key \
                    WHERE l.tenant_id IN UNNEST(@tenant_ids) \
                      AND l.is_reportable AND cnote.is_reportable \
                      AND cnote.document_date BETWEEN DATE '{from}' AND DATE '{to}' \
                 ) \
                 SELECT tenant_id, DATE_TRUNC(document_date, MONTH) AS month_start, item_code, \
                        product_description, currency_code, department_category, department_name, \
                        is_product_attributed, SUM(signed_quantity) AS net_quantity, \
                        SUM(signed_net_amount) AS net_amount_in_currency, \
                        SUM(document_line_count) AS document_line_count \
                   FROM document_lines \
                  GROUP BY 1, 2, 3, 4, 5, 6, 7, 8 \
                  ORDER BY month_start DESC, tenant_id, net_amount_in_currency DESC \
                  LIMIT {}",
                self.project_id,
                self.project_id,
                self.project_id,
                self.project_id,
                self.project_id,
                self.project_id,
                self.project_id,
                self.project_id,
                request.limit,
            ),
        }
    }

    fn tenant_scoped_tables(&self) -> Result<Vec<String>, Failure> {
        if WAREHOUSE_OVERVIEW_TABLES.len() > MAX_WAREHOUSE_TABLES {
            return Err(Failure::new(
                "warehouse_unavailable",
                "The warehouse table limit is exceeded.",
                false,
            ));
        }
        Ok(WAREHOUSE_OVERVIEW_TABLES
            .iter()
            .map(|table_name| (*table_name).to_owned())
            .collect())
    }

    async fn count_rows(
        &self,
        table_names: &[String],
        tenant_ids: &[String],
    ) -> Result<Vec<(String, u64)>, Failure> {
        let statements = table_names
            .iter()
            .map(|table_name| {
                format!(
                    "SELECT '{}' AS table_name, COUNT(*) AS row_count FROM `{}.{}.{}` WHERE tenant_id IN UNNEST(@tenant_ids)",
                    table_name, self.project_id, self.dataset_id, table_name
                )
            })
            .collect::<Vec<_>>();
        let query = format!(
            "SELECT table_name, row_count FROM ({}) ORDER BY table_name",
            statements.join(" UNION ALL ")
        );
        let rows = self.query(&query, Some(tenant_ids)).await?;
        let mut counts = Vec::with_capacity(rows.len());
        for row in rows {
            let table_name = string_cell(&row, 0)?;
            let row_count = string_cell(&row, 1)?.parse::<u64>().map_err(|_| {
                Failure::new(
                    "warehouse_unavailable",
                    "The warehouse returned an invalid row count.",
                    false,
                )
            })?;
            counts.push((table_name, row_count));
        }
        if counts.len() != table_names.len() {
            return Err(Failure::new(
                "warehouse_unavailable",
                "The warehouse returned an incomplete count overview.",
                true,
            ));
        }
        Ok(counts)
    }

    async fn query(
        &self,
        query: &str,
        tenant_ids: Option<&[String]>,
    ) -> Result<Vec<BigQueryRow>, Failure> {
        let authorization = self
            .token_provider
            .token_source()
            .token()
            .await
            .map_err(|_| {
                Failure::new(
                    "warehouse_unavailable",
                    "The MCP service cannot acquire Google credentials for the warehouse.",
                    true,
                )
            })?;
        let mut body = json!({
            "query": query,
            "useLegacySql": false,
            "useQueryCache": false,
            "timeoutMs": 55000,
            "location": self.location,
            "maximumBytesBilled": self.max_bytes_billed.to_string(),
        });
        if let Some(tenant_ids) = tenant_ids {
            body.as_object_mut()
                .expect("warehouse query body is an object")
                .insert(
                    "queryParameters".into(),
                    json!([{
                        "name": "tenant_ids",
                        "parameterType": {"type": "ARRAY", "arrayType": {"type": "STRING"}},
                        "parameterValue": {"arrayValues": tenant_ids.iter().map(|tenant_id| json!({"value": tenant_id})).collect::<Vec<_>>()},
                    }]),
                );
        }
        let response = self
            .http
            .post(format!(
                "{BIGQUERY_API}/projects/{}/queries",
                self.project_id
            ))
            .header(AUTHORIZATION, authorization)
            .json(&body)
            .send()
            .await
            .map_err(|_| {
                Failure::new(
                    "warehouse_unavailable",
                    "The MCP service could not reach BigQuery.",
                    true,
                )
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(warehouse_http_failure(status));
        }
        let result: BigQueryQueryResponse = response.json().await.map_err(|_| {
            Failure::new(
                "warehouse_unavailable",
                "BigQuery returned an invalid warehouse response.",
                true,
            )
        })?;
        if result.error_result.is_some() || !result.errors.is_empty() {
            return Err(Failure::new(
                "warehouse_query_failed",
                "BigQuery could not complete the warehouse query.",
                false,
            ));
        }
        if !result.job_complete {
            return Err(Failure::new(
                "warehouse_query_pending",
                "BigQuery did not complete the warehouse query within the MCP time limit; retry shortly.",
                true,
            ));
        }
        Ok(result.rows)
    }
}

fn validate_project_id(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.len() <= 63
            && value.chars().all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'),
        "XERO_MCP_WAREHOUSE_PROJECT is invalid"
    );
    Ok(())
}

fn validate_dataset_id(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        is_dataset_identifier(value),
        "XERO_MCP_WAREHOUSE_DATASET is invalid"
    );
    Ok(())
}

fn validate_location(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 63
            && value.chars().all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'),
        "XERO_MCP_WAREHOUSE_LOCATION is invalid"
    );
    Ok(())
}

fn is_dataset_identifier(value: &str) -> bool {
    value.len() <= 1_024
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn string_cell(row: &BigQueryRow, index: usize) -> Result<String, Failure> {
    let value = row
        .f
        .get(index)
        .map(|cell| &cell.v)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::new(
                "warehouse_unavailable",
                "The warehouse returned an invalid overview field.",
                false,
            )
        })?;
    Ok(value.to_owned())
}

fn optional_string_cell(row: &BigQueryRow, index: usize) -> Result<Option<String>, Failure> {
    let value = row.f.get(index).map(|cell| &cell.v).ok_or_else(|| {
        Failure::new(
            "warehouse_unavailable",
            "The warehouse returned an incomplete metric row.",
            false,
        )
    })?;
    if value.is_null() {
        return Ok(None);
    }
    value.as_str().map(str::to_owned).map(Some).ok_or_else(|| {
        Failure::new(
            "warehouse_unavailable",
            "The warehouse returned an invalid metric field.",
            false,
        )
    })
}

fn product_volume_row(row: &BigQueryRow) -> Result<Value, Failure> {
    let is_product_attributed = string_cell(row, 7)?.parse::<bool>().map_err(|_| {
        Failure::new(
            "warehouse_unavailable",
            "The warehouse returned an invalid product attribution flag.",
            false,
        )
    })?;
    Ok(json!({
        "tenant_id": string_cell(row, 0)?,
        "month_start": string_cell(row, 1)?,
        "item_code": optional_string_cell(row, 2)?,
        "product_description": optional_string_cell(row, 3)?,
        "currency_code": optional_string_cell(row, 4)?,
        "department_category": optional_string_cell(row, 5)?,
        "department_name": optional_string_cell(row, 6)?,
        "is_product_attributed": is_product_attributed,
        "net_quantity": string_cell(row, 8)?,
        "net_amount_in_currency": string_cell(row, 9)?,
        "document_line_count": string_cell(row, 10)?,
    }))
}

fn warehouse_http_failure(status: reqwest::StatusCode) -> Failure {
    match status.as_u16() {
        401 | 403 => Failure::new(
            "warehouse_access_denied",
            "The MCP service is not authorized to read the configured Xero warehouse.",
            false,
        ),
        429 | 500..=599 => Failure::new(
            "warehouse_unavailable",
            "BigQuery is temporarily unavailable for the warehouse query.",
            true,
        ),
        _ => Failure::new(
            "warehouse_query_failed",
            "BigQuery rejected the warehouse query.",
            false,
        ),
    }
}

#[async_trait]
pub trait Backend: Send + Sync {
    async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure>;
    async fn fetch(&self, request: &Query) -> Result<Response, Failure>;
    async fn write(&self, _command: &WriteCommand) -> Result<Value, Failure> {
        Err(Failure::new(
            "writes_unavailable",
            "Xero writes are not configured for this service.",
            false,
        ))
    }
    async fn extract(
        &self,
        job_id: Uuid,
        tenant: &str,
        plan: &ExtractionPlan,
        destination: Destination,
        root: &Path,
    ) -> Result<Value, Failure>;
    async fn warehouse_overview(
        &self,
        _tenant_ids: &[String],
    ) -> Result<WarehouseOverview, Failure> {
        Err(Failure::new(
            "warehouse_unavailable",
            "Warehouse overview is not configured for this MCP service.",
            false,
        ))
    }
    async fn warehouse_metric(
        &self,
        _tenant_ids: &[String],
        _request: &WarehouseMetricRequest,
    ) -> Result<WarehouseMetricResult, Failure> {
        Err(Failure::new(
            "warehouse_unavailable",
            "Warehouse metrics are not configured for this MCP service.",
            false,
        ))
    }
}

pub struct LiveBackend {
    http: reqwest::Client,
    auth: Arc<dyn TokenProvider>,
    connections: Vec<ConnectionInfo>,
    pkce_store: Option<Arc<dyn ConnectionStore>>,
    send_tenant_header: bool,
    warehouse: Option<WarehouseClient>,
}

impl LiveBackend {
    pub async fn from_env() -> anyhow::Result<Self> {
        Self::from_env_with_store(None).await
    }

    pub async fn from_env_with_store(
        shared_store: Option<Arc<dyn ConnectionStore>>,
    ) -> anyhow::Result<Self> {
        let warehouse = WarehouseClient::from_env().await?;
        let mode = std::env::var("XERO_AUTH_MODE").unwrap_or_else(|_| "pkce".into());
        match mode.as_str() {
            "pkce" => {
                let id = std::env::var("XERO_PKCE_CLIENT_ID")
                    .map_err(|_| anyhow::anyhow!("XERO_PKCE_CLIENT_ID is required"))?;
                anyhow::ensure!(!id.trim().is_empty(), "XERO_PKCE_CLIENT_ID is required");
                let secret = std::env::var("XERO_PKCE_CLIENT_SECRET")
                    .ok()
                    .filter(|s| !s.is_empty());
                let store = match shared_store {
                    Some(store) => store,
                    None => xero_store::open_from_env().await?,
                };
                Ok(Self {
                    http: XeroApiClient::new_http_client()?,
                    auth: Arc::new(xero_auth::PkceTokenProvider::new(id, secret, store.clone())),
                    connections: Vec::new(),
                    pkce_store: Some(store),
                    send_tenant_header: true,
                    warehouse,
                })
            }
            "custom" | "cc" | "client_credentials" => {
                let configs = xero_common::AppConfig::from_env()?.xero_cc_connections;
                let connections = configs
                    .iter()
                    .map(|c| ConnectionInfo {
                        tenant_id: c.tenant_id.clone(),
                        tenant_name: c.tenant_name.clone().unwrap_or_default(),
                        scopes: Vec::new(),
                        auth_mode: "custom".into(),
                    })
                    .collect();
                Ok(Self {
                    http: XeroApiClient::new_http_client()?,
                    auth: Arc::new(xero_auth::MultiTenantCustomConnectionClient::new(configs)),
                    connections,
                    pkce_store: None,
                    send_tenant_header: false,
                    warehouse,
                })
            }
            _ => anyhow::bail!("XERO_AUTH_MODE must be pkce or custom"),
        }
    }
    async fn ensure_connected(&self, tenant: &str) -> Result<(), Failure> {
        if self
            .connections()
            .await?
            .iter()
            .any(|c| c.tenant_id == tenant)
        {
            Ok(())
        } else {
            Err(Failure::new(
                "tenant_not_connected",
                "The selected Xero tenant is not connected to this service.",
                false,
            ))
        }
    }

    fn api_client(&self, tenant: &str, family: &str) -> XeroApiClient {
        XeroApiClient::new_with_http_client(
            tenant,
            self.send_tenant_header && family != "identity",
            self.http.clone(),
        )
    }
}

#[async_trait]
impl Backend for LiveBackend {
    async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure> {
        let Some(store) = &self.pkce_store else {
            return Ok(self.connections.clone());
        };
        let records = store.list().await.map_err(|_| {
            Failure::new(
                "connection_store_unavailable",
                "Connected Xero organizations are temporarily unavailable.",
                true,
            )
        })?;
        Ok(records
            .into_iter()
            .map(|record| ConnectionInfo {
                tenant_id: record.tenant_id,
                tenant_name: record.tenant_name,
                scopes: record.scopes,
                auth_mode: "pkce".into(),
            })
            .collect())
    }

    async fn fetch(&self, request: &Query) -> Result<Response, Failure> {
        request.validate()?;
        self.ensure_connected(&request.tenant_id).await?;
        let op = catalog::operation(&request.family, &request.operation_id)?;
        let prepared = op.prepare(&request.path, &request.query, &request.header)?;
        let token = self
            .auth
            .get_valid_token(&request.tenant_id)
            .await
            .map_err(Failure::upstream)?;
        let api = self.api_client(&request.tenant_id, &request.family);
        let response = api
            .fetch_accounting_get_with_limit(
                &token.access_token,
                &prepared,
                crate::results::MAX_RESULT_BYTES,
            )
            .await
            .map_err(Failure::upstream)?;
        let body = scoped_response_body(request, response.http_status, response.body)?;
        Ok(Response {
            body,
            content_type: response.content_type,
            http_status: response.http_status,
            fetched_at: response.fetched_at,
        })
    }

    async fn write(&self, command: &WriteCommand) -> Result<Value, Failure> {
        self.ensure_connected(&command.tenant_id).await?;
        let token = self
            .auth
            .get_valid_token(&command.tenant_id)
            .await
            .map_err(Failure::upstream)?;
        let base_url = writes::write_base_url(&command.action)
            .ok_or_else(|| Failure::invalid("Invalid allowlisted Xero write action."))?;
        let mut url = reqwest::Url::parse(&format!("{}/", base_url.trim_end_matches('/')))
            .map_err(|_| Failure::invalid("Invalid allowlisted Xero write base URL."))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| Failure::invalid("Invalid Xero Accounting path."))?;
            segments.pop_if_empty();
            for segment in command.path.split('/') {
                segments.push(segment);
            }
        }
        let mut request = self
            .http
            .request(command.method.clone(), url)
            .bearer_auth(&token.access_token)
            .header("Accept", "application/json")
            .header("Idempotency-Key", &command.idempotency_key);
        if !command.body.is_null() {
            request = request.json(&command.body);
        }
        if self.send_tenant_header {
            request = request.header("Xero-tenant-id", &command.tenant_id);
        }
        let limiter = xero_client::rate_limit::for_tenant(&command.tenant_id);
        let _permit = limiter.acquire().await.map_err(|_| {
            Failure::new(
                "write_capacity",
                "Xero rate-limit capacity is unavailable; no write was sent.",
                true,
            )
        })?;
        let mut response = request.send().await.map_err(|_| {
            Failure::new(
                "write_outcome_uncertain",
                "Xero may have processed the request; inspect Xero before preparing another write.",
                false,
            )
        })?;
        if limiter
            .update_from_headers(response.headers())
            .await
            .is_err()
        {
            tracing::error!(tenant_id = %command.tenant_id, "Xero quota state failed to update after write response");
        }
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let wait = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(60)
                .min(3600);
            if limiter
                .defer_for(std::time::Duration::from_secs(wait))
                .await
                .is_err()
            {
                tracing::error!(tenant_id = %command.tenant_id, "Xero write rate-limit cooldown could not be stored");
            }
            return Err(Failure::new(
                "xero_rate_limited",
                "Xero rate-limited the write. Inspect its status before preparing another request.",
                false,
            ));
        }
        if !status.is_success() {
            return Err(write_http_failure(status));
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(
                json!({"http_status":status.as_u16(),"action":command.action,"resource_ids":[]}),
            );
        }
        if response
            .content_length()
            .is_some_and(|size| size > 32 * 1024)
        {
            return Err(Failure::new("write_outcome_uncertain", "Xero accepted the write, but its response exceeded the inspection limit; inspect Xero before another write.", false));
        }
        let mut bytes = BytesMut::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            Failure::new(
                "write_outcome_uncertain",
                "Xero may have processed the request; inspect Xero before preparing another write.",
                false,
            )
        })? {
            if bytes.len().saturating_add(chunk.len()) > 32 * 1024 {
                return Err(Failure::new("write_outcome_uncertain", "Xero accepted the write, but its response exceeded the inspection limit; inspect Xero before another write.", false));
            }
            bytes.extend_from_slice(&chunk);
        }
        summarize_write_response(&command.action, status.as_u16(), &bytes)
    }

    async fn extract(
        &self,
        job_id: Uuid,
        tenant: &str,
        plan: &ExtractionPlan,
        destination: Destination,
        root: &Path,
    ) -> Result<Value, Failure> {
        self.ensure_connected(tenant).await?;
        execute_extract(self, job_id, tenant, plan, destination, root).await.map_err(|_| Failure::new("extraction_failed", "Extraction failed; inspect the server's private raw run manifests for diagnostics.", true))
    }

    async fn warehouse_overview(
        &self,
        tenant_ids: &[String],
    ) -> Result<WarehouseOverview, Failure> {
        let warehouse = self.warehouse.as_ref().ok_or_else(|| {
            Failure::new(
                "warehouse_unavailable",
                "Warehouse overview is not configured for this MCP service.",
                false,
            )
        })?;
        warehouse.overview(tenant_ids).await
    }

    async fn warehouse_metric(
        &self,
        tenant_ids: &[String],
        request: &WarehouseMetricRequest,
    ) -> Result<WarehouseMetricResult, Failure> {
        let warehouse = self.warehouse.as_ref().ok_or_else(|| {
            Failure::new(
                "warehouse_unavailable",
                "Warehouse metrics are not configured for this MCP service.",
                false,
            )
        })?;
        warehouse.metric(tenant_ids, request).await
    }
}

fn write_http_failure(status: reqwest::StatusCode) -> Failure {
    if status.is_server_error() || status == reqwest::StatusCode::REQUEST_TIMEOUT {
        Failure::new("write_outcome_uncertain", "Xero returned an error after the write was sent; inspect Xero before preparing another request.", false)
    } else {
        Failure::new("xero_write_rejected", "Xero rejected the write. Review the tenant's scope, record state and Xero validation details outside MCP.", false)
    }
}

fn summarize_write_response(action: &str, status: u16, bytes: &[u8]) -> Result<Value, Failure> {
    let body: Value = serde_json::from_slice(bytes).map_err(|_| Failure::new("write_outcome_uncertain", "Xero accepted the write, but its response could not be verified; inspect Xero before another request.", false))?;
    let item_has_errors = contains_validation_errors(&body, 0);
    if body.get("HasErrors") == Some(&json!(true))
        || body.get("ErrorNumber").is_some()
        || item_has_errors
    {
        return Err(Failure::new(
            "xero_write_rejected",
            "Xero reported validation errors for the write; inspect Xero before retrying.",
            false,
        ));
    }
    let expected_collection = writes::write_response_collection(action);
    let resource_ids = expected_collection
        .map(|(collection, id_field)| collect_resource_ids(&body, collection, id_field))
        .unwrap_or_default();
    if expected_collection.is_some() && resource_ids.is_empty() {
        return Err(Failure::new("write_outcome_uncertain", "Xero accepted the write, but its resource ID could not be verified; inspect Xero before another request.", false));
    }
    let mut response = json!({"http_status":status,"action":action,"resource_ids":resource_ids});
    if let Some(id) = response["resource_ids"]
        .as_array()
        .and_then(|ids| (ids.len() == 1).then(|| ids[0].clone()))
    {
        response["resource_id"] = id;
    }
    Ok(response)
}

fn collect_resource_ids(body: &Value, collection: &str, id_field: &str) -> Vec<String> {
    let collection_value = object_field(body, collection);
    let values = match collection_value {
        Some(Value::Array(items)) => items.iter().collect::<Vec<_>>(),
        Some(Value::Object(_)) => collection_value.into_iter().collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .filter_map(|item| object_field(item, id_field).and_then(Value::as_str))
        .filter(|id| Uuid::parse_str(id).is_ok())
        .map(str::to_owned)
        .collect()
}

fn object_field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value))
}

fn contains_validation_errors(value: &Value, depth: usize) -> bool {
    if depth > 12 {
        return true;
    }
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| contains_validation_errors(value, depth + 1)),
        Value::Object(values) => {
            values.get("HasValidationErrors") == Some(&json!(true))
                || values.get("HasErrors") == Some(&json!(true))
                || values
                    .get("ValidationErrors")
                    .and_then(Value::as_array)
                    .is_some_and(|errors| !errors.is_empty())
                || values
                    .values()
                    .any(|value| contains_validation_errors(value, depth + 1))
        }
        _ => false,
    }
}

fn scoped_response_body(request: &Query, status: u16, body: Bytes) -> Result<Bytes, Failure> {
    if request.family == "identity" && status != 304 {
        filter_connections(&body, &request.tenant_id)
    } else {
        Ok(body)
    }
}

fn filter_connections(body: &[u8], tenant: &str) -> Result<Bytes, Failure> {
    let connections: Vec<Value> = serde_json::from_slice(body).map_err(|_| {
        Failure::new(
            "invalid_xero_response",
            "Xero returned an invalid connection list.",
            true,
        )
    })?;
    let selected = connections
        .into_iter()
        .filter(|connection| connection["tenantId"] == tenant)
        .collect::<Vec<_>>();
    serde_json::to_vec(&selected)
        .map(Bytes::from)
        .map_err(|_| Failure::invalid("Cannot encode Xero connections."))
}

async fn execute_extract(
    backend: &LiveBackend,
    run_id: Uuid,
    tenant: &str,
    plan: &ExtractionPlan,
    destination: Destination,
    root: &Path,
) -> anyhow::Result<Value> {
    let date = Utc::now().date_naive();
    let mut gcs = if destination == Destination::Local {
        GcsConfig {
            bucket: "local".into(),
            prefix: "raw/xero".into(),
        }
    } else {
        GcsConfig::from_env()?
    };
    anyhow::ensure!(
        gcs.prefix == "raw/xero",
        "warehouse extraction requires raw/xero prefix"
    );
    if destination == Destination::Gcs {
        anyhow::ensure!(
            std::env::var("XERO_CONNECTION_STORE_GCS_BUCKET")
                .ok()
                .as_ref()
                != Some(&gcs.bucket),
            "raw data must not share the PKCE token bucket"
        );
    }
    if destination == Destination::Gcs && plan.sensitive()? {
        let variable = if matches!(plan, ExtractionPlan::Operation { family, operation_id, .. } if catalog::operation(family,operation_id)?.binary())
        {
            "XERO_DOCUMENT_GCS_BUCKET"
        } else {
            "XERO_SENSITIVE_GCS_BUCKET"
        };
        let bucket = std::env::var(variable)?;
        anyhow::ensure!(
            !bucket.trim().is_empty()
                && bucket != gcs.bucket
                && std::env::var("XERO_CONNECTION_STORE_GCS_BUCKET")
                    .ok()
                    .as_ref()
                    != Some(&bucket),
            "sensitive destination must be separate"
        );
        gcs.bucket = bucket;
    }
    let folder = root.join("runs").join(run_id.to_string());
    let sink: Arc<dyn RawSink> = match destination {
        Destination::Local => {
            crate::jobs::private_dir(root)?;
            crate::jobs::private_dir(&root.join("runs"))?;
            crate::jobs::private_dir(&folder)?;
            Arc::new(LocalDirSink::new(&folder))
        }
        Destination::Gcs => Arc::new(GcsRawSink::new(gcs.bucket.clone()).await?),
    };
    let names: HashMap<String, String> = backend
        .connections
        .iter()
        .map(|c| (c.tenant_id.clone(), c.tenant_name.clone()))
        .collect();
    let tenants = vec![tenant.to_owned()];
    let manifest: RunManifest = match plan {
        ExtractionPlan::Accounting {
            entities,
            mode,
            from,
            to,
            as_of,
            window_days,
            expand_details,
            max_detail_records,
        } => {
            let entities = entities
                .iter()
                .map(|e| e.parse())
                .collect::<xero_common::Result<Vec<_>>>()?;
            let mut config = SyncConfig::from_env()?;
            config.window_days = *window_days;
            let mut job = xero_sync::SyncJob::new(
                backend.auth.clone(),
                sink.clone(),
                gcs.clone(),
                config,
                names,
                backend.send_tenant_header,
            );
            if *expand_details {
                job = job.with_detail_expansion(*max_detail_records);
            }
            job.run(
                run_id,
                date,
                &tenants,
                &entities,
                crate::jobs::sync_mode(mode, *from, *to, *as_of)?,
                1,
            )
            .await
        }
        ExtractionPlan::Extended {
            family,
            from,
            to,
            bank_account_ids,
            details,
        } => {
            let family = family.parse()?;
            let options = xero_sync::extended::ExtendedOptions {
                from: *from,
                to: *to,
                bank_account_ids: bank_account_ids.clone(),
                details: *details,
            };
            xero_sync::extended::ExtendedSyncJob::new(
                backend.auth.clone(),
                sink.clone(),
                gcs.clone(),
                names,
                backend.send_tenant_header,
            )
            .run(run_id, date, &tenants, &[family], &options)
            .await
        }
        ExtractionPlan::Operation {
            family,
            operation_id,
            path,
            query,
            header,
        } => {
            anyhow::ensure!(family == "accounting", "extended single operations are available through query; use extended extraction for warehouse loading");
            let op = catalog::operation(family, operation_id)?;
            xero_sync::accounting_get::AccountingGetSyncJob::new(
                backend.auth.clone(),
                sink.clone(),
                gcs.clone(),
                names,
                backend.send_tenant_header,
            )
            .run(
                run_id,
                date,
                &tenants,
                operation_id,
                &op.pairs("path", path)?,
                &op.pairs("query", query)?,
                &op.pairs("header", header)?,
            )
            .await?
        }
        ExtractionPlan::Attachments { entity, parent_ids } => {
            let entity = entity.parse()?;
            xero_sync::SyncJob::new(
                backend.auth.clone(),
                sink.clone(),
                gcs.clone(),
                SyncConfig::from_env()?,
                names,
                backend.send_tenant_header,
            )
            .run_attachments(run_id, date, &tenants, &entity, parent_ids)
            .await
        }
    };
    xero_gcs::write_manifest(sink.as_ref(), date, run_id, &manifest).await?;
    let complete = manifest
        .entities
        .iter()
        .all(|e| e.error.is_none() && !e.termination.contains("cap") && e.termination != "error");
    Ok(
        json!({"run_id":run_id,"complete":complete,"complete_collection":false,"destination":destination,"manifest_key":format!("_manifests/xero/{date}/{run_id}.json"),"outcome_count":manifest.entities.len(),"outcomes_truncated":manifest.entities.len()>100,"outcomes":manifest.entities.iter().take(100).map(|e| json!({"endpoint":e.endpoint,"pages":e.pages,"records":e.records,"termination":e.termination,"failed":e.error.is_some()})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warehouse_overview_allowlist_excludes_control_and_legacy_tables() {
        assert!(WAREHOUSE_OVERVIEW_TABLES.contains(&"invoices"));
        assert!(WAREHOUSE_OVERVIEW_TABLES.contains(&"invoice_lines"));
        assert!(WAREHOUSE_OVERVIEW_TABLES.contains(&"credit_notes"));
        assert!(WAREHOUSE_OVERVIEW_TABLES.contains(&"credit_note_lines"));
        assert!(!WAREHOUSE_OVERVIEW_TABLES.iter().any(|name| {
            name.starts_with("raw_") || name.starts_with("v2_") || name.starts_with('_')
        }));
        assert!(WAREHOUSE_OVERVIEW_TABLES
            .iter()
            .all(|name| is_dataset_identifier(name)));
    }

    #[test]
    fn write_response_returns_only_resource_id_and_checks_validation() {
        let id = "5f3c9a2e-0000-0000-0000-000000000000";
        let body = json!({"Contacts":[{"ContactID":id,"BankAccountDetails":"private","EmailAddress":"private@example.com"}]});
        let summary =
            summarize_write_response("create_contact", 200, &serde_json::to_vec(&body).unwrap())
                .unwrap();
        assert_eq!(summary["resource_id"], id);
        assert!(!summary.to_string().contains("private"));
        assert_eq!(
            summarize_write_response("create_contact", 200, b"not-json")
                .unwrap_err()
                .code,
            "write_outcome_uncertain"
        );
        let invalid = json!({"Contacts":[{"ContactID":id,"HasValidationErrors":true,"ValidationErrors":[{"Message":"invalid"}]}]});
        assert_eq!(
            summarize_write_response(
                "create_contact",
                200,
                &serde_json::to_vec(&invalid).unwrap()
            )
            .unwrap_err()
            .code,
            "xero_write_rejected"
        );
        assert_eq!(
            write_http_failure(reqwest::StatusCode::INTERNAL_SERVER_ERROR).code,
            "write_outcome_uncertain"
        );
        assert_eq!(
            write_http_failure(reqwest::StatusCode::BAD_REQUEST).code,
            "xero_write_rejected"
        );
    }

    #[tokio::test]
    async fn repeated_reads_reuse_connection_without_sharing_tenant_or_credentials() {
        use axum::{extract::ConnectInfo, http::HeaderMap, routing::get, Json, Router};
        use std::{collections::HashSet, net::SocketAddr};
        let seen = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let capture = seen.clone();
        let router = Router::new().route(
            "/read",
            get(
                move |ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap| {
                    let capture = capture.clone();
                    async move {
                        capture.lock().await.push((
                            peer,
                            headers
                                .get("xero-tenant-id")
                                .and_then(|v| v.to_str().ok())
                                .map(String::from),
                            headers
                                .get("authorization")
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned(),
                        ));
                        Json(json!({"Invoices":[]}))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/read", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let cancellation = tokio_util::sync::CancellationToken::new();
        let shutdown = cancellation.clone();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
        });
        let backend = backend();
        let prepared = xero_client::accounting_get::PreparedAccountingGet {
            url,
            headers: HeaderMap::new(),
            binary: false,
            accept: "application/json",
        };
        for (tenant, family, token) in [
            (TENANT, "accounting", "first"),
            (
                "aeb4f270-0000-0000-0000-000000000000",
                "accounting",
                "second",
            ),
            (TENANT, "identity", "identity"),
        ] {
            backend
                .api_client(tenant, family)
                .fetch_accounting_get(token, &prepared)
                .await
                .unwrap();
        }
        let requests = seen.lock().await;
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests.iter().map(|v| v.0).collect::<HashSet<_>>().len(),
            1
        );
        assert_eq!(requests[0].1.as_deref(), Some(TENANT));
        assert_ne!(requests[0].1, requests[1].1);
        assert_eq!(requests[2].1, None);
        assert_eq!(
            requests.iter().map(|v| v.2.as_str()).collect::<Vec<_>>(),
            vec!["Bearer first", "Bearer second", "Bearer identity"]
        );
        cancellation.cancel();
        task.await.unwrap();
    }
    const TENANT: &str = "5f3c9a2e-0000-0000-0000-000000000000";
    struct DeniedToken;
    #[async_trait]
    impl TokenProvider for DeniedToken {
        async fn get_valid_token(&self, _: &str) -> xero_common::Result<xero_auth::TokenData> {
            Err(xero_common::Error::Auth(
                "private credential details".into(),
            ))
        }
    }
    fn backend() -> LiveBackend {
        LiveBackend {
            http: XeroApiClient::new_http_client().unwrap(),
            auth: Arc::new(DeniedToken),
            connections: vec![ConnectionInfo {
                tenant_id: TENANT.into(),
                tenant_name: "Fixture".into(),
                scopes: vec![],
                auth_mode: "fixture".into(),
            }],
            pkce_store: None,
            send_tenant_header: true,
            warehouse: None,
        }
    }
    #[test]
    fn provider_errors_do_not_leak_private_diagnostics() {
        for (error, code) in [
            (
                xero_common::Error::Auth("private secret".into()),
                "reconnect_required",
            ),
            (
                xero_common::Error::StateStore("private secret".into()),
                "reconnect_required",
            ),
            (
                xero_common::Error::XeroApi("HTTP 403 private secret".into()),
                "xero_access_denied",
            ),
            (
                xero_common::Error::XeroApi("HTTP 404 private secret".into()),
                "xero_resource_unavailable",
            ),
            (
                xero_common::Error::XeroApi("HTTP 400 private secret".into()),
                "xero_request_rejected",
            ),
            (
                xero_common::Error::XeroApi("HTTP 429 private secret".into()),
                "xero_rate_limited",
            ),
            (
                xero_common::Error::XeroApi("HTTP 500 private secret".into()),
                "xero_service_unavailable",
            ),
            (
                xero_common::Error::XeroApi(
                    "Xero GET response exceeds the configured byte limit".into(),
                ),
                "result_too_large",
            ),
            (
                xero_common::Error::Config("private secret".into()),
                "invalid_arguments",
            ),
        ] {
            let failure = Failure::upstream(error);
            assert_eq!(failure.code, code);
            assert!(!failure.to_string().contains("private secret"));
        }
    }
    #[test]
    fn identity_connections_are_filtered_before_becoming_model_results() {
        let body = serde_json::to_vec(&json!([
            {"tenantId":TENANT,"tenantName":"Allowed"},
            {"tenantId":Uuid::nil(),"tenantName":"Other organisation"},
            {"tenantName":"Unbound organisation"}
        ]))
        .unwrap();
        let filtered: Value =
            serde_json::from_slice(&filter_connections(&body, TENANT).unwrap()).unwrap();
        assert_eq!(filtered.as_array().unwrap().len(), 1);
        assert_eq!(filtered[0]["tenantId"], TENANT);
        assert!(filter_connections(b"{}", TENANT).is_err());
    }
    #[test]
    fn every_identity_success_is_tenant_filtered_and_malformed_bodies_fail_closed() {
        let request: Query = serde_json::from_value(
            json!({"tenant_id":TENANT,"family":"identity","operation_id":"getConnections"}),
        )
        .unwrap();
        let body = Bytes::from(
            serde_json::to_vec(&json!([
                {"tenantId":TENANT},
                {"tenantId":Uuid::nil()}
            ]))
            .unwrap(),
        );
        for status in [200, 202, 206] {
            let filtered: Value = serde_json::from_slice(
                &scoped_response_body(&request, status, body.clone()).unwrap(),
            )
            .unwrap();
            assert_eq!(filtered.as_array().unwrap().len(), 1);
            assert_eq!(filtered[0]["tenantId"], TENANT);
        }
        assert!(scoped_response_body(&request, 204, Bytes::new()).is_err());
        assert!(scoped_response_body(&request, 304, Bytes::new())
            .unwrap()
            .is_empty());
    }
    #[tokio::test]
    async fn connected_scope_and_authentication_fail_before_network() {
        let backend = backend();
        assert_eq!(backend.connections().await.unwrap().len(), 1);
        let request: Query = serde_json::from_value(
            json!({"tenant_id":TENANT,"family":"accounting","operation_id":"getInvoices"}),
        )
        .unwrap();
        assert_eq!(
            backend.fetch(&request).await.err().unwrap().code,
            "reconnect_required"
        );
        let other = Query {
            tenant_id: Uuid::nil().to_string(),
            ..request
        };
        assert_eq!(
            backend.fetch(&other).await.err().unwrap().code,
            "tenant_not_connected"
        );
    }
    #[tokio::test]
    async fn pkce_connection_store_refreshes_connected_tenants_without_restart() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            xero_store::FileConnectionStore::load(directory.path().join("connections.json"))
                .unwrap(),
        );
        let backend = LiveBackend {
            pkce_store: Some(store.clone()),
            ..backend()
        };
        assert!(backend.connections().await.unwrap().is_empty());
        store
            .upsert(xero_store::StoredConnection {
                tenant_id: TENANT.into(),
                tenant_name: "New org".into(),
                tenant_type: "ORGANISATION".into(),
                connected_at: chrono::Utc::now(),
                scopes: vec!["accounting.transactions.read".into()],
                refresh_token: "private-refresh-token".into(),
            })
            .await
            .unwrap();
        let connections = backend.connections().await.unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].tenant_id, TENANT);
        assert_eq!(connections[0].tenant_name, "New org");
        assert!(backend.ensure_connected(TENANT).await.is_ok());
    }
    #[tokio::test]
    async fn native_extraction_plans_persist_failed_manifests_without_reporting_success() {
        let directory = tempfile::tempdir().unwrap();
        let backend = backend();
        for plan in [
            json!({"kind":"accounting","entities":["invoices"],"expand_details":true}),
            json!({"kind":"accounting","entities":["report_balance_sheet"],"mode":"reports","as_of":"2026-01-01"}),
            json!({"kind":"extended","family":"assets"}),
            json!({"kind":"operation","family":"accounting","operation_id":"getInvoices"}),
            json!({"kind":"attachments","entity":"invoices","parent_ids":[TENANT]}),
        ] {
            let plan = serde_json::from_value::<ExtractionPlan>(plan).unwrap();
            plan.validate().unwrap();
            let id = Uuid::new_v4();
            let result = backend
                .extract(id, TENANT, &plan, Destination::Local, directory.path())
                .await
                .unwrap();
            assert_eq!(result["complete"], false);
            assert!(result["outcomes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|outcome| outcome["failed"] == true));
            assert!(directory
                .path()
                .join("runs")
                .join(id.to_string())
                .join(result["manifest_key"].as_str().unwrap())
                .is_file());
            assert!(!result.to_string().contains("private credential"));
        }
    }
}
