//! Pagination for the Visma Expense Public API.
//!
//! Different from ERP REST:
//!   - Zero-based `page` index (not 1-based `pageNumber`)
//!   - `size` param (not `pageSize`)
//!   - Response is Spring Data `Page<T>`: `{ content: [...], last: bool, ... }`
//!   - Watermark filter: `approvalFromDate` (`date-time`, see [`format_api_datetime`])

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use tracing::debug;

use crate::client::ExpenseClient;
use vsv2_common::{Error, PageFetchMetric, Result, TenantId};

/// Pagination options for Expense API endpoints.
#[derive(Debug, Clone, Default)]
pub struct ExpensePageOpts {
    pub page_size: u32,
    pub max_pages: Option<u32>,
    /// `approvalFromDate` filter — only approved on-or-after this timestamp.
    pub approval_from: Option<DateTime<Utc>>,
    /// `approvalToDate` filter — only approved on-or-before this timestamp.
    pub approval_to: Option<DateTime<Utc>>,
}

impl ExpensePageOpts {
    pub fn new(page_size: u32) -> Self {
        Self {
            page_size,
            ..Default::default()
        }
    }
}

#[derive(Debug, Deserialize)]
struct SpringPage {
    content: Vec<serde_json::Value>,
    #[serde(default)]
    last: bool,
    #[serde(rename = "totalElements")]
    total_elements: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PaginatedRecords {
    pub records: Vec<serde_json::Value>,
    pub page_metrics: Vec<PageFetchMetric>,
}

/// Render a timestamp the way the Expense API declares its `date-time` filters
/// (its OpenAPI example is `2022-10-01T12:00:00.000Z`). Its Java `Date` binder
/// answers 400 `typeMismatch` to RFC 3339 with a `+00:00` offset and no
/// milliseconds, which is what `DateTime::to_rfc3339` produces.
fn format_api_datetime(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Walk an Expense API collection page by page until `last == true`.
pub async fn paginate(
    client: &ExpenseClient,
    tenant: &TenantId,
    path: &str,
    opts: &ExpensePageOpts,
) -> Result<Vec<serde_json::Value>> {
    Ok(paginate_with_metrics(client, tenant, path, opts)
        .await?
        .records)
}

pub async fn paginate_with_metrics(
    client: &ExpenseClient,
    tenant: &TenantId,
    path: &str,
    opts: &ExpensePageOpts,
) -> Result<PaginatedRecords> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut page_metrics = Vec::new();
    let mut page: u32 = 0;

    let size_str = opts.page_size.to_string();
    let approval_str = opts.approval_from.map(format_api_datetime);
    let approval_to_str = opts.approval_to.map(format_api_datetime);

    loop {
        let page_str = page.to_string();
        let mut q: Vec<(&str, &str)> = vec![("page", &page_str), ("size", &size_str)];
        if let Some(ref a) = approval_str {
            q.push(("approvalFromDate", a.as_str()));
        }
        if let Some(ref a) = approval_to_str {
            q.push(("approvalToDate", a.as_str()));
        }

        let resp = client.get_json_with_meta(tenant, path, &q).await?;

        let sp: SpringPage = serde_json::from_value(resp.body).map_err(|e| {
            Error::Http(format!(
                "expense: expected Spring Page object from {path}: {e}"
            ))
        })?;

        debug!(page, n = sp.content.len(), last = sp.last, %path, "expense: page");
        page_metrics.push(PageFetchMetric {
            upstream_path: path.to_string(),
            page_param: Some("page".into()),
            page_value: Some(page as i64),
            page_size: Some(opts.page_size),
            status: resp.status,
            elapsed_ms: resp.elapsed_ms,
            records: sp.content.len() as i64,
            total_count: sp.total_elements,
            has_next: Some(!sp.last),
            rate_limit: resp.rate_limit,
            background_mode: None,
        });

        if sp.content.is_empty() {
            break;
        }
        let is_last = sp.last || (sp.content.len() as u32) < opts.page_size;
        out.extend(sp.content);

        if let Some(cap) = opts.max_pages {
            if page + 1 >= cap {
                break;
            }
        }
        if is_last {
            break;
        }
        page += 1;
    }

    Ok(PaginatedRecords {
        records: out,
        page_metrics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn api_datetime_matches_the_documented_openapi_example() {
        let at = Utc.with_ymd_and_hms(2022, 10, 1, 12, 0, 0).unwrap();
        assert_eq!(format_api_datetime(at), "2022-10-01T12:00:00.000Z");
    }

    #[test]
    fn api_datetime_keeps_milliseconds_and_utc_designator() {
        let at = Utc
            .with_ymd_and_hms(2026, 8, 30, 12, 51, 39)
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::milliseconds(7);
        let rendered = format_api_datetime(at);
        assert_eq!(rendered, "2026-08-30T12:51:39.007Z");
        assert!(
            !rendered.contains('+'),
            "a '+' offset breaks the Java Date binder"
        );
    }
}
