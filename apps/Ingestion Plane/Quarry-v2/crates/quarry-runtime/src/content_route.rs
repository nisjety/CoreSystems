//! Bounded, off-thread text extraction for fetched documents (PDF, DOCX).
//!
//! The parsers (`lopdf`, `zip`) are synchronous and CPU-bound, and a hostile
//! file can make them slow or large. Parsing therefore runs on the blocking
//! pool behind a process-wide permit count, under a deadline, with an output
//! cap:
//!
//! - **Concurrency.** At most [`DOC_PARSE_CONCURRENCY`] parses run at once. The
//!   permit moves into the blocking closure, so a parse that times out keeps
//!   its slot until the thread actually finishes: a stuck file cannot be used
//!   to start unlimited threads.
//! - **Deadline.** [`DOC_PARSE_TIMEOUT`] covers waiting for a permit and the
//!   parse itself. A timeout is a typed `TIMEOUT`, never a hang.
//! - **Output.** Extracted text is cut at [`MAX_DOC_TEXT_BYTES`].
//!
//! Input size caps (25 MiB) and the DOCX zip bounds live in the parsers.
//!
//! XLSX, PPTX, OpenDocument and EPUB are not parsed here. With
//! `QUARRY_EDGE__OFFICE_VIA_IMPORTS` on, [`ImportsExtractClient`] sends them to
//! imports-core's ephemeral extract endpoint (the approved Office hybrid, E3.6-6);
//! with it off (the default) they stay a typed 415.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_transform::sniff::ContentKind;
use tokio::sync::Semaphore;

use crate::service_tokens::{ServiceBearer, ServiceTokenRequest, SharedServiceTokenProvider};

pub const DOC_PARSE_TIMEOUT: Duration = Duration::from_secs(15);
pub const DOC_PARSE_CONCURRENCY: usize = 2;
pub const MAX_DOC_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Text extracted from a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    pub text: String,
    /// The text was cut at [`MAX_DOC_TEXT_BYTES`].
    pub truncated: bool,
}

/// Page facts of a PDF, read in the same bounded parse as its text (INNH-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PdfPageSummary {
    /// Pages in the document.
    pub pages: u32,
    /// Pages that carry no text layer: scanned (an image and no text) or empty.
    pub pages_without_text: u32,
}

/// A parsed document with the page facts of a PDF. `pdf` is `None` for a DOCX and
/// whenever the page classification itself could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocumentInfo {
    pub document: ParsedDocument,
    pub pdf: Option<PdfPageSummary>,
}

/// A caller that gives up waiting for a parse slot after its own queue timeout is
/// told to come back after this many seconds.
pub const PARSER_BUSY_RETRY_AFTER_S: u64 = 5;

fn permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(DOC_PARSE_CONCURRENCY)))
}

/// Extract text from a PDF or DOCX body, in a child process under resource
/// limits where that is available (see [`ParseBackend`]).
pub async fn parse_document(kind: ContentKind, body: Vec<u8>) -> QuarryResult<ParsedDocument> {
    match parse_backend() {
        ParseBackend::Process(backend) => {
            parse_in_child(backend, kind, body, permits().clone(), DOC_PARSE_TIMEOUT).await
        }
        ParseBackend::InProcess => {
            parse_with(
                kind,
                body,
                permits().clone(),
                DOC_PARSE_TIMEOUT,
                |kind, body| extract(kind, body),
            )
            .await
        }
    }
}

/// The process-wide imports-core extract client (E3.6-6a) the pipeline's Office
/// route calls. Installed once by the edge wiring, built from
/// [`IMPORTS_CORE_URL_ENV`] with a [`ManifestImportsBearer`] and
/// `.with_enabled(office_via_imports_enabled())`.
static OFFICE_EXTRACT_CLIENT: OnceLock<Arc<ImportsExtractClient>> = OnceLock::new();

/// Install the client [`extract_office_via_imports`] uses. The first install
/// wins; a second answers `false` and changes nothing.
pub fn install_office_extract_client(client: ImportsExtractClient) -> bool {
    OFFICE_EXTRACT_CLIENT.set(Arc::new(client)).is_ok()
}

/// The installed client, if any.
pub fn office_extract_client() -> Option<Arc<ImportsExtractClient>> {
    OFFICE_EXTRACT_CLIENT.get().cloned()
}

/// Extract an XLSX, PPTX, OpenDocument or EPUB body through imports-core's
/// ephemeral `POST /api/v1/import/extract` (E3.6-6a), for the pipeline's Office
/// route (E3.6-6b). The bearer is minted for `org_id`; `filename_hint` (the URL
/// path) only picks the ODF subtype and is never sent.
///
/// With no client installed every body is the typed 415
/// `imports_client_unavailable` and nothing is sent; with the client disabled
/// (the flag off) it is the 415 `office_via_imports_off`.
pub async fn extract_office_via_imports(
    org_id: &str,
    kind: ContentKind,
    body: Vec<u8>,
    filename_hint: Option<&str>,
) -> QuarryResult<ParsedDocument> {
    let Some(client) = OFFICE_EXTRACT_CLIENT.get() else {
        drop(body);
        return Err(office_refusal(
            kind,
            "imports_client_unavailable",
            format!(
                "the {} could not be read: the imports extract client is not available",
                kind.label()
            ),
        ));
    };
    client
        .extract(OfficeExtractRequest {
            org_id: org_id.to_owned(),
            kind,
            body,
            filename_hint: filename_hint.map(str::to_owned),
        })
        .await
}

/// Parse with an explicit backend and deadline: what [`parse_document`] does,
/// for callers (and tests) that bring their own.
pub async fn parse_document_with(
    backend: &ParseBackend,
    kind: ContentKind,
    body: Vec<u8>,
    deadline: Duration,
) -> QuarryResult<ParsedDocument> {
    match backend {
        ParseBackend::Process(process) => {
            parse_in_child(process, kind, body, permits().clone(), deadline).await
        }
        ParseBackend::InProcess => {
            parse_with(kind, body, permits().clone(), deadline, |kind, body| {
                extract(kind, body)
            })
            .await
        }
    }
}

/// As [`parse_document`], and the page facts of a PDF (INNH-11, `POST /v1/parse`).
/// `queue_timeout` bounds the wait for a parse slot: past it the answer is
/// `RATE_LIMITED` with `details.retry_after_seconds`, not a long hang.
pub async fn parse_document_info(
    kind: ContentKind,
    body: Vec<u8>,
    queue_timeout: Duration,
) -> QuarryResult<ParsedDocumentInfo> {
    parse_document_info_with(parse_backend(), kind, body, DOC_PARSE_TIMEOUT, queue_timeout).await
}

/// [`parse_document_info`] with an explicit backend and deadline.
pub async fn parse_document_info_with(
    backend: &ParseBackend,
    kind: ContentKind,
    body: Vec<u8>,
    deadline: Duration,
    queue_timeout: Duration,
) -> QuarryResult<ParsedDocumentInfo> {
    match backend {
        ParseBackend::Process(process) => {
            parse_in_child_info(
                process,
                kind,
                body,
                permits().clone(),
                deadline,
                true,
                Some(queue_timeout),
            )
            .await
        }
        ParseBackend::InProcess => {
            parse_with_info(
                kind,
                body,
                permits().clone(),
                deadline,
                Some(queue_timeout),
                |kind, body| extract_info(kind, body),
            )
            .await
        }
    }
}

