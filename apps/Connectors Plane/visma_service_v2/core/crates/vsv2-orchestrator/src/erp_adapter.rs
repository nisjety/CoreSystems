use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use serde_json::Value;
use tracing::info;
use vsv2_common::{Error, PageSink, PrimaryKey, Result, TenantId};
use vsv2_erp_rest::{
    paginate, paginate_stream, BackgroundOpts, ErpRestClient, PageOpts, WalkSummary,
};

use crate::adapter::{FetchAdapter, FetchPartitionResult, FetchWindow};
use crate::open_set::OpenSetSource;
use crate::partition::{build_partitions_from_dimensions, PartitionPlan};
use crate::registry::{EndpointDefinition, PeriodScanConfig};

pub struct ErpRestAdapter {
    pub client: ErpRestClient,
    pub page_size: u32,
}

#[async_trait]
impl FetchAdapter for ErpRestAdapter {
    async fn plan_partitions(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
    ) -> Result<Vec<PartitionPlan>> {
        if let Some(partitioning) = &def.partitioning {
            let mut dimension_records = HashMap::new();
            for dim in &partitioning.dimensions {
                if dimension_records.contains_key(&dim.source_endpoint) {
                    continue;
                }
                // A dimension must be listed completely: a customer or project the listing
                // misses is one whose rows are never fetched. So its walk is checked against the
                // API's own total, keyed on the value it will supply.
                let key_fields = vec![dim.source_path.clone()];
                let records = paginate(
                    &self.client,
                    tenant,
                    &dim.source_endpoint,
                    &PageOpts {
                        page_size: def.page_size.unwrap_or(self.page_size),
                        pagination_profile: dim
                            .source_pagination_profile
                            .unwrap_or(def.pagination_profile),
                        background: BackgroundOpts::default(),
                        key_fields,
                        ..Default::default()
                    },
                )
                .await?;
                dimension_records.insert(dim.source_endpoint.clone(), records);
            }

            build_partitions_from_dimensions(partitioning, &dimension_records)
        } else {
            Ok(vec![PartitionPlan::default_partition()])
        }
    }

    async fn fetch_partition(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        partition: &PartitionPlan,
        window: FetchWindow,
        sink: &mut dyn PageSink,
    ) -> Result<FetchPartitionResult> {
        let FetchWindow {
            watermark,
            window_end,
            ..
        } = window;
        let template = def
            .api_endpoint
            .as_deref()
            .or(def.api_method.as_deref())
            .ok_or_else(|| {
                Error::Other(format!(
                    "endpoint '{}' has no api_endpoint/api_method",
                    def.endpoint_name
                ))
            })?;
        let resolved = resolve_path(template, &partition.path)?;
        let path = resolved.as_str();
        let mut opts = PageOpts {
            page_size: def.page_size.unwrap_or(self.page_size),
            pagination_profile: def.pagination_profile,
            background: BackgroundOpts {
                mode: def.background_mode,
                poll_secs: def.background_poll_secs.unwrap_or(5),
                timeout_secs: def.background_timeout_secs.unwrap_or(900),
            },
            default_query: def.default_query.clone(),
            required_query: def.required_query.clone(),
            modified_after: watermark,
            modified_before: window_end,
            key_fields: page_key_fields(&def.primary_key)
                .into_iter()
                // added after each month's walk, so the API's rows do not have it yet
                .filter(|field| {
                    def.period_scan
                        .as_ref()
                        .and_then(|scan| scan.period_field.as_deref())
                        != Some(field.as_str())
                })
                .collect(),
            ..Default::default()
        };

        for (key, value) in &partition.query {
            opts.default_query.insert(key.clone(), value.clone());
        }
        let row_copies = def
            .partitioning
            .as_ref()
            .map(|p| &p.row_copies)
            .filter(|copies| !copies.is_empty());
        // Every row is stamped on its way out, so the sink receives each row with its partition.
        let mut stamped = PartitionStamp {
            partition,
            row_copies,
            stamp_partition_key: def.partitioning.is_some() || !partition.is_default(),
            sink,
        };

        // Journal transactions paginate by *line*, not by batch, so a batch can span pages: each
        // page carries a record with the same `(module, batchNumber)` key but a different subset of
        // `journalTransactionLines`. Such a batch is merged before it reaches dedup/MERGE (Bug E
        // lineage), and `walk_unit` gathers its lines first. A period walk is merged within the
        // period, which is exact: no batch spans two periods.
        let merge_batches = is_journal_transactions(def);
        let summary = if let Some(scan) = &def.period_scan {
            self.fetch_financial_periods(tenant, path, &opts, scan, merge_batches, &mut stamped)
                .await?
        } else {
            walk_unit(
                &self.client,
                tenant,
                path,
                &opts,
                None,
                merge_batches,
                &mut stamped,
            )
            .await?
        };
        Ok(FetchPartitionResult {
            page_metrics: summary.page_metrics,
        })
    }

