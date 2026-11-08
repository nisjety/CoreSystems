//! Per-run results on the edge (INNH-12): the pages a crawl, a batch or any other durable
//! job fetched, the Apify dataset equivalent.
//!
//! A row is read from control's `page_fetched` events: the page URL, closed fetch metadata
//! and the ids of the stored artifacts. The content itself is not returned; a client reads
//! it through the existing org-bound `GET /v1/artifacts/{id}`.
//!
//! `format=csv` and `format=jsonl` export the same rows as text (E4.1-4): `fields` projects
//! the columns, a read takes at most 100 rows and a body is at most 10 MiB. An export passes
//! the same gates as the JSON read, and its headers name the kind (`page_index`) and the next
//! cursor.
//!
//! Dark by default: `QUARRY_EDGE__JOB_RESULTS` must be on, otherwise every route answers 501
//! before control is called. The rows carry page URLs, so a caller under Zero Data Retention
//! gets a 403 (the same floor as the errors routes), and a service principal is refused by
//! `auth::require_service_route_scope`, which does not name these paths. The caller's org
//! comes from the verified token; control answers a foreign or unknown id with 404, and a
//! kind mismatch (a batch id on the crawl path) is a 404 too.

use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};

use quarry_core::envelope::{Envelope, PageMeta};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::resources::{JobResultView, JobResultsView};

use crate::auth::Claims;
use crate::control_client;
use crate::job_lifecycle_routes::{fail, require_job_of_kind};
use crate::state::AppState;

/// Turns the results routes on. Off (the default), every route answers 501.
pub const ENV_JOB_RESULTS: &str = "QUARRY_EDGE__JOB_RESULTS";

/// Default and largest page of results.
pub const DEFAULT_LIMIT: u32 = 100;
pub const MAX_LIMIT: u32 = 500;

/// Whether a flag value switches the feature on.
pub fn flag_is_on(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Whether the results routes are enabled. Read per request, so a flip needs no restart.
pub fn job_results_enabled() -> bool {
    flag_is_on(std::env::var(ENV_JOB_RESULTS).ok().as_deref())
}

/// The checks every results read makes before control is asked: the flag first (a dark
/// feature answers 501 whoever asks), then the ZDR floor.
pub fn gate(enabled: bool, zdr_active: bool) -> Result<(), QuarryError> {
    if !enabled {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            format!(
                "job results are not enabled on this edge: set {ENV_JOB_RESULTS}=1 \
                 (see docs/RUNBOOKS.md, \"Job results\")"
            ),
        ));
    }
    if zdr_active {
        // A row is a page URL and its title; a ZDR caller gets neither.
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "job results are not available under zero data retention",
        )
        .with_details(serde_json::json!({ "reason": "job_result_urls" })));
    }
    Ok(())
}

/// The query of a results read, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsParams {
    pub after_seq: Option<u64>,
    pub limit: u32,
    pub latest: bool,
}

fn bad(field: &str, message: &str) -> QuarryError {
    QuarryError::new(ErrorCode::BadRequest, message)
        .with_details(serde_json::json!({ "field": field }))
}

/// Parses `after_seq`, `limit` (1..=500, default 100) and `latest`. A bad value is a 400
/// that names the field, never a silent default.
pub fn parse_params(query: &HashMap<String, String>) -> Result<ResultsParams, QuarryError> {
    let after_seq = match query.get("after_seq") {
        None => None,
        Some(raw) => Some(
            raw.parse::<u64>()
                .map_err(|_| bad("after_seq", "after_seq must be a non-negative integer"))?,
        ),
    };
    let limit = match query.get("limit") {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<u32>() {
            Ok(n) if (1..=MAX_LIMIT).contains(&n) => n,
            _ => return Err(bad("limit", "limit must be an integer from 1 to 500")),
        },
    };
    let latest = match query.get("latest").map(String::as_str) {
        None | Some("false" | "0") => false,
        Some("true" | "1") => true,
        Some(_) => return Err(bad("latest", "latest must be true or false")),
    };
    Ok(ResultsParams {
        after_seq,
        limit,
        latest,
    })
}

