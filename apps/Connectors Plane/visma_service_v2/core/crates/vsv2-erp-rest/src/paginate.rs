use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, info, warn};

use crate::client::{BackgroundOpts, ErpRestClient};
use crate::walk_guard;
use vsv2_common::{
    BackgroundMode, Error, PageFetchMetric, PageSink, PaginationProfile, RateLimitSnapshot, Result,
    TenantId,
};

/// Pagination knobs for ERP REST.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageOpts {
    pub page_size: u32,
    pub max_pages: Option<u32>,
    pub pagination_profile: PaginationProfile,
    pub background: BackgroundOpts,
    pub default_query: HashMap<String, serde_json::Value>,
    pub required_query: Vec<String>,
    /// `lastModifiedDateTime >= this`. Sent as ISO-8601.
    pub modified_after: Option<DateTime<Utc>>,
    /// `lastModifiedDateTime <= this`. Sent as ISO-8601.
    pub modified_before: Option<DateTime<Utc>>,
    /// Fields (dotted paths) that make a record unique. With these and an API `totalCount` the
    /// walk checks that no row was lost at a page boundary and, if one was, walks again.
    /// Empty disables the check.
    #[serde(default)]
    pub key_fields: Vec<String>,
}

impl Default for PageOpts {
    fn default() -> Self {
        Self {
            page_size: 100,
            max_pages: None,
            pagination_profile: PaginationProfile::FinancePageNumber,
            background: BackgroundOpts::default(),
            default_query: HashMap::new(),
            required_query: vec![],
            modified_after: None,
            modified_before: None,
            key_fields: vec![],
        }
    }
}

struct ExtractedPage {
    items: Vec<serde_json::Value>,
    /// What the API counts per page: items for most endpoints, journal *lines*
    /// for `/v2/journaltransaction` (see [`page_units`]).
    units: usize,
    total_count: Option<usize>,
    has_next: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct PaginatedRecords {
    pub records: Vec<serde_json::Value>,
    pub page_metrics: Vec<PageFetchMetric>,
    /// Full walks made: 1 normally, more when a walk lost rows at a page boundary.
    pub walks: u32,
    /// Rows a later walk added because an earlier one had lost them.
    pub filled_in: usize,
}

/// One full page-by-page pass. Its records went to the sink as the pages arrived.
struct Walk {
    page_metrics: Vec<PageFetchMetric>,
    /// The API's `totalCount` from the newest page that carried one.
    total_count: Option<usize>,
    /// Records counted in the API's own unit (differs from the record count for journals).
    units_seen: usize,
    /// Records handed to the sink.
    rows: usize,
}

/// What a whole walk leaves behind. Its records went to the sink, not into this summary.
#[derive(Debug, Clone, Default)]
pub struct WalkSummary {
    pub page_metrics: Vec<PageFetchMetric>,
    /// Full walks made: 1 normally, more when a walk lost rows at a page boundary.
    pub walks: u32,
    /// Rows a later walk added because an earlier one had lost them.
    pub filled_in: usize,
    /// Rows handed to the sink: the first walk's, and the rows the repair added.
    pub records: usize,
}

/// Walk an ERP REST collection page-by-page using the endpoint's configured
/// pagination profile.
/// Terminates on:
///   - empty page
///   - page < page_size
///   - upstream `totalCount`/`nextPage` metadata when available
///   - `max_pages` cap
pub async fn paginate(
    client: &ErpRestClient,
    tenant: &TenantId,
    path: &str,
    opts: &PageOpts,
) -> Result<Vec<serde_json::Value>> {
    Ok(paginate_with_metrics(client, tenant, path, opts)
        .await?
        .records)
}

pub async fn paginate_with_metrics(
    client: &ErpRestClient,
    tenant: &TenantId,
    path: &str,
    opts: &PageOpts,
) -> Result<PaginatedRecords> {
    let mut records: Vec<serde_json::Value> = Vec::new();
    let summary = paginate_stream(client, tenant, path, opts, &mut records).await?;
    Ok(PaginatedRecords {
        records,
        page_metrics: summary.page_metrics,
        walks: summary.walks,
        filled_in: summary.filled_in,
    })
}

/// Walk an ERP REST collection page by page, handing each page to `sink` as it arrives.
///
/// The walk checks its own completeness (see `walk_guard`). When it lost a row at a page boundary
/// it walks again and hands over only the rows the first walk never delivered. A row the source
/// served twice is handed over twice, as before: the executor drops the copy.
pub async fn paginate_stream(
    client: &ErpRestClient,
    tenant: &TenantId,
    path: &str,
    opts: &PageOpts,
    sink: &mut dyn PageSink,
) -> Result<WalkSummary> {
    let mut tally = (!opts.key_fields.is_empty()).then(walk_guard::Tally::default);
    let mut first = walk_once(
        client,
        tenant,
        path,
        opts,
        &mut TallyingSink {
            sink: &mut *sink,
            tally: &mut tally,
            key_fields: &opts.key_fields,
        },
    )
    .await?;
    let mut walks = 1;
    let mut filled_in = 0;
    let mut delivered = first.rows;

    let checkable = first.units_seen == first.rows && !opts.key_fields.is_empty();
    if let Some(mut tally) = tally.filter(|_| checkable) {
        for size in walk_guard::rewalk_page_sizes(opts.page_size)
            .into_iter()
            .take(walk_guard::MAX_REWALKS)
        {
            if !walk_guard::fell_short(first.total_count, &tally) {
                break;
            }
            warn!(
                %path,
                total = first.total_count,
                rows = tally.rows,
                distinct = tally.distinct_keys(),
                duplicates = tally.exact_duplicates,
                retry_page_size = size,
                "erp_rest walk lost rows at a page boundary, walking again"
            );
            let mut fill = FillIn {
                sink: &mut *sink,
                tally: &mut tally,
                key_fields: &opts.key_fields,
                added: 0,
            };
            let again = walk_once(
                client,
                tenant,
                path,
                &PageOpts {
                    page_size: size,
                    ..opts.clone()
                },
                &mut fill,
            )
            .await?;
            let added = fill.added;
            walks += 1;
            first.total_count = again.total_count.or(first.total_count);
            first.page_metrics.extend(again.page_metrics);
            filled_in += added;
            delivered += added;
        }
        if walk_guard::fell_short(first.total_count, &tally) {
            return Err(Error::Http(format!(
                "erp_rest {path}: walk incomplete after {walks} walks: {} distinct rows of \
                 totalCount {} ({} rows received, {} served twice)",
                tally.distinct_keys(),
                first.total_count.unwrap_or_default(),
                tally.rows,
                tally.exact_duplicates,
            )));
        }
        if walks > 1 {
            info!(%path, walks, filled_in, "erp_rest walk completed after walking again");
        }
    }

    Ok(WalkSummary {
        page_metrics: first.page_metrics,
        walks,
        filled_in,
        records: delivered,
    })
}

/// Counts each page's rows in the walk's tally, then passes the page on. A row without its key
/// ends the tally: the walk's completeness cannot be judged, so it is not checked.
struct TallyingSink<'a> {
    sink: &'a mut dyn PageSink,
    tally: &'a mut Option<walk_guard::Tally>,
    key_fields: &'a [String],
}