    fn open_set_source(&self) -> Option<&dyn OpenSetSource> {
        Some(self)
    }
}

/// `template` with each `{name}` replaced by the percent-encoded partition value.
///
/// A placeholder without a value, or a value without a placeholder, is a configuration error:
/// either would send a request for the wrong resource.
fn resolve_path(template: &str, values: &HashMap<String, String>) -> Result<String> {
    let mut path = template.to_string();
    for (name, value) in values {
        let placeholder = format!("{{{name}}}");
        if !path.contains(&placeholder) {
            return Err(Error::Config(format!(
                "partition value '{name}' has no {placeholder} in {template}"
            )));
        }
        path = path.replace(&placeholder, &percent_encode(value));
    }
    if let (Some(open), Some(close)) = (path.find('{'), path.find('}')) {
        if open < close {
            return Err(Error::Config(format!(
                "{template} has {} with no partition dimension filling it",
                &path[open..=close]
            )));
        }
    }
    Ok(path)
}

/// RFC 3986 unreserved characters pass through; every other byte becomes `%XX`.
pub(crate) fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Add the partition's own fields to a fetched row, then the configured copies.
///
/// A field the API already returned is kept: the partition only fills what the row lacks.
fn add_partition_fields(
    row: &mut Value,
    inject: &HashMap<String, Value>,
    copies: Option<&std::collections::BTreeMap<String, String>>,
) {
    let Value::Object(obj) = row else { return };
    for (field, value) in inject {
        obj.entry(field.clone()).or_insert_with(|| value.clone());
    }
    for (target, source) in copies.into_iter().flatten() {
        let copied = source
            .split('.')
            .try_fold(&*row, |current, part| current.get(part))
            .filter(|v| !v.is_null())
            .cloned();
        if let (Some(value), Value::Object(obj)) = (copied, &mut *row) {
            obj.entry(target.clone()).or_insert(value);
        }
    }
}

/// The fields that identify a row in the API's own response, for the walk completeness check.
///
/// `tenant_id` and `_partition_key` are added to rows after the fetch, so they are not there yet.
fn page_key_fields(primary_key: &PrimaryKey) -> Vec<String> {
    let fields = match primary_key {
        PrimaryKey::Single(field) => vec![field.clone()],
        PrimaryKey::Composite(fields) => fields.clone(),
    };
    fields
        .into_iter()
        .filter(|f| f != "tenant_id" && f != "_partition_key")
        .collect()
}

/// True for the `/v2/journaltransaction` endpoint, which paginates by line and
/// therefore needs post-pagination batch merging (see `merge_journal_batches`).
fn is_journal_transactions(def: &EndpointDefinition) -> bool {
    def.endpoint_name == "journal_transactions"
        || def
            .api_endpoint
            .as_deref()
            .map(|p| p.eq_ignore_ascii_case("/v2/journaltransaction"))
            .unwrap_or(false)
}

