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

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_transform::sniff::ContentKind;
use tokio::sync::Semaphore;

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
