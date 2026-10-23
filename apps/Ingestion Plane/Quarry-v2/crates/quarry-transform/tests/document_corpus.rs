//! Fixture corpus for document parsing and PII (E3.6-10).
//!
//! `tests/fixtures/documents/corpus.json` is the ground truth, and
//! `tests/fixtures/documents/generate.py` writes both it and the files. The
//! corpus is ten synthetic Norwegian public-sector documents (eight PDFs, two
//! DOCX), with text, scanned and mixed pages. The tests read the files, run the
//! in-process readers (`pdf`, `docx`, `pii`) and compare them with the recorded
//! truth. Nothing here runs a stack or a route.
//!
//! The tests are named `corpus_*` so `cargo test -p quarry-transform corpus_`
//! selects them.

use std::path::PathBuf;

use quarry_transform::{docx, pdf, pii};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MANIFEST: &str = include_str!("fixtures/documents/corpus.json");
const SCHEMA: &str = "quarry-document-corpus/1";
const NORWEGIAN_SET: usize = 10;
const PII_KINDS: [&str; 5] = ["email", "phone", "iban", "card", "fodselsnummer"];

#[derive(Deserialize)]
struct Corpus {
    schema: String,
    documents: Vec<Document>,
}

#[derive(Deserialize)]
struct Document {
    id: String,
    file: String,
    format: String,
    language: String,
    summary: String,
    bytes: u64,
    sha256: String,
    /// PDF only: the page kinds the document must classify to, in page order.
    #[serde(default)]
    pages: Vec<ExpectedPage>,
    /// Scanned pages only: the text a correct OCR run should read, line by line.
    #[serde(default)]
    ocr_truth: Vec<String>,
    /// Personal data a correct detector must redact, as written in the document.
    #[serde(default)]
    pii: Vec<Truth>,
    /// Strings that look like personal data but are not. A detector must leave them.
    #[serde(default)]
    near_miss: Vec<String>,
}

#[derive(Deserialize)]
struct ExpectedPage {
    page: u32,
    kind: String,
}

#[derive(Deserialize)]
struct Truth {
    kind: String,
    value: String,
}

fn corpus() -> Corpus {
    serde_json::from_str(MANIFEST).expect("corpus manifest parses")
}

fn fixture_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/documents")
        .join(file)
}

fn read_fixture(doc: &Document) -> Vec<u8> {
    std::fs::read(fixture_path(&doc.file)).unwrap_or_else(|e| panic!("{}: {e}", doc.file))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn page_kind_label(kind: pdf::PageKind) -> &'static str {
    match kind {
        pdf::PageKind::Text => "text",
        pdf::PageKind::Scanned => "scanned",
        pdf::PageKind::Empty => "empty",
    }
}

/// The text the in-process reader gives for the document.
fn document_text(doc: &Document, body: &[u8]) -> String {
    match doc.format.as_str() {
        "pdf" => pdf::extract_text(body)
            .map_err(|e| e.message)
            .expect("pdf text reads"),
        "docx" => docx::extract_text(body)
            .map_err(|e| e.message)
            .expect("docx text reads"),
        other => panic!("{}: unknown format {other}", doc.id),
    }
}

fn validator_accepts(kind: &str, value: &str) -> bool {
    match kind {
        "email" => pii::is_email(value),
        "phone" => pii::is_phone(value),
        "iban" => pii::is_iban(value),
        "card" => pii::is_payment_card(value),
        "fodselsnummer" => pii::is_fodselsnummer(value),
        other => panic!("unknown PII kind {other}"),
    }
}

/// Whether `value` appears in the document's text or, for a scan, its OCR truth.
fn appears_in(doc: &Document, text: &str, value: &str) -> bool {
    text.contains(value) || doc.ocr_truth.iter().any(|line| line.contains(value))
}