/// The page facts of a PDF (Q3-E3.6-7: the OCR lane asks how many pages a
/// text-less PDF has). Same backend, permit and deadline as [`parse_document`]
/// (a child process under limits where that is available), and no queue
/// timeout: a busy parser waits, then times out, as a parse does. `None` when
/// the page classification itself could not be read.
pub async fn pdf_page_summary(body: Vec<u8>) -> QuarryResult<Option<PdfPageSummary>> {
    parse_document_info_with(
        parse_backend(),
        ContentKind::Pdf,
        body,
        DOC_PARSE_TIMEOUT,
        DOC_PARSE_TIMEOUT,
    )
    .await
    .map(|info| info.pdf)
}

/// Text and, for a PDF, the page classification. A classification that cannot be
/// read leaves the summary out; it never fails a parse that produced text.
fn extract_info(
    kind: ContentKind,
    body: &[u8],
) -> QuarryResult<(String, Option<PdfPageSummary>)> {
    let text = extract(kind, body)?;
    let summary = if kind == ContentKind::Pdf {
        quarry_transform::pdf::classify_pages(body).ok().map(|pages| PdfPageSummary {
            pages: pages.len() as u32,
            pages_without_text: pages
                .iter()
                .filter(|page| page.kind != quarry_transform::pdf::PageKind::Text)
                .count() as u32,
        })
    } else {
        None
    };
    Ok((text, summary))
}

/// Wait for a parse slot. With a `queue_timeout` shorter than the deadline, running
/// out of that wait is the parser being busy (a 429 to retry), not a timeout.
async fn acquire_slot(
    permits: Arc<Semaphore>,
    kind: ContentKind,
    deadline: Duration,
    queue_timeout: Option<Duration>,
) -> QuarryResult<tokio::sync::OwnedSemaphorePermit> {
    let busy = queue_timeout.filter(|queue| *queue < deadline);
    let wait = busy.unwrap_or(deadline);
    match tokio::time::timeout(wait, permits.acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(_)) => Err(QuarryError::new(
            ErrorCode::Internal,
            "document parser is shut down",
        )),
        Err(_) if busy.is_some() => Err(QuarryError::new(
            ErrorCode::RateLimited,
            "the document parser is busy, try again shortly",
        )
        .with_details(serde_json::json!({
            "reason": "parser_busy",
            "retry_after_seconds": PARSER_BUSY_RETRY_AFTER_S,
        }))),
        Err(_) => Err(timeout_error(kind)),
    }
}

fn extract(kind: ContentKind, body: &[u8]) -> QuarryResult<String> {
    match kind {
        ContentKind::Pdf => quarry_transform::pdf::extract_text(body),
        ContentKind::Docx => quarry_transform::docx::extract_text(body),
        other => Err(QuarryError::new(
            ErrorCode::UnsupportedMedia,
            format!("no text extractor for {}", other.label()),
        )),
    }
}

/// The testable core: injectable permits, deadline and extractor.
pub(crate) async fn parse_with(
    kind: ContentKind,
    body: Vec<u8>,
    permits: Arc<Semaphore>,
    deadline: Duration,
    extractor: impl FnOnce(ContentKind, &[u8]) -> QuarryResult<String> + Send + 'static,
) -> QuarryResult<ParsedDocument> {
    parse_with_info(kind, body, permits, deadline, None, move |kind, body| {
        extractor(kind, body).map(|text| (text, None))
    })
    .await
    .map(|info| info.document)
}

/// [`parse_with`] with the page facts and an optional queue timeout.
pub(crate) async fn parse_with_info(
    kind: ContentKind,
    body: Vec<u8>,
    permits: Arc<Semaphore>,
    deadline: Duration,
    queue_timeout: Option<Duration>,
    extractor: impl FnOnce(ContentKind, &[u8]) -> QuarryResult<(String, Option<PdfPageSummary>)>
        + Send
        + 'static,
) -> QuarryResult<ParsedDocumentInfo> {
    let started = tokio::time::Instant::now();
    let permit = acquire_slot(permits, kind, deadline, queue_timeout).await?;
    let remaining = deadline.saturating_sub(started.elapsed());
    let handle = tokio::task::spawn_blocking(move || {
        // Held until the parse thread ends, even if the caller gave up.
        let _permit = permit;
        extractor(kind, &body)
    });
    let (text, pdf) = match tokio::time::timeout(remaining, handle).await {
        Err(_) => return Err(timeout_error(kind)),
        Ok(Err(join)) => {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                format!("document parser panicked: {join}"),
            ))
        }
        Ok(Ok(result)) => result.map_err(|error| as_unsupported(kind, error))?,
    };
    let (text, truncated) =
        quarry_core::scrape::truncate_on_char_boundary(&text, MAX_DOC_TEXT_BYTES);
    Ok(ParsedDocumentInfo {
        document: ParsedDocument { text, truncated },
        pdf,
    })
}

fn timeout_error(kind: ContentKind) -> QuarryError {
    QuarryError::new(
        ErrorCode::Timeout,
        format!("{} text extraction exceeded its deadline", kind.label()),
    )
}

/// A file the parser cannot read is a client condition (415 with a reason), not
/// a server fault; an over-size file keeps its own typed code.
fn as_unsupported(kind: ContentKind, error: QuarryError) -> QuarryError {
    if error.code == ErrorCode::UnsupportedMedia {
        return error;
    }
    let too_large = error.message.contains("too large") || error.message.contains("exceeds");
    let reason = if too_large {
        "too_large"
    } else if kind == ContentKind::Pdf {
        "pdf_parse_failed"
    } else {
        "docx_parse_failed"
    };
    QuarryError::new(
        ErrorCode::UnsupportedMedia,
        format!("the {} could not be read: {}", kind.label(), error.message),
    )
    .with_details(serde_json::json!({
        "reason": reason,
        "kind": kind.label(),
    }))
}

// ---- running the parser in a child process --------------------------------

/// Where a document is parsed.
///
/// `lopdf` 0.42 inflates a compressed stream with no output cap, so a few
/// megabytes of PDF can ask for gigabytes of memory, and the permit and deadline
/// above cannot stop an allocation. Run in a child process, the parser gets its
/// own address-space and CPU limits: the worst a hostile file can do is kill a
/// process that holds nothing else.
#[derive(Debug, Clone)]
pub enum ParseBackend {
    /// On the blocking pool of this process (the previous behaviour).
    InProcess,
    /// In a child process under resource limits.
    Process(ProcessBackend),
}

/// How to start the child: an executable and the arguments that come before the
/// document kind (`pdf` or `docx`).
#[derive(Debug, Clone)]
pub struct ProcessBackend {
    pub exe: std::path::PathBuf,
    pub args: Vec<String>,
    pub limits: ChildLimits,
}

/// Limits applied to the child with `setrlimit` before it runs (Unix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLimits {
    /// `RLIMIT_AS`, bytes of address space.
    pub address_space: u64,
    /// `RLIMIT_CPU`, seconds of CPU time.
    pub cpu_seconds: u64,
    /// `RLIMIT_NOFILE`.
    pub open_files: u64,
}

impl Default for ChildLimits {
    fn default() -> Self {
        Self {
            address_space: 768 * 1024 * 1024,
            cpu_seconds: 20,
            open_files: 16,
        }
    }
}

/// The child's stdout is one JSON line of extracted text; more than this is not
/// a parser answer.
const MAX_CHILD_STDOUT: u64 = 16 * 1024 * 1024;

/// The hidden argument the edge binary answers with the child protocol.
pub const CHILD_ARG: &str = "--doc-parse";

impl ParseBackend {
    pub const ENV: &'static str = "QUARRY_EDGE__DOC_PARSE_ISOLATION";