impl PageSink for TallyingSink<'_> {
    fn accept(&mut self, page: Vec<serde_json::Value>) -> Result<()> {
        let keyed = match self.tally.as_mut() {
            Some(tally) => page.iter().all(|record| tally.add(record, self.key_fields)),
            None => true,
        };
        if !keyed {
            *self.tally = None;
        }
        self.sink.accept(page)
    }
}

/// Passes a repair walk's rows on, but only those the first walk did not deliver: a row whose key
/// is new to the tally. A row without a key is not passed on, as before.
struct FillIn<'a> {
    sink: &'a mut dyn PageSink,
    tally: &'a mut walk_guard::Tally,
    key_fields: &'a [String],
    added: usize,
}

impl PageSink for FillIn<'_> {
    fn accept(&mut self, page: Vec<serde_json::Value>) -> Result<()> {
        let tally = &mut *self.tally;
        let key_fields = self.key_fields;
        let fresh: Vec<serde_json::Value> = page
            .into_iter()
            .filter(|record| walk_guard::admit(tally, record, key_fields))
            .collect();
        self.added += fresh.len();
        if fresh.is_empty() {
            return Ok(());
        }
        self.sink.accept(fresh)
    }
}

async fn walk_once(
    client: &ErpRestClient,
    tenant: &TenantId,
    path: &str,
    opts: &PageOpts,
    sink: &mut dyn PageSink,
) -> Result<Walk> {
    let mut total_count: Option<usize> = None;
    let mut page_metrics = Vec::new();
    let mut page: u32 = first_page(opts.pagination_profile);
    let mut pages_seen: u32 = 0;
    let mut units_seen: usize = 0;
    let mut rows: usize = 0;

    let modified_str = opts.modified_after.map(|dt| dt.to_rfc3339());
    let modified_before_str = opts.modified_before.map(|dt| dt.to_rfc3339());

    loop {
        let q = build_query(
            opts,
            page,
            modified_str.as_deref(),
            modified_before_str.as_deref(),
        )?;

        let resp = client
            .get_json_with_meta(tenant, path, &q, &opts.background)
            .await?;
        let status = resp.status;
        let elapsed_ms = resp.elapsed_ms;
        let rate_limit = resp.rate_limit;
        let extracted = extract_page(path, resp.body)?;
        let n = extracted.items.len();
        total_count = extracted.total_count.or(total_count);

        debug!(page, n, %path, "erp_rest: page");
        page_metrics.push(page_metric_from_response(
            path, opts, page, status, elapsed_ms, &extracted, rate_limit,
        ));
        if n == 0 {
            break;
        }
        units_seen += extracted.units;
        rows += n;
        sink.accept(extracted.items)?;
        pages_seen += 1;

        if let Some(cap) = opts.max_pages {
            if pages_seen >= cap {
                break;
            }
        }

        if is_last_page(
            extracted.units,
            units_seen,
            opts.page_size,
            extracted.total_count,
            extracted.has_next,
        ) {
            break;
        }

        if opts.pagination_profile == PaginationProfile::None {
            break;
        }

        page += 1;
    }

    Ok(Walk {
        page_metrics,
        total_count,
        units_seen,
        rows,
    })
}

