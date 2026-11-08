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
use lopdf::{Dictionary, Document, Object, ObjectId};
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
/// passes this.
pub const INFLATE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

/// Most bytes the `/FlateDecode` streams that call themselves images may inflate
/// to, together. Text extraction does not read a real image, so a large scan is
/// allowed more room, but the label is the file's own claim and a content stream
/// can wear it too, so it is capped (lower than the 768 MiB address-space limit
/// of the parser child, which stays behind it).
pub const IMAGE_INFLATE_BUDGET_BYTES: u64 = 384 * 1024 * 1024;

// ---- per-page classification: text layer or scan (E3.6-3) -------------------

/// Non-white-space characters a page must show before its text layer counts
/// as real text. Below this, a page that also paints an image is treated as a
/// scan: a page number or a running header laid over a scanned image is not a
/// text layer, and reading such a page natively would drop the scan's content.
pub const MIN_TEXT_CHARS: usize = 20;

/// Most parent links [`resources_of`] follows. A page tree is a few levels deep;
/// the bound only stops a cyclic `/Parent` chain from looping.
const PAGE_TREE_DEPTH_LIMIT: usize = 32;

/// What a page holds, decided from its text layer and its image resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    /// Real text: the native extractor reads this page.
    Text,
    /// An image and no real text: a scan. Only scanned pages are candidates for
    /// a layout or OCR route; nothing routes them yet.
    Scanned,
    /// Neither text nor an image (blank, or vector art alone).
    Empty,
}

/// One page's classification. `page` is 1-based, as lopdf numbers pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageClassification {
    pub page: u32,
    pub kind: PageKind,
    /// Non-white-space characters the page's text produced.
    pub text_chars: usize,
    /// Whether the page's resources name an image XObject.
    pub has_image: bool,
}

/// The decision for one page from the two facts the page gives. Biased towards
/// `Scanned` when an image is present and the text is short: a scanned page
/// sent to OCR costs a call, while a scan read as text loses its content.
pub fn classify_page(text_chars: usize, has_image: bool) -> PageKind {
    if text_chars >= MIN_TEXT_CHARS {
        PageKind::Text
    } else if has_image {
        PageKind::Scanned
    } else if text_chars > 0 {
        PageKind::Text
    } else {
        PageKind::Empty
    }
}

/// Classifies every page of the PDF, in page order. It parses through the same
/// guarded loader as [`extract_text`]: a document over the inflate budget or
/// one lopdf cannot parse is an error, never a classification.
pub fn classify_pages(body: &[u8]) -> QuarryResult<Vec<PageClassification>> {
    let doc = load_doc(body)?;
    let mut pages = Vec::new();
    for (page_no, page_id) in doc.get_pages() {
        let text_chars = page_text_chars(&doc, page_no, page_id);
        let has_image = page_has_image(&doc, page_id);
        pages.push(PageClassification {
            page: page_no,
            kind: classify_page(text_chars, has_image),
            text_chars,
            has_image,
        });
    }
    Ok(pages)
}

/// Non-white-space characters of the page's text. The font-aware read first (the
/// one [`extract_text`] uses); when that finds nothing, the raw operands of the
/// page's text-showing operators, so a page whose font the reader cannot map
/// still counts as having a text layer.
fn page_text_chars(doc: &Document, page_no: u32, page_id: ObjectId) -> usize {
    let font_aware = doc.extract_text(&[page_no]).map(|text| non_white(&text)).unwrap_or(0);
    if font_aware > 0 {
        return font_aware;
    }
    doc.get_and_decode_page_content(page_id)
        .map(|content| {
            content
                .operations
                .iter()
                .filter_map(extract_text_op)
                .map(|text| non_white(&text))
                .sum()
        })
        .unwrap_or(0)
}

