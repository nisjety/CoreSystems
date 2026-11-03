//! INNH-11: `POST /v1/parse`, in the real router with real token verification.
//!
//! Every test here is `#[serial]`: the route's limiter is process-wide (one parse per org),
//! and the tests that hold a slot would otherwise turn their neighbours away.
//!
//! The route is switched on, and in-process parsing allowed, before the first request (the
//! test binary is not the edge, so the isolated child is not available to it; the child is
//! tested in `parse_route_process.rs` and `quarry-runtime/tests/doc_parse_sandbox.rs`).
//! `parse_route_off.rs` is the flag-off default.

mod support;

#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use std::io::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use quarry_runtime::artifact_store::{ArtifactStore, InMemoryStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use quarry_runtime::EventSink;
use serde_json::Value;
use serial_test::serial;
use tokio::sync::mpsc;
use zdr_harness::{
    golden_token, token_with_scopes, user_in_standard_org, user_in_zdr_org, Harness,
};

const PDF_TEXT: &[u8] =
    include_bytes!("../../quarry-transform/tests/fixtures/documents/no/no-01-vedtak-bostotte.pdf");
const PDF_TWO_PAGES: &[u8] = include_bytes!(
    "../../quarry-transform/tests/fixtures/documents/no/no-02-soknad-barnehageplass.pdf"
);
const PDF_SCANNED: &[u8] = include_bytes!(
    "../../quarry-transform/tests/fixtures/documents/no/no-04-skannet-innkalling.pdf"
);
const PDF_MIXED: &[u8] = include_bytes!(
    "../../quarry-transform/tests/fixtures/documents/no/no-05-blandet-notat-og-vedlegg.pdf"
);
const DOCX: &[u8] =
    include_bytes!("../../quarry-transform/tests/fixtures/documents/no/no-08-vedtak-tilskudd.docx");

const DOCX_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const STANDARD_ORG: &str = "org-standard-golden";

fn enable() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("QUARRY_EDGE__PARSE_ROUTE", "on");
        std::env::set_var("QUARRY_EDGE__PARSE_ALLOW_INPROCESS", "1");
    });
}

struct Reply {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Value,
}

async fn post_raw(
    harness: &Harness,
    bearer: Option<&str>,
    content_type: Option<&str>,
    body: impl Into<reqwest::Body>,
) -> Reply {
    let mut request = reqwest::Client::new().post(harness.url("/v1/parse"));
    if let Some(bearer) = bearer {
        request = request.bearer_auth(bearer);
    }
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    let response = request.body(body).send().await.expect("the edge answers");
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response.text().await.unwrap_or_default();
    Reply {
        status,
        headers,
        body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
    }
}

async fn parse(harness: &Harness, content_type: Option<&str>, body: &[u8]) -> Reply {
    post_raw(harness, Some(&user_in_standard_org()), content_type, body.to_vec()).await
}

fn details(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]
}

