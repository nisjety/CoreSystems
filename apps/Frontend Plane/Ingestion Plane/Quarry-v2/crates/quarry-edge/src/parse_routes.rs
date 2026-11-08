//! `POST /v1/parse`: a PDF or DOCX in, its text out, nothing kept (INNH-11).
//!
//! The caller sends the document's bytes as the request body; the answer is the text of
//! the document in the typed envelope. Nothing is stored: no artifact, no cache entry, no
//! index row, no durable event, no usage record, no ingest. The handler takes no
//! `AppState`, so it has no store, cache, index, sink or meter to write to, and
//! `tests/parse_route.rs` pins that by reading this file and by counting writes.
//!
//! Safety, in the order a request meets it:
//!
//! 1. The route is off unless `QUARRY_EDGE__PARSE_ROUTE` is on (501 naming the variable),
//!    and it refuses to serve while documents would be parsed inside this process
//!    (`QUARRY_EDGE__DOC_PARSE_ISOLATION=inprocess`): a hostile file could then take the
//!    edge down. `QUARRY_EDGE__PARSE_ALLOW_INPROCESS=1` lifts that for a single-tenant
//!    development box.
//! 2. A service principal needs the `parse:write` scope (`auth::require_service_route_scope`).
//! 3. The declared `Content-Type` must be absent, `application/octet-stream`,
//!    `application/pdf` or the DOCX type, and the declared length within the body limit.
//!    Both are checked before a byte of the body is read.
//! 4. One org may have `QUARRY_EDGE__PARSE_CONCURRENCY_PER_ORG` (default 1) parses in
//!    flight, and the process at most `QUARRY_EDGE__PARSE_MAX_INFLIGHT` (default 4)
//!    buffered uploads. Past either, a 429 with `Retry-After`, before the body is read.
//! 5. The body is read up to `QUARRY_EDGE__PARSE_MAX_BYTES` (default and ceiling 25 MiB, the
//!    parsers' own cap). Past it, 413.
//! 6. The bytes decide the kind (`quarry_transform::sniff`). The header is only a check:
//!    a declared type that the bytes contradict, or bytes that are not a PDF or DOCX, is
//!    a 415 with a reason.
//! 7. The parse runs through `quarry_runtime::content_route`: in a child process under
//!    memory and CPU limits, behind two process-wide parse slots, under a deadline.
//!
//! The text is plain: paragraphs, no headings, lists or tables (`structure: "plain"`).
//! A PDF page with no text layer (a scan) is reported in `warnings`; there is no OCR. No
//! redaction is applied to the text.
//!
//! Logging: this module logs nothing about a request. It reads no filename header.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures_util::StreamExt;
use serde::Serialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::content_route::{
    parse_backend, parse_document_info, ParsedDocumentInfo, MAX_DOC_TEXT_BYTES,
};
use quarry_transform::sniff::{detect, ContentKind, KindSource};

use crate::auth::Claims;

/// Turns the route on. Unset or anything but `on`, `1`, `true` or `yes` is off.
pub const ENV_PARSE_ROUTE: &str = "QUARRY_EDGE__PARSE_ROUTE";
/// Largest body, in bytes. Default and ceiling [`DEFAULT_MAX_BYTES`].
pub const ENV_PARSE_MAX_BYTES: &str = "QUARRY_EDGE__PARSE_MAX_BYTES";
/// Parses one org may have in flight. Default 1, at most 8.
pub const ENV_PARSE_CONCURRENCY_PER_ORG: &str = "QUARRY_EDGE__PARSE_CONCURRENCY_PER_ORG";
/// Uploads the process buffers at once. Default 4, at most 32.
pub const ENV_PARSE_MAX_INFLIGHT: &str = "QUARRY_EDGE__PARSE_MAX_INFLIGHT";
/// Serve even when documents are parsed in this process (single-tenant development only).
pub const ENV_PARSE_ALLOW_INPROCESS: &str = "QUARRY_EDGE__PARSE_ALLOW_INPROCESS";