    /// From the setting (`process` or `inprocess`) and the path of the running
    /// executable. Unset means `process` on Unix when the running executable is
    /// the edge (the only binary that answers the child argument) and `inprocess`
    /// otherwise: elsewhere the limits do not exist, and a test or tool binary
    /// would not know what `--doc-parse` means. Setting `process` explicitly
    /// takes the executable as it is.
    pub fn from_setting(
        setting: Option<&str>,
        current_exe: std::io::Result<std::path::PathBuf>,
    ) -> Self {
        let is_edge = current_exe.as_ref().is_ok_and(|exe| {
            exe.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.starts_with("quarry-edge-rs"))
        });
        let default = cfg!(unix) && is_edge;
        let wanted = match setting.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
            Some("inprocess" | "in_process" | "in-process") => false,
            Some("process") => cfg!(unix),
            Some("") | None => default,
            Some(other) => {
                tracing::warn!(
                    value = other,
                    "QUARRY_EDGE__DOC_PARSE_ISOLATION is not process or inprocess; using the default"
                );
                default
            }
        };
        match (wanted, current_exe) {
            (true, Ok(exe)) => Self::Process(ProcessBackend {
                exe,
                args: vec![CHILD_ARG.to_owned()],
                limits: ChildLimits::default(),
            }),
            (true, Err(_)) => {
                tracing::warn!("cannot find the running executable; parsing documents in process");
                Self::InProcess
            }
            (false, _) => Self::InProcess,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::InProcess => "inprocess",
            Self::Process(_) => "process",
        }
    }
}

/// The backend for this process, chosen once from the environment.
pub fn parse_backend() -> &'static ParseBackend {
    static BACKEND: OnceLock<ParseBackend> = OnceLock::new();
    BACKEND.get_or_init(|| {
        let backend = ParseBackend::from_setting(
            std::env::var(ParseBackend::ENV).ok().as_deref(),
            std::env::current_exe(),
        );
        tracing::info!(isolation = backend.label(), "document parser isolation");
        backend
    })
}

/// What the child writes on stdout.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChildReply {
    Ok {
        text: String,
        truncated: bool,
        /// Present when the caller asked for the page facts of a PDF (`pdf-pages`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pdf: Option<PdfPageSummary>,
    },
    Error { code: String, message: String },
}

/// The child side: read the document from stdin, write one JSON line on stdout,
/// and return the process exit code (0 parsed, 2 not parsed). No sockets, no
/// configuration, no async runtime. The kind is `pdf`, `docx`, or `pdf-pages`
/// (a PDF whose page classification is also answered, INNH-11).
pub fn run_child(kind: Option<&str>) -> i32 {
    use std::io::{Read, Write};

    let (kind, with_pages) = match kind {
        Some("pdf") => (ContentKind::Pdf, false),
        Some("pdf-pages") => (ContentKind::Pdf, true),
        Some("docx") => (ContentKind::Docx, false),
        _ => return 2,
    };
    let mut body = Vec::new();
    // The parsers refuse more than 25 MiB themselves; this only bounds the read.
    if std::io::stdin()
        .lock()
        .take(26 * 1024 * 1024)
        .read_to_end(&mut body)
        .is_err()
    {
        return 2;
    }
    let reply = match extract_child(kind, with_pages, &body) {
        Ok((text, pdf)) => {
            let (text, truncated) =
                quarry_core::scrape::truncate_on_char_boundary(&text, MAX_DOC_TEXT_BYTES);
            ChildReply::Ok {
                text,
                truncated,
                pdf,
            }
        }
        Err(error) => ChildReply::Error {
            code: error.code.wire_name().to_owned(),
            message: error.message,
        },
    };
    let ok = matches!(reply, ChildReply::Ok { .. });
    let mut line = serde_json::to_vec(&reply).unwrap_or_default();
    line.push(b'\n');
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&line).and_then(|()| stdout.flush()).is_err() {
        return 2;
    }
    if ok {
        0
    } else {
        2
    }
}

fn extract_child(
    kind: ContentKind,
    with_pages: bool,
    body: &[u8],
) -> QuarryResult<(String, Option<PdfPageSummary>)> {
    if with_pages {
        extract_info(kind, body)
    } else {
        extract(kind, body).map(|text| (text, None))
    }
}

/// Parse in a child process. The permit and the deadline are the same as for the
/// in-process path; the child is killed when the deadline passes or this future
/// is dropped.
async fn parse_in_child(
    backend: &ProcessBackend,
    kind: ContentKind,
    body: Vec<u8>,
    permits: Arc<Semaphore>,
    deadline: Duration,
) -> QuarryResult<ParsedDocument> {
    parse_in_child_info(backend, kind, body, permits, deadline, false, None)
        .await
        .map(|info| info.document)
}

async fn parse_in_child_info(
    backend: &ProcessBackend,
    kind: ContentKind,
    body: Vec<u8>,
    permits: Arc<Semaphore>,
    deadline: Duration,
    want_pages: bool,
    queue_timeout: Option<Duration>,
) -> QuarryResult<ParsedDocumentInfo> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let started = tokio::time::Instant::now();
    let _permit = acquire_slot(permits, kind, deadline, queue_timeout).await?;
    let remaining = deadline.saturating_sub(started.elapsed());

    let kind_arg = match kind {
        ContentKind::Pdf if want_pages => "pdf-pages",
        ContentKind::Pdf => "pdf",
        ContentKind::Docx => "docx",
        other => {
            return Err(QuarryError::new(
                ErrorCode::UnsupportedMedia,
                format!("no text extractor for {}", other.label()),
            ))
        }
    };
    let mut command = tokio::process::Command::new(&backend.exe);
    command
        .args(&backend.args)
        .arg(kind_arg)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        let limits = backend.limits;
        // SAFETY: the closure runs between fork and exec and only calls
        // setrlimit, which is async-signal-safe.
        unsafe {
            command.pre_exec(move || apply_limits(limits));
        }
    }
    let spawn_failed = |error: std::io::Error| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("the document parser could not be started: {error}"),
        )
    };
    let mut child = command.spawn().map_err(spawn_failed)?;
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();

    let work = async {
        // A child that dies early closes the pipe: ignore a broken pipe here, the
        // exit status says what happened.
        let writer = async move {
            if let Some(mut stdin) = stdin.take() {
                let _ = stdin.write_all(&body).await;
                let _ = stdin.shutdown().await;
            }
        };
        let reader = async move {
            let mut out = Vec::new();
            if let Some(stdout) = stdout {
                let _ = stdout.take(MAX_CHILD_STDOUT).read_to_end(&mut out).await;
            }
            out
        };
        let ((), out) = tokio::join!(writer, reader);
        let status = child.wait().await;
        (out, status)
    };
    let (out, status) = match tokio::time::timeout(remaining, work).await {
        Ok(done) => done,
        // Dropping the future drops the child, which kills it (kill_on_drop).
        Err(_) => return Err(timeout_error(kind)),
    };

    let succeeded = status.as_ref().is_ok_and(|status| status.success());
    let reply = serde_json::from_slice::<ChildReply>(out.split(|byte| *byte == b'\n').next().unwrap_or_default()).ok();
    match (succeeded, reply) {
        (true, Some(ChildReply::Ok { text, truncated, pdf })) => Ok(ParsedDocumentInfo {
            document: ParsedDocument { text, truncated },
            pdf,
        }),
        (_, Some(ChildReply::Error { code, message })) => {
            let code = if code == ErrorCode::UnsupportedMedia.wire_name() {
                ErrorCode::UnsupportedMedia
            } else {
                ErrorCode::DriverFailed
            };
            Err(as_unsupported(kind, QuarryError::new(code, message)))
        }
        // Killed (memory or CPU limit), crashed, or answered nothing usable.
        _ => Err(stopped_error(kind)),
    }
}