fn warning_codes(reply: &Reply) -> Vec<String> {
    reply.body["data"]["warnings"]
        .as_array()
        .expect("warnings is always an array")
        .iter()
        .map(|w| w["code"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
#[serial]
async fn a_pdf_comes_back_as_its_text_with_its_page_count() {
    enable();
    let harness = Harness::start().await;
    let reply = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let data = &reply.body["data"];
    assert_eq!(data["kind"], "pdf");
    assert_eq!(data["structure"], "plain");
    assert_eq!(data["pages"], 1);
    assert_eq!(data["bytes"], PDF_TEXT.len());
    assert_eq!(data["truncated"], false);
    assert_eq!(warning_codes(&reply), Vec::<String>::new());
    // The text is what the extractor reads from the document, no more and no less.
    let expected = quarry_transform::pdf::extract_text(PDF_TEXT).unwrap();
    assert_eq!(data["markdown"], expected.as_str());
    assert!(!expected.trim().is_empty());
    assert!(reply.body["meta"]["request_id"].is_string());
    assert_eq!(
        reply.headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-store")
    );

    let two = parse(&harness, Some("application/pdf"), PDF_TWO_PAGES).await;
    assert_eq!(two.status, 200, "{}", two.body);
    assert_eq!(two.body["data"]["pages"], 2);
}

#[tokio::test]
#[serial]
async fn a_docx_comes_back_as_its_text_and_has_no_page_count() {
    enable();
    let harness = Harness::start().await;
    let reply = parse(&harness, Some(DOCX_TYPE), DOCX).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let data = &reply.body["data"];
    assert_eq!(data["kind"], "docx");
    assert_eq!(data["structure"], "plain");
    assert!(data.get("pages").is_none(), "no null placeholder: {data}");
    let expected = quarry_transform::docx::extract_text(DOCX).unwrap();
    assert_eq!(data["markdown"], expected.as_str());
    assert!(warning_codes(&reply).is_empty());
}

#[tokio::test]
#[serial]
async fn the_bytes_decide_the_kind_when_the_type_is_octet_stream_or_missing() {
    enable();
    let harness = Harness::start().await;
    for content_type in [Some("application/octet-stream"), None] {
        let pdf = parse(&harness, content_type, PDF_TEXT).await;
        assert_eq!(pdf.status, 200, "{content_type:?}: {}", pdf.body);
        assert_eq!(pdf.body["data"]["kind"], "pdf");
        let docx = parse(&harness, content_type, DOCX).await;
        assert_eq!(docx.status, 200, "{content_type:?}: {}", docx.body);
        assert_eq!(docx.body["data"]["kind"], "docx");
    }
    // Parameters and case on the declared type do not matter.
    let reply = parse(&harness, Some("Application/PDF; charset=binary"), PDF_TEXT).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[tokio::test]
#[serial]
async fn a_declared_type_the_bytes_contradict_is_refused() {
    enable();
    let harness = Harness::start().await;
    // A DOCX sent as a PDF, and a PDF sent as a DOCX.
    for (content_type, body, detected, declared) in [
        ("application/pdf", DOCX, "docx", "pdf"),
        (DOCX_TYPE, PDF_TEXT, "pdf", "docx"),
    ] {
        let reply = parse(&harness, Some(content_type), body).await;
        assert_eq!(reply.status, 415, "{}", reply.body);
        assert_eq!(reply.body["error"]["code"], "UNSUPPORTED_MEDIA");
        assert_eq!(details(&reply)["reason"], "content_type_mismatch");
        assert_eq!(details(&reply)["detected"], detected);
        assert_eq!(details(&reply)["declared"], declared);
        assert_eq!(details(&reply)["supported"], serde_json::json!(["pdf", "docx"]));
    }
    // Bytes that are no document at all, under a document type.
    let reply = parse(&harness, Some("application/pdf"), b"<html><body>hi</body></html>").await;
    assert_eq!(reply.status, 415, "{}", reply.body);
    assert_eq!(details(&reply)["reason"], "unsupported_media");
    assert_eq!(details(&reply)["detected"], "html");
}

#[tokio::test]
#[serial]
async fn bytes_that_are_not_a_pdf_or_docx_are_a_415_that_lists_what_is_supported() {
    enable();
    let harness = Harness::start().await;
    let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
    let mut zip = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        writer.start_file("hello.txt", options).unwrap();
        writer.write_all(b"hello").unwrap();
        writer.finish().unwrap();
    }
    let cases: [(&str, &[u8], &str); 5] = [
        ("html", b"<!doctype html><html><body>hi</body></html>", "html"),
        ("png", &png, "image"),
        ("zip", &zip, "archive"),
        ("plain text", b"just some words, no document", "html"),
        ("random bytes", &[0, 1, 2, 3, 4, 5, 6, 7, 0, 0, 0], "binary"),
    ];
    for (name, body, detected) in cases {
        let reply = parse(&harness, Some("application/octet-stream"), body).await;
        assert_eq!(reply.status, 415, "{name}: {}", reply.body);
        assert_eq!(reply.body["error"]["code"], "UNSUPPORTED_MEDIA", "{name}");
        assert_eq!(details(&reply)["reason"], "unsupported_media", "{name}");
        assert_eq!(details(&reply)["detected"], detected, "{name}");
        assert_eq!(details(&reply)["supported"], serde_json::json!(["pdf", "docx"]));
    }
}

#[tokio::test]
#[serial]
async fn a_declared_type_outside_the_list_is_refused_without_reading_the_body() {
    enable();
    let harness = Harness::start().await;
    for content_type in ["text/html", "image/png", "application/json", "multipart/form-data; boundary=x"] {
        let reply = parse(&harness, Some(content_type), PDF_TEXT).await;
        assert_eq!(reply.status, 415, "{content_type}: {}", reply.body);
        assert_eq!(details(&reply)["reason"], "unsupported_media");
        assert!(details(&reply)["supported_content_types"].is_array());
    }
}

#[tokio::test]
#[serial]
async fn an_empty_body_is_a_400() {
    enable();
    let harness = Harness::start().await;
    let reply = parse(&harness, Some("application/pdf"), b"").await;
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert_eq!(reply.body["error"]["code"], "BAD_REQUEST");
    assert_eq!(details(&reply)["reason"], "empty_body");
}

/// A request that declares `length` bytes of body and sends none, over a raw socket: the
/// route must answer from the header alone, before it reads a byte.
async fn declare_only(harness: &Harness, length: usize) -> (u16, Value) {
    let address = harness.url("/v1/parse");
    let host = address.trim_start_matches("http://").split('/').next().unwrap().to_owned();
    let mut socket = tokio::net::TcpStream::connect(&host).await.unwrap();
    let request = format!(
        "POST /v1/parse HTTP/1.1\r\nhost: {host}\r\nauthorization: Bearer {}\r\ncontent-type: application/pdf\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n",
        user_in_standard_org()
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut raw)).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status: u16 = text.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    // A chunked or plain body: the JSON starts at the first brace.
    let json = body.find('{').map(|at| &body[at..]).and_then(|b| b.rfind('}').map(|e| &b[..=e]));
    (status, json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or(Value::Null))
}

#[tokio::test]
#[serial]
async fn a_body_over_the_limit_is_a_413_whether_or_not_it_declares_its_length() {
    enable();
    let harness = Harness::start().await;
    let limit = quarry_edge::parse_routes::DEFAULT_MAX_BYTES;
    assert_eq!(limit, 25 * 1024 * 1024);

    // The default limit, declared: refused from the header, before the body is read.
    let started = Instant::now();
    let (status, body) = declare_only(&harness, limit + 1).await;
    assert_eq!(status, 413, "{body}");
    assert_eq!(body["error"]["details"]["reason"], "too_large");
    assert_eq!(body["error"]["details"]["max_bytes"], limit);
    assert!(started.elapsed() < Duration::from_secs(5));

    // A lower limit from the environment, with the length declared and with it not.
    std::env::set_var("QUARRY_EDGE__PARSE_MAX_BYTES", "1048576");
    let mut big = PDF_TEXT.to_vec();
    big.resize(1_572_864, b' ');
    let declared = parse(&harness, Some("application/pdf"), &big).await;
    let chunks = (0..3).map(|i| {
        let mut chunk = vec![b' '; 512 * 1024];
        if i == 0 {
            chunk[..PDF_TEXT.len()].copy_from_slice(PDF_TEXT);
        }
        Ok::<_, std::io::Error>(bytes::Bytes::from(chunk))
    });
    let body = reqwest::Body::wrap_stream(futures_util::stream::iter(chunks));
    let chunked =
        post_raw(&harness, Some(&user_in_standard_org()), Some("application/pdf"), body).await;
    // A value above the ceiling is the ceiling.
    std::env::set_var("QUARRY_EDGE__PARSE_MAX_BYTES", "999999999999");
    assert_eq!(quarry_edge::parse_routes::max_bytes(), limit);
    std::env::remove_var("QUARRY_EDGE__PARSE_MAX_BYTES");
    for (name, reply) in [("declared", declared), ("chunked", chunked)] {
        assert_eq!(reply.status, 413, "{name}: {}", reply.body);
        assert_eq!(details(&reply)["reason"], "too_large", "{name}");
        assert_eq!(details(&reply)["max_bytes"], 1_048_576, "{name}");
    }
}

#[tokio::test]
#[serial]
async fn a_corrupt_document_is_a_415_with_the_parser_s_reason() {
    enable();
    let harness = Harness::start().await;
    let corrupt_pdf = b"%PDF-1.4\n1 0 obj << /Broken >> endobj\nthis is not a pdf".to_vec();
    let reply = parse(&harness, Some("application/pdf"), &corrupt_pdf).await;
    assert_eq!(reply.status, 415, "{}", reply.body);
    assert_eq!(details(&reply)["reason"], "pdf_parse_failed");
    assert_eq!(details(&reply)["kind"], "pdf");
    assert_eq!(details(&reply)["supported"], serde_json::json!(["pdf", "docx"]));

    // A zip that says it is a Word document and has no word/document.xml.
    let mut docx = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut docx));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        writer.start_file("[Content_Types].xml", options).unwrap();
        writer
            .write_all(b"<Types><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>")
            .unwrap();
        writer.finish().unwrap();
    }
    let reply = parse(&harness, Some(DOCX_TYPE), &docx).await;
    assert_eq!(reply.status, 415, "{}", reply.body);
    assert_eq!(details(&reply)["reason"], "docx_parse_failed");
    assert_eq!(details(&reply)["kind"], "docx");
}