/// The results of a job, checked against the kind the path names (`None`: any kind).
async fn results_for(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
    query: &HashMap<String, String>,
) -> Result<ResultsRead, QuarryError> {
    gate(job_results_enabled(), claims.effective_zdr(false))?;
    let params = parse_params(query)?;
    let export = parse_export(query, params.limit)?;
    // The summary settles existence, org and kind before the rows are read.
    require_job_of_kind(state, claims, id, kind).await?;
    let mut forward: Vec<(&str, String)> = vec![("limit", params.limit.to_string())];
    if let Some(after) = params.after_seq {
        forward.push(("after_seq", after.to_string()));
    }
    if params.latest {
        forward.push(("latest", "true".to_string()));
    }
    let raw = control_client::get_json(
        state,
        &format!("/v1/jobs/{id}/results"),
        &claims.org_id,
        &forward,
    )
    .await?;
    let view: JobResultsView = serde_json::from_value(raw)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control results unreadable"))?;
    Ok(ResultsRead {
        view,
        limit: params.limit,
        export,
    })
}

/// A checked results read: the rows, the page size, and the export options when the caller
/// asked for CSV or JSON Lines (`None` keeps the JSON envelope).
struct ResultsRead {
    view: JobResultsView,
    limit: u32,
    export: Option<Export>,
}

/// The export formats of a results read. `json`, the default, is the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Jsonl,
}

impl ExportFormat {
    fn content_type(self) -> &'static str {
        match self {
            Self::Csv => "text/csv; charset=utf-8",
            Self::Jsonl => "application/x-ndjson; charset=utf-8",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Csv => "job-results.csv",
            Self::Jsonl => "job-results.jsonl",
        }
    }
}

/// The fields of a row, in the order a default export writes them. A `fields` projection
/// names a subset of these, each at most once.
pub const ROW_FIELDS: [&str; 11] = [
    "seq",
    "url",
    "status",
    "content_type",
    "title",
    "fetched_at",
    "word_count",
    "driver",
    "markdown_artifact_id",
    "source_artifact_id",
    "bytes",
];

/// An export reads at most this many rows (D14b's 100-item cap; the JSON read allows 500).
pub const MAX_EXPORT_LIMIT: u32 = 100;

/// An export body is at most this many bytes (D14b's 10 MiB cap).
pub const MAX_EXPORT_BYTES: usize = 10 * 1024 * 1024;

/// Names the kind of result a read returns: an index of the pages a job fetched, not the page
/// content, which stays behind `GET /v1/artifacts/{id}`.
pub const RESULT_KIND_HEADER: &str = "x-quarry-result-kind";

/// Carries the cursor of the next export page, when there is one.
pub const NEXT_SEQ_HEADER: &str = "x-quarry-next-seq";

/// The checked export options of a results read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub format: ExportFormat,
    pub fields: Vec<&'static str>,
}

/// Parses `format` and `fields`. `json` (or no format) is `None` and keeps the envelope. A
/// `fields` without an export format is a 400, and so is an export whose `limit` is over 100.
pub fn parse_export(
    query: &HashMap<String, String>,
    limit: u32,
) -> Result<Option<Export>, QuarryError> {
    let format = match query.get("format").map(String::as_str) {
        None | Some("json") => None,
        Some("csv") => Some(ExportFormat::Csv),
        Some("jsonl") => Some(ExportFormat::Jsonl),
        Some(_) => return Err(bad("format", "format must be json, csv or jsonl")),
    };
    let Some(format) = format else {
        if query.contains_key("fields") {
            return Err(bad(
                "fields",
                "fields applies only with format=csv or format=jsonl",
            ));
        }
        return Ok(None);
    };
    if limit > MAX_EXPORT_LIMIT {
        return Err(bad(
            "limit",
            "an export reads 1 to 100 rows: use a smaller limit",
        ));
    }
    let fields = match query.get("fields") {
        None => ROW_FIELDS.to_vec(),
        Some(raw) => parse_fields(raw)?,
    };
    Ok(Some(Export { format, fields }))
}