/// The child did not finish: a resource limit, a crash or a malformed answer. A
/// file that does this is a file the service will not read.
fn stopped_error(kind: ContentKind) -> QuarryError {
    let reason = if kind == ContentKind::Pdf {
        "pdf_parse_failed"
    } else {
        "docx_parse_failed"
    };
    QuarryError::new(
        ErrorCode::UnsupportedMedia,
        format!(
            "the {} could not be read: the parser was stopped after exceeding its limits",
            kind.label()
        ),
    )
    .with_details(serde_json::json!({
        "reason": reason,
        "kind": kind.label(),
        "limit": "parser_stopped",
    }))
}

#[cfg(unix)]
fn apply_limits(limits: ChildLimits) -> std::io::Result<()> {
    fn set(resource: i32, value: u64) -> std::io::Result<()> {
        let limit = libc::rlimit {
            rlim_cur: value as libc::rlim_t,
            rlim_max: value as libc::rlim_t,
        };
        // SAFETY: `limit` is a valid rlimit for the duration of the call.
        if unsafe { libc::setrlimit(resource as _, &limit) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    set(libc::RLIMIT_AS as i32, limits.address_space)?;
    set(libc::RLIMIT_CPU as i32, limits.cpu_seconds)?;
    set(libc::RLIMIT_NOFILE as i32, limits.open_files)?;
    set(libc::RLIMIT_CORE as i32, 0)?;
    Ok(())
}

// ---- Office hybrid: imports-core extract client (E3.6-6a) -------------------

/// The flag that sends XLSX, PPTX, OpenDocument and EPUB bodies to imports-core's
/// ephemeral extract endpoint. Default off: those kinds stay a typed 415.
pub const OFFICE_VIA_IMPORTS_FLAG: &str = "QUARRY_EDGE__OFFICE_VIA_IMPORTS";

/// The imports-core origin (for example `http://imports-core:8000`) the wiring
/// reads to build an [`ImportsExtractClient`].
pub const IMPORTS_CORE_URL_ENV: &str = "QUARRY_EDGE__IMPORTS_CORE_URL";

/// imports-core's ephemeral extract route: no job, no storage, no model call, no
/// third-party egress, `Cache-Control: no-store` (`imports-core/app/main.py`).
pub const OFFICE_EXTRACT_PATH: &str = "/api/v1/import/extract";

/// The budget of one extraction: the slot wait, the token and the call (and the
/// one retry after a refused bearer).
pub const OFFICE_EXTRACT_TIMEOUT: Duration = Duration::from_secs(15);
const OFFICE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// At most this many extractions are in flight to imports-core from this process.
pub const OFFICE_EXTRACT_CONCURRENCY: usize = 4;

/// The default input cap: imports-core's own default (`CHAT_EXTRACT_MAX_BYTES`,
/// 1 MB). A larger body is refused here, before any call.
pub const DEFAULT_OFFICE_MAX_INPUT_BYTES: usize = 1_000_000;

/// The ceiling for [`ImportsExtractClient::with_max_input_bytes`]: the same 25 MiB
/// the in-process parsers refuse above.
pub const MAX_OFFICE_INPUT_BYTES: usize = 25 * 1024 * 1024;

/// imports-core answers at most 60 000 characters of text in a small envelope; an
/// answer larger than this is not an extract answer.
const MAX_OFFICE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Whether [`OFFICE_VIA_IMPORTS_FLAG`] is on in this process.
pub fn office_via_imports_enabled() -> bool {
    flag_on(std::env::var(OFFICE_VIA_IMPORTS_FLAG).ok().as_deref())
}

/// The environment variable that sets the input cap of the imports-core client
/// (see [`office_max_input_bytes_from_env`]).
pub const OFFICE_MAX_INPUT_BYTES_ENV: &str = "QUARRY_EDGE__OFFICE_MAX_INPUT_BYTES";

/// The input cap from [`OFFICE_MAX_INPUT_BYTES_ENV`]: unset or blank is
/// [`DEFAULT_OFFICE_MAX_INPUT_BYTES`], and a value above [`MAX_OFFICE_INPUT_BYTES`]
/// is capped there. Zero, a negative or a non-number is an error that names the
/// variable and never its value, so the edge stops at boot rather than guess.
pub fn office_max_input_bytes_from_env() -> Result<usize, String> {
    let Some(raw) = std::env::var(OFFICE_MAX_INPUT_BYTES_ENV)
        .ok()
        .filter(|raw| !raw.trim().is_empty())
    else {
        return Ok(DEFAULT_OFFICE_MAX_INPUT_BYTES);
    };
    match raw.trim().parse::<usize>() {
        Ok(bytes) if bytes > 0 => Ok(bytes.min(MAX_OFFICE_INPUT_BYTES)),
        _ => Err(format!(
            "{OFFICE_MAX_INPUT_BYTES_ENV} must be a positive whole number of bytes"
        )),
    }
}

/// `1`, `true`, `yes` or `on` (any case); anything else, or unset, is off.
fn flag_on(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// The kinds that go to imports-core when the flag is on. PDF and DOCX stay in
/// Rust (the parsers above); the legacy OLE2 formats are not sent anywhere.
pub fn is_office_via_imports_kind(kind: ContentKind) -> bool {
    matches!(
        kind,
        ContentKind::Xlsx | ContentKind::Pptx | ContentKind::Odf | ContentKind::Epub
    )
}

/// The file extension imports-core chooses its parser by. Only the extension of
/// the hint is used; the name sent is always `document.<ext>`, never the real
/// file name or anything from the URL.
fn office_extension(kind: ContentKind, filename_hint: Option<&str>) -> Option<&'static str> {
    match kind {
        ContentKind::Xlsx => Some("xlsx"),
        ContentKind::Pptx => Some("pptx"),
        ContentKind::Epub => Some("epub"),
        ContentKind::Odf => {
            let hinted = filename_hint
                .and_then(|name| name.rsplit_once('.'))
                .map(|(_, ext)| ext.trim().to_ascii_lowercase());
            Some(match hinted.as_deref() {
                Some("ods") => "ods",
                Some("odp") => "odp",
                _ => "odt",
            })
        }
        _ => None,
    }
}

/// The bearer for imports-core (`aud=ingestion`, scope `imports:read`), minted for
/// the run's organization. A trait so the client never builds a token request by
/// hand: production passes [`ManifestImportsBearer`].
#[async_trait::async_trait]
pub trait ImportsBearer: Send + Sync {
    async fn bearer(&self, org_id: &str, force_refresh: bool) -> QuarryResult<ServiceBearer>;
    async fn invalidate(&self, org_id: &str);
}

/// [`ImportsBearer`] over the service-token provider and one manifest row. The
/// production row is `office-extract` ([`ManifestImportsBearer::for_office_extract`]);
/// `new` takes any request so a test can inject its own.
pub struct ManifestImportsBearer {
    tokens: SharedServiceTokenProvider,
    request: ServiceTokenRequest,
}

impl ManifestImportsBearer {
    pub fn new(tokens: SharedServiceTokenProvider, request: ServiceTokenRequest) -> Self {
        Self { tokens, request }
    }

    /// The production bearer: the `office-extract` row of the token-request
    /// manifest (audience `ingestion`, scope `imports:read`, ZDR lane). The
    /// request is read from the manifest and never built by hand. The row name
    /// is a literal in this file, the file the row's `used_by` lists, and the
    /// constructor is not named `from_manifest` because the manifest's source
    /// scan refuses a `from_manifest(` that takes no literal.
    pub fn for_office_extract(tokens: SharedServiceTokenProvider) -> QuarryResult<Self> {
        Ok(Self::new(
            tokens,
            ServiceTokenRequest::from_manifest("office-extract")?,
        ))
    }
}

#[async_trait::async_trait]
impl ImportsBearer for ManifestImportsBearer {
    async fn bearer(&self, org_id: &str, force_refresh: bool) -> QuarryResult<ServiceBearer> {
        self.tokens
            .token_for_org(org_id, &self.request, force_refresh)
            .await
    }

    async fn invalidate(&self, org_id: &str) {
        self.tokens.invalidate(org_id, &self.request).await;
    }
}

/// One body to extract.
#[derive(Debug, Clone)]
pub struct OfficeExtractRequest {
    /// The organization the run belongs to; the bearer is minted for it.
    pub org_id: String,
    pub kind: ContentKind,
    pub body: Vec<u8>,
    /// The fetched file name or URL path, used only for the ODF subtype.
    pub filename_hint: Option<String>,
}

#[derive(serde::Serialize)]
struct ImportsExtractBody<'a> {
    filename: &'a str,
    content_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<&'static str>,
}

#[derive(serde::Deserialize)]
struct ImportsExtractEnvelope {
    data: ImportsExtractData,
}

#[derive(serde::Deserialize)]
struct ImportsExtractData {
    content: String,
    /// imports-core always answers `false`; anything else is refused.
    #[serde(default)]
    persisted: Option<bool>,
    #[serde(default)]
    truncated: bool,
}

fn office_content_type(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "epub" => "application/epub+zip",
        _ => return None,
    })
}