#[tokio::test]
#[serial]
async fn a_scanned_pdf_says_it_has_no_text_layer_and_a_mixed_one_says_which_part() {
    enable();
    let harness = Harness::start().await;
    let scan = parse(&harness, Some("application/pdf"), PDF_SCANNED).await;
    assert_eq!(scan.status, 200, "{}", scan.body);
    assert_eq!(warning_codes(&scan), vec!["document_has_no_text_layer"]);
    assert_eq!(scan.body["data"]["pages"], 1);
    assert!(
        scan.body["data"]["markdown"].as_str().unwrap().trim().is_empty(),
        "there is no OCR: {}",
        scan.body["data"]["markdown"]
    );

    let mixed = parse(&harness, Some("application/pdf"), PDF_MIXED).await;
    assert_eq!(mixed.status, 200, "{}", mixed.body);
    assert_eq!(warning_codes(&mixed), vec!["some_pages_have_no_text_layer"]);
    assert_eq!(mixed.body["data"]["pages"], 2);
    assert!(!mixed.body["data"]["markdown"].as_str().unwrap().trim().is_empty());
}

#[tokio::test]
#[serial]
async fn who_may_call_it() {
    enable();
    let harness = Harness::start().await;

    // No token, or a token that is not one.
    let reply = post_raw(&harness, None, Some("application/pdf"), PDF_TEXT.to_vec()).await;
    assert_eq!(reply.status, 401);
    let reply = post_raw(&harness, Some("not-a-jwt"), Some("application/pdf"), PDF_TEXT.to_vec()).await;
    assert_eq!(reply.status, 401);

    // A user needs no scope.
    let user = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    assert_eq!(user.status, 200, "{}", user.body);

    // A service principal needs parse:write; no other scope stands in for it.
    for case in ["service_model_gateway", "service_execution_core"] {
        let reply =
            post_raw(&harness, Some(&golden_token(case)), Some("application/pdf"), PDF_TEXT.to_vec()).await;
        assert_eq!(reply.status, 403, "{case}");
    }
    let other = token_with_scopes("service_execution_core", &["extract:ai", "scrape:interact"]);
    let reply = post_raw(&harness, Some(&other), Some("application/pdf"), PDF_TEXT.to_vec()).await;
    assert_eq!(reply.status, 403);
    let allowed = token_with_scopes("service_execution_core", &["parse:write"]);
    let reply = post_raw(&harness, Some(&allowed), Some("application/pdf"), PDF_TEXT.to_vec()).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.body["data"]["kind"], "pdf");
}