/// The grouping key of a journal record: `(module, batchNumber, financialPeriod)`.
///
/// A batch belongs to one financial period, and a batch number can be used again in a later period.
/// Those are different batches, so the period is part of the key. Without it, the lines of the later
/// batch that share a `lineNumber` with the earlier one would be dropped as duplicates, with no error.
/// Visma occasionally spells the batch field `batchNbr`; accept both. Values are stringified so
/// numeric and string batch numbers collapse to one key.
type BatchKey = (String, String, String);

fn journal_batch_key(record: &Value) -> Option<BatchKey> {
    let obj = record.as_object()?;
    let module = obj
        .get("module")
        .map(json_scalar_to_string)
        .unwrap_or_default();
    let batch = obj
        .get("batchNumber")
        .or_else(|| obj.get("batchNbr"))
        .map(json_scalar_to_string)
        .unwrap_or_default();
    let batch = batch.trim().to_string();
    if batch.is_empty() {
        return None;
    }
    let period = obj
        .get("financialPeriod")
        .map(json_scalar_to_string)
        .unwrap_or_default();
    Some((module.trim().to_string(), batch, period.trim().to_string()))
}

fn json_scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Merge journal records that share a `(module, batchNumber, financialPeriod)` key by
/// concatenating their `journalTransactionLines` arrays, de-duplicating lines
/// by `lineNumber`. Insertion order of first-seen batches is preserved.
///
/// Records without a resolvable batch key (or non-object records) pass through
/// untouched so we never silently drop data.
fn merge_journal_batches(records: Vec<Value>) -> Vec<Value> {
    use std::collections::HashMap;

    let mut order: Vec<BatchKey> = Vec::new();
    let mut merged: HashMap<BatchKey, Value> = HashMap::new();
    let mut passthrough: Vec<Value> = Vec::new();

    for record in records {
        let Some(key) = journal_batch_key(&record) else {
            passthrough.push(record);
            continue;
        };

        match merged.get_mut(&key) {
            None => {
                order.push(key.clone());
                merged.insert(key, record);
            }
            Some(existing) => {
                merge_journal_lines_into(existing, &record);
            }
        }
    }

    let mut out = Vec::with_capacity(order.len() + passthrough.len());
    for key in order {
        if let Some(record) = merged.remove(&key) {
            out.push(record);
        }
    }
    out.extend(passthrough);
    out
}

/// Append `incoming`'s journal lines onto `target`, skipping any line whose
/// `lineNumber` is already present on the target.
fn merge_journal_lines_into(target: &mut Value, incoming: &Value) {
    let incoming_lines = incoming
        .get("journalTransactionLines")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if incoming_lines.is_empty() {
        return;
    }

    let Some(obj) = target.as_object_mut() else {
        return;
    };
    let existing = obj
        .entry("journalTransactionLines")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(existing_lines) = existing.as_array_mut() else {
        return;
    };

    let mut seen: std::collections::HashSet<String> = existing_lines
        .iter()
        .filter_map(|line| line.get("lineNumber").map(json_scalar_to_string))
        .collect();

    for line in incoming_lines {
        let line_key = line.get("lineNumber").map(json_scalar_to_string);
        match line_key {
            Some(k) if seen.contains(&k) => continue,
            Some(k) => {
                seen.insert(k);
                existing_lines.push(line);
            }
            // Lines with no lineNumber can't be deduped; keep them all.
            None => existing_lines.push(line),
        }
    }
}

/// Longest period range one run may walk: 20 years of months.
const MAX_PERIOD_WALK: usize = 240;