#[test]
fn corpus_manifest_is_the_norwegian_set_of_ten() {
    let corpus = corpus();
    assert_eq!(corpus.schema, SCHEMA);

    let norwegian = corpus
        .documents
        .iter()
        .filter(|d| d.language == "nb")
        .count();
    assert_eq!(
        norwegian, NORWEGIAN_SET,
        "the Norwegian set is ten documents"
    );
    assert_eq!(
        corpus.documents.len(),
        NORWEGIAN_SET,
        "no other documents yet"
    );

    let mut ids: Vec<&str> = corpus.documents.iter().map(|d| d.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), corpus.documents.len(), "document ids are unique");

    for doc in &corpus.documents {
        assert!(
            matches!(doc.format.as_str(), "pdf" | "docx"),
            "{}: format {}",
            doc.id,
            doc.format
        );
        assert!(!doc.summary.is_empty(), "{} has a summary", doc.id);
    }

    // The set must keep the shapes the parser work needs: a text PDF, a scanned
    // PDF, a mixed PDF, and a DOCX.
    let pdfs = || corpus.documents.iter().filter(|d| d.format == "pdf");
    assert!(
        pdfs().any(|d| d.pages.iter().all(|p| p.kind == "text")),
        "a text-only PDF"
    );
    assert!(
        pdfs().any(|d| d.pages.iter().all(|p| p.kind == "scanned")),
        "a scanned-only PDF"
    );
    assert!(
        pdfs().any(|d| d.pages.iter().any(|p| p.kind == "text")
            && d.pages.iter().any(|p| p.kind == "scanned")),
        "a mixed PDF"
    );
    assert!(
        corpus.documents.iter().any(|d| d.format == "docx"),
        "a DOCX"
    );
}

#[test]
fn corpus_files_match_their_recorded_size_and_sha256() {
    let corpus = corpus();
    for doc in &corpus.documents {
        let body = read_fixture(doc);
        assert_eq!(body.len() as u64, doc.bytes, "{}: size", doc.id);
        assert_eq!(
            hex(&Sha256::digest(&body)),
            doc.sha256,
            "{}: sha256",
            doc.id
        );
        match doc.format.as_str() {
            "pdf" => assert!(body.starts_with(b"%PDF-"), "{}: pdf header", doc.id),
            _ => assert!(body.starts_with(b"PK\x03\x04"), "{}: zip header", doc.id),
        }
    }
}

#[test]
fn corpus_pdf_pages_classify_as_recorded() {
    let corpus = corpus();
    for doc in corpus.documents.iter().filter(|d| d.format == "pdf") {
        let body = read_fixture(doc);
        let pages = pdf::classify_pages(&body)
            .map_err(|e| e.message)
            .expect("pdf classifies");
        let got: Vec<(u32, &str)> = pages
            .iter()
            .map(|p| (p.page, page_kind_label(p.kind)))
            .collect();
        let want: Vec<(u32, &str)> = doc
            .pages
            .iter()
            .map(|p| (p.page, p.kind.as_str()))
            .collect();
        assert_eq!(got, want, "{}: page kinds", doc.id);
    }
}

#[test]
fn corpus_text_layers_hold_every_recorded_personal_value() {
    let corpus = corpus();
    for doc in &corpus.documents {
        let body = read_fixture(doc);
        let text = document_text(doc, &body);
        for truth in &doc.pii {
            assert!(
                appears_in(doc, &text, &truth.value),
                "{}: {} `{}` is not in the document",
                doc.id,
                truth.kind,
                truth.value
            );
        }
        for near in &doc.near_miss {
            assert!(
                appears_in(doc, &text, near),
                "{}: near miss `{}` is not in the document",
                doc.id,
                near
            );
        }
    }
}

#[test]
fn corpus_pii_truth_passes_the_validators() {
    let corpus = corpus();
    for doc in &corpus.documents {
        for truth in &doc.pii {
            assert!(
                PII_KINDS.contains(&truth.kind.as_str()),
                "{}: kind {}",
                doc.id,
                truth.kind
            );
            assert!(
                validator_accepts(&truth.kind, &truth.value),
                "{}: {} `{}` fails its validator",
                doc.id,
                truth.kind,
                truth.value
            );
        }
    }
}

#[test]
fn corpus_covers_every_pii_kind() {
    let corpus = corpus();
    for kind in PII_KINDS {
        assert!(
            corpus
                .documents
                .iter()
                .any(|d| d.pii.iter().any(|t| t.kind == kind)),
            "the corpus holds at least one {kind}"
        );
    }
}

#[test]
fn corpus_scanned_pages_carry_ocr_truth_and_text_pages_do_not() {
    let corpus = corpus();
    for doc in corpus.documents.iter().filter(|d| d.format == "pdf") {
        let has_scan = doc.pages.iter().any(|p| p.kind == "scanned");
        if has_scan {
            assert!(
                !doc.ocr_truth.is_empty(),
                "{}: scanned page without OCR truth",
                doc.id
            );
        } else {
            assert!(
                doc.ocr_truth.is_empty(),
                "{}: OCR truth on a text-only PDF",
                doc.id
            );
        }
    }
}