/// The scope a service principal needs for `POST /v1/parse`. A user token needs none.
pub const SCOPE_PARSE_WRITE: &str = "parse:write";

/// The parsers refuse a document over 25 MiB, so the body limit is the same number.
pub const DEFAULT_MAX_BYTES: usize = 25 * 1024 * 1024;
const DEFAULT_PER_ORG: u32 = 1;
const MAX_PER_ORG: u32 = 8;
const DEFAULT_INFLIGHT: u32 = 4;
const MAX_INFLIGHT: u32 = 32;

/// How long a request waits for one of the process's parse slots before a 429.
pub const QUEUE_TIMEOUT: Duration = Duration::from_secs(3);
/// What a 429 for a busy org or a full process asks the caller to wait.
const BUSY_RETRY_AFTER_S: u64 = 2;

const DOCX_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
/// The kinds the route reads, as the wire names them.
const SUPPORTED_KINDS: [&str; 2] = ["pdf", "docx"];
/// The declared types the route takes. The bytes decide; a type outside this list is refused.
const SUPPORTED_TYPES: [&str; 3] = ["application/pdf", DOCX_TYPE, "application/octet-stream"];

/// Whether the route is on. Read per request: it is one variable.
pub fn route_enabled() -> bool {
    std::env::var(ENV_PARSE_ROUTE)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "on" | "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

fn allow_inprocess() -> bool {
    std::env::var(ENV_PARSE_ALLOW_INPROCESS)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on" | "yes"
            )
        })
        .unwrap_or(false)
}

/// True when documents would be parsed inside this process and nothing allows it: the route
/// then answers 501 rather than put a hostile file next to the edge's own memory.
pub fn blocked_by_isolation() -> bool {
    parse_backend().label() == "inprocess" && !allow_inprocess()
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

/// The body limit in force: [`ENV_PARSE_MAX_BYTES`] within `1..=DEFAULT_MAX_BYTES`.
pub fn max_bytes() -> usize {
    env_number(ENV_PARSE_MAX_BYTES)
        .map(|value| (value as usize).clamp(1, DEFAULT_MAX_BYTES))
        .unwrap_or(DEFAULT_MAX_BYTES)
}

// ---- per-org and process limits -------------------------------------------------

/// Why a request could not take a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// The org already has its allowed parses in flight.
    Org,
    /// The process is buffering as many uploads as it will.
    Process,
}

/// How many parses one org may run, and how many uploads the process buffers.
pub struct ParseLimiter {
    per_org: Mutex<HashMap<String, u32>>,
    per_org_limit: u32,
    inflight: Arc<Semaphore>,
}

/// A held slot. Dropping it frees the org's count and the upload permit.
pub struct ParseSlot {
    limiter: Arc<ParseLimiter>,
    org: String,
    _upload: OwnedSemaphorePermit,
}

impl Drop for ParseSlot {
    fn drop(&mut self) {
        let mut map = self
            .limiter
            .per_org
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(count) = map.get_mut(&self.org) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                map.remove(&self.org);
            }
        }
    }
}

impl ParseLimiter {
    pub fn new(per_org_limit: u32, inflight: u32) -> Arc<Self> {
        Arc::new(Self {
            per_org: Mutex::new(HashMap::new()),
            per_org_limit: per_org_limit.max(1),
            inflight: Arc::new(Semaphore::new(inflight.max(1) as usize)),
        })
    }

    /// Takes a slot for `org` without waiting, or says which limit is full.
    pub fn try_acquire(self: &Arc<Self>, org: &str) -> Result<ParseSlot, Busy> {
        let upload = self
            .inflight
            .clone()
            .try_acquire_owned()
            .map_err(|_| Busy::Process)?;
        let mut map = self.per_org.lock().unwrap_or_else(|p| p.into_inner());
        let count = map.entry(org.to_owned()).or_insert(0);
        if *count >= self.per_org_limit {
            return Err(Busy::Org);
        }
        *count += 1;
        drop(map);
        Ok(ParseSlot {
            limiter: self.clone(),
            org: org.to_owned(),
            _upload: upload,
        })
    }

