//! PDF parsing: flat text + heading extraction via font-size walk.
//!
//! Two entry points:
//!   - [`extract_text`]: concatenated flat text suitable for full-text
//!     indexing / chunking.
//!   - [`extract_headings`]: text runs flagged as headings because their
//!     font size sits in the upper percentile of the document. Useful
//!     for downstream summarisation, table-of-contents generation, and
//!     chunk-boundary placement on long technical PDFs.
//!
//! Implementation: we parse the PDF with `lopdf` and walk every page's
//! content stream. Each operator is one of ~80 PDF operators; we only
//! care about the small subset that affects text + font state:
//!   - `Tf` (set font + size)         → updates current font size
//!   - `Tj`, `'`, `"` (show string)   → emits a text run
//!   - `TJ` (show array)              → emits a text run from an array
//!     of strings (PDF uses arrays for kerned text)
//!   - `cm`, `Tm`, `T*`               → page transforms; the effective
//!     font size for ranking purposes is `current_font_size * |sy|` of
//!     the cumulative text-matrix scale on Y, but for heading ranking
//!     the raw `Tf` size is a good enough proxy in 95% of PDFs.
//!
//! We rank font sizes seen across the whole document and treat the
//! top ~15% as headings. This works well on typical reports / papers
//! where headings are visibly larger than body text. PDFs that emit
//! the same font size for everything (rare in modern documents)
//! yield zero headings — by design.

use std::collections::HashMap;

use lopdf::content::Operation;
use lopdf::{Document, Object};
use serde::{Deserialize, Serialize};

use quarry_core::{
    error::{ErrorCode, QuarryError},
    QuarryResult,
};

const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;

/// Fraction of distinct font sizes (counted from the largest) that
/// qualify as headings. With `0.15`, the largest ~15% of the size
/// distribution is treated as headings. Tuned by eye on a corpus of
/// 30 mixed PDFs (academic papers, product datasheets, government
/// reports). Pure body-text PDFs end up with 0 headings — correct.
const HEADING_TOP_FRACTION: f32 = 0.15;

/// Below this font size we never call a run a heading even if it's in
/// the upper percentile — protects against PDFs where the "biggest"
/// text is still tiny (e.g. all-uppercase tracked-out subheadings at
/// 8pt). Empirical floor.
const HEADING_MIN_PT: f32 = 9.0;

/// PDF heading run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfHeading {
    /// 1-based page number where the heading appears.
    pub page: u32,
    /// Font size in points as declared by the most recent `Tf`
    /// operator. Not adjusted for cumulative text-matrix scaling —
    /// good enough for ranking.
    pub size_pt: f32,
    /// Plain text content of the heading. Multiple text-show operators
    /// at the same font size on the same line are concatenated.
    pub text: String,
}