fn office_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(OFFICE_EXTRACT_CONCURRENCY)))
}

/// The client of imports-core's ephemeral `POST /api/v1/import/extract` for the
/// Office kinds the Rust parsers do not read (approved hybrid, E3.6-6).
///
/// - **Flag.** Built disabled; [`Self::with_enabled`] takes
///   [`office_via_imports_enabled`]. Disabled, every call is the typed 415 the
///   pipeline answers today, and nothing is sent.
/// - **Bounds.** The input is capped before the call, the answer is read to a
///   cap, a process-wide slot count limits calls in flight, and one deadline
///   covers the slot wait, the bearer and the call. Redirects are never followed
///   (a redirect could move the bearer).
/// - **No storage.** The endpoint persists nothing; an answer that says it did
///   (`persisted` not `false`) is refused and its text dropped. That is why ZDR
///   runs may use it.
/// - **Privacy.** The name sent is `document.<ext>`. No log line or error carries
///   the body, the text, the URL or imports-core's message.
/// - **Refusals.** Unsupported type, too large, or unreadable are a typed 415
///   with a reason; busy is a 429 with `retry_after_seconds`; a slow answer is a
///   `TIMEOUT`; an unreachable or broken service is `DRIVER_FAILED`.
pub struct ImportsExtractClient {
    http: reqwest::Client,
    base_url: String,
    bearer: Arc<dyn ImportsBearer>,
    enabled: bool,
    deadline: Duration,
    max_input_bytes: usize,
    permits: Arc<Semaphore>,
}

impl ImportsExtractClient {
    /// A disabled client for the imports-core origin `base_url`.
    pub fn new(base_url: impl Into<String>, bearer: Arc<dyn ImportsBearer>) -> QuarryResult<Self> {
        let base_url = base_url.into().trim().trim_end_matches('/').to_owned();
        let parsed = reqwest::Url::parse(&base_url).map_err(|_| {
            QuarryError::new(
                ErrorCode::Internal,
                "the imports-core URL for Office extraction must be an absolute URL",
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "the imports-core URL for Office extraction must be a plain http(s) origin",
            ));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(OFFICE_CONNECT_TIMEOUT)
            .timeout(OFFICE_EXTRACT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("office extract client: {}", error.without_url()),
                )
            })?;
        Ok(Self {
            http,
            base_url,
            bearer,
            enabled: false,
            deadline: OFFICE_EXTRACT_TIMEOUT,
            max_input_bytes: DEFAULT_OFFICE_MAX_INPUT_BYTES,
            permits: office_permits().clone(),
        })
    }

    /// Turn the client on or off: pass [`office_via_imports_enabled`].
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The input cap; set it to imports-core's `CHAT_EXTRACT_MAX_BYTES` when the
    /// operator raises that. Never above [`MAX_OFFICE_INPUT_BYTES`], never zero.
    pub fn with_max_input_bytes(mut self, max: usize) -> Self {
        self.max_input_bytes = max.clamp(1, MAX_OFFICE_INPUT_BYTES);
        self
    }

    /// The deadline of one extraction; it never exceeds [`OFFICE_EXTRACT_TIMEOUT`].
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline.min(OFFICE_EXTRACT_TIMEOUT);
        self
    }

    #[cfg(test)]
    fn with_permits(mut self, permits: Arc<Semaphore>) -> Self {
        self.permits = permits;
        self
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Extract the text of an XLSX, PPTX, OpenDocument or EPUB body. The text is
    /// cut at [`MAX_DOC_TEXT_BYTES`] like every other document.
    pub async fn extract(&self, request: OfficeExtractRequest) -> QuarryResult<ParsedDocument> {
        let kind = request.kind;
        if !self.enabled {
            return Err(office_refusal(
                kind,
                "office_via_imports_off",
                format!(
                    "no text extractor for {} ({OFFICE_VIA_IMPORTS_FLAG} is off)",
                    kind.label()
                ),
            ));
        }
        let Some(extension) = office_extension(kind, request.filename_hint.as_deref()) else {
            return Err(office_refusal(
                kind,
                "no_extractor",
                format!("no text extractor for {}", kind.label()),
            ));
        };
        if request.body.is_empty() {
            return Err(office_refusal(
                kind,
                "office_parse_failed",
                format!("the {} is empty", kind.label()),
            ));
        }
        if request.body.len() > self.max_input_bytes {
            return Err(office_refusal(
                kind,
                "too_large",
                format!(
                    "the {} is larger than the {} bytes Office extraction accepts",
                    kind.label(),
                    self.max_input_bytes
                ),
            )
            .with_details(serde_json::json!({
                "reason": "too_large",
                "kind": kind.label(),
                "limit_bytes": self.max_input_bytes,
            })));
        }
        if request.org_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "Office extraction needs the run's organization",
            ));
        }

        let started = tokio::time::Instant::now();
        let deadline = self.deadline;
        let _permit = acquire_slot(self.permits.clone(), kind, deadline, Some(deadline / 3)).await?;
        let remaining = deadline.saturating_sub(started.elapsed());
        let filename = format!("document.{extension}");
        let body = ImportsExtractBody {
            filename: &filename,
            content_base64: {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD.encode(&request.body)
            },
            content_type: office_content_type(extension),
        };
        let payload = serde_json::to_vec(&body).map_err(|_| {
            QuarryError::new(ErrorCode::Internal, "the Office extract request could not be encoded")
        })?;
        drop(body);

        let outcome = tokio::time::timeout(remaining, self.call(&request.org_id, kind, payload)).await;
        let result = match outcome {
            Ok(result) => result,
            Err(_) => Err(timeout_error(kind)),
        };
        match &result {
            Ok(parsed) => tracing::debug!(
                kind = kind.label(),
                truncated = parsed.truncated,
                "office document extracted by imports-core"
            ),
            Err(error) => tracing::warn!(
                kind = kind.label(),
                error_code = ?error.code,
                "office extraction by imports-core failed"
            ),
        }
        result
    }

    async fn call(
        &self,
        org_id: &str,
        kind: ContentKind,
        payload: Vec<u8>,
    ) -> QuarryResult<ParsedDocument> {
        let url = format!("{}{}", self.base_url, OFFICE_EXTRACT_PATH);
        let bearer = self.bearer.bearer(org_id, false).await?;
        let mut response = self.send(&url, kind, payload.clone(), bearer.expose()).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Refused before any parse: one retry with a fresh bearer.
            drain_quietly(response).await;
            self.bearer.invalidate(org_id).await;
            let refreshed = self.bearer.bearer(org_id, true).await?;
            response = self.send(&url, kind, payload, refreshed.expose()).await?;
        }
        decode_office_response(kind, response).await
    }

    async fn send(
        &self,
        url: &str,
        kind: ContentKind,
        payload: Vec<u8>,
        bearer: &str,
    ) -> QuarryResult<reqwest::Response> {
        self.http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {bearer}"))
            .body(payload)
            .send()
            .await
            .map_err(|error| office_transport_error(kind, error))
    }
}