    /// Parses `org` has in flight (a test aid).
    pub fn in_flight(&self, org: &str) -> u32 {
        self.per_org
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(org)
            .copied()
            .unwrap_or(0)
    }
}

/// The process's limiter, built once from the environment.
pub fn limiter() -> &'static Arc<ParseLimiter> {
    static LIMITER: OnceLock<Arc<ParseLimiter>> = OnceLock::new();
    LIMITER.get_or_init(|| {
        let per_org = env_number(ENV_PARSE_CONCURRENCY_PER_ORG)
            .map(|value| (value as u32).clamp(1, MAX_PER_ORG))
            .unwrap_or(DEFAULT_PER_ORG);
        let inflight = env_number(ENV_PARSE_MAX_INFLIGHT)
            .map(|value| (value as u32).clamp(1, MAX_INFLIGHT))
            .unwrap_or(DEFAULT_INFLIGHT);
        ParseLimiter::new(per_org, inflight)
    })
}

// ---- the answer ----------------------------------------------------------------

#[derive(Debug, Serialize, PartialEq)]
pub struct ParseWarning {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ParseData {
    /// The text of the document. Plain paragraphs, see `structure`.
    pub markdown: String,
    /// `pdf` or `docx`, as the bytes were read.
    pub kind: &'static str,
    /// Size of the uploaded document.
    pub bytes: usize,
    /// `plain`: paragraphs only. Headings, lists and tables are not marked up.
    pub structure: &'static str,
    /// The text was cut at 8 MiB.
    pub truncated: bool,
    /// Pages of a PDF. Absent for a DOCX.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    pub warnings: Vec<ParseWarning>,
}

/// The response data for a parsed document. Pure, so the warnings are tested without a route.
pub fn build_data(kind: ContentKind, bytes: usize, info: ParsedDocumentInfo) -> ParseData {
    let mut warnings = Vec::new();
    let blank = info.document.text.chars().all(char::is_whitespace);
    let pages = info.pdf.map(|pdf| pdf.pages);
    match info.pdf {
        Some(pdf) if pdf.pages > 0 && pdf.pages_without_text == pdf.pages => {
            warnings.push(ParseWarning {
                code: "document_has_no_text_layer",
                message: format!(
                    "none of the {} pages has a text layer (a scan); there is no OCR, so the text is empty",
                    pdf.pages
                ),
            });
        }
        Some(pdf) if pdf.pages_without_text > 0 => {
            warnings.push(ParseWarning {
                code: "some_pages_have_no_text_layer",
                message: format!(
                    "{} of {} pages have no text layer (scans); there is no OCR, so their text is missing",
                    pdf.pages_without_text, pdf.pages
                ),
            });
        }
        // The page classification could not be read: an empty PDF text is still a scan's sign.
        None if kind == ContentKind::Pdf && blank => {
            warnings.push(ParseWarning {
                code: "document_has_no_text_layer",
                message: "the PDF produced no text; it may be a scan, and there is no OCR"
                    .to_owned(),
            });
        }
        _ => {}
    }
    if blank && kind == ContentKind::Docx {
        warnings.push(ParseWarning {
            code: "document_has_no_text",
            message: "the document has no text".to_owned(),
        });
    }
    if info.document.truncated {
        warnings.push(ParseWarning {
            code: "truncated_at_8mib",
            message: format!(
                "the text was cut at {} MiB",
                MAX_DOC_TEXT_BYTES / (1024 * 1024)
            ),
        });
    }
    ParseData {
        markdown: info.document.text,
        kind: kind.label(),
        bytes,
        structure: "plain",
        truncated: info.document.truncated,
        pages,
        warnings,
    }
}

// ---- refusals --------------------------------------------------------------------

fn refuse(
    request_id: &str,
    status: StatusCode,
    code: ErrorCode,
    message: &str,
    details: serde_json::Value,
) -> Response {
    let error = QuarryError::new(code, message).with_details(details);
    (status, Json(Envelope::<()>::err(request_id, error))).into_response()
}

fn busy(request_id: &str, why: Busy) -> Response {
    let (reason, message) = match why {
        Busy::Org => (
            "org_parse_busy",
            "this organisation already has a document being parsed; retry shortly",
        ),
        Busy::Process => (
            "parse_capacity",
            "the service is parsing as many documents as it will; retry shortly",
        ),
    };
    refuse(
        request_id,
        StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::RateLimited,
        message,
        serde_json::json!({ "reason": reason, "retry_after_seconds": BUSY_RETRY_AFTER_S }),
    )
}

fn too_large(request_id: &str, limit: usize) -> Response {
    refuse(
        request_id,
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::BadRequest,
        "the document is larger than the body limit",
        serde_json::json!({ "reason": "too_large", "max_bytes": limit }),
    )
}

fn unsupported(
    request_id: &str,
    reason: &str,
    message: &str,
    extra: serde_json::Value,
) -> Response {
    let mut details = serde_json::json!({ "reason": reason, "supported": SUPPORTED_KINDS });
    if let (Some(details), Some(extra)) = (details.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            details.insert(key.clone(), value.clone());
        }
    }
    refuse(
        request_id,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ErrorCode::UnsupportedMedia,
        message,
        details,
    )
}

