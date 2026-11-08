//! The document parser in a child process under resource limits (WS5.C).
//!
//! Linux only: `RLIMIT_AS` is what stops a decompression bomb, and it is not
//! enforced on every Unix. The child is the real `quarry-doc-parse` executable
//! (the same code `quarry-edge-rs --doc-parse` runs), not a stub.

#![cfg(target_os = "linux")]

use std::io::Write;
use std::time::{Duration, Instant};

use quarry_core::error::ErrorCode;
use quarry_runtime::content_route::{
    parse_document_info_with, parse_document_with, ChildLimits, ParseBackend, PdfPageSummary,
    ProcessBackend,
};
use quarry_transform::sniff::ContentKind;

fn child_backend(limits: ChildLimits) -> ParseBackend {
    ParseBackend::Process(ProcessBackend {
        exe: env!("CARGO_BIN_EXE_quarry-doc-parse").into(),
        args: Vec::new(),
        limits,
    })
}

/// A one-page PDF whose content stream is `stream` under `filter` (empty for none).
fn pdf_with_stream(stream: &[u8], filter: &str) -> Vec<u8> {
    pdf_with_stream_syntax(stream, filter, "\n", "\n")
}

/// As [`pdf_with_stream`], with the text between `>>` and `stream` (`gap`) and
/// between `stream` and the data (`tail`) chosen by the caller.
fn pdf_with_stream_syntax(stream: &[u8], filter: &str, gap: &str, tail: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let number = offsets.len();
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    object(
        &mut out,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
    );
    let mut content = format!("<< /Length {} {filter} >>{gap}stream{tail}", stream.len()).into_bytes();
    content.extend_from_slice(stream);
    content.extend_from_slice(b"\nendstream");
    object(&mut out, &content);
    object(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn plain_pdf(text: &str) -> Vec<u8> {
    pdf_with_stream(format!("BT /F1 24 Tf 100 700 Td ({text}) Tj ET").as_bytes(), "")
}

/// `bytes` zeros, deflated: about a thousandth of the size.
fn deflated_zeros(bytes: u64) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![0u8; 1024 * 1024];
    for _ in 0..(bytes / chunk.len() as u64) {
        encoder.write_all(&chunk).unwrap();
    }
    encoder.finish().unwrap()
}

/// `bytes` as ASCII85 (with the `~>` end marker): a filter chain the inflate
/// pre-scan of `quarry_transform::pdf` does not look inside.
fn ascii85(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() / 4 * 5 + 8);
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(word);
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

#[tokio::test]
async fn a_normal_pdf_round_trips_through_the_child_with_the_same_text() {
    let pdf = plain_pdf("Hemmelig dokument");
    let in_process =
        parse_document_with(&ParseBackend::InProcess, ContentKind::Pdf, pdf.clone(), Duration::from_secs(15))
            .await
            .expect("in-process parse");
    let in_child = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        pdf,
        Duration::from_secs(15),
    )
    .await
    .expect("child parse");
    assert!(in_child.text.contains("Hemmelig dokument"), "{}", in_child.text);
    assert_eq!(in_child, in_process);
}

#[tokio::test]
async fn a_normal_flate_compressed_pdf_still_parses_through_the_child() {
    // The inflate budget reads the same stream the parser then reads: an ordinary
    // compressed page must pass it with the default limits.
    let content = deflate(b"BT /F1 24 Tf 100 700 Td (Komprimert side) Tj ET");
    let pdf = pdf_with_stream(&content, "/Filter /FlateDecode");
    let parsed = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        pdf,
        Duration::from_secs(15),
    )
    .await
    .expect("a compressed PDF parses");
    assert!(parsed.text.contains("Komprimert side"), "{}", parsed.text);
}