/// A 415 with a reason, the shape [`as_unsupported`] gives the Rust parsers.
fn office_refusal(kind: ContentKind, reason: &str, message: String) -> QuarryError {
    QuarryError::new(ErrorCode::UnsupportedMedia, message).with_details(serde_json::json!({
        "reason": reason,
        "kind": kind.label(),
        "extractor": "imports-core",
    }))
}

fn office_transport_error(kind: ContentKind, error: reqwest::Error) -> QuarryError {
    if error.is_timeout() {
        return timeout_error(kind);
    }
    QuarryError::new(
        ErrorCode::DriverFailed,
        format!("imports-core could not be reached: {}", error.without_url()),
    )
    .with_details(serde_json::json!({
        "reason": "imports_unavailable",
        "kind": kind.label(),
    }))
}

async fn drain_quietly(response: reqwest::Response) {
    let _ = read_bounded(response, MAX_OFFICE_RESPONSE_BYTES).await;
}

fn refused_reason(status: u16) -> &'static str {
    if matches!(status, 401 | 403) {
        "imports_refused_credentials"
    } else {
        "imports_failed"
    }
}

/// The body, read to `max` bytes; more is an error, never a partial answer.
async fn read_bounded(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>, ReadFailure> {
    if response
        .content_length()
        .is_some_and(|length| length > max as u64)
    {
        return Err(ReadFailure::TooLarge);
    }
    let mut out = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if out.len() + chunk.len() > max {
                    return Err(ReadFailure::TooLarge);
                }
                out.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(out),
            Err(error) => return Err(ReadFailure::Transport(error)),
        }
    }
}

enum ReadFailure {
    TooLarge,
    Transport(reqwest::Error),
}