pub fn extract_text(body: &[u8]) -> QuarryResult<String> {
    let doc = load_doc(body)?;
    let mut out = String::new();
    for (page_no, page_id) in doc.get_pages() {
        // Font-aware first. Most PDFs embed a subset font whose strings are glyph
        // codes, not characters (the text is in the font's ToUnicode map or
        // encoding); reading the raw string operands then yields control bytes.
        // lopdf applies the page's fonts, so use that and keep the raw walk below
        // for a page it cannot read or finds no text on.
        if let Ok(text) = doc.extract_text(&[page_no]) {
            if !text.trim().is_empty() {
                out.push_str(&text);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }
        if let Ok(stream) = doc.get_and_decode_page_content(page_id) {
            for op in stream.operations {
                if let Some(s) = extract_text_op(&op) {
                    out.push_str(&s);
                    if !s.ends_with(' ') {
                        out.push('\n');
                    }
                }
            }
        }
    }
    Ok(normalize_text(&out))
}

/// Extract headings ranked by font size. See module docs for the
/// ranking heuristic.
pub fn extract_headings(body: &[u8]) -> QuarryResult<Vec<PdfHeading>> {
    let doc = load_doc(body)?;

    // First pass: gather every (page, size, text) tuple so we can rank
    // font sizes globally. Streaming would be cheaper but PDFs over
    // 25 MB are already rejected by `load_doc`, so the all-at-once
    // approach is bounded.
    let mut runs: Vec<PdfHeading> = Vec::new();
    let mut size_counts: HashMap<u32, usize> = HashMap::new();

    for (page_no, page_id) in doc.get_pages() {
        let Ok(content) = doc.get_and_decode_page_content(page_id) else {
            continue;
        };
        let mut current_size: f32 = 12.0; // PDF default if Tf never set
        let mut accum_text = String::new();
        let mut accum_size = current_size;

        let flush = |runs: &mut Vec<PdfHeading>, text: &mut String, size: f32, page: u32| {
            let t = text.trim().to_string();
            if !t.is_empty() {
                runs.push(PdfHeading {
                    page,
                    size_pt: size,
                    text: t,
                });
            }
            text.clear();
        };

        for op in content.operations {
            match op.operator.as_str() {
                "Tf" => {
                    // Operands: [font_name, size]
                    if let Some(size) = op.operands.last().and_then(object_to_f32) {
                        // Flush previous run before switching size.
                        if (size - accum_size).abs() > f32::EPSILON {
                            flush(&mut runs, &mut accum_text, accum_size, page_no);
                            accum_size = size;
                        }
                        current_size = size;
                        let bucket = (size * 10.0).round() as u32;
                        *size_counts.entry(bucket).or_insert(0) += 1;
                    }
                }
                "Tj" | "'" => {
                    if let Some(s) = op.operands.first().and_then(object_to_text) {
                        accum_text.push_str(&s);
                        accum_text.push(' ');
                    }
                }
                "\"" => {
                    // Operands: aw ac string — use last operand.
                    if let Some(s) = op.operands.last().and_then(object_to_text) {
                        accum_text.push_str(&s);
                        accum_text.push(' ');
                    }
                }
                "TJ" => {
                    if let Some(Object::Array(arr)) = op.operands.first() {
                        for el in arr {
                            if let Some(s) = object_to_text(el) {
                                accum_text.push_str(&s);
                            }
                        }
                        accum_text.push(' ');
                    }
                }
                // Text object boundary → flush so headings don't bleed
                // into the next paragraph that happens to share a size.
                "ET" => {
                    flush(&mut runs, &mut accum_text, accum_size, page_no);
                    accum_size = current_size;
                }
                _ => {}
            }
        }
        flush(&mut runs, &mut accum_text, accum_size, page_no);
    }

    // Rank sizes. `size_counts` is keyed on size*10 (so 12.0 -> 120) to
    // collapse minor float jitter from PDF producers. Largest sizes
    // first; cumulative-frequency cutoff selects the top
    // HEADING_TOP_FRACTION of the size distribution.
    let mut sizes: Vec<(u32, usize)> = size_counts.into_iter().collect();
    sizes.sort_by(|a, b| b.0.cmp(&a.0));
    let total: usize = sizes.iter().map(|(_, n)| *n).sum();
    if total == 0 {
        return Ok(Vec::new());
    }
    let cutoff_target = ((total as f32) * HEADING_TOP_FRACTION).ceil() as usize;
    let mut acc = 0usize;
    let mut min_heading_bucket: u32 = u32::MAX;
    for (bucket, n) in &sizes {
        acc += *n;
        min_heading_bucket = (*bucket).min(min_heading_bucket);
        if acc >= cutoff_target {
            break;
        }
    }

    let min_heading_size_pt = (min_heading_bucket as f32) / 10.0;
    if min_heading_size_pt < HEADING_MIN_PT {
        // Doc is all small text — no headings to surface.
        return Ok(Vec::new());
    }

    Ok(runs
        .into_iter()
        .filter(|r| r.size_pt >= min_heading_size_pt)
        .collect())
}

/// Most bytes the `/FlateDecode` streams of one PDF may inflate to, together.
///
/// `lopdf` 0.42 inflates a stream with no output cap and slowly (a 1 GiB bomb in
/// a 1 MB file was still inflating when the 15 s deadline fired, which answered
/// 504 instead of a typed 415). [`check_inflate_budget`] inflates the same
/// streams first, into nothing, and refuses the document as soon as the total
/// passes this. Image streams are not counted: text extraction never inflates
/// them.
pub const INFLATE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

/// How far back from the `stream` keyword the dictionary is looked for.
const STREAM_DICT_LOOKBACK: usize = 8 * 1024;

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

/// Inflates `input` into `scratch`, over and over, and adds what it produced to
/// `total`. Returns what this stream produced, or `Err` as soon as `total` passes
/// the budget. A corrupt or truncated stream just ends (the parser copes with it).
fn inflate_counted(
    decoder: &mut flate2::Decompress,
    zlib_header: bool,
    input: &[u8],
    scratch: &mut [u8],
    total: &mut u64,
) -> Result<u64, ()> {
    use flate2::{FlushDecompress, Status};

    decoder.reset(zlib_header);
    let mut consumed = 0usize;
    let mut produced_here = 0u64;
    loop {
        let (in_before, out_before) = (decoder.total_in(), decoder.total_out());
        let status = decoder.decompress(&input[consumed..], scratch, FlushDecompress::None);
        let took = (decoder.total_in() - in_before) as usize;
        let produced = decoder.total_out() - out_before;
        consumed += took;
        produced_here += produced;
        *total += produced;
        if *total > INFLATE_BUDGET_BYTES {
            return Err(());
        }
        match status {
            Ok(Status::Ok) if took > 0 || produced > 0 => {}
            _ => return Ok(produced_here),
        }
    }
}

/// Refuses a PDF whose `/FlateDecode` streams would inflate past
/// [`INFLATE_BUDGET_BYTES`], before `lopdf` gets to inflate anything.
///
/// A bounded pre-scan of the raw bytes: every `stream` keyword that follows a
/// dictionary naming `/FlateDecode` starts a zlib stream, which is inflated into a
/// scratch buffer that is thrown away, and counted. The work is bounded by the
/// budget (a stream is abandoned the moment the total passes it), not by what the
/// file claims, so a bomb costs a few hundred megabytes of inflate time, not
/// gigabytes of memory.
///
/// This is a guard for the common shape of the attack, not a proof: a stream
/// that hides its dictionary (a filter chain such as ASCII85 then Flate, an
/// encrypted stream, a dictionary further than [`STREAM_DICT_LOOKBACK`] back)
/// is not counted. The address-space and CPU limits of the parser child and the
/// deadline stay behind it.
pub fn check_inflate_budget(body: &[u8]) -> QuarryResult<()> {
    let mut total: u64 = 0;
    let mut scratch = vec![0u8; 64 * 1024];
    let mut decoder = flate2::Decompress::new(true);
    let mut from = 0usize;
    while let Some(found) = find(&body[from..], b"stream") {
        let keyword = from + found;
        from = keyword + b"stream".len();
        if body[..keyword].ends_with(b"end") {
            continue;
        }
        // `stream` is followed by CRLF or LF (a lone CR is tolerated by readers).
        let start = if body[from..].starts_with(b"\r\n") {
            from + 2
        } else if matches!(body.get(from), Some(b'\n' | b'\r')) {
            from + 1
        } else {
            continue;
        };
        let before = &body[keyword.saturating_sub(STREAM_DICT_LOOKBACK)..keyword];
        let dict = &before[rfind(before, b"obj").map_or(0, |at| at + 3)..];
        if find(dict, b"/FlateDecode").is_none() || find(dict, b"/Image").is_some() {
            continue;
        }
        let data = &body[start..];
        let counted = inflate_counted(&mut decoder, true, data, &mut scratch, &mut total).and_then(|produced| {
            // Like lopdf: raw deflate behind the two header bytes is the fallback,
            // and only when the zlib reading produced nothing.
            if produced == 0 && data.len() > 2 {
                inflate_counted(&mut decoder, false, &data[2..], &mut scratch, &mut total).map(|_| ())
            } else {
                Ok(())
            }
        });
        if counted.is_err() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                format!(
                    "PDF decompressed size exceeds the {} MiB budget",
                    INFLATE_BUDGET_BYTES / (1024 * 1024)
                ),
            ));
        }
    }
    Ok(())
}