#[tokio::test]
async fn a_flate_bomb_is_refused_by_the_inflate_budget_under_the_default_limits_as_415() {
    // 1 GiB of zeros in about a megabyte, the construction of the rebuilt-edge
    // evidence run (WS5.C, F7), which answered 504 at the 15 s deadline because the
    // parser was still inflating when the clock ran out. The limits are the
    // production ones (768 MiB address space, 20 s CPU) and so is the deadline:
    // quarry_transform::pdf::check_inflate_budget stops the document at 256 MiB of
    // inflated stream data, before lopdf allocates anything, and the answer is the
    // typed 415 with the reason the DOCX bomb gets (the parser's own size limit).
    let compressed = deflated_zeros(1024 * 1024 * 1024);
    assert!(compressed.len() < 4 * 1024 * 1024, "the bomb is small: {}", compressed.len());
    let bomb = pdf_with_stream(&compressed, "/Filter /FlateDecode");

    let started = Instant::now();
    let error = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        bomb,
        Duration::from_secs(15),
    )
    .await
    .expect_err("the bomb must not parse");
    let took = started.elapsed();
    assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{error:?} after {took:?}");
    assert_eq!(error.details.as_ref().unwrap()["reason"], "too_large", "{error:?}");
    assert!(took < Duration::from_secs(10), "stopped by the budget, not by the deadline: {took:?}");

    let after = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        plain_pdf("Etter budsjettet"),
        Duration::from_secs(15),
    )
    .await
    .expect("a normal document still parses");
    assert!(after.text.contains("Etter budsjettet"));
}

#[tokio::test]
async fn a_bomb_in_a_spelling_the_first_pre_scan_missed_is_still_a_typed_415() {
    // Each of these is read by the parser (the control below parses a normal page
    // written the same way) and each answered 504 at the deadline before the
    // pre-scan learned to see through it: a comment inside the dictionary holding a
    // bracket, a comment between `>>` and `stream`, spaces after `stream`, and a
    // #-escaped filter name. 300 MiB is past the 256 MiB budget and cheap to build.
    let spellings: [(&str, &str, &str); 4] = [
        ("% (\n/Filter /FlateDecode", "\n", "\n"),
        ("/Filter /FlateDecode", " % note\n", "\n"),
        ("/Filter /FlateDecode", "\n", " \n"),
        ("/Filter /Flate#44ecode", "\n", "\n"),
    ];
    let normal = deflate(b"BT /F1 24 Tf 100 700 Td (Skjult side) Tj ET");
    let bomb_stream = deflated_zeros(300 * 1024 * 1024);
    for (entries, gap, tail) in spellings {
        let control = parse_document_with(
            &child_backend(ChildLimits::default()),
            ContentKind::Pdf,
            pdf_with_stream_syntax(&normal, entries, gap, tail),
            Duration::from_secs(15),
        )
        .await
        .unwrap_or_else(|e| panic!("control {entries:?} {gap:?} {tail:?}: {e:?}"));
        assert!(control.text.contains("Skjult side"), "{entries:?}: {}", control.text);

        let started = Instant::now();
        let error = parse_document_with(
            &child_backend(ChildLimits::default()),
            ContentKind::Pdf,
            pdf_with_stream_syntax(&bomb_stream, entries, gap, tail),
            Duration::from_secs(15),
        )
        .await
        .expect_err("the bomb must not parse");
        let took = started.elapsed();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{entries:?} {gap:?} {tail:?}: {error:?} after {took:?}");
        assert_eq!(error.details.as_ref().unwrap()["reason"], "too_large", "{error:?}");
        assert!(took < Duration::from_secs(10), "{entries:?}: stopped by the budget, not the deadline: {took:?}");
    }
}

#[tokio::test]
async fn a_bomb_behind_a_filter_chain_is_still_stopped_by_the_address_space_limit_and_answered_as_415() {
    // The inflate budget (quarry_transform::pdf) reads a /FlateDecode stream
    // directly; a stream behind an ASCII85 filter is not looked inside, so this
    // bomb reaches lopdf and only the address-space limit stops it: the backstop
    // behind the budget, kept under test.
    //
    // 1 GiB of zeros in about a megabyte. Inflating it needs more address space
    // than the child is allowed, so the allocation fails in the child. The test
    // narrows the production limit (768 MiB) to 256 MiB: lopdf inflates at roughly
    // 60 MiB a second, so on a slow machine the full limit would be met after the
    // 15 s deadline, which also kills the child but answers 504 instead of 415.
    let limits = ChildLimits {
        address_space: 256 * 1024 * 1024,
        ..ChildLimits::default()
    };
    let compressed = deflated_zeros(1024 * 1024 * 1024);
    assert!(compressed.len() < 4 * 1024 * 1024, "the bomb is small: {}", compressed.len());
    let bomb = pdf_with_stream(&ascii85(&compressed), "/Filter [/ASCII85Decode /FlateDecode]");

    let started = Instant::now();
    let error = parse_document_with(
        &child_backend(limits),
        ContentKind::Pdf,
        bomb,
        Duration::from_secs(15),
    )
    .await
    .expect_err("the bomb must not parse");
    assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{error:?}");
    assert_eq!(error.details.as_ref().unwrap()["reason"], "pdf_parse_failed");
    // A kill by the address-space limit says so; a plain rejection of the filter chain by
    // lopdf would carry no limit, and would not be the backstop this test keeps.
    assert_eq!(
        error.details.as_ref().unwrap()["limit"],
        "parser_stopped",
        "the address-space limit stopped it: {error:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(14), "stopped by the limit, not by the deadline");

    // The service is fine afterwards: the next document parses.
    let after = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        plain_pdf("Etter bomben"),
        Duration::from_secs(15),
    )
    .await
    .expect("a normal document still parses");
    assert!(after.text.contains("Etter bomben"));
}