/// Map imports-core's answer. Its error messages are never copied: a parser
/// message can carry document text.
async fn decode_office_response(
    kind: ContentKind,
    response: reqwest::Response,
) -> QuarryResult<ParsedDocument> {
    let status = response.status();
    if !status.is_success() {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|seconds| (1..=300).contains(seconds))
            .unwrap_or(PARSER_BUSY_RETRY_AFTER_S);
        drain_quietly(response).await;
        let upstream = status.as_u16();
        return Err(match upstream {
            415 => office_refusal(
                kind,
                "imports_unsupported_type",
                format!("imports-core does not read {} documents", kind.label()),
            ),
            413 => office_refusal(
                kind,
                "too_large",
                format!("the {} is too large to extract", kind.label()),
            ),
            422 => office_refusal(
                kind,
                "office_parse_failed",
                format!("the {} could not be read", kind.label()),
            ),
            429 | 503 => QuarryError::new(
                ErrorCode::RateLimited,
                "the Office extractor is busy, try again shortly",
            )
            .with_details(serde_json::json!({
                "reason": "office_extractor_busy",
                "kind": kind.label(),
                "retry_after_seconds": retry_after,
            })),
            408 | 504 => timeout_error(kind),
            _ => QuarryError::new(
                ErrorCode::DriverFailed,
                format!("imports-core refused the Office extraction (status {upstream})"),
            )
            .with_details(serde_json::json!({
                "reason": refused_reason(upstream),
                "kind": kind.label(),
                "upstream_status": upstream,
            })),
        });
    }
    let bytes = match read_bounded(response, MAX_OFFICE_RESPONSE_BYTES).await {
        Ok(bytes) => bytes,
        Err(ReadFailure::TooLarge) => {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "imports-core's Office extract answer is larger than allowed",
            )
            .with_details(serde_json::json!({
                "reason": "imports_answer_too_large",
                "kind": kind.label(),
            })))
        }
        Err(ReadFailure::Transport(error)) => return Err(office_transport_error(kind, error)),
    };
    let envelope: ImportsExtractEnvelope = serde_json::from_slice(&bytes).map_err(|_| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "imports-core answered the Office extraction with an unreadable body",
        )
        .with_details(serde_json::json!({
            "reason": "imports_failed",
            "kind": kind.label(),
        }))
    })?;
    drop(bytes);
    let data = envelope.data;
    if data.persisted != Some(false) {
        // The route is ephemeral by contract; an answer that says otherwise is
        // not used, so a ZDR run never depends on a stored copy.
        return Err(QuarryError::new(
            ErrorCode::DriverFailed,
            "imports-core did not confirm the Office extraction was ephemeral",
        )
        .with_details(serde_json::json!({
            "reason": "imports_not_ephemeral",
            "kind": kind.label(),
        })));
    }
    if data.content.trim().is_empty() {
        return Err(office_refusal(
            kind,
            "office_parse_failed",
            format!("no readable text found in the {}", kind.label()),
        ));
    }
    let (text, cut) = quarry_core::scrape::truncate_on_char_boundary(&data.content, MAX_DOC_TEXT_BYTES);
    Ok(ParsedDocument {
        text,
        truncated: cut || data.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extractor_output_is_returned() {
        let parsed = parse_with(
            ContentKind::Pdf,
            b"x".to_vec(),
            Arc::new(Semaphore::new(1)),
            Duration::from_secs(2),
            |_, _| Ok("hello".to_owned()),
        )
        .await
        .unwrap();
        assert_eq!(
            parsed,
            ParsedDocument {
                text: "hello".into(),
                truncated: false
            }
        );
    }

    #[tokio::test]
    async fn output_is_capped() {
        let parsed = parse_with(
            ContentKind::Pdf,
            Vec::new(),
            Arc::new(Semaphore::new(1)),
            Duration::from_secs(5),
            |_, _| Ok("a".repeat(MAX_DOC_TEXT_BYTES + 10)),
        )
        .await
        .unwrap();
        assert!(parsed.truncated);
        assert_eq!(parsed.text.len(), MAX_DOC_TEXT_BYTES);
    }

    #[tokio::test]
    async fn page_facts_travel_with_the_text() {
        let info = parse_with_info(
            ContentKind::Pdf,
            b"x".to_vec(),
            Arc::new(Semaphore::new(1)),
            Duration::from_secs(2),
            None,
            |_, _| {
                Ok((
                    "hello".to_owned(),
                    Some(PdfPageSummary {
                        pages: 3,
                        pages_without_text: 1,
                    }),
                ))
            },
        )
        .await
        .unwrap();
        assert_eq!(info.document.text, "hello");
        assert_eq!(
            info.pdf,
            Some(PdfPageSummary {
                pages: 3,
                pages_without_text: 1
            })
        );
    }

    #[tokio::test]
    async fn a_busy_parser_is_a_retryable_429_after_the_queue_timeout() {
        // No slot is ever free: the caller waits its queue timeout, not the deadline.
        let started = std::time::Instant::now();
        let error = parse_with_info(
            ContentKind::Pdf,
            b"x".to_vec(),
            Arc::new(Semaphore::new(0)),
            Duration::from_secs(30),
            Some(Duration::from_millis(50)),
            |_, _| Ok((String::new(), None)),
        )
        .await
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(error.code, ErrorCode::RateLimited);
        let details = error.details.expect("details");
        assert_eq!(details["reason"], "parser_busy");
        assert_eq!(details["retry_after_seconds"], PARSER_BUSY_RETRY_AFTER_S);
    }

    #[tokio::test]
    async fn waiting_past_the_deadline_is_still_a_timeout() {
        // A queue timeout at or past the deadline is the deadline.
        let error = parse_with_info(
            ContentKind::Pdf,
            b"x".to_vec(),
            Arc::new(Semaphore::new(0)),
            Duration::from_millis(50),
            Some(Duration::from_secs(10)),
            |_, _| Ok((String::new(), None)),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn a_parse_error_is_a_typed_415_with_a_reason() {
        for (kind, reason) in [
            (ContentKind::Pdf, "pdf_parse_failed"),
            (ContentKind::Docx, "docx_parse_failed"),
        ] {
            let error = parse_with(
                kind,
                Vec::new(),
                Arc::new(Semaphore::new(1)),
                Duration::from_secs(2),
                |_, _| {
                    Err(QuarryError::new(
                        ErrorCode::DriverFailed,
                        "PDF parse failed",
                    ))
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::UnsupportedMedia);
            assert_eq!(error.details.unwrap()["reason"], reason);
        }
    }

    #[tokio::test]
    async fn an_oversize_input_keeps_a_too_large_reason() {
        let error = parse_with(
            ContentKind::Pdf,
            Vec::new(),
            Arc::new(Semaphore::new(1)),
            Duration::from_secs(2),
            |_, _| {
                Err(QuarryError::new(
                    ErrorCode::BadRequest,
                    "PDF is too large to parse in-process",
                ))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.details.unwrap()["reason"], "too_large");
    }

    #[tokio::test]
    async fn a_slow_parse_times_out_and_keeps_its_permit_until_it_ends() {
        let permits = Arc::new(Semaphore::new(1));
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let error = parse_with(
            ContentKind::Pdf,
            Vec::new(),
            permits.clone(),
            Duration::from_millis(100),
            move |_, _| {
                // Blocks until the test lets the thread finish.
                let _ = release_rx.recv();
                Ok(String::new())
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        // The caller gave up, but the thread still occupies the only slot.
        assert_eq!(permits.available_permits(), 0);
        release_tx.send(()).unwrap();
        for _ in 0..100 {
            if permits.available_permits() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            permits.available_permits(),
            1,
            "the slot frees once the thread ends"
        );
    }

    #[tokio::test]
    async fn waiting_for_a_permit_counts_against_the_deadline() {
        let permits = Arc::new(Semaphore::new(1));
        let _held = permits.clone().acquire_owned().await.unwrap();
        let error = parse_with(
            ContentKind::Docx,
            Vec::new(),
            permits,
            Duration::from_millis(50),
            |_, _| Ok(String::new()),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn concurrency_is_capped() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let permits = Arc::new(Semaphore::new(2));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let (permits, running, peak) = (permits.clone(), running.clone(), peak.clone());
            tasks.push(tokio::spawn(async move {
                parse_with(
                    ContentKind::Pdf,
                    Vec::new(),
                    permits,
                    Duration::from_secs(10),
                    move |_, _| {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(30));
                        running.fetch_sub(1, Ordering::SeqCst);
                        Ok(String::new())
                    },
                )
                .await
            }));
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "peak {}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn only_pdf_and_docx_have_extractors() {
        let error = parse_document(ContentKind::Xlsx, Vec::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
    }
}

#[cfg(test)]
mod office_tests {
    use super::*;
    use std::sync::Mutex;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Mints `first.imports.token`, or `refreshed.imports.token` on a forced refresh.
    #[derive(Default)]
    struct FakeBearer {
        calls: Mutex<Vec<(String, bool)>>,
        invalidated: Mutex<usize>,
    }

    #[async_trait::async_trait]
    impl ImportsBearer for FakeBearer {
        async fn bearer(&self, org_id: &str, force_refresh: bool) -> QuarryResult<ServiceBearer> {
            self.calls
                .lock()
                .unwrap()
                .push((org_id.to_owned(), force_refresh));
            Ok(ServiceBearer::from_test_token(if force_refresh {
                "refreshed.imports.token"
            } else {
                "first.imports.token"
            }))
        }

        async fn invalidate(&self, _org_id: &str) {
            *self.invalidated.lock().unwrap() += 1;
        }
    }

    fn client(server: &MockServer, bearer: Arc<FakeBearer>) -> ImportsExtractClient {
        ImportsExtractClient::new(server.uri(), bearer)
            .expect("client")
            .with_enabled(true)
            .with_permits(Arc::new(Semaphore::new(2)))
    }

    fn request(kind: ContentKind, body: &[u8]) -> OfficeExtractRequest {
        OfficeExtractRequest {
            org_id: "org-1".into(),
            kind,
            body: body.to_vec(),
            filename_hint: None,
        }
    }

    fn answer(content: &str) -> serde_json::Value {
        json!({ "data": { "name": "document.xlsx", "content": content, "persisted": false } })
    }

    fn reason(error: &QuarryError) -> String {
        error.details.as_ref().expect("details")["reason"]
            .as_str()
            .expect("a reason")
            .to_owned()
    }

    #[test]
    fn the_flag_is_off_unless_it_says_on() {
        assert!(!flag_on(None));
        for off in ["", "0", "false", "off", "no", "maybe"] {
            assert!(!flag_on(Some(off)), "{off}");
        }
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(flag_on(Some(on)), "{on}");
        }
    }

    #[test]
    fn only_the_office_kinds_go_to_imports_core() {
        for kind in [
            ContentKind::Xlsx,
            ContentKind::Pptx,
            ContentKind::Odf,
            ContentKind::Epub,
        ] {
            assert!(is_office_via_imports_kind(kind), "{}", kind.label());
        }
        for kind in [
            ContentKind::Pdf,
            ContentKind::Docx,
            ContentKind::OleCompound,
            ContentKind::Html,
            ContentKind::Archive,
        ] {
            assert!(!is_office_via_imports_kind(kind), "{}", kind.label());
        }
    }

    #[test]
    fn the_name_sent_is_generic_and_odf_takes_its_subtype_from_the_hint() {
        assert_eq!(office_extension(ContentKind::Xlsx, Some("salary.ods")), Some("xlsx"));
        assert_eq!(office_extension(ContentKind::Odf, Some("/files/q3.ODS")), Some("ods"));
        assert_eq!(office_extension(ContentKind::Odf, Some("deck.odp")), Some("odp"));
        assert_eq!(office_extension(ContentKind::Odf, Some("noext")), Some("odt"));
        assert_eq!(office_extension(ContentKind::Odf, None), Some("odt"));
        assert_eq!(office_extension(ContentKind::Pdf, None), None);
    }

    #[test]
    fn the_url_must_be_a_plain_origin() {
        let bearer: Arc<dyn ImportsBearer> = Arc::new(FakeBearer::default());
        for bad in [
            "imports-core:8000",
            "ftp://imports-core",
            "http://imports-core:8000/?q=1",
            "http://user:pw@imports-core:8000",
        ] {
            assert!(ImportsExtractClient::new(bad, bearer.clone()).is_err(), "{bad}");
        }
        let client = ImportsExtractClient::new("http://imports-core:8000/", bearer).unwrap();
        assert!(!client.enabled(), "built disabled");
    }

    #[tokio::test]
    async fn a_disabled_client_is_the_415_of_today_and_sends_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .expect(0)
            .mount(&server)
            .await;
        let bearer = Arc::new(FakeBearer::default());
        let client = ImportsExtractClient::new(server.uri(), bearer.clone()).unwrap();
        let error = client
            .extract(request(ContentKind::Xlsx, b"PK"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(reason(&error), "office_via_imports_off");
        assert!(bearer.calls.lock().unwrap().is_empty(), "no mint");
    }

    #[tokio::test]
    async fn extracts_with_the_bearer_and_a_generic_name() {
        use base64::Engine as _;
        let server = MockServer::start().await;
        let body = b"PK\x03\x04 a workbook";
        Mock::given(method("POST"))
            .and(path(OFFICE_EXTRACT_PATH))
            .and(header("authorization", "Bearer first.imports.token"))
            .and(body_partial_json(json!({
                "filename": "document.xlsx",
                "content_base64": base64::engine::general_purpose::STANDARD.encode(body),
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("Sheet1\nA1\t42")))
            .expect(1)
            .mount(&server)
            .await;
        let bearer = Arc::new(FakeBearer::default());
        let mut req = request(ContentKind::Xlsx, body);
        req.filename_hint = Some("secret-payroll.xlsx".into());
        let parsed = client(&server, bearer.clone()).extract(req).await.unwrap();
        assert_eq!(
            parsed,
            ParsedDocument {
                text: "Sheet1\nA1\t42".into(),
                truncated: false
            }
        );
        assert_eq!(*bearer.calls.lock().unwrap(), vec![("org-1".to_owned(), false)]);
    }

    #[tokio::test]
    async fn a_cut_reported_by_imports_core_is_kept() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OFFICE_EXTRACT_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "name": "document.xlsx", "content": "rows", "persisted": false, "truncated": true }
            })))
            .mount(&server)
            .await;
        let parsed = client(&server, Arc::new(FakeBearer::default()))
            .extract(request(ContentKind::Xlsx, b"PK"))
            .await
            .unwrap();
        assert!(parsed.truncated);
    }

    #[tokio::test]
    async fn an_answer_that_is_not_ephemeral_is_refused() {
        for data in [
            json!({ "name": "d", "content": "text", "persisted": true }),
            json!({ "name": "d", "content": "text" }),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
                .mount(&server)
                .await;
            let error = client(&server, Arc::new(FakeBearer::default()))
                .extract(request(ContentKind::Pptx, b"PK"))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::DriverFailed);
            assert_eq!(reason(&error), "imports_not_ephemeral");
            assert!(!error.message.contains("text"));
        }
    }

    #[tokio::test]
    async fn refusals_are_typed_and_never_copy_the_upstream_message() {
        for (status, code, why) in [
            (415, ErrorCode::UnsupportedMedia, "imports_unsupported_type"),
            (413, ErrorCode::UnsupportedMedia, "too_large"),
            (422, ErrorCode::UnsupportedMedia, "office_parse_failed"),
            (429, ErrorCode::RateLimited, "office_extractor_busy"),
            (403, ErrorCode::DriverFailed, "imports_refused_credentials"),
            (500, ErrorCode::DriverFailed, "imports_failed"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(json!({ "detail": "SECRET CELL VALUE" })),
                )
                .mount(&server)
                .await;
            let error = client(&server, Arc::new(FakeBearer::default()))
                .extract(request(ContentKind::Epub, b"PK"))
                .await
                .unwrap_err();
            assert_eq!(error.code, code, "{status}");
            assert_eq!(reason(&error), why, "{status}");
            assert!(!error.message.contains("SECRET"), "{status}");
            assert!(
                !error.details.as_ref().unwrap().to_string().contains("SECRET"),
                "{status}"
            );
        }
    }

    #[tokio::test]
    async fn busy_carries_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "12"))
            .mount(&server)
            .await;
        let error = client(&server, Arc::new(FakeBearer::default()))
            .extract(request(ContentKind::Xlsx, b"PK"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert_eq!(error.details.unwrap()["retry_after_seconds"], 12);
    }

    #[tokio::test]
    async fn an_oversize_or_empty_body_is_refused_before_any_call() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .expect(0)
            .mount(&server)
            .await;
        let bearer = Arc::new(FakeBearer::default());
        let client = client(&server, bearer.clone()).with_max_input_bytes(8);
        let error = client
            .extract(request(ContentKind::Xlsx, &[0u8; 9]))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(reason(&error), "too_large");
        assert_eq!(error.details.unwrap()["limit_bytes"], 8);
        let error = client
            .extract(request(ContentKind::Xlsx, b""))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert!(bearer.calls.lock().unwrap().is_empty(), "no mint");
    }

    #[tokio::test]
    async fn pdf_and_docx_are_not_sent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .expect(0)
            .mount(&server)
            .await;
        for kind in [ContentKind::Pdf, ContentKind::Docx, ContentKind::OleCompound] {
            let error = client(&server, Arc::new(FakeBearer::default()))
                .extract(request(kind, b"%PDF"))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::UnsupportedMedia);
            assert_eq!(reason(&error), "no_extractor");
        }
    }

    #[tokio::test]
    async fn a_refused_bearer_is_refreshed_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer first.imports.token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer refreshed.imports.token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("slide one")))
            .expect(1)
            .mount(&server)
            .await;
        let bearer = Arc::new(FakeBearer::default());
        let parsed = client(&server, bearer.clone())
            .extract(request(ContentKind::Pptx, b"PK"))
            .await
            .unwrap();
        assert_eq!(parsed.text, "slide one");
        assert_eq!(*bearer.invalidated.lock().unwrap(), 1);
        assert_eq!(
            *bearer.calls.lock().unwrap(),
            vec![("org-1".to_owned(), false), ("org-1".to_owned(), true)]
        );
    }

    #[tokio::test]
    async fn a_slow_answer_is_a_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(answer("late"))
                    .set_delay(Duration::from_secs(5)),
            )
            .mount(&server)
            .await;
        let error = client(&server, Arc::new(FakeBearer::default()))
            .with_deadline(Duration::from_millis(200))
            .extract(request(ContentKind::Odf, b"PK"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn no_free_slot_is_a_retryable_429() {
        let server = MockServer::start().await;
        let error = ImportsExtractClient::new(server.uri(), Arc::new(FakeBearer::default()))
            .unwrap()
            .with_enabled(true)
            .with_permits(Arc::new(Semaphore::new(0)))
            .with_deadline(Duration::from_millis(300))
            .extract(request(ContentKind::Xlsx, b"PK"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert_eq!(reason(&error), "parser_busy");
    }

    #[tokio::test]
    async fn an_oversize_answer_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("x".repeat(MAX_OFFICE_RESPONSE_BYTES + 1)),
            )
            .mount(&server)
            .await;
        let error = client(&server, Arc::new(FakeBearer::default()))
            .extract(request(ContentKind::Xlsx, b"PK"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DriverFailed);
        assert_eq!(reason(&error), "imports_answer_too_large");
    }

    /// No test in this crate installs the process-wide client, so the pipeline
    /// entry answers the typed 415 and sends nothing.
    #[tokio::test]
    async fn the_pipeline_entry_without_an_installed_client_is_a_typed_415() {
        assert!(office_extract_client().is_none());
        let error = extract_office_via_imports("org_a", ContentKind::Xlsx, b"PK".to_vec(), Some("/a/b.xlsx"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(reason(&error), "imports_client_unavailable");
    }
}