impl ErpRestAdapter {
    /// Fetch an endpoint month by month, each month with its own paginated walk.
    ///
    /// A document or balance belongs to exactly one period, so the months are disjoint and every
    /// walk is complete on its own: each gets its own completeness check against the API's
    /// `totalCount`. See `PeriodScanConfig` for why one wide request is not an alternative.
    async fn fetch_financial_periods(
        &self,
        tenant: &TenantId,
        path: &str,
        opts: &PageOpts,
        scan: &PeriodScanConfig,
        merge_batches: bool,
        sink: &mut dyn PageSink,
    ) -> Result<WalkSummary> {
        // The window is the one `opts` already carries (`modified_after`, `modified_before`).
        let (range_start, range_end) =
            period_range(scan, opts.modified_after, opts.modified_before);
        let periods = financial_periods(scan, range_start, range_end, Utc::now())?;
        info!(
            tenant = %tenant,
            path,
            first = %periods[0],
            last = %periods[periods.len() - 1],
            periods = periods.len(),
            "walking financial periods one at a time"
        );

        let mut all = WalkSummary::default();
        for period in &periods {
            let mut month = opts.clone();
            for param in &scan.query_params {
                month
                    .default_query
                    .insert(param.clone(), Value::String(period.clone()));
            }
            let tag = scan
                .period_field
                .as_deref()
                .map(|field| (field, period.as_str()));
            let walk = walk_unit(
                &self.client,
                tenant,
                path,
                &month,
                tag,
                merge_batches,
                &mut *sink,
            )
            .await?;
            info!(tenant = %tenant, period, rows = walk.records, "financial period fetched");
            all.page_metrics.extend(walk.page_metrics);
            all.walks += walk.walks;
            all.filled_in += walk.filled_in;
            all.records += walk.records;
        }
        Ok(all)
    }
}

/// Stamps each row of a partition with the partition's fields and key, then passes it on.
struct PartitionStamp<'a> {
    partition: &'a PartitionPlan,
    row_copies: Option<&'a std::collections::BTreeMap<String, String>>,
    stamp_partition_key: bool,
    sink: &'a mut dyn PageSink,
}

impl PageSink for PartitionStamp<'_> {
    fn accept(&mut self, mut page: Vec<Value>) -> Result<()> {
        for row in &mut page {
            add_partition_fields(row, &self.partition.inject, self.row_copies);
            if self.stamp_partition_key {
                if let Value::Object(obj) = row {
                    obj.insert(
                        "_partition_key".into(),
                        Value::String(self.partition.key.clone()),
                    );
                }
            }
        }
        self.sink.accept(page)
    }
}

/// Tags each row of a financial period with the period it was asked for, then passes it on.
struct TagPeriod<'a> {
    /// `(period field, period)` when the endpoint records its period on each row.
    period: Option<(&'a str, &'a str)>,
    sink: &'a mut dyn PageSink,
}

impl PageSink for TagPeriod<'_> {
    fn accept(&mut self, mut page: Vec<Value>) -> Result<()> {
        if let Some((field, period)) = self.period {
            tag_rows_with_period(&mut page, field, period);
        }
        self.sink.accept(page)
    }
}

/// One paginated walk, its rows tagged with `period` when given, and handed to `sink`.
///
/// A journal walk is merged before it goes on, since a batch can span pages: its rows are gathered
/// first, merged, then handed over. For a financial period that gathering is bounded by the period.
async fn walk_unit(
    client: &ErpRestClient,
    tenant: &TenantId,
    path: &str,
    opts: &PageOpts,
    period: Option<(&str, &str)>,
    merge_batches: bool,
    sink: &mut dyn PageSink,
) -> Result<WalkSummary> {
    if merge_batches {
        let mut rows: Vec<Value> = Vec::new();
        let summary = paginate_stream(
            client,
            tenant,
            path,
            opts,
            &mut TagPeriod {
                period,
                sink: &mut rows,
            },
        )
        .await?;
        sink.accept(merge_journal_batches(rows))?;
        Ok(summary)
    } else {
        paginate_stream(client, tenant, path, opts, &mut TagPeriod { period, sink }).await
    }
}

/// The window that bounds which periods are walked.
///
/// With `resume`, the checkpoint watermark (or an explicit window) is a modification filter
/// applied inside each period, and the periods are always the whole range. Without it the
/// window is the range.
fn period_range(
    scan: &PeriodScanConfig,
    window_start: Option<DateTime<Utc>>,
    window_end: Option<DateTime<Utc>>,
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    if scan.resume {
        (None, None)
    } else {
        (window_start, window_end)
    }
}