#[tokio::test]
async fn a_cpu_spinning_child_is_stopped_by_the_cpu_limit() {
    // A shell that never stops, standing in for a parser stuck in a loop. One
    // second of CPU time, a fifteen second deadline: the limit must win.
    let backend = ParseBackend::Process(ProcessBackend {
        exe: "/bin/sh".into(),
        args: vec!["-c".into(), "while :; do :; done".into(), "spin".into()],
        limits: ChildLimits {
            cpu_seconds: 1,
            ..ChildLimits::default()
        },
    });
    let started = Instant::now();
    let error = parse_document_with(&backend, ContentKind::Pdf, b"%PDF-1.4".to_vec(), Duration::from_secs(15))
        .await
        .expect_err("a spinning parser must not return");
    assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
}

#[tokio::test]
async fn a_child_that_outlives_the_deadline_is_killed_and_the_error_is_a_timeout() {
    let backend = ParseBackend::Process(ProcessBackend {
        exe: "/bin/sh".into(),
        args: vec!["-c".into(), "exec sleep 30".into(), "sleeper".into()],
        limits: ChildLimits::default(),
    });
    let started = Instant::now();
    let error = parse_document_with(&backend, ContentKind::Docx, b"PK".to_vec(), Duration::from_secs(2))
        .await
        .expect_err("must time out");
    assert_eq!(error.code, ErrorCode::Timeout, "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn garbage_the_parser_cannot_read_is_a_typed_415_with_a_reason() {
    let error = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        b"%PDF-1.4 this is not a pdf".to_vec(),
        Duration::from_secs(15),
    )
    .await
    .expect_err("garbage");
    assert_eq!(error.code, ErrorCode::UnsupportedMedia);
    assert_eq!(error.details.unwrap()["reason"], "pdf_parse_failed");
}

#[test]
fn the_isolation_setting_defaults_to_a_child_on_unix_and_can_be_turned_off() {
    let exe = || Ok(std::path::PathBuf::from("/usr/bin/quarry-edge-rs"));
    assert_eq!(ParseBackend::from_setting(None, exe()).label(), "process");
    assert_eq!(ParseBackend::from_setting(Some("process"), exe()).label(), "process");
    assert_eq!(ParseBackend::from_setting(Some("inprocess"), exe()).label(), "inprocess");
    assert_eq!(ParseBackend::from_setting(Some(" InProcess "), exe()).label(), "inprocess");
    // A typo falls back to the default, not to no isolation by accident of parsing.
    assert_eq!(ParseBackend::from_setting(Some("proces"), exe()).label(), "process");
    // Not the edge binary (a test, a tool): the child argument means nothing to
    // it, so the default is in process; asking for process takes it as it is.
    let other = || Ok(std::path::PathBuf::from("/usr/bin/some-test-harness"));
    assert_eq!(ParseBackend::from_setting(None, other()).label(), "inprocess");
    assert_eq!(ParseBackend::from_setting(Some("proces"), other()).label(), "inprocess");
    assert_eq!(ParseBackend::from_setting(Some("process"), other()).label(), "process");
    // Without a path to re-execute, in process, with a warning.
    let missing: std::io::Result<std::path::PathBuf> = Err(std::io::Error::other("no exe"));
    assert_eq!(ParseBackend::from_setting(None, missing).label(), "inprocess");
}