fn non_white(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

/// The resource dictionary that applies to a page: its own `/Resources`, or the
/// nearest ancestor's (Resources are inheritable through `/Parent`). `None` when
/// there is none, or the chain does not reach one within the depth limit.
fn resources_of(doc: &Document, page_id: ObjectId) -> Option<&Dictionary> {
    let mut node = page_id;
    for _ in 0..PAGE_TREE_DEPTH_LIMIT {
        let dict = doc.get_dictionary(node).ok()?;
        if let Ok(resources) = dict.get(b"Resources") {
            return match resources {
                Object::Dictionary(direct) => Some(direct),
                Object::Reference(id) => doc.get_dictionary(*id).ok(),
                _ => None,
            };
        }
        node = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// True when the page's resources name an image XObject. An image painted by a
/// content stream that does not name it in the resources is not seen, and an
/// image inside a form XObject is not followed; both are noted in the depth list.
fn page_has_image(doc: &Document, page_id: ObjectId) -> bool {
    let Some(resources) = resources_of(doc, page_id) else {
        return false;
    };
    let xobjects = match resources.get(b"XObject") {
        Ok(Object::Dictionary(direct)) => direct,
        Ok(Object::Reference(id)) => match doc.get_dictionary(*id) {
            Ok(dict) => dict,
            Err(_) => return false,
        },
        _ => return false,
    };
    xobjects.iter().any(|(_, entry)| {
        let stream = match entry {
            Object::Reference(id) => doc.get_object(*id).ok(),
            direct => Some(direct),
        };
        match stream {
            Some(Object::Stream(stream)) => stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|name| name == &b"Image"[..]),
            _ => false,
        }
    })
}

fn is_pdf_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0c | 0)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// The length of the `endobj` or `N G obj` keyword at `at`, if one starts there.
/// Nothing a string or a dictionary left open reaches past one of these.
fn object_boundary(body: &[u8], at: usize) -> Option<usize> {
    match body[at] {
        b'e' if body[at..].starts_with(b"endobj") && (at == 0 || is_pdf_space(body[at - 1])) => Some(6),
        b'o' if body[at..].starts_with(b"obj")
            && at > 0
            && is_pdf_space(body[at - 1])
            && body.get(at + 3).is_none_or(|b| is_pdf_space(*b) || matches!(b, b'<' | b'[')) =>
        {
            Some(3)
        }
        _ => None,
    }
}

/// The entries of a stream dictionary that sit directly in it: strings are
/// blanked and nested dictionaries are left out, so that a `/Subtype /Image`, a
/// `/FlateDecode` or a `/Length` that is only text inside a string or another
/// dictionary neither counts nor hides anything. Comments are skipped (a `(`,
/// `<<` or `<` inside one is not syntax: the reader treats a comment as white
/// space), and `#xx` escapes in names are decoded, as the reader does, so that
/// `/Flate#44ecode` is `/FlateDecode`.
fn top_level_entries(dict: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dict.len());
    let mut depth = 0u32;
    let mut i = 0usize;
    while i < dict.len() {
        match dict[i] {
            b'%' => {
                i += dict[i..].iter().position(|b| matches!(b, b'\r' | b'\n')).unwrap_or(dict.len() - i);
                out.push(b' ');
            }
            b'(' => {
                let mut nesting = 0u32;
                while i < dict.len() {
                    match dict[i] {
                        b'\\' => i += 1,
                        b'(' => nesting += 1,
                        b')' => {
                            nesting -= 1;
                            if nesting == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
                out.push(b' ');
            }
            b'<' if dict.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 2;
                out.push(b' ');
            }
            b'<' => {
                i += dict[i..].iter().position(|b| *b == b'>').unwrap_or(dict.len() - i) + 1;
                out.push(b' ');
            }
            b'>' if dict.get(i + 1) == Some(&b'>') => {
                depth = depth.saturating_sub(1);
                i += 2;
                out.push(b' ');
            }
            b'#' if depth == 1 && dict.len() > i + 2 && dict[i + 1].is_ascii_hexdigit() && dict[i + 2].is_ascii_hexdigit() => {
                let hex = std::str::from_utf8(&dict[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'#'));
                i += 3;
            }
            byte => {
                if depth == 1 {
                    out.push(byte);
                }
                i += 1;
            }
        }
    }
    out
}

/// True when `gap` holds nothing but white space and comments, which is all the
/// reader allows between a stream dictionary's `>>` and the `stream` keyword.
fn is_blank_or_comments(gap: &[u8]) -> bool {
    let mut i = 0usize;
    while i < gap.len() {
        if gap[i] == b'%' {
            i += gap[i..].iter().position(|b| matches!(b, b'\r' | b'\n')).unwrap_or(gap.len() - i);
        } else if is_pdf_space(gap[i]) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

/// True when `dict` has `/Subtype /Image` (spacing between the two names is free).
fn dict_is_image(dict: &[u8]) -> bool {
    const KEY: &[u8] = b"/Subtype";
    let mut from = 0usize;
    while let Some(found) = find(&dict[from..], KEY) {
        let mut at = from + found + KEY.len();
        while dict.get(at).is_some_and(|b| is_pdf_space(*b)) {
            at += 1;
        }
        if dict[at..].starts_with(b"/Image")
            && !dict.get(at + 6).is_some_and(|b| b.is_ascii_alphanumeric())
        {
            return true;
        }
        from += found + KEY.len();
    }
    false
}

/// The direct `/Length N` of a stream dictionary (not an indirect `N G R`).
fn direct_length(dict: &[u8]) -> Option<usize> {
    let at = find(dict, b"/Length")? + b"/Length".len();
    let rest = &dict[at..];
    let rest = &rest[rest.iter().take_while(|b| is_pdf_space(**b)).count()..];
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 12 {
        return None;
    }
    let value: usize = std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()?;
    let after = &rest[digits..];
    let after = &after[after.iter().take_while(|b| is_pdf_space(**b)).count()..];
    // `12 0 R` is a reference to a length stored elsewhere.
    if after.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    Some(value)
}

/// Inflates `input` into `scratch`, over and over, and adds what it produced to
/// `total`. Returns what this stream produced, or `Err` as soon as `total` passes
/// `budget`. A corrupt or truncated stream just ends (the parser copes with it);
/// what a truncated stream inflated before it ended is counted, because the
/// parser would inflate the same bytes.
fn inflate_counted(
    decoder: &mut flate2::Decompress,
    zlib_header: bool,
    input: &[u8],
    scratch: &mut [u8],
    total: &mut u64,
    budget: u64,
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
        if *total > budget {
            return Err(());
        }
        match status {
            Ok(Status::Ok) if took > 0 || produced > 0 => {}
            _ => return Ok(produced_here),
        }
    }
}

/// Refuses a PDF whose `/FlateDecode` streams would inflate past
/// [`INFLATE_BUDGET_BYTES`] (or, for streams labelled images,
/// [`IMAGE_INFLATE_BUDGET_BYTES`]), before `lopdf` gets to inflate anything.
///
/// One forward pass over the raw bytes, linear in the file size: it follows the
/// `<< >>` nesting, strings and comments, remembers the last top-level
/// dictionary, and when the `stream` keyword follows one directly and the
/// dictionary names `/FlateDecode`, inflates the stream's bytes (the direct
/// `/Length` when it is sane, else up to the next `endstream`) into a scratch
/// buffer that is thrown away, counting what came out. Each stream's bytes are
/// read once and the stream's data is skipped by the pass, so a file full of
/// `stream` keywords costs the file's size and nothing more; the inflate work is
/// bounded by the budgets (a stream is abandoned the moment a total passes its
/// budget), not by what the file claims.
///
/// This is a guard for the common shape of the attack, not a proof: a stream
/// behind a filter chain (ASCII85 then Flate), an encrypted stream, or a
/// dictionary this pass cannot read is not counted, and a stream that lies about
/// where it ends is cut at the place the pass found. The address-space and CPU
/// limits of the parser child and the deadline stay behind it.
pub fn check_inflate_budget(body: &[u8]) -> QuarryResult<()> {
    let mut total: u64 = 0;
    let mut image_total: u64 = 0;
    let mut scratch = vec![0u8; 64 * 1024];
    let mut decoder = flate2::Decompress::new(true);
    let mut depth = 0u32;
    let mut dict_start = 0usize;
    // The last dictionary closed at depth zero, as (start, end).
    let mut last_dict: Option<(usize, usize)> = None;
    let mut i = 0usize;
    while i < body.len() {
        // Whatever a stray `<<` or `(` left open does not reach into the next object.
        if let Some(length) = object_boundary(body, i) {
            depth = 0;
            last_dict = None;
            i += length;
            continue;
        }
        match body[i] {
            b'%' => {
                i += body[i..].iter().position(|b| matches!(b, b'\r' | b'\n')).unwrap_or(body.len() - i);
            }
            b'(' => {
                // A literal string: balanced parentheses, backslash escapes.
                let mut nesting = 0u32;
                while i < body.len() {
                    match body[i] {
                        b'\\' => i += 1,
                        b'(' => nesting += 1,
                        b')' => {
                            nesting -= 1;
                            if nesting == 0 {
                                i += 1;
                                break;
                            }
                        }
                        // An unterminated string ends where its object does.
                        _ if object_boundary(body, i).is_some() => break,
                        _ => {}
                    }
                    i += 1;
                }
            }
            b'<' if body.get(i + 1) == Some(&b'<') => {
                if depth == 0 {
                    dict_start = i;
                }
                depth += 1;
                i += 2;
            }
            b'<' => {
                // A hex string (an unterminated one ends where its object does).
                i += 1;
                while i < body.len() && body[i] != b'>' && object_boundary(body, i).is_none() {
                    i += 1;
                }
                i += usize::from(body.get(i) == Some(&b'>'));
            }
            b'>' if body.get(i + 1) == Some(&b'>') => {
                i += 2;
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        last_dict = Some((dict_start, i));
                    }
                }
            }
            b's' if depth == 0 && body[i..].starts_with(b"stream") => {
                let keyword = i;
                i += b"stream".len();
                let Some((dict_start, dict_end)) = last_dict.take() else { continue };
                if !is_blank_or_comments(&body[dict_end..keyword]) {
                    continue;
                }
                // `stream` is followed by spaces or tabs (the reader allows them) and then
                // CRLF, LF or a lone CR.
                let mut data_at = i;
                while matches!(body.get(data_at), Some(b' ' | b'\t')) {
                    data_at += 1;
                }
                let start = if body[data_at..].starts_with(b"\r\n") {
                    data_at + 2
                } else if matches!(body.get(data_at), Some(b'\n' | b'\r')) {
                    data_at + 1
                } else {
                    continue;
                };
                let entries = top_level_entries(&body[dict_start..dict_end]);
                let dict = entries.as_slice();
                // Where the data ends: the declared length when `endstream` follows
                // it, else the next `endstream`, else the end of the file.
                let declared_end = direct_length(dict)
                    .and_then(|length| start.checked_add(length))
                    .filter(|end| {
                        *end <= body.len()
                            && body[*end..].iter().skip_while(|b| is_pdf_space(**b)).take(9).copied().eq(*b"endstream")
                    });
                let end = declared_end
                    .or_else(|| find(&body[start..], b"endstream").map(|at| start + at))
                    .unwrap_or(body.len());
                i = end;
                if find(dict, b"/FlateDecode").is_none() {
                    continue;
                }
                let (counter, budget) = if dict_is_image(dict) {
                    (&mut image_total, IMAGE_INFLATE_BUDGET_BYTES)
                } else {
                    (&mut total, INFLATE_BUDGET_BYTES)
                };
                let data = &body[start..end];
                let counted = inflate_counted(&mut decoder, true, data, &mut scratch, counter, budget).and_then(|produced| {
                    // Like lopdf: raw deflate behind the two header bytes is the fallback,
                    // and only when the zlib reading produced nothing.
                    if produced == 0 && data.len() > 2 {
                        inflate_counted(&mut decoder, false, &data[2..], &mut scratch, counter, budget).map(|_| ())
                    } else {
                        Ok(())
                    }
                });
                if counted.is_err() {
                    return Err(QuarryError::new(
                        ErrorCode::BadRequest,
                        format!("PDF decompressed size exceeds the {} MiB budget", budget / (1024 * 1024)),
                    ));
                }
            }
            _ => i += 1,
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
        pdf_with_stream_syntax(stream, filter, "\n", "\n")
    }

    /// Like [`pdf_with_stream`], with the text between the dictionary's `>>` and
    /// the `stream` keyword (`gap`) and the text between the keyword and the data
    /// (`tail`) chosen by the caller.
    fn pdf_with_stream_syntax(stream: &[u8], filter: &str, gap: &str, tail: &str) -> Vec<u8> {
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
    fn image_streams_have_their_own_larger_budget_and_unfiltered_streams_are_not_counted() {
        let big = deflated_zeros(300 * 1024 * 1024);
        // Text extraction does not read a real image, so 300 MiB of it is not a bomb here
        // (it would be, as a content stream: the 256 MiB budget).
        let image = raw_streams("/Type /XObject /Subtype /Image /Filter /FlateDecode", &[&big]);
        check_inflate_budget(&image).expect("an image stream is held to the image budget");
        // Spacing between the two names is free.
        let image = raw_streams("/Subtype/Image /Filter /FlateDecode", &[&big]);
        check_inflate_budget(&image).expect("same, written without a space");
        // But the image budget is a cap, not an exemption.
        let huge = deflated_zeros(500 * 1024 * 1024);
        let error = check_inflate_budget(&raw_streams("/Subtype /Image /Filter /FlateDecode", &[&huge]))
            .expect_err("500 MiB of image is past the 384 MiB image budget");
        assert!(error.message.contains("384 MiB"), "{}", error.message);
        // No /FlateDecode in the dictionary: nothing to inflate.
        let plain = raw_streams("", &[&big]);
        check_inflate_budget(&plain).expect("an unfiltered stream is skipped");
        // `endstream` is not the start of a stream.
        check_inflate_budget(b"%PDF-1.4\nendstream\nendobj\n").unwrap();
    }

    #[test]
    fn a_content_stream_that_only_names_image_is_still_counted() {
        // lopdf reads a stream as page content whatever its dictionary claims, so
        // the word /Image anywhere but as a /Subtype value buys nothing.
        let big = deflated_zeros(300 * 1024 * 1024);
        for dict in [
            "/Filter /FlateDecode /Note /Image",
            "/Filter /FlateDecode /Note (/Subtype /Image)",
            "/Filter /FlateDecode /Subtype /ImageMask",
        ] {
            let error = check_inflate_budget(&raw_streams(dict, &[&big])).expect_err(dict);
            assert!(error.message.contains("256 MiB"), "{dict}: {}", error.message);
        }
    }

    #[test]
    fn obj_text_after_the_filter_does_not_hide_a_bomb() {
        let big = deflated_zeros(300 * 1024 * 1024);
        for dict in [
            "/Filter /FlateDecode /Note (obj)",
            "/Filter /FlateDecode /Note (1 0 obj) /Other <6f626a>",
            "/Filter /FlateDecode /Note /obj",
            "/Filter /FlateDecode /Nested << /A (>>) /B << /C 1 >> >>",
        ] {
            check_inflate_budget(&raw_streams(dict, &[&big])).expect_err(dict);
        }
        // Junk that opens a dictionary and never closes it ahead of the object does not
        // swallow the object.
        let mut junk = b"%PDF-1.4\n<< (unfinished\n".to_vec();
        junk.extend_from_slice(&raw_streams("/Filter /FlateDecode", &[&big])[9..]);
        check_inflate_budget(&junk).expect_err("the bomb behind junk is still found");
    }

    /// Spellings the reader (`lopdf`) accepts for a Flate stream that the first pre-scan
    /// did not see through: (dictionary entries, gap before `stream`, text after it).
    const READABLE_SPELLINGS: &[(&str, &str, &str)] = &[
        // A comment inside the dictionary, before the filter, with the brackets
        // the scan tracks in it.
        ("% (\n/Filter /FlateDecode", "\n", "\n"),
        ("% <<\n/Filter /FlateDecode", "\n", "\n"),
        ("% <\r/Filter /FlateDecode", "\n", "\n"),
        ("/Filter /FlateDecode % )\n/Note 1", "\n", "\n"),
        // A comment between `>>` and `stream`.
        ("/Filter /FlateDecode", " % note\n", "\n"),
        ("/Filter /FlateDecode", "\n% a (\n% b <<\n", "\n"),
        // Spaces or tabs between `stream` and the line end.
        ("/Filter /FlateDecode", "\n", " \n"),
        ("/Filter /FlateDecode", "\n", "\t \r\n"),
        ("/Filter /FlateDecode", "\n", "  \r"),
        // An escaped name.
        ("/Filter /Flate#44ecode", "\n", "\n"),
        ("/Fil#74er /FlateDecode", "\n", "\n"),
        ("/Filter [/Flate#44ecode]", "\n", "\n"),
        // All of them at once.
        ("% (\n/Fil#74er /Flate#44ecode", "\n%c\n", " \r\n"),
    ];

    #[test]
    fn the_reader_accepts_every_spelling_the_scan_must_see_through() {
        // The control: a small compressed page written each way is read by lopdf, so
        // a bomb written the same way is one the parser would inflate.
        let content = deflate(b"BT /F1 24 Tf 100 700 Td (Skjult tekst) Tj ET");
        for (entries, gap, tail) in READABLE_SPELLINGS {
            let pdf = pdf_with_stream_syntax(&content, entries, gap, tail);
            let text = extract_text(&pdf).unwrap_or_else(|e| panic!("{entries:?} {gap:?} {tail:?}: {e:?}"));
            assert!(text.contains("Skjult tekst"), "{entries:?} {gap:?} {tail:?}: {text:?}");
        }
    }

    #[test]
    fn a_bomb_written_in_any_readable_spelling_is_still_counted() {
        let big = deflated_zeros(300 * 1024 * 1024);
        for (entries, gap, tail) in READABLE_SPELLINGS {
            let pdf = pdf_with_stream_syntax(&big, entries, gap, tail);
            let error = check_inflate_budget(&pdf).expect_err(&format!("{entries:?} {gap:?} {tail:?}"));
            assert!(error.message.contains("256 MiB"), "{entries:?} {gap:?} {tail:?}: {}", error.message);
        }
    }

    #[test]
    fn comments_and_escapes_do_not_make_a_stream_an_image_or_hide_its_length() {
        // /Subtype /Image written inside a comment is not an image (the 256 MiB budget
        // applies), and one written with an escape is (the larger one).
        let big = deflated_zeros(300 * 1024 * 1024);
        let commented = raw_streams("/Filter /FlateDecode % /Subtype /Image\n", &[&big]);
        assert!(check_inflate_budget(&commented).is_err());
        let escaped = raw_streams("/Filter /FlateDecode /Subtype /Ima#67e", &[&big]);
        check_inflate_budget(&escaped).expect("an escaped /Image is an image");
        // A comment does not hide the /Length either: both streams are seen.
        let part = deflated_zeros(200 * 1024 * 1024);
        let two = raw_streams("% (\n/Filter /FlateDecode", &[&part, &part]);
        assert!(check_inflate_budget(&two).is_err());
    }

    #[test]
    fn a_truncated_stream_counts_what_it_inflated() {
        // The parser would inflate the same bytes, so they are counted; a corrupt or
        // short stream below the budget is simply not an error.
        let mut cut = deflated_zeros(600 * 1024 * 1024);
        cut.truncate(cut.len() * 3 / 4);
        let error = check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[&cut]))
            .expect_err("about 450 MiB inflated before the cut");
        assert!(error.message.contains("exceeds"), "{}", error.message);
        let mut half = deflate(&vec![b'a'; 100_000]);
        half.truncate(half.len() / 2);
        check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[&half])).unwrap();
        check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[b"not deflate data at all"])).unwrap();
        check_inflate_budget(&raw_streams("/Filter /FlateDecode", &[b""])).unwrap();
    }

    #[test]
    fn the_scan_is_linear_on_hostile_input() {
        // Dense `stream` keywords, with and without dictionaries, and unbalanced
        // brackets: the cost is the size of the file, not a lookback per keyword.
        let started = std::time::Instant::now();
        let dense = b"stream\n".repeat(600_000); // 4.2 MB
        check_inflate_budget(&dense).unwrap();
        let mut with_filter = b"1 0 obj\n<< /Filter /FlateDecode >>\n".to_vec();
        with_filter.extend_from_slice(&dense);
        check_inflate_budget(&with_filter).unwrap();
        let dicts = b"<< /Filter /FlateDecode >>\nstream\n".repeat(120_000); // 4 MB
        check_inflate_budget(&dicts).unwrap();
        let open = b"<<(".repeat(1_400_000);
        check_inflate_budget(&open).unwrap();
        let mixed = b"1 0 obj << /Length 4 /Filter /FlateDecode >> stream\nabcd\nendstream endobj ".repeat(60_000);
        check_inflate_budget(&mixed).unwrap();
        assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
    }

    #[test]
    fn the_declared_length_and_endstream_both_bound_a_stream() {
        // Two streams back to back, the first with a direct /Length: the second is
        // still seen (and the first is not read past its end).
        let part = deflated_zeros(200 * 1024 * 1024);
        let two = raw_streams("/Filter /FlateDecode", &[&part, &part]);
        check_inflate_budget(&two).expect_err("400 MiB in two streams");
        // An indirect /Length falls back to endstream.
        let mut out = b"%PDF-1.4\n1 0 obj\n<< /Length 9 0 R /Filter /FlateDecode >>\nstream\n".to_vec();
        out.extend_from_slice(&deflated_zeros(300 * 1024 * 1024));
        out.extend_from_slice(b"\nendstream\nendobj\n");
        check_inflate_budget(&out).expect_err("a bomb with an indirect length");
    }

    // ---- per-page classification (E3.6-3) -----------------------------------

    /// One page of a [`build_pdf`] document: `text` is shown in Helvetica when
    /// present, and `image` paints the one-pixel image XObject `Im1` over the page.
    struct PageSpec {
        text: Option<&'static str>,
        image: bool,
    }

    fn text_page(text: &'static str) -> PageSpec {
        PageSpec { text: Some(text), image: false }
    }

    fn scan_page() -> PageSpec {
        PageSpec { text: None, image: true }
    }

    /// A PDF with one page per spec, built with lopdf so the objects are well formed.
    /// `resources_on_root` puts the font and image resources on the page tree root
    /// instead of each page, so the pages inherit them.
    fn build_pdf(pages: &[PageSpec], resources_on_root: bool) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1,
                "Height" => 1,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
            },
            vec![0x80],
        ));
        let resources = dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Im1" => image },
        };
        let mut kids: Vec<Object> = Vec::new();
        for spec in pages {
            let mut operations = Vec::new();
            if let Some(text) = spec.text {
                operations.push(Operation::new("BT", vec![]));
                operations.push(Operation::new("Tf", vec!["F1".into(), 12.into()]));
                operations.push(Operation::new("Td", vec![72.into(), 700.into()]));
                operations.push(Operation::new("Tj", vec![Object::string_literal(text.as_bytes().to_vec())]));
                operations.push(Operation::new("ET", vec![]));
            }
            if spec.image {
                operations.push(Operation::new("q", vec![]));
                operations.push(Operation::new(
                    "cm",
                    vec![595.into(), 0.into(), 0.into(), 842.into(), 0.into(), 0.into()],
                ));
                operations.push(Operation::new("Do", vec!["Im1".into()]));
                operations.push(Operation::new("Q", vec![]));
            }
            let content = doc.add_object(Stream::new(dictionary! {}, Content { operations }.encode().unwrap()));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            };
            if !resources_on_root {
                // A page names the image only when it paints it. The classifier reads the
                // resources, so a text page that named the image would look like a scan.
                let mut own = resources.clone();
                if !spec.image {
                    own.remove(b"XObject");
                }
                page.set("Resources", own);
            }
            kids.push(doc.add_object(page).into());
        }
        let mut pages_dict = dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => pages.len() as i64,
        };
        if resources_on_root {
            pages_dict.set("Resources", resources);
        }
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn the_decision_table_reads_text_then_image_then_nothing() {
        assert_eq!(classify_page(0, false), PageKind::Empty);
        assert_eq!(classify_page(0, true), PageKind::Scanned);
        assert_eq!(classify_page(1, false), PageKind::Text, "short text with no image is still text");
        assert_eq!(classify_page(MIN_TEXT_CHARS - 1, true), PageKind::Scanned);
        assert_eq!(classify_page(MIN_TEXT_CHARS, true), PageKind::Text);
        assert_eq!(classify_page(MIN_TEXT_CHARS, false), PageKind::Text);
    }

    #[test]
    fn a_text_page_and_a_scanned_page_are_told_apart_in_page_order() {
        let pdf = build_pdf(&[text_page("This is a text page with enough characters."), scan_page()], false);
        let pages = classify_pages(&pdf).expect("classify");
        assert_eq!(pages.iter().map(|p| p.page).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(pages[0].kind, PageKind::Text);
        assert!(pages[0].text_chars >= MIN_TEXT_CHARS, "{:?}", pages[0]);
        assert!(!pages[0].has_image);
        assert_eq!(pages[1].kind, PageKind::Scanned);
        assert_eq!(pages[1].text_chars, 0);
        assert!(pages[1].has_image);
    }

    #[test]
    fn a_page_number_over_a_scan_is_still_a_scan() {
        // The footer is one character: below the text threshold, and the page paints an image.
        let footer = PageSpec { text: Some("1"), image: true };
        let pages = classify_pages(&build_pdf(&[footer], false)).expect("classify");
        assert_eq!(pages[0].kind, PageKind::Scanned, "{:?}", pages[0]);
        assert_eq!(pages[0].text_chars, 1);
    }

    #[test]
    fn a_blank_page_is_empty_and_a_short_text_page_is_text() {
        let blank = PageSpec { text: None, image: false };
        let pages = classify_pages(&build_pdf(&[blank, text_page("Hi")], false)).expect("classify");
        assert_eq!(pages[0].kind, PageKind::Empty);
        assert_eq!(pages[1].kind, PageKind::Text);
        assert!(!pages[1].has_image);
    }

    #[test]
    fn resources_inherited_from_the_page_tree_root_are_seen() {
        // The image is named only on the root: the scan page must still see it.
        let pages = classify_pages(&build_pdf(&[scan_page(), text_page("Inherited resources page text.")], true))
            .expect("classify");
        assert!(pages[0].has_image, "{:?}", pages[0]);
        assert_eq!(pages[0].kind, PageKind::Scanned);
        assert_eq!(pages[1].kind, PageKind::Text);
    }

    #[test]
    fn a_cyclic_page_tree_is_bounded_not_followed_forever() {
        // A page whose /Parent is itself and that has no resources anywhere.
        let mut doc = Document::with_version("1.5");
        let page_id = doc.new_object_id();
        doc.objects.insert(
            page_id,
            Object::Dictionary(lopdf::dictionary! { "Type" => "Page", "Parent" => page_id }),
        );
        assert!(resources_of(&doc, page_id).is_none());
        assert!(!page_has_image(&doc, page_id));
    }

    #[test]
    fn the_page_kind_serialises_as_a_snake_case_word() {
        assert_eq!(serde_json::to_string(&PageKind::Scanned).unwrap(), "\"scanned\"");
        assert_eq!(serde_json::to_string(&PageKind::Empty).unwrap(), "\"empty\"");
    }

    #[test]
    fn classification_sits_behind_the_inflate_budget() {
        // A flate bomb is refused before any page is read, so it gets no classification.
        let bomb = pdf_with_stream(&deflated_zeros(300 * 1024 * 1024), "/Filter /FlateDecode");
        let error = classify_pages(&bomb).expect_err("the bomb must not be classified");
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("exceeds"), "{}", error.message);
    }

    #[test]
    fn extract_headings_on_malformed_returns_error() {
        let body = b"%PDF-1.4 garbage";
        let result = extract_headings(body);
        assert!(result.is_err());
    }
}