#[tokio::test]
#[serial]
async fn an_org_with_its_parse_busy_gets_a_429_with_retry_after_and_another_org_is_served() {
    enable();
    let harness = Harness::start().await;
    let limiter = quarry_edge::parse_routes::limiter();

    let held = limiter.try_acquire(STANDARD_ORG).expect("the org's one slot");
    let started = Instant::now();
    let reply = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    assert_eq!(reply.status, 429, "{}", reply.body);
    assert!(started.elapsed() < Duration::from_secs(5), "turned away, not queued");
    assert_eq!(reply.body["error"]["code"], "RATE_LIMITED");
    assert_eq!(details(&reply)["reason"], "org_parse_busy");
    assert_eq!(details(&reply)["retry_after_seconds"], 2);
    assert_eq!(
        reply.headers.get("retry-after").and_then(|v| v.to_str().ok()),
        Some("2")
    );

    // Fairness: the org that holds the slot does not hold anyone else's.
    let other = post_raw(
        &harness,
        Some(&user_in_zdr_org()),
        Some("application/pdf"),
        PDF_TEXT.to_vec(),
    )
    .await;
    assert_eq!(other.status, 200, "{}", other.body);

    drop(held);
    assert_eq!(limiter.in_flight(STANDARD_ORG), 0);
    let again = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    assert_eq!(again.status, 200, "{}", again.body);
}

#[tokio::test]
#[serial]
async fn a_request_that_fails_gives_its_slot_back() {
    enable();
    let harness = Harness::start().await;
    let limiter = quarry_edge::parse_routes::limiter();
    for body in [&b""[..], &b"<html>"[..], &b"%PDF-1.4 broken"[..]] {
        let reply = parse(&harness, Some("application/octet-stream"), body).await;
        assert!(reply.status >= 400, "{}", reply.body);
        assert_eq!(limiter.in_flight(STANDARD_ORG), 0, "a refusal left a slot taken");
    }
}