fn load_doc(body: &[u8]) -> QuarryResult<Document> {
    if body.len() > MAX_PDF_BYTES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "PDF is too large to parse in-process",
        ));
    }
    check_inflate_budget(body)?;
    Document::load_mem(body).map_err(|error| {
        QuarryError::new(ErrorCode::DriverFailed, "PDF parse failed")
            .with_details(serde_json::json!({ "error": error.to_string() }))
    })
}

fn extract_text_op(op: &Operation) -> Option<String> {
    match op.operator.as_str() {
        "Tj" | "'" => op.operands.first().and_then(object_to_text),
        "\"" => op.operands.last().and_then(object_to_text),
        "TJ" => match op.operands.first() {
            Some(Object::Array(arr)) => Some(
                arr.iter()
                    .filter_map(object_to_text)
                    .collect::<Vec<_>>()
                    .join(""),
            ),
            _ => None,
        },
        _ => None,
    }
}

fn object_to_text(obj: &Object) -> Option<String> {
    match obj {
        Object::String(bytes, _) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

fn object_to_f32(obj: &Object) -> Option<f32> {
    match obj {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn normalize_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_pdf_is_rejected_before_parsing() {
        let body = vec![0; MAX_PDF_BYTES + 1];
        let error = extract_text(&body).expect_err("oversized PDF rejected");
        assert_eq!(error.code, ErrorCode::BadRequest);
    }

    #[test]
    fn malformed_pdf_returns_driver_failed() {
        let body = b"not a pdf";
        let error = extract_text(body).expect_err("malformed PDF rejected");
        assert_eq!(error.code, ErrorCode::DriverFailed);
    }

    /// A one-page PDF whose only font is a subset font: the page shows the glyph
    /// codes 1 and 2, and the font's ToUnicode map says they are "H" and "i".
    fn subset_font_pdf() -> Vec<u8> {
        use lopdf::content::Content;
        use lopdf::{dictionary, Stream};

        let cmap = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
            /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
            /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
            1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
            2 beginbfchar\n<01> <0048>\n<02> <0069>\nendbfchar\n\
            endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let to_unicode = doc.add_object(Stream::new(dictionary! {}, cmap.as_bytes().to_vec()));
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "TrueType",
            "BaseFont" => "ABCDEF+Subset",
            "FirstChar" => 0,
            "LastChar" => 2,
            "Widths" => vec![0.into(), 500.into(), 500.into()],
            "ToUnicode" => to_unicode,
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(vec![1u8, 2u8])]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn subset_font_text_is_read_through_the_tounicode_map() {
        let text = extract_text(&subset_font_pdf()).expect("parse");
        assert_eq!(text, "Hi", "glyph codes must map to characters, got {text:?}");
        assert!(
            !text.chars().any(char::is_control),
            "no raw glyph codes in the output: {text:?}"
        );
    }

    // ---- the inflate budget (R7, WS5.C) -------------------------------------

    /// `bytes` zeros, zlib-deflated: about a thousandth of the size.
    fn deflated_zeros(bytes: usize) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..(bytes / chunk.len()) {
            encoder.write_all(&chunk).unwrap();
        }
        encoder.finish().unwrap()
    }

    /// Indirect objects holding `stream` under the dictionary entries `dict`, one
    /// after the other. Not a loadable PDF: the pre-scan reads raw bytes only.
    fn raw_streams(dict: &str, streams: &[&[u8]]) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        for (index, stream) in streams.iter().enumerate() {
            out.extend_from_slice(format!("{} 0 obj\n<< /Length {} {dict} >>\nstream\n", index + 1, stream.len()).as_bytes());
            out.extend_from_slice(stream);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        }
        out
    }

    /// A one-page PDF whose content stream is `stream` with the dictionary entry `filter`.
    fn pdf_with_stream(stream: &[u8], filter: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |out: &mut Vec<u8>, body: &[u8]| {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        };
        object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        object(
            &mut out,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        );
        let mut content = format!("<< /Length {} {filter} >>\nstream\n", stream.len()).into_bytes();
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
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n", offsets.len() + 1).as_bytes(),
        );
        out
    }

    fn deflate(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn a_normal_compressed_pdf_is_within_the_budget_and_still_parses() {
        let content = deflate(b"BT /F1 24 Tf 100 700 Td (Komprimert tekst) Tj ET");
        let pdf = pdf_with_stream(&content, "/Filter /FlateDecode");
        check_inflate_budget(&pdf).expect("a small compressed stream is within the budget");
        let text = extract_text(&pdf).expect("parse");
        assert!(text.contains("Komprimert tekst"), "{text:?}");
    }

    #[test]
    fn a_flate_bomb_is_refused_before_lopdf_inflates_it() {
        // 1 GiB of zeros in about a megabyte.
        let bomb = pdf_with_stream(&deflated_zeros(1024 * 1024 * 1024), "/Filter /FlateDecode");
        assert!(bomb.len() < 4 * 1024 * 1024, "the bomb is small: {}", bomb.len());
        let started = std::time::Instant::now();
        let error = extract_text(&bomb).expect_err("the bomb must not parse");
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("exceeds"), "{}", error.message);
        // It stops at the budget (256 MiB), well before the whole gigabyte.
        assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
        assert!(extract_headings(&bomb).is_err(), "the heading entry point has the same guard");
    }

    #[test]
    fn the_budget_counts_every_stream_together() {
        let part = deflated_zeros(100 * 1024 * 1024);
        let two = raw_streams("/Filter /FlateDecode", &[&part, &part]);
        check_inflate_budget(&two).expect("200 MiB in two streams fits 256 MiB");
        let three = raw_streams("/Filter /FlateDecode", &[&part, &part, &part]);
        let error = check_inflate_budget(&three).expect_err("300 MiB in three streams does not");
        assert!(error.message.contains("exceeds"), "{}", error.message);
    }

    #[test]
    fn image_and_uncompressed_streams_are_not_counted() {
        let big = deflated_zeros(300 * 1024 * 1024);
        // Text extraction never inflates an image, so a large one is not a bomb here.
        let image = raw_streams("/Type /XObject /Subtype /Image /Filter /FlateDecode", &[&big]);
        check_inflate_budget(&image).expect("an image stream is skipped");
        // No /FlateDecode in the dictionary: nothing to inflate.
        let plain = raw_streams("", &[&big]);
        check_inflate_budget(&plain).expect("an unfiltered stream is skipped");
        // `endstream` is not the start of a stream.
        check_inflate_budget(b"%PDF-1.4\nendstream\nendobj\n").unwrap();
    }

    #[test]
    fn a_corrupt_or_truncated_flate_stream_is_not_a_budget_error() {
        let mut half = deflate(&vec![b'a'; 100_000]);
        half.truncate(half.len() / 2);
        check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[&half])).unwrap();
        check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[b"not deflate data at all"])).unwrap();
        check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[b""])).unwrap();
    }

    #[test]
    fn extract_headings_on_malformed_returns_error() {
        let body = b"%PDF-1.4 garbage";
        let result = extract_headings(body);
        assert!(result.is_err());
    }
}