/// Whether the page just read was the last one.
///
/// `units_*` are in the unit the API paginates and counts in (see
/// [`page_units`]), so `totalCount` and `page_size` are compared like for like.
fn is_last_page(
    units_in_page: usize,
    units_seen: usize,
    page_size: u32,
    total_count: Option<usize>,
    has_next: Option<bool>,
) -> bool {
    if total_count.is_some_and(|total| units_seen >= total) {
        return true;
    }
    match has_next {
        Some(has_next) => !has_next,
        None => (units_in_page as u32) < page_size,
    }
}

/// `/v2/journaltransaction` paginates by journal *line*: a page holds
/// `pageSize` lines that arrive grouped into a handful of batches, and its
/// `totalCount` is a line count. Counting batches made every page look short,
/// so the walk stopped after page one (8 batches / 100 lines of 46,225).
fn page_units(path: &str, items: &[serde_json::Value]) -> usize {
    if !path
        .trim_end_matches('/')
        .eq_ignore_ascii_case("/v2/journaltransaction")
    {
        return items.len();
    }
    items
        .iter()
        .map(|batch| {
            batch
                .get("journalTransactionLines")
                .and_then(serde_json::Value::as_array)
                .map_or(1, Vec::len)
        })
        .sum()
}

fn build_query(
    opts: &PageOpts,
    page: u32,
    modified_after: Option<&str>,
    modified_before: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let mut query: Vec<(String, String)> = Vec::new();
    let sorted: BTreeMap<_, _> = opts.default_query.iter().collect();

    for (key, value) in sorted {
        extend_query_value(&mut query, key, value)?;
    }

    match opts.pagination_profile {
        PaginationProfile::None => {}
        PaginationProfile::NumberToReadSkip => {
            // No pageNumber on these endpoints: read a row count and skip the rows before this page.
            let skipped = u64::from(page.saturating_sub(1)) * u64::from(opts.page_size);
            set_query(&mut query, "numberToRead", opts.page_size.to_string());
            set_query(&mut query, "skipRecords", skipped.to_string());
        }
        PaginationProfile::FinancePageNumber
        | PaginationProfile::SalesOrderPageIndex
        | PaginationProfile::JournalTxPageIndex => {
            set_query(&mut query, "pageSize", opts.page_size.to_string());
            set_query(
                &mut query,
                page_param(opts.pagination_profile),
                page.to_string(),
            );
        }
    }

    // The modified window applies to `none` too: a whole-list endpoint that filters by it stays incremental.
    if let Some(modified_after) = modified_after {
        match opts.pagination_profile {
            PaginationProfile::FinancePageNumber
            | PaginationProfile::NumberToReadSkip
            | PaginationProfile::None => {
                let modified_after = normalize_finance_datetime(modified_after);
                set_query(&mut query, "lastModifiedDateTime", modified_after);
                set_query(
                    &mut query,
                    "lastModifiedDateTimeCondition",
                    ">=".to_string(),
                );
            }
            PaginationProfile::SalesOrderPageIndex | PaginationProfile::JournalTxPageIndex => {
                set_query(&mut query, "modifiedSince", modified_after.to_string());
            }
        }
    }

    if let Some(modified_before) = modified_before {
        match opts.pagination_profile {
            PaginationProfile::FinancePageNumber
            | PaginationProfile::NumberToReadSkip
            | PaginationProfile::None => {
                let modified_before = normalize_finance_datetime(modified_before);
                // Keep the lower bound in API query and enforce the upper
                // bound inside the orchestrator for strict closed windows.
                set_query(&mut query, "lastModifiedDateTimeTo", modified_before);
            }
            PaginationProfile::SalesOrderPageIndex | PaginationProfile::JournalTxPageIndex => {
                set_query(&mut query, "modifiedTo", modified_before.to_string());
            }
        }
    }

    for required in &opts.required_query {
        let present = query
            .iter()
            .any(|(key, value)| key == required && !value.trim().is_empty());
        if !present {
            return Err(Error::Config(format!(
                "required query parameter '{required}' has no value"
            )));
        }
    }

    Ok(query)
}