/// A `fields` list: names from [`ROW_FIELDS`], comma separated, each at most once.
fn parse_fields(raw: &str) -> Result<Vec<&'static str>, QuarryError> {
    let mut fields: Vec<&'static str> = Vec::new();
    for name in raw.split(',').map(str::trim) {
        let Some(known) = ROW_FIELDS.iter().copied().find(|field| *field == name) else {
            return Err(bad(
                "fields",
                "fields must name row fields from docs/CONTRACTS.md, comma separated",
            ));
        };
        if fields.contains(&known) {
            return Err(bad("fields", "fields must not name a field twice"));
        }
        fields.push(known);
    }
    Ok(fields)
}

/// A row as the JSON envelope writes it, so an export and a JSON read carry the same keys.
fn row_object(row: &JobResultView) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(row) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// One CSV cell (RFC 4180). A value that a spreadsheet would read as a formula (a leading `=`,
/// `+`, `-`, `@`, tab or carriage return) gets a leading apostrophe, so a page title or URL
/// stays text when the file is opened.
fn csv_cell(value: Option<&serde_json::Value>) -> String {
    let raw = match value {
        None | Some(serde_json::Value::Null) => return String::new(),
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    };
    let guarded = if raw.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{raw}")
    } else {
        raw
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// The rows as CSV: a header line of the field names, then one CRLF-ended line per row.
pub fn render_csv(rows: &[JobResultView], fields: &[&str]) -> String {
    let mut out = fields.join(",");
    out.push_str("\r\n");
    for row in rows {
        let object = row_object(row);
        let cells: Vec<String> = fields
            .iter()
            .map(|field| csv_cell(object.get(*field)))
            .collect();
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }
    out
}

/// The rows as JSON Lines: one object per row, with the projected fields only. A field the
/// row does not have is `null`, so every line has the same keys.
pub fn render_jsonl(rows: &[JobResultView], fields: &[&str]) -> String {
    let mut out = String::new();
    for row in rows {
        let object = row_object(row);
        let projected: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|field| {
                (
                    (*field).to_owned(),
                    object
                        .get(*field)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
            })
            .collect();
        out.push_str(&serde_json::Value::Object(projected).to_string());
        out.push('\n');
    }
    out
}

/// Refuses an export body over the cap with a 400 that names the limit. The file is never
/// cut short: a caller who gets this reads fewer rows or projects fewer fields.
pub fn check_export_size(len: usize) -> Result<(), QuarryError> {
    if len > MAX_EXPORT_BYTES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "the export is larger than 10 MiB: use a smaller limit or fewer fields",
        )
        .with_details(serde_json::json!({ "field": "limit", "reason": "export_too_large" })));
    }
    Ok(())
}

/// The export of a read: the rows as CSV or JSON Lines, with the kind and the cursors in
/// headers. The gates ran before, so this only renders.
fn export_response(export: &Export, view: &JobResultsView) -> Result<Response, QuarryError> {
    let body = match export.format {
        ExportFormat::Csv => render_csv(&view.items, &export.fields),
        ExportFormat::Jsonl => render_jsonl(&view.items, &export.fields),
    };
    check_export_size(body.len())?;
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, export.format.content_type())
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", export.format.file_name()),
        )
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(RESULT_KIND_HEADER, "page_index");
    if let Some(next) = view.next_seq {
        builder = builder.header(NEXT_SEQ_HEADER, next.to_string());
    }
    if view.scan_capped {
        builder = builder.header("x-quarry-scan-capped", "true");
    }
    builder
        .body(Body::from(body))
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "export response could not be built"))
}

/// Renders a read: the export when the caller asked for one, otherwise the JSON envelope
/// `{data: {items, next_seq?}, meta: {request_id, page: {next?, limit}}}`.
fn render(request_id: String, result: Result<ResultsRead, QuarryError>) -> Response {
    match result {
        Ok(ResultsRead {
            view,
            export: Some(export),
            ..
        }) => match export_response(&export, &view) {
            Ok(response) => response,
            Err(err) => fail(&request_id, err).into_response(),
        },
        Ok(ResultsRead {
            view,
            limit,
            export: None,
        }) => {
            let next = view.next_seq.map(|seq| seq.to_string());
            let mut envelope = Envelope::ok(request_id, view);
            envelope.meta.page = Some(PageMeta {
                cursor: None,
                next,
                limit,
            });
            Json(envelope).into_response()
        }
        Err(err) => fail(&request_id, err).into_response(),
    }
}