/// A DOCX (a zip) whose `word/document.xml` is `document`, deflated.
fn docx_with_document(document: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document).unwrap();
        zip.finish().unwrap();
    }
    buf
}

#[tokio::test]
async fn a_docx_bomb_is_a_typed_415_and_the_next_document_still_parses() {
    // 40 MiB of text inside word/document.xml: a few tens of kilobytes deflated. The
    // DOCX parser caps word/document.xml at 32 MiB (docx::extract_text), which the
    // child reaches long before its address-space limit, so the answer is the cap
    // error mapped to too_large. A child killed by its own limits would instead give
    // docx_parse_failed with limit parser_stopped.
    let mut document = b"<w:document><w:body><w:p><w:r><w:t>".to_vec();
    document.resize(40 * 1024 * 1024, b'a');
    let bomb = docx_with_document(&document);
    assert!(bomb.len() < 512 * 1024, "the bomb is small: {}", bomb.len());

    let started = Instant::now();
    let error = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Docx,
        bomb,
        Duration::from_secs(15),
    )
    .await
    .expect_err("the bomb must not parse");
    assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{error:?}");
    let reason = error.details.as_ref().unwrap()["reason"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(reason, "too_large", "{error:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "stopped by a cap, not by the deadline: {:?}",
        started.elapsed()
    );

    // The service is fine afterwards: the next document parses.
    let normal = docx_with_document(
        b"<w:document><w:body><w:p><w:r><w:t>Etter bomben</w:t></w:r></w:p></w:body></w:document>",
    );
    let after = parse_document_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Docx,
        normal,
        Duration::from_secs(15),
    )
    .await
    .expect("a normal document still parses");
    assert_eq!(after.text, "Etter bomben");
}

// ---- the page-facts path of POST /v1/parse (INNH-11) ----------------------------

#[tokio::test]
async fn the_page_facts_come_back_from_the_child_the_same_as_in_process() {
    let pdf = plain_pdf("Hemmelig dokument");
    let queue = Duration::from_secs(5);
    let in_process = parse_document_info_with(
        &ParseBackend::InProcess,
        ContentKind::Pdf,
        pdf.clone(),
        Duration::from_secs(15),
        queue,
    )
    .await
    .expect("in-process parse");
    let in_child = parse_document_info_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        pdf,
        Duration::from_secs(15),
        queue,
    )
    .await
    .expect("child parse");
    assert!(in_child.document.text.contains("Hemmelig dokument"));
    assert_eq!(
        in_child.pdf,
        Some(PdfPageSummary {
            pages: 1,
            pages_without_text: 0
        })
    );
    assert_eq!(in_child, in_process);

    // A DOCX has no pages to report.
    let docx = docx_with_document(
        b"<w:document><w:body><w:p><w:r><w:t>Avtale</w:t></w:r></w:p></w:body></w:document>",
    );
    let parsed = parse_document_info_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Docx,
        docx,
        Duration::from_secs(15),
        queue,
    )
    .await
    .expect("docx");
    assert_eq!(parsed.document.text, "Avtale");
    assert_eq!(parsed.pdf, None);
}

#[tokio::test]
async fn a_bomb_on_the_page_facts_path_is_the_same_typed_415_and_the_next_document_parses() {
    // The page classification parses the PDF a second time; it runs inside the same child,
    // under the same limits, so a bomb the text read lets through cannot reach it either.
    let limits = ChildLimits {
        address_space: 256 * 1024 * 1024,
        ..ChildLimits::default()
    };
    let compressed = deflated_zeros(1024 * 1024 * 1024);
    let bomb = pdf_with_stream(&ascii85(&compressed), "/Filter [/ASCII85Decode /FlateDecode]");
    let error = parse_document_info_with(
        &child_backend(limits),
        ContentKind::Pdf,
        bomb,
        Duration::from_secs(15),
        Duration::from_secs(5),
    )
    .await
    .expect_err("the bomb must not parse");
    assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{error:?}");
    assert_eq!(error.details.as_ref().unwrap()["reason"], "pdf_parse_failed");

    let after = parse_document_info_with(
        &child_backend(ChildLimits::default()),
        ContentKind::Pdf,
        plain_pdf("Etter bomben"),
        Duration::from_secs(15),
        Duration::from_secs(5),
    )
    .await
    .expect("a normal document still parses");
    assert!(after.document.text.contains("Etter bomben"));
}
