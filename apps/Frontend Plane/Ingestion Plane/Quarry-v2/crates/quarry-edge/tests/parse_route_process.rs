//! INNH-11: `POST /v1/parse` against the real edge binary, with the real isolation.
//!
//! The edge is started as a process (`quarry-edge-rs`, dev auth bypass, route on) and left to
//! choose its own parser isolation: on Unix that is the child process under `RLIMIT_AS`,
//! `RLIMIT_CPU` and `RLIMIT_NOFILE`. A decompression bomb goes through the whole HTTP path
//! and must come back as a typed 415 while the edge keeps serving; the same edge then parses a
//! normal document. This is the evidence the flag needs before it is turned on anywhere.

#![cfg(unix)]

use std::io::Write as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_quarry-edge-rs");

const PDF: &[u8] =
    include_bytes!("../../quarry-transform/tests/fixtures/documents/no/no-01-vedtak-bostotte.pdf");
const SCANNED: &[u8] = include_bytes!(
    "../../quarry-transform/tests/fixtures/documents/no/no-04-skannet-innkalling.pdf"
);

struct Edge {
    child: Child,
    base: String,
}

impl Drop for Edge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn start_edge() -> Edge {
    let port = free_port();
    let mut command = Command::new(BIN);
    for (name, _) in std::env::vars() {
        if name.starts_with("QUARRY") || name == "ENVIRONMENT" || name == "APP_ENV" {
            command.env_remove(name);
        }
    }
    let child = command
        .env("ENVIRONMENT", "dev")
        .env("QUARRY_EDGE_AUTH_DEV_BYPASS", "1")
        .env("QUARRY_EDGE__PORT", port.to_string())
        .env("QUARRY_EDGE__PARSE_ROUTE", "on")
        // Not set: QUARRY_EDGE__DOC_PARSE_ISOLATION and QUARRY_EDGE__PARSE_ALLOW_INPROCESS.
        // The edge must pick the child process by itself, or the route would refuse.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the edge binary starts");
    let edge = Edge {
        child,
        base: format!("http://127.0.0.1:{port}"),
    };
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Ok(response) = reqwest::get(format!("{}/health", edge.base)).await {
            if response.status().is_success() {
                return edge;
            }
        }
        assert!(Instant::now() < deadline, "the edge did not come up");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn parse(edge: &Edge, content_type: &str, body: Vec<u8>) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("{}/v1/parse", edge.base))
        .bearer_auth("dev")
        .header("content-type", content_type)
        .timeout(Duration::from_secs(60))
        .body(body)
        .send()
        .await
        .expect("the edge answers");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

/// A one-page PDF whose content stream is `stream` under `filter`.
fn pdf_with_stream(stream: &[u8], filter: &str) -> Vec<u8> {
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
    let mut content = format!("<< /Length {} {filter} >>\nstream\n", stream.len()).into_bytes();
    content.extend_from_slice(stream);
    content.extend_from_slice(b"\nendstream");
    object(&mut out, &content);
    object(
        &mut out,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    );
    let xref_at = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
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

/// 1 GiB of zeros, deflated to about a megabyte.
fn deflated_zeros() -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![0u8; 1024 * 1024];
    for _ in 0..1024 {
        encoder.write_all(&chunk).unwrap();
    }
    encoder.finish().unwrap()
}

#[tokio::test]
async fn a_decompression_bomb_through_the_whole_route_is_a_415_and_the_edge_keeps_serving() {
    let edge = start_edge().await;

    // The edge chose the isolated child by itself: the route would be a 501 otherwise.
    let (status, body) = parse(&edge, "application/pdf", PDF.to_vec()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["kind"], "pdf");
    assert_eq!(body["data"]["pages"], 1);
    assert!(!body["data"]["markdown"].as_str().unwrap().trim().is_empty());

    let compressed = deflated_zeros();
    assert!(compressed.len() < 4 * 1024 * 1024, "the bomb is small");
    let bomb = pdf_with_stream(&compressed, "/Filter /FlateDecode");
    let started = Instant::now();
    let (status, body) = parse(&edge, "application/pdf", bomb).await;
    assert_eq!(status, 415, "{body}");
    assert_eq!(body["error"]["code"], "UNSUPPORTED_MEDIA");
    assert_eq!(body["error"]["details"]["reason"], "too_large");
    assert_eq!(
        body["error"]["details"]["supported"],
        serde_json::json!(["pdf", "docx"])
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "stopped by a limit, not by waiting"
    );

    // The edge is alive and parses the next document, a scan with its warning included.
    let (status, body) = parse(&edge, "application/octet-stream", SCANNED.to_vec()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["warnings"][0]["code"], "document_has_no_text_layer");
    let health = reqwest::get(format!("{}/health", edge.base)).await.unwrap();
    assert!(health.status().is_success());
}