fn normalize_finance_datetime(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| {
            dt.with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| raw.to_string())
}

fn extend_query_value(
    query: &mut Vec<(String, String)>,
    key: &str,
    value: &serde_json::Value,
) -> Result<()> {
    match value {
        serde_json::Value::Null => Ok(()),
        serde_json::Value::String(s) => {
            query.push((key.to_string(), s.clone()));
            Ok(())
        }
        serde_json::Value::Bool(b) => {
            query.push((key.to_string(), b.to_string()));
            Ok(())
        }
        serde_json::Value::Number(n) => {
            query.push((key.to_string(), n.to_string()));
            Ok(())
        }
        serde_json::Value::Array(values) => {
            for value in values {
                extend_query_value(query, key, value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(_) => Err(Error::Config(format!(
            "default_query.{key} must be a scalar or array of scalars"
        ))),
    }
}

fn set_query(query: &mut Vec<(String, String)>, key: &str, value: String) {
    query.retain(|(existing, _)| existing != key);
    query.push((key.to_string(), value));
}

fn first_page(profile: PaginationProfile) -> u32 {
    match profile {
        PaginationProfile::FinancePageNumber | PaginationProfile::NumberToReadSkip => 1,
        PaginationProfile::SalesOrderPageIndex | PaginationProfile::JournalTxPageIndex => 0,
        PaginationProfile::None => 0,
    }
}

fn page_param(profile: PaginationProfile) -> &'static str {
    match profile {
        PaginationProfile::FinancePageNumber => "pageNumber",
        PaginationProfile::NumberToReadSkip => "skipRecords",
        PaginationProfile::SalesOrderPageIndex | PaginationProfile::JournalTxPageIndex => {
            "pageIndex"
        }
        PaginationProfile::None => "pageNumber",
    }
}

fn page_metric_from_response(
    path: &str,
    opts: &PageOpts,
    page: u32,
    status: u16,
    elapsed_ms: i64,
    extracted: &ExtractedPage,
    rate_limit: RateLimitSnapshot,
) -> PageFetchMetric {
    PageFetchMetric {
        upstream_path: path.to_string(),
        page_param: (opts.pagination_profile != PaginationProfile::None)
            .then(|| page_param(opts.pagination_profile).to_string()),
        page_value: (opts.pagination_profile != PaginationProfile::None).then_some(page as i64),
        page_size: (opts.pagination_profile != PaginationProfile::None).then_some(opts.page_size),
        status,
        elapsed_ms,
        records: extracted.items.len() as i64,
        total_count: extracted.total_count.map(|n| n as i64),
        has_next: extracted.has_next,
        rate_limit,
        background_mode: background_mode_name(opts.background.mode).map(str::to_string),
    }
}

fn background_mode_name(mode: BackgroundMode) -> Option<&'static str> {
    match mode {
        BackgroundMode::Off => None,
        BackgroundMode::None => Some("none"),
        BackgroundMode::Subscription => Some("subscription"),
    }
}

fn extract_page(path: &str, resp: serde_json::Value) -> Result<ExtractedPage> {
    match resp {
        serde_json::Value::Array(items) => {
            let total_count = items
                .first()
                .and_then(|first| first.get("metadata"))
                .and_then(total_count_from);
            Ok(ExtractedPage {
                units: page_units(path, &items),
                items,
                total_count,
                has_next: None,
            })
        }
        serde_json::Value::Object(mut obj) => {
            for key in ["value", "items", "data", "results", "records"] {
                if let Some(value) = obj.remove(key) {
                    let serde_json::Value::Array(items) = value else {
                        return Err(Error::Http(format!(
                            "expected array in {path}.{key}; got {}",
                            short(&value)
                        )));
                    };
                    let total_count = obj
                        .get("totalCount")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as usize)
                        .or_else(|| obj.get("metadata").and_then(total_count_from));
                    let has_next = obj.get("nextPage").map(|v| match v {
                        serde_json::Value::Null => false,
                        serde_json::Value::String(s) => !s.trim().is_empty(),
                        _ => true,
                    });

                    return Ok(ExtractedPage {
                        units: page_units(path, &items),
                        items,
                        total_count,
                        has_next,
                    });
                }
            }

            Err(Error::Http(format!(
                "expected array or paged object from {path}; got {}",
                short(&serde_json::Value::Object(obj))
            )))
        }
        other => Err(Error::Http(format!(
            "expected array or paged object from {path}; got {}",
            short(&other)
        ))),
    }
}