/// The financial periods (YYYYMM, oldest first) one run walks.
///
/// Start: the explicit window, else `scan.first_period`. End: the explicit window, else the
/// current month plus `scan.look_ahead_months`.
fn financial_periods(
    scan: &PeriodScanConfig,
    window_start: Option<DateTime<Utc>>,
    window_end: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let month_of = |dt: DateTime<Utc>| (dt.year(), dt.month());
    let first = match window_start {
        Some(start) => month_of(start),
        None => parse_period(scan.first_period.trim())?,
    };
    let last = match window_end {
        Some(end) => month_of(end),
        None => add_months(month_of(now), scan.look_ahead_months),
    };

    if first > last {
        return Err(Error::Config(format!(
            "period range runs backwards: {:04}{:02} is after {:04}{:02}",
            first.0, first.1, last.0, last.1
        )));
    }
    let count = (last.0 - first.0) as usize * 12 + last.1 as usize - first.1 as usize + 1;
    if count > MAX_PERIOD_WALK {
        return Err(Error::Config(format!(
            "period range {:04}{:02}..{:04}{:02} is {count} months; the limit is {MAX_PERIOD_WALK}",
            first.0, first.1, last.0, last.1
        )));
    }

    let mut periods = Vec::with_capacity(count);
    let mut current = first;
    while current <= last {
        periods.push(format!("{:04}{:02}", current.0, current.1));
        current = add_months(current, 1);
    }
    Ok(periods)
}

/// Record the period a row was fetched under. A field the API already returned is kept.
fn tag_rows_with_period(rows: &mut [Value], field: &str, period: &str) {
    for row in rows {
        if let Value::Object(object) = row {
            object
                .entry(field.to_string())
                .or_insert_with(|| Value::String(period.to_string()));
        }
    }
}

fn add_months((year, month): (i32, u32), months: u32) -> (i32, u32) {
    let zero_based = year * 12 + month as i32 - 1 + months as i32;
    (
        zero_based.div_euclid(12),
        zero_based.rem_euclid(12) as u32 + 1,
    )
}