/// The media type of a `Content-Type` value: no parameters, lower case.
fn essence(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// The kind a declared type names, if it names one.
fn declared_kind(essence: &str) -> Option<ContentKind> {
    match essence {
        "application/pdf" | "application/x-pdf" => Some(ContentKind::Pdf),
        DOCX_TYPE => Some(ContentKind::Docx),
        _ => None,
    }
}

/// Decides the kind from the bytes and checks it against the declared type. `Err` is the
/// refusal. Only a magic-number verdict counts: a body that merely lacks a marker is not a
/// PDF because a header said so.
fn decide_kind(
    request_id: &str,
    declared: Option<&str>,
    bytes: &[u8],
) -> Result<ContentKind, Response> {
    let found = detect(None, bytes);
    let kind = match (found.source, found.kind) {
        (KindSource::Magic, kind @ (ContentKind::Pdf | ContentKind::Docx)) => kind,
        _ => {
            return Err(unsupported(
                request_id,
                "unsupported_media",
                "the body is not a PDF or a DOCX document",
                serde_json::json!({ "detected": found.kind.label() }),
            ))
        }
    };
    if let Some(declared) = declared.and_then(declared_kind) {
        if declared != kind {
            return Err(unsupported(
                request_id,
                "content_type_mismatch",
                "the Content-Type names a different document type than the bytes",
                serde_json::json!({ "detected": kind.label(), "declared": declared.label() }),
            ));
        }
    }
    Ok(kind)
}

fn parse_error(request_id: &str, mut error: QuarryError) -> Response {
    let status =
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if error.code == ErrorCode::UnsupportedMedia {
        let mut details = error
            .details
            .take()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(object) = details.as_object_mut() {
            object.insert("supported".to_owned(), serde_json::json!(SUPPORTED_KINDS));
        }
        error.details = Some(details);
    }
    (status, Json(Envelope::<()>::err(request_id, error))).into_response()
}

// ---- the handler -------------------------------------------------------------------

/// `POST /v1/parse`.
pub async fn parse(
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = crate::request_id::current();
    let request_id = request_id.as_str();

    if !route_enabled() {
        return refuse(
            request_id,
            StatusCode::NOT_IMPLEMENTED,
            ErrorCode::Unsupported,
            "document parsing is not enabled on this deployment",
            serde_json::json!({ "reason": "parse_route_disabled", "env": ENV_PARSE_ROUTE }),
        );
    }
    if blocked_by_isolation() {
        return refuse(
            request_id,
            StatusCode::NOT_IMPLEMENTED,
            ErrorCode::Unsupported,
            "document parsing needs the isolated parser",
            serde_json::json!({
                "reason": "isolation_required",
                "env": "QUARRY_EDGE__DOC_PARSE_ISOLATION",
                "hint": "set it to process (Linux), or allow in-process parsing on a single-tenant box with QUARRY_EDGE__PARSE_ALLOW_INPROCESS=1",
            }),
        );
    }

    // The declared type and length are checked before any of the body is read.
    let declared = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(essence)
        .filter(|value| !value.is_empty());
    if let Some(declared) = declared.as_deref() {
        if !SUPPORTED_TYPES.contains(&declared) && declared != "application/x-pdf" {
            return unsupported(
                request_id,
                "unsupported_media",
                "the Content-Type is not one this route reads",
                serde_json::json!({ "supported_content_types": SUPPORTED_TYPES }),
            );
        }
    }
    let limit = max_bytes();
    let declared_length = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok());
    if declared_length.is_some_and(|length| length > limit as u64) {
        return too_large(request_id, limit);
    }

    // A slot for this org and for the process, held until the answer is built. Taken before
    // the body is read: a caller over its share is turned away without being buffered.
    let _slot = match limiter().try_acquire(&claims.org_id) {
        Ok(slot) => slot,
        Err(why) => return busy(request_id, why),
    };

    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return refuse(
                request_id,
                StatusCode::BAD_REQUEST,
                ErrorCode::BadRequest,
                "the request body could not be read",
                serde_json::json!({ "reason": "body_unreadable" }),
            );
        };
        if bytes.len() + chunk.len() > limit {
            return too_large(request_id, limit);
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return refuse(
            request_id,
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            "the request body is empty; send the document's bytes",
            serde_json::json!({ "reason": "empty_body" }),
        );
    }

    let kind = match decide_kind(request_id, declared.as_deref(), &bytes) {
        Ok(kind) => kind,
        Err(refusal) => return refusal,
    };
    let size = bytes.len();
    match parse_document_info(kind, bytes, QUEUE_TIMEOUT).await {
        Ok(info) => (
            StatusCode::OK,
            [(CACHE_CONTROL, "no-store")],
            Json(Envelope::ok(request_id, build_data(kind, size, info))),
        )
            .into_response(),
        Err(error) => parse_error(request_id, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_runtime::content_route::{ParsedDocument, PdfPageSummary};

    fn info(text: &str, truncated: bool, pdf: Option<PdfPageSummary>) -> ParsedDocumentInfo {
        ParsedDocumentInfo {
            document: ParsedDocument {
                text: text.to_owned(),
                truncated,
            },
            pdf,
        }
    }

    fn codes(data: &ParseData) -> Vec<&'static str> {
        data.warnings.iter().map(|warning| warning.code).collect()
    }

    #[test]
    fn a_pdf_with_text_on_every_page_has_no_warning_and_reports_its_pages() {
        let data = build_data(
            ContentKind::Pdf,
            10,
            info(
                "hello",
                false,
                Some(PdfPageSummary {
                    pages: 2,
                    pages_without_text: 0,
                }),
            ),
        );
        assert!(data.warnings.is_empty());
        assert_eq!(data.pages, Some(2));
        assert_eq!(data.structure, "plain");
        assert_eq!(data.kind, "pdf");
    }

    #[test]
    fn a_scan_gets_the_no_text_layer_warning_and_a_mixed_pdf_the_partial_one() {
        let scan = build_data(
            ContentKind::Pdf,
            10,
            info(
                "",
                false,
                Some(PdfPageSummary {
                    pages: 3,
                    pages_without_text: 3,
                }),
            ),
        );
        assert_eq!(codes(&scan), vec!["document_has_no_text_layer"]);
        let mixed = build_data(
            ContentKind::Pdf,
            10,
            info(
                "text",
                false,
                Some(PdfPageSummary {
                    pages: 3,
                    pages_without_text: 1,
                }),
            ),
        );
        assert_eq!(codes(&mixed), vec!["some_pages_have_no_text_layer"]);
    }

    #[test]
    fn a_blank_pdf_without_page_facts_is_still_flagged_and_a_docx_has_no_pages() {
        let blank = build_data(ContentKind::Pdf, 10, info("  \n", false, None));
        assert_eq!(codes(&blank), vec!["document_has_no_text_layer"]);
        assert_eq!(blank.pages, None);
        let docx = build_data(ContentKind::Docx, 10, info("", false, None));
        assert_eq!(codes(&docx), vec!["document_has_no_text"]);
        let json = serde_json::to_value(&docx).unwrap();
        assert!(
            json.get("pages").is_none(),
            "an absent page count is omitted, not null"
        );
    }

    #[test]
    fn truncation_is_a_flag_and_a_warning() {
        let data = build_data(ContentKind::Docx, 10, info("text", true, None));
        assert!(data.truncated);
        assert_eq!(codes(&data), vec!["truncated_at_8mib"]);
    }

    #[test]
    fn the_limiter_holds_one_parse_per_org_and_frees_it_on_drop() {
        let limiter = ParseLimiter::new(1, 4);
        let first = limiter.try_acquire("org_a").expect("first slot");
        assert_eq!(limiter.try_acquire("org_a").err(), Some(Busy::Org));
        // Another org is not held up by the first.
        let other = limiter.try_acquire("org_b").expect("a different org");
        drop(first);
        assert_eq!(limiter.in_flight("org_a"), 0);
        limiter.try_acquire("org_a").expect("the slot came back");
        drop(other);
    }

    #[test]
    fn the_limiter_caps_buffered_uploads_across_orgs() {
        let limiter = ParseLimiter::new(1, 2);
        let _a = limiter.try_acquire("org_a").unwrap();
        let _b = limiter.try_acquire("org_b").unwrap();
        assert_eq!(limiter.try_acquire("org_c").err(), Some(Busy::Process));
        // A refused request leaves no count behind.
        assert_eq!(limiter.in_flight("org_c"), 0);
    }

    #[test]
    fn a_refused_org_request_gives_its_upload_permit_back() {
        let limiter = ParseLimiter::new(1, 2);
        let _held = limiter.try_acquire("org_a").unwrap();
        for _ in 0..5 {
            assert_eq!(limiter.try_acquire("org_a").err(), Some(Busy::Org));
        }
        limiter
            .try_acquire("org_b")
            .expect("the refused attempts did not use the permits");
    }

    async fn answer(response: Response) -> (StatusCode, serde_json::Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn the_parser_s_errors_keep_their_status_and_the_busy_one_carries_a_wait() {
        // Busy: a 429 whose details.retry_after_seconds the retry_after layer turns into a header.
        let busy = QuarryError::new(ErrorCode::RateLimited, "busy").with_details(
            serde_json::json!({ "reason": "parser_busy", "retry_after_seconds": 5 }),
        );
        let (status, body) = answer(parse_error("req_1", busy)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"]["details"]["retry_after_seconds"], 5);

        // The deadline: a typed TIMEOUT (504), never a hang.
        let (status, body) =
            answer(parse_error("req_1", QuarryError::new(ErrorCode::Timeout, "slow"))).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body["error"]["code"], "TIMEOUT");

        // A file the parser cannot read: 415 with the parser's reason and the supported list.
        let unreadable = QuarryError::new(ErrorCode::UnsupportedMedia, "bad")
            .with_details(serde_json::json!({ "reason": "pdf_parse_failed", "kind": "pdf" }));
        let (status, body) = answer(parse_error("req_1", unreadable)).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body["error"]["details"]["reason"], "pdf_parse_failed");
        assert_eq!(body["error"]["details"]["supported"], serde_json::json!(["pdf", "docx"]));
    }

    #[test]
    fn the_declared_type_is_read_without_parameters_or_case() {
        assert_eq!(
            essence("Application/PDF; charset=binary"),
            "application/pdf"
        );
        assert_eq!(declared_kind("application/x-pdf"), Some(ContentKind::Pdf));
        assert_eq!(declared_kind(DOCX_TYPE), Some(ContentKind::Docx));
        assert_eq!(declared_kind("application/octet-stream"), None);
    }
}