#[tokio::test]
#[serial]
async fn a_full_process_turns_every_org_away_with_a_429() {
    enable();
    let harness = Harness::start().await;
    let limiter = quarry_edge::parse_routes::limiter();
    // The default is four buffered uploads in the process.
    let held: Vec<_> = (0..4)
        .map(|i| limiter.try_acquire(&format!("org_fill_{i}")).expect("a free upload slot"))
        .collect();
    let reply = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    assert_eq!(reply.status, 429, "{}", reply.body);
    assert_eq!(details(&reply)["reason"], "parse_capacity");
    assert_eq!(
        reply.headers.get("retry-after").and_then(|v| v.to_str().ok()),
        Some("2")
    );
    drop(held);
    let reply = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[tokio::test]
#[serial]
async fn it_refuses_to_serve_while_documents_would_be_parsed_in_the_edge_itself() {
    enable();
    let harness = Harness::start().await;
    std::env::remove_var("QUARRY_EDGE__PARSE_ALLOW_INPROCESS");
    let reply = parse(&harness, Some("application/pdf"), PDF_TEXT).await;
    std::env::set_var("QUARRY_EDGE__PARSE_ALLOW_INPROCESS", "1");
    assert_eq!(reply.status, 501, "{}", reply.body);
    assert_eq!(reply.body["error"]["code"], "UNSUPPORTED");
    assert_eq!(details(&reply)["reason"], "isolation_required");
    assert_eq!(details(&reply)["env"], "QUARRY_EDGE__DOC_PARSE_ISOLATION");
}

/// A recording stand-in for everything a handler could write to: the artifact store, the
/// event sink and the usage meter are the real doubles of the harness, with their counts read
/// back after the requests.
struct Probes {
    store: Arc<InMemoryStore>,
    events: Arc<AtomicUsize>,
}

async fn start_with_probes() -> (Harness, Probes) {
    enable();
    let store = Arc::new(InMemoryStore::new());
    let events = Arc::new(AtomicUsize::new(0));
    let (tx, mut rx) = mpsc::channel::<quarry_core::event::Event>(64);
    let counter = events.clone();
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    let wired = store.clone();
    let harness = Harness::start_with(move |mut state| {
        state.artifacts = wired;
        state.event_sink = EventSink::new(tx);
        state
    })
    .await;
    (harness, Probes { store, events })
}

#[tokio::test]
#[serial]
async fn nothing_is_stored_for_a_standard_or_a_zdr_caller() {
    let (harness, probes) = start_with_probes().await;
    for bearer in [user_in_standard_org(), user_in_zdr_org()] {
        for (content_type, body) in [
            (Some("application/pdf"), PDF_TEXT),
            (Some(DOCX_TYPE), DOCX),
            (Some("application/pdf"), PDF_SCANNED),
            // refusals too
            (Some("application/pdf"), &b"%PDF-1.4 broken"[..]),
            (None, &b"<html></html>"[..]),
        ] {
            let _ = post_raw(&harness, Some(&bearer), content_type, body.to_vec()).await;
        }
    }
    // Let any task a handler might have spawned run.
    tokio::time::sleep(Duration::from_millis(300)).await;

    for org in [STANDARD_ORG, "org-zdr-golden"] {
        assert_eq!(probes.store.count(org).await.unwrap(), Some(0), "{org}: an artifact was put");
    }
    assert_eq!(probes.events.load(Ordering::SeqCst), 0, "an event was emitted");
    assert!(
        harness.usage.events.lock().unwrap().is_empty(),
        "a usage record was written"
    );
}

/// The handler is given no `AppState`: it has no store, cache, index, sink or meter to write
/// to. This reads its source (comments aside, tests excluded) and fails if one appears.
#[test]
fn the_handler_cannot_reach_a_store() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/parse_routes.rs"))
        .expect("the route module")
        .replace("\r\n", "\n");
    let production = source.split("#[cfg(test)]").next().unwrap();
    let code: String = production
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "AppState",
        "State(",
        "artifacts",
        "event_sink",
        "EventSink",
        "UsageMeter",
        "usage",
        "ingest",
        "local_index",
        "vector_index",
        "PageCache",
        "state.cache",
        "tracing::",
        "filename",
        "x-quarry-filename",
    ] {
        assert!(
            !code.contains(forbidden),
            "parse_routes.rs production code mentions `{forbidden}`"
        );
    }
}