fn parse_period(text: &str) -> Result<(i32, u32)> {
    let bad = || Error::Config(format!("'{text}' is not a YYYYMM financial period"));
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let year: i32 = text[..4].parse().map_err(|_| bad())?;
    let month: u32 = text[4..].parse().map_err(|_| bad())?;
    if !(1..=12).contains(&month) || year < 1990 {
        return Err(bad());
    }
    Ok((year, month))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_path_template_is_filled_with_the_encoded_partition_value() {
        let url = "https://salesorder.visma.net/api/v3/Customers/{customerId}/locations";

        assert_eq!(
            resolve_path(url, &values(&[("customerId", "300001")])).unwrap(),
            "https://salesorder.visma.net/api/v3/Customers/300001/locations"
        );
        assert_eq!(
            resolve_path(url, &values(&[("customerId", "A/B 1?æ")])).unwrap(),
            "https://salesorder.visma.net/api/v3/Customers/A%2FB%201%3F%C3%A6/locations",
            "a value must not be able to change the path it is put into"
        );
    }

    #[test]
    fn a_path_without_placeholders_is_left_alone() {
        assert_eq!(
            resolve_path("/v1/customer", &HashMap::new()).unwrap(),
            "/v1/customer"
        );
    }

    #[test]
    fn placeholder_and_value_must_match_each_other() {
        let url = "/api/v3/Customers/{customerId}/locations";
        let unfilled = resolve_path(url, &HashMap::new()).unwrap_err().to_string();
        assert!(unfilled.contains("{customerId}"), "{unfilled}");

        let unused = resolve_path("/v1/customer", &values(&[("customerId", "1")]))
            .unwrap_err()
            .to_string();
        assert!(unused.contains("no {customerId}"), "{unused}");
    }

    #[test]
    fn partition_fields_fill_gaps_and_never_overwrite_the_api() {
        let inject = HashMap::from([
            ("customerId".to_string(), json!("300001")),
            ("defaultLocationId".to_string(), json!("HOVED")),
        ]);
        let copies = std::collections::BTreeMap::from([
            ("locationId".to_string(), "id".to_string()),
            ("missing".to_string(), "nowhere".to_string()),
        ]);
        let mut row = json!({"id": "HS", "defaultLocationId": "FROM_API"});

        add_partition_fields(&mut row, &inject, Some(&copies));

        assert_eq!(row["customerId"], "300001");
        assert_eq!(row["defaultLocationId"], "FROM_API");
        assert_eq!(row["locationId"], "HS");
        assert!(row.get("missing").is_none());
    }

    #[test]
    fn page_key_fields_follow_the_primary_key_without_lineage_columns() {
        let single = PrimaryKey::Single("refNbr".into());
        assert_eq!(page_key_fields(&single), vec!["refNbr"]);

        let composite = PrimaryKey::Composite(vec![
            "tenant_id".into(),
            "type".into(),
            "refNbr".into(),
            "_partition_key".into(),
        ]);
        assert_eq!(page_key_fields(&composite), vec!["type", "refNbr"]);
    }

    fn scan(first_period: &str, look_ahead_months: u32) -> PeriodScanConfig {
        PeriodScanConfig {
            first_period: first_period.into(),
            query_params: vec!["fromPeriod".into(), "toPeriod".into()],
            look_ahead_months,
            period_field: None,
            resume: false,
        }
    }

    #[test]
    fn rows_are_tagged_with_the_period_they_were_fetched_under() {
        let mut rows = vec![
            json!({"module": "ModulePM", "refNbr": "1400592", "lines": [1, 2]}),
            json!({"module": "ModuleAP", "refNbr": "9", "_scan_period": "from_api"}),
            json!("not an object"),
        ];

        tag_rows_with_period(&mut rows, "_scan_period", "202311");

        assert_eq!(rows[0]["_scan_period"], "202311");
        assert_eq!(
            rows[1]["_scan_period"], "from_api",
            "the API's own value wins"
        );
        assert_eq!(rows[2], json!("not an object"));
    }

    #[test]
    fn one_key_fetched_in_two_periods_stays_two_distinct_rows() {
        let mut october = vec![json!({"module": "ModulePM", "refNbr": "1400592", "lines": [1]})];
        let mut november = vec![json!({"module": "ModulePM", "refNbr": "1400592", "lines": [2]})];
        tag_rows_with_period(&mut october, "_scan_period", "202310");
        tag_rows_with_period(&mut november, "_scan_period", "202311");

        let key = |row: &Value| {
            ["module", "refNbr", "_scan_period"]
                .map(|field| row[field].as_str().unwrap().to_string())
                .join("|")
        };

        assert_ne!(key(&october[0]), key(&november[0]));
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn without_resume_the_window_bounds_the_periods_and_with_it_the_periods_stay_whole() {
        let (from, to) = (at("2026-08-10T00:00:00Z"), at("2026-08-20T00:00:00Z"));
        let mut resuming = scan("202301", 0);
        resuming.resume = true;

        assert_eq!(
            period_range(&scan("202301", 0), Some(from), Some(to)),
            (Some(from), Some(to))
        );
        assert_eq!(period_range(&resuming, Some(from), Some(to)), (None, None));
        let periods = financial_periods(&resuming, None, None, at("2026-02-10T00:00:00Z")).unwrap();
        assert_eq!(periods.first().map(String::as_str), Some("202301"));
        assert_eq!(periods.last().map(String::as_str), Some("202602"));
    }

    #[test]
    fn an_explicit_window_walks_the_months_it_touches() {
        let periods = financial_periods(
            &scan("202301", 0),
            Some(at("2026-01-15T00:00:00Z")),
            Some(at("2026-03-02T12:30:00Z")),
            at("2026-09-30T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(periods, ["202601", "202602", "202603"]);
    }

    #[test]
    fn without_a_window_the_walk_runs_from_the_first_period_to_now() {
        let periods =
            financial_periods(&scan("202511", 0), None, None, at("2026-02-10T00:00:00Z")).unwrap();
        assert_eq!(periods, ["202511", "202512", "202601", "202602"]);
    }

    #[test]
    fn look_ahead_extends_the_end_and_crosses_the_year_end() {
        let periods =
            financial_periods(&scan("202511", 3), None, None, at("2025-12-10T00:00:00Z")).unwrap();
        assert_eq!(periods, ["202511", "202512", "202601", "202602", "202603"]);
    }

    #[test]
    fn an_explicit_window_ignores_the_first_period_and_the_look_ahead() {
        let periods = financial_periods(
            &scan("202301", 6),
            Some(at("2026-08-10T00:00:00Z")),
            Some(at("2026-08-20T00:00:00Z")),
            at("2026-09-30T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(periods, ["202608"]);
    }

    #[test]
    fn a_window_without_an_end_still_reaches_the_look_ahead() {
        let periods = financial_periods(
            &scan("202301", 1),
            Some(at("2026-08-10T00:00:00Z")),
            None,
            at("2026-09-30T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(periods, ["202608", "202609", "202610"]);
    }

    #[test]
    fn malformed_backwards_and_oversized_ranges_are_rejected() {
        let now = at("2026-09-30T00:00:00Z");
        for bad in ["2026", "202613", "202600", "20260x", "1899-01", ""] {
            assert!(
                financial_periods(&scan(bad, 0), None, None, now).is_err(),
                "{bad}"
            );
        }
        let backwards = financial_periods(
            &scan("202610", 0),
            None,
            Some(at("2026-09-01T00:00:00Z")),
            now,
        );
        assert!(backwards.unwrap_err().to_string().contains("backwards"));
        let huge = financial_periods(&scan("199001", 0), None, None, now);
        assert!(huge.unwrap_err().to_string().contains("limit"));
    }

    fn lines(record: &Value) -> Vec<i64> {
        record
            .get("journalTransactionLines")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.get("lineNumber").and_then(Value::as_i64))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn merges_batch_spanning_pages_concatenating_lines() {
        let page1 = json!({
            "module": "ModulePM",
            "batchNumber": "000123",
            "journalTransactionLines": [
                {"lineNumber": 1, "project": {"id": "AS10278"}},
                {"lineNumber": 2, "project": {"id": "X"}}
            ]
        });
        let page2 = json!({
            "module": "ModulePM",
            "batchNumber": "000123",
            "journalTransactionLines": [
                {"lineNumber": 3, "project": {"id": "AS10300"}}
            ]
        });

        let merged = merge_journal_batches(vec![page1, page2]);
        assert_eq!(merged.len(), 1, "one batch -> one record");
        assert_eq!(lines(&merged[0]), vec![1, 2, 3]);
    }

    #[test]
    fn merge_preserves_line_level_project_id() {
        let page1 = json!({
            "module": "ModulePM",
            "batchNumber": "B1",
            "journalTransactionLines": [{"lineNumber": 1, "project": {"id": "AS10278"}}]
        });
        let page2 = json!({
            "module": "ModulePM",
            "batchNumber": "B1",
            "journalTransactionLines": [{"lineNumber": 2, "project": {"id": "X"}}]
        });

        let merged = merge_journal_batches(vec![page1, page2]);
        let lines = merged[0]["journalTransactionLines"].as_array().unwrap();
        assert_eq!(lines[0]["project"]["id"], "AS10278");
        assert_eq!(lines[1]["project"]["id"], "X");
    }

    #[test]
    fn merge_dedups_overlapping_lines_by_line_number() {
        let page1 = json!({
            "module": "ModulePM",
            "batchNumber": "B1",
            "journalTransactionLines": [
                {"lineNumber": 1, "project": {"id": "AS1"}},
                {"lineNumber": 2, "project": {"id": "AS2"}}
            ]
        });
        // Page boundary repeats line 2; keep the first-seen copy only.
        let page2 = json!({
            "module": "ModulePM",
            "batchNumber": "B1",
            "journalTransactionLines": [
                {"lineNumber": 2, "project": {"id": "SHOULD_NOT_REPLACE"}},
                {"lineNumber": 3, "project": {"id": "AS3"}}
            ]
        });

        let merged = merge_journal_batches(vec![page1, page2]);
        assert_eq!(lines(&merged[0]), vec![1, 2, 3]);
        let kept = merged[0]["journalTransactionLines"].as_array().unwrap();
        assert_eq!(kept[1]["project"]["id"], "AS2", "first-seen line wins");
    }

    #[test]
    fn merge_keeps_distinct_modules_separate() {
        let pm = json!({
            "module": "ModulePM",
            "batchNumber": "B1",
            "journalTransactionLines": [{"lineNumber": 1}]
        });
        let gl = json!({
            "module": "ModuleGL",
            "batchNumber": "B1",
            "journalTransactionLines": [{"lineNumber": 1}]
        });

        let merged = merge_journal_batches(vec![pm, gl]);
        assert_eq!(
            merged.len(),
            2,
            "same batch number, different module -> 2 records"
        );
    }

    #[test]
    fn merge_passes_through_records_without_batch_key() {
        let no_batch = json!({"module": "ModulePM", "journalTransactionLines": []});
        let merged = merge_journal_batches(vec![no_batch.clone()]);
        assert_eq!(merged, vec![no_batch]);
    }

    #[test]
    fn merge_accepts_batchnbr_alias_and_numeric_batch() {
        let page1 = json!({
            "module": "ModulePM",
            "batchNbr": 42,
            "journalTransactionLines": [{"lineNumber": 1}]
        });
        let page2 = json!({
            "module": "ModulePM",
            "batchNbr": 42,
            "journalTransactionLines": [{"lineNumber": 2}]
        });

        let merged = merge_journal_batches(vec![page1, page2]);
        assert_eq!(merged.len(), 1);
        assert_eq!(lines(&merged[0]), vec![1, 2]);
    }

    #[test]
    fn same_batch_number_in_two_periods_stays_two_batches() {
        // The number is reused in a later period: another batch, not a page of this one.
        let august = json!({
            "module": "ModuleGL",
            "batchNumber": "000123",
            "financialPeriod": "202408",
            "journalTransactionLines": [{"lineNumber": 1, "project": {"id": "AUG"}}]
        });
        let september = json!({
            "module": "ModuleGL",
            "batchNumber": "000123",
            "financialPeriod": "202409",
            "journalTransactionLines": [{"lineNumber": 1, "project": {"id": "SEP"}}]
        });

        let merged = merge_journal_batches(vec![august, september]);
        assert_eq!(merged.len(), 2, "two periods -> two batches");
        let first = merged[0]["journalTransactionLines"].as_array().unwrap();
        let second = merged[1]["journalTransactionLines"].as_array().unwrap();
        assert_eq!(first[0]["project"]["id"], "AUG");
        assert_eq!(
            second[0]["project"]["id"], "SEP",
            "line 1 of the later batch is kept, not dropped as a duplicate of the earlier one"
        );
    }

    #[test]
    fn pages_of_one_batch_in_one_period_still_merge() {
        let page1 = json!({
            "module": "ModuleGL",
            "batchNumber": "000123",
            "financialPeriod": "202408",
            "journalTransactionLines": [{"lineNumber": 1}, {"lineNumber": 2}]
        });
        let page2 = json!({
            "module": "ModuleGL",
            "batchNumber": "000123",
            "financialPeriod": "202408",
            "journalTransactionLines": [{"lineNumber": 3}]
        });

        let merged = merge_journal_batches(vec![page1, page2]);
        assert_eq!(merged.len(), 1, "one batch in one period -> one record");
        assert_eq!(lines(&merged[0]), vec![1, 2, 3]);
    }
}