/// `GET /v1/jobs/:id/results`.
pub async fn job_results(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    render(
        request_id,
        results_for(&state, &claims, &id, None, &query).await,
    )
}

/// `GET /v1/crawl/:id/results`.
pub async fn crawl_results(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    render(
        request_id,
        results_for(&state, &claims, &id, Some("crawl"), &query).await,
    )
}

/// `GET /v1/batch/:id/results`.
pub async fn batch_results(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    render(
        request_id,
        results_for(&state, &claims, &id, Some("batch"), &query).await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_are_100_rows_from_the_start_with_every_fetch() {
        let p = parse_params(&q(&[])).unwrap();
        assert_eq!(
            p,
            ResultsParams {
                after_seq: None,
                limit: 100,
                latest: false
            }
        );
    }

    #[test]
    fn limit_must_be_1_to_500() {
        assert_eq!(parse_params(&q(&[("limit", "1")])).unwrap().limit, 1);
        assert_eq!(parse_params(&q(&[("limit", "500")])).unwrap().limit, 500);
        for bad_limit in ["0", "501", "-1", "abc", ""] {
            let err = parse_params(&q(&[("limit", bad_limit)])).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "limit={bad_limit:?}");
            assert_eq!(err.details.as_ref().unwrap()["field"], "limit");
        }
    }

    #[test]
    fn after_seq_and_latest_are_checked() {
        let p = parse_params(&q(&[("after_seq", "42"), ("latest", "true")])).unwrap();
        assert_eq!(p.after_seq, Some(42));
        assert!(p.latest);
        assert!(!parse_params(&q(&[("latest", "0")])).unwrap().latest);
        assert_eq!(
            parse_params(&q(&[("after_seq", "x")])).unwrap_err().code,
            ErrorCode::BadRequest
        );
        assert_eq!(
            parse_params(&q(&[("latest", "maybe")])).unwrap_err().code,
            ErrorCode::BadRequest
        );
    }

    #[test]
    fn the_flag_is_off_unless_set_to_a_true_word() {
        assert!(!flag_is_on(None));
        assert!(!flag_is_on(Some("")));
        assert!(!flag_is_on(Some("0")));
        assert!(flag_is_on(Some("1")));
        assert!(flag_is_on(Some(" true ")));
    }

    #[test]
    fn off_is_501_and_wins_over_the_zdr_refusal() {
        let err = gate(false, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert_eq!(err.code.http_status(), 501);
    }

    #[test]
    fn zdr_is_403_with_the_reason() {
        let err = gate(true, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert_eq!(err.details.as_ref().unwrap()["reason"], "job_result_urls");
        assert!(gate(true, false).is_ok());
    }

    #[test]
    fn json_is_the_default_and_an_export_names_its_format() {
        assert_eq!(parse_export(&q(&[]), 100).unwrap(), None);
        assert_eq!(parse_export(&q(&[("format", "json")]), 100).unwrap(), None);
        let csv = parse_export(&q(&[("format", "csv")]), 100)
            .unwrap()
            .unwrap();
        assert_eq!(csv.format, ExportFormat::Csv);
        let jsonl = parse_export(&q(&[("format", "jsonl")]), 100)
            .unwrap()
            .unwrap();
        assert_eq!(jsonl.format, ExportFormat::Jsonl);
        let err = parse_export(&q(&[("format", "xml")]), 100).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert_eq!(err.details.as_ref().unwrap()["field"], "format");
    }

    #[test]
    fn fields_need_an_export_and_name_each_row_field_once() {
        let err = parse_export(&q(&[("fields", "url")]), 100).unwrap_err();
        assert_eq!(err.details.as_ref().unwrap()["field"], "fields");

        let projected = parse_export(&q(&[("format", "csv"), ("fields", "url, seq")]), 100)
            .unwrap()
            .unwrap();
        assert_eq!(projected.fields, vec!["url", "seq"]);
        let all = parse_export(&q(&[("format", "csv")]), 100)
            .unwrap()
            .unwrap();
        assert_eq!(all.fields, ROW_FIELDS.to_vec());

        for bad in ["url,nope", "url,url", "", "url,"] {
            let err = parse_export(&q(&[("format", "csv"), ("fields", bad)]), 100).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "fields={bad:?}");
            assert_eq!(
                err.details.as_ref().unwrap()["field"],
                "fields",
                "fields={bad:?}"
            );
        }
    }

    #[test]
    fn an_export_reads_at_most_100_rows_and_the_json_read_keeps_500() {
        assert!(parse_export(&q(&[("format", "csv")]), 100).is_ok());
        let err = parse_export(&q(&[("format", "csv")]), 101).unwrap_err();
        assert_eq!(err.details.as_ref().unwrap()["field"], "limit");
        assert_eq!(parse_export(&q(&[]), 101).unwrap(), None);
    }

    fn sample_row(seq: u64, url: &str, title: Option<&str>) -> JobResultView {
        use chrono::TimeZone;
        JobResultView {
            seq,
            url: url.to_owned(),
            status: 200,
            content_type: Some("text/html".to_owned()),
            title: title.map(str::to_owned),
            fetched_at: chrono::Utc
                .with_ymd_and_hms(2026, 10, 23, 10, 0, seq as u32)
                .unwrap(),
            word_count: Some(120),
            driver: Some("static".to_owned()),
            markdown_artifact_id: Some("art_01MD".to_owned()),
            source_artifact_id: None,
            bytes: Some(900),
        }
    }

    #[test]
    fn csv_cells_are_rfc4180_and_formula_safe() {
        let text = |s: &str| serde_json::Value::String(s.to_owned());
        assert_eq!(csv_cell(Some(&text("plain"))), "plain");
        assert_eq!(csv_cell(Some(&text("a,b"))), "\"a,b\"");
        assert_eq!(csv_cell(Some(&text("say \"hi\""))), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell(Some(&text("line\nbreak"))), "\"line\nbreak\"");
        assert_eq!(csv_cell(Some(&text("=1+1"))), "'=1+1");
        assert_eq!(csv_cell(Some(&text("+x"))), "'+x");
        assert_eq!(csv_cell(Some(&text("@SUM(A1)"))), "'@SUM(A1)");
        assert_eq!(csv_cell(Some(&text("=1,2"))), "\"'=1,2\"");
        assert_eq!(csv_cell(Some(&serde_json::json!(42))), "42");
        assert_eq!(csv_cell(None), "");
        assert_eq!(csv_cell(Some(&serde_json::Value::Null)), "");
    }

    #[test]
    fn csv_writes_the_projected_header_and_one_crlf_line_per_row() {
        let rows = vec![
            sample_row(1, "https://a.test/x", Some("A, B")),
            sample_row(2, "https://a.test/y", None),
        ];
        assert_eq!(
            render_csv(&rows, &["seq", "url", "title", "bytes"]),
            "seq,url,title,bytes\r\n1,https://a.test/x,\"A, B\",900\r\n2,https://a.test/y,,900\r\n"
        );
        assert_eq!(render_csv(&[], &["seq"]), "seq\r\n");
    }

    #[test]
    fn jsonl_projects_each_row_and_nulls_what_a_row_lacks() {
        let rows = vec![sample_row(1, "https://a.test/x", None)];
        let body = render_jsonl(&rows, &["seq", "source_artifact_id"]);
        assert_eq!(body, "{\"seq\":1,\"source_artifact_id\":null}\n");
        let line: serde_json::Value = serde_json::from_str(body.trim_end()).unwrap();
        assert_eq!(
            line,
            serde_json::json!({ "seq": 1, "source_artifact_id": null })
        );
    }

    #[test]
    fn an_export_over_10_mib_is_refused_with_the_reason_named() {
        assert!(check_export_size(MAX_EXPORT_BYTES).is_ok());
        let err = check_export_size(MAX_EXPORT_BYTES + 1).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert_eq!(err.details.as_ref().unwrap()["reason"], "export_too_large");
    }
}