fn total_count_from(value: &serde_json::Value) -> Option<usize> {
    value
        .get("totalCount")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
}

fn short(v: &serde_json::Value) -> String {
    let s = v.to_string();
    s.chars().take(96).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn salesorder_query_uses_zero_based_page_index_and_repeated_array_values() {
        let opts = PageOpts {
            pagination_profile: PaginationProfile::SalesOrderPageIndex,
            default_query: HashMap::from([(
                "expand".to_string(),
                json!(["Customer", "LinesExcludingAllocations"]),
            )]),
            ..Default::default()
        };

        let query = build_query(
            &opts,
            0,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-02T00:00:00Z"),
        )
        .unwrap();

        assert!(query.contains(&("pageIndex".to_string(), "0".to_string())));
        assert!(query.contains(&("pageSize".to_string(), "100".to_string())));
        assert!(query.contains(&(
            "modifiedSince".to_string(),
            "2026-01-01T00:00:00Z".to_string()
        )));
        assert!(query.contains(&("modifiedTo".to_string(), "2026-01-02T00:00:00Z".to_string())));
        assert_eq!(
            query
                .iter()
                .filter(|(key, _)| key == "expand")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            vec!["Customer", "LinesExcludingAllocations"]
        );
    }

    #[test]
    fn numbertoread_query_reads_a_row_count_and_skips_the_rows_before_the_page() {
        let opts = PageOpts {
            page_size: 50,
            pagination_profile: PaginationProfile::NumberToReadSkip,
            ..PageOpts::default()
        };
        let query = build_query(&opts, 3, None, None).unwrap();

        assert!(query.contains(&("numberToRead".to_string(), "50".to_string())));
        assert!(query.contains(&("skipRecords".to_string(), "100".to_string())));
        assert!(!query
            .iter()
            .any(|(k, _)| k == "pageNumber" || k == "pageSize"));
    }

    #[test]
    fn a_whole_list_profile_still_sends_the_modified_window() {
        let opts = PageOpts {
            pagination_profile: PaginationProfile::None,
            ..PageOpts::default()
        };
        let query = build_query(&opts, 1, Some("2026-01-01T00:00:00Z"), None).unwrap();

        assert!(query.contains(&(
            "lastModifiedDateTime".to_string(),
            "2026-01-01 00:00:00".to_string()
        )));
        assert!(!query.iter().any(|(k, _)| k.starts_with("page")));
    }

    #[test]
    fn finance_query_uses_page_number_and_last_modified_condition() {
        let opts = PageOpts::default();
        let query = build_query(
            &opts,
            1,
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-02T00:00:00Z"),
        )
        .unwrap();

        assert!(query.contains(&("pageNumber".to_string(), "1".to_string())));
        assert!(query.contains(&(
            "lastModifiedDateTime".to_string(),
            "2026-01-01 00:00:00".to_string()
        )));
        assert!(query.contains(&(
            "lastModifiedDateTimeCondition".to_string(),
            ">=".to_string()
        )));
        assert!(query.contains(&(
            "lastModifiedDateTimeTo".to_string(),
            "2026-01-02 00:00:00".to_string()
        )));
    }

    /// Eight batches holding exactly one page (100 lines) of a 46,225-line
    /// result: the real shape that made the walk stop after page one.
    fn journal_page_of_100_lines_in_8_batches() -> serde_json::Value {
        let batch = |lines: usize| {
            json!({
                "module": "GL",
                "batchNumber": "059712",
                "journalTransactionLines": vec![json!({"lineNumber": 1}); lines],
                "metadata": {"totalCount": 46225}
            })
        };
        json!([
            batch(22),
            batch(20),
            batch(19),
            batch(12),
            batch(10),
            batch(9),
            batch(4),
            batch(4)
        ])
    }

    #[test]
    fn journal_page_units_are_lines_not_batches() {
        let page = extract_page(
            "/v2/journaltransaction",
            journal_page_of_100_lines_in_8_batches(),
        )
        .unwrap();

        assert_eq!(page.items.len(), 8);
        assert_eq!(page.units, 100);
        assert_eq!(page.total_count, Some(46225));
    }

    #[test]
    fn a_full_journal_page_is_not_the_last_page() {
        let page = extract_page(
            "/v2/journaltransaction",
            journal_page_of_100_lines_in_8_batches(),
        )
        .unwrap();

        assert!(!is_last_page(
            page.units,
            page.units,
            100,
            page.total_count,
            page.has_next
        ));
    }

    #[test]
    fn a_short_journal_page_is_the_last_page() {
        let short = json!([{
            "module": "GL", "batchNumber": "1",
            "journalTransactionLines": vec![json!({}); 37]
        }]);
        let page = extract_page("/v2/journaltransaction/", short).unwrap();

        assert_eq!(page.units, 37);
        assert!(is_last_page(page.units, 46_225, 100, None, page.has_next));
    }

    #[test]
    fn journal_walk_ends_when_all_lines_are_fetched() {
        assert!(is_last_page(100, 46_225, 100, Some(46_225), None));
        assert!(!is_last_page(100, 46_100, 100, Some(46_225), None));
    }

    #[test]
    fn other_endpoints_still_count_items() {
        let items = json!([{"accountCD": "1000"}, {"accountCD": "2000"}]);
        let page = extract_page("/v1/account", items).unwrap();

        assert_eq!(page.units, 2);
        assert!(
            is_last_page(2, 2, 100, None, None),
            "short page ends the walk"
        );
        assert!(
            !is_last_page(100, 100, 100, None, None),
            "full page continues"
        );
        assert!(
            is_last_page(100, 100, 100, Some(100), None),
            "totalCount reached"
        );
        assert!(
            is_last_page(100, 100, 100, Some(500), Some(false)),
            "has_next=false wins"
        );
        assert!(
            !is_last_page(1, 1, 100, Some(500), Some(true)),
            "has_next=true wins"
        );
    }

    #[test]
    fn required_query_missing_value_is_config_error() {
        let opts = PageOpts {
            required_query: vec!["ledger".to_string()],
            ..Default::default()
        };

        let err = build_query(&opts, 1, None, None).unwrap_err();
        assert!(err
            .to_string()
            .contains("required query parameter 'ledger'"));
    }

    #[test]
    fn extracts_salesorder_paged_value_wrapper() {
        let page = extract_page(
            "/api/v3/SalesOrders",
            json!({
                "value": [{"orderNo": "1"}],
                "nextPage": null,
                "totalCount": 1
            }),
        )
        .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.total_count, Some(1));
        assert_eq!(page.has_next, Some(false));
    }

    #[test]
    fn builds_page_metric_from_extracted_page_and_rate_limit_headers() {
        let metric = page_metric_from_response(
            "/v1/customer",
            &PageOpts {
                page_size: 50,
                ..Default::default()
            },
            3,
            200,
            42,
            &ExtractedPage {
                items: vec![serde_json::json!({}); 9],
                units: 9,
                total_count: Some(9),
                has_next: Some(true),
            },
            vsv2_common::RateLimitSnapshot {
                limit: Some(10),
                remaining: Some(7),
                reset: Some(1_777_777_777),
                policy: Some("tenant".into()),
                retry_after_secs: None,
            },
        );

        assert_eq!(metric.upstream_path, "/v1/customer");
        assert_eq!(metric.page_param.as_deref(), Some("pageNumber"));
        assert_eq!(metric.page_value, Some(3));
        assert_eq!(metric.page_size, Some(50));
        assert_eq!(metric.status, 200);
        assert_eq!(metric.elapsed_ms, 42);
        assert_eq!(metric.records, 9);
        assert_eq!(metric.total_count, Some(9));
        assert_eq!(metric.has_next, Some(true));
        assert_eq!(metric.rate_limit.remaining, Some(7));
    }

    // ── walk completeness, end to end against a scripted server ──────────────────────

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use vsv2_auth::{OAuthClient, Scope};
    use vsv2_common::Source;
    use vsv2_ratelimit::Limiter;

    const TENANT: &str = "33f319b9-4d57-11ee-960c-025417856183";

    fn payment_rows(n: usize) -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| json!({"type": "Payment", "refNbr": format!("{i:03}"), "amount": 1.0}))
            .collect()
    }

    /// A server that answers `/v1/payment` pages from `serve(page, page_size)` and counts
    /// them. Every row carries `metadata.totalCount`, as the Finance API's rows do.
    async fn scripted_client(
        total: usize,
        serve: impl Fn(u32, u32) -> Vec<serde_json::Value> + Send + Sync + 'static,
    ) -> (ErpRestClient, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr");
        let served = Arc::new(AtomicUsize::new(0));
        let counter = served.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut buffer = [0_u8; 4096];
                let n = socket.read(&mut buffer).await.expect("read");
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                let body = if request.starts_with("POST /token ") {
                    r#"{"access_token":"t","expires_in":3600}"#.to_string()
                } else {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let query = |name: &str| -> u32 {
                        request
                            .split(&format!("{name}="))
                            .nth(1)
                            .and_then(|rest| rest.split(['&', ' ']).next())
                            .and_then(|v| v.parse().ok())
                            .expect("query parameter")
                    };
                    let mut rows = serve(query("pageNumber"), query("pageSize"));
                    for row in &mut rows {
                        row["metadata"] = json!({"totalCount": total});
                    }
                    serde_json::to_string(&rows).expect("json")
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.expect("write");
            }
        });
        let oauth = OAuthClient::new(format!("http://{address}/token"), "id", "secret")
            .with_scope(Scope::Erp, "financials:read");
        let client = ErpRestClient::new(
            format!("http://{address}"),
            oauth,
            Limiter::new(Source::ErpRest, 1_000, 100_000),
        );
        (client, server, served)
    }

    fn opts(page_size: u32) -> PageOpts {
        PageOpts {
            page_size,
            key_fields: vec!["type".into(), "refNbr".into()],
            ..Default::default()
        }
    }

    /// Row `rows[from..to]` of the fixed data set, in ascending order.
    fn slice(rows: &[serde_json::Value], page: u32, size: u32) -> Vec<serde_json::Value> {
        let start = ((page - 1) * size) as usize;
        rows.iter()
            .skip(start)
            .take(size as usize)
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn a_row_served_twice_at_a_page_boundary_is_recovered_by_a_second_walk() {
        let rows = payment_rows(25);
        let data = rows.clone();
        // With page size 10, page 2 serves row 9 again instead of row 10: the tie flip.
        let (client, server, served) = scripted_client(25, move |page, size| {
            let mut out = slice(&data, page, size);
            if size == 10 && page == 2 {
                out[0] = data[9].clone();
            }
            out
        })
        .await;

        let got = paginate_with_metrics(&client, &TenantId::new(TENANT), "/v1/payment", &opts(10))
            .await
            .expect("the walk must be repaired, not fail");

        let keys: std::collections::HashSet<String> = got
            .records
            .iter()
            .map(|r| r["refNbr"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys.len(), 25, "every payment present after the repair");
        assert_eq!(got.walks, 2);
        assert_eq!(got.filled_in, 1);
        assert_eq!(
            got.records.len(),
            26,
            "the duplicate stays for the executor to drop"
        );
        assert_eq!(
            served.load(Ordering::SeqCst),
            3 + 4,
            "3 pages, then 4 at size 8"
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_permanent_shortfall_is_an_error_after_the_bounded_rewalks() {
        let rows = payment_rows(25);
        // Whatever the page size, the list holds row 9 twice and never row 10.
        let mut faulty = rows.clone();
        faulty[10] = rows[9].clone();
        let (client, server, served) =
            scripted_client(25, move |page, size| slice(&faulty, page, size)).await;

        let err = paginate_with_metrics(&client, &TenantId::new(TENANT), "/v1/payment", &opts(10))
            .await
            .expect_err("rows that never arrive must not pass silently");

        let message = err.to_string();
        assert!(
            message.contains("walk incomplete after 3 walks"),
            "{message}"
        );
        assert!(
            message.contains("24 distinct rows of totalCount 25"),
            "{message}"
        );
        assert_eq!(
            served.load(Ordering::SeqCst),
            3 + 4 + 4,
            "1 walk + 2 re-walks"
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_complete_walk_is_made_once() {
        let rows = payment_rows(25);
        let (client, server, served) =
            scripted_client(25, move |page, size| slice(&rows, page, size)).await;

        let got = paginate_with_metrics(&client, &TenantId::new(TENANT), "/v1/payment", &opts(10))
            .await
            .expect("walk");

        assert_eq!((got.records.len(), got.walks, got.filled_in), (25, 1, 0));
        assert_eq!(served.load(Ordering::SeqCst), 3);
        server.abort();
    }

    #[tokio::test]
    async fn without_key_fields_the_walk_is_not_checked() {
        let rows = payment_rows(25);
        let mut faulty = rows.clone();
        faulty[10] = rows[9].clone();
        let (client, server, served) =
            scripted_client(25, move |page, size| slice(&faulty, page, size)).await;

        let unchecked = PageOpts {
            key_fields: vec![],
            ..opts(10)
        };
        let got = paginate_with_metrics(&client, &TenantId::new(TENANT), "/v1/payment", &unchecked)
            .await
            .expect("no key, no check");

        assert_eq!((got.records.len(), got.walks), (25, 1));
        assert_eq!(served.load(Ordering::SeqCst), 3);
        server.abort();
    }

    /// Keeps each page as it arrives, and how many pages the server had answered by then.
    #[derive(Default)]
    struct Probe {
        served: Option<Arc<AtomicUsize>>,
        pages: Vec<Vec<serde_json::Value>>,
        served_at_arrival: Vec<usize>,
    }

    impl PageSink for Probe {
        fn accept(&mut self, page: Vec<serde_json::Value>) -> Result<()> {
            if let Some(served) = &self.served {
                self.served_at_arrival.push(served.load(Ordering::SeqCst));
            }
            self.pages.push(page);
            Ok(())
        }
    }

    /// Page 2 serves row 9 again instead of row 10 (the tie flip), but only at page size 10.
    fn tie_flip_server(
        rows: &[serde_json::Value],
    ) -> impl Fn(u32, u32) -> Vec<serde_json::Value> + Send + Sync + 'static {
        let data = rows.to_vec();
        move |page, size| {
            let mut out = slice(&data, page, size);
            if size == 10 && page == 2 {
                out[0] = data[9].clone();
            }
            out
        }
    }

    #[tokio::test]
    async fn pages_reach_the_sink_as_the_walk_fetches_them() {
        let rows = payment_rows(25);
        let (client, server, served) =
            scripted_client(25, move |page, size| slice(&rows, page, size)).await;
        let mut probe = Probe {
            served: Some(served),
            ..Default::default()
        };

        let summary = paginate_stream(
            &client,
            &TenantId::new(TENANT),
            "/v1/payment",
            &opts(10),
            &mut probe,
        )
        .await
        .expect("walk");

        assert_eq!(
            probe.served_at_arrival,
            vec![1, 2, 3],
            "each page is handed over before the next one is fetched"
        );
        assert_eq!(
            probe.pages.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![10, 10, 5]
        );
        assert_eq!(summary.records, 25);
        server.abort();
    }

    #[tokio::test]
    async fn the_stream_hands_over_exactly_what_the_collecting_walk_returns() {
        let rows = payment_rows(25);
        let (client, server, _served) = scripted_client(25, tie_flip_server(&rows)).await;
        let collected =
            paginate_with_metrics(&client, &TenantId::new(TENANT), "/v1/payment", &opts(10))
                .await
                .expect("collecting walk");
        let mut probe = Probe::default();

        let summary = paginate_stream(
            &client,
            &TenantId::new(TENANT),
            "/v1/payment",
            &opts(10),
            &mut probe,
        )
        .await
        .expect("streaming walk");

        assert_eq!(probe.pages.concat(), collected.records);
        assert_eq!(
            (summary.records, summary.walks, summary.filled_in),
            (
                collected.records.len(),
                collected.walks,
                collected.filled_in
            )
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_repaired_walk_hands_over_only_the_rows_the_first_walk_lacked() {
        let rows = payment_rows(25);
        let (client, server, _served) = scripted_client(25, tie_flip_server(&rows)).await;
        let mut probe = Probe::default();

        paginate_stream(
            &client,
            &TenantId::new(TENANT),
            "/v1/payment",
            &opts(10),
            &mut probe,
        )
        .await
        .expect("the walk must be repaired, not fail");

        // The first walk delivered three pages, one of them with row 9 twice. The repair walks again
        // at page size 8 and hands over row 10 alone: the only row the first walk never delivered.
        assert_eq!(probe.pages.len(), 4, "three pages, then one repaired row");
        assert_eq!(probe.pages[3].len(), 1);
        assert_eq!(probe.pages[3][0]["refNbr"], "010");
        server.abort();
    }

    /// Fails on the first page, as a sink does when the run cannot take more rows.
    struct Refuse;

    impl PageSink for Refuse {
        fn accept(&mut self, _page: Vec<serde_json::Value>) -> Result<()> {
            Err(Error::Other("sink full".into()))
        }
    }

    #[tokio::test]
    async fn a_refusing_sink_stops_the_walk_at_once_with_its_error() {
        let rows = payment_rows(25);
        let (client, server, served) =
            scripted_client(25, move |page, size| slice(&rows, page, size)).await;

        let err = paginate_stream(
            &client,
            &TenantId::new(TENANT),
            "/v1/payment",
            &opts(10),
            &mut Refuse,
        )
        .await
        .expect_err("the sink's error ends the walk");

        assert!(err.to_string().contains("sink full"), "{err}");
        assert_eq!(
            served.load(Ordering::SeqCst),
            1,
            "no page after the refused one"
        );
        server.abort();
    }
}
