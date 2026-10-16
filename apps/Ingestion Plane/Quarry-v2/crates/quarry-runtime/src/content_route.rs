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

fn permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(DOC_PARSE_CONCURRENCY)))
}

/// Extract text from a PDF or DOCX body.
pub async fn parse_document(kind: ContentKind, body: Vec<u8>) -> QuarryResult<ParsedDocument> {
    parse_with(
        kind,
        body,
        permits().clone(),
        DOC_PARSE_TIMEOUT,
        |kind, body| extract(kind, body),
    )
    .await
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
    let started = tokio::time::Instant::now();
    let permit = tokio::time::timeout(deadline, permits.acquire_owned())
        .await
        .map_err(|_| timeout_error(kind))?
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "document parser is shut down"))?;
    let remaining = deadline.saturating_sub(started.elapsed());
    let handle = tokio::task::spawn_blocking(move || {
        // Held until the parse thread ends, even if the caller gave up.
        let _permit = permit;
        extractor(kind, &body)
    });
    let text = match tokio::time::timeout(remaining, handle).await {
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
    Ok(ParsedDocument { text, truncated })
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
