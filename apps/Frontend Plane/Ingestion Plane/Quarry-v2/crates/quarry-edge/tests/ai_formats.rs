//! INNH-1 U3: the AI formats (`json`, `summary`, `question`, `highlights`) of
//! `POST /v1/scrape`, through the real router, with the switch ON.
//!
//! The switch is read once per process, so this binary turns it on before the
//! first request; `tests/ai_formats_off.rs` runs with the committed default (off)
//! and `tests/ai_formats_real_token.rs` runs on real tokens. The model is a strict
//! double of the gateway's `POST /v1/ai/chat` (`support/ai_model_double.rs`), the
//! page is a fixed fake driver, and both count what reaches them: a refusal that
//! must come before the fetch shows as zero fetches, a ZDR request as zero model
//! calls.
//!
//! This binary runs on the auth dev bypass (every caller is the stub user). What a
//! real service principal or a real ZDR token may do is in
//! `tests/ai_formats_real_token.rs`.
//!
//! No Redis here: the page-cache test at the bottom skips without one, like
//! `tests/cache_policy.rs`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::artifact_store::{ArtifactStore, InMemoryStore};
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;
use url::Url;

mod support;

#[path = "support/ai_model_double.rs"]
mod ai_model_double;

use ai_model_double::{chat, standard, Double, Reply, Seen};

const TEST_BEARER: &str = "ai-formats-dev-token";
const MODEL_TOKEN: &str = "model-dev-token";
const INFERENCE_TOKEN: &str = "inference-dev-token";
const ORG: &str = "org_placeholder";
const INPUT_CAP_ENV: &str = "QUARRY_EDGE__AI_FORMATS_MAX_INPUT_CHARS";
/// The model input cap this binary runs with (the lowest the setting allows).
const INPUT_CAP: usize = 1_000;

const PAGE: &str = "<html><head><title>Kaffebrenneriet</title></head><body><main>\
<h1>Kaffebrenneriet AS</h1>\
<p>Kaffebrenneriet AS selger kaffe fra Bergen. Prisen for en pose er 199 kr. Levering tar to dager.</p>\
<p>Æøå er norske bokstaver, og de står i navnet på butikken i sentrum av byen.</p>\
</main></body></html>";

// ---- process setup --------------------------------------------------------

#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;
    fn make_writer(&'a self) -> LogCapture {
        self.clone()
    }
}

fn log() -> &'static LogCapture {
    static LOG: OnceLock<LogCapture> = OnceLock::new();
    LOG.get_or_init(LogCapture::default)
}

fn logged() -> String {
    String::from_utf8_lossy(&log().0.lock().unwrap()).into_owned()
}

/// Everything the switches read once, set before the first request, and a global
/// subscriber that keeps what Quarry's own code logs (any level).
fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("QUARRY_EDGE__AI_FORMATS", "on");
        // The lowest input cap, so a page over it is small enough to build here.
        std::env::set_var(INPUT_CAP_ENV, INPUT_CAP.to_string());
        std::env::set_var("QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN", INFERENCE_TOKEN);
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(log().clone())
            .with_ansi(false)
            .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                meta.target().starts_with("quarry")
            }));
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer));
    });
}

// ---- the edge -------------------------------------------------------------

/// A static driver that serves one page and counts its fetches.
struct PageDriver {
    page: &'static str,
    fetches: Arc<AtomicUsize>,
}

#[async_trait]
impl Driver for PageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: self.page.as_bytes().to_vec(),
            duration_ms: 2,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

struct Edge {
    addr: SocketAddr,
    store: Arc<InMemoryStore>,
    fetches: Arc<AtomicUsize>,
}

impl Edge {
    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

async fn spawn_edge_with(
    page: &'static str,
    model_url: Option<String>,
    model_token: Option<&str>,
) -> Edge {
    init();
    let (tx, mut rx) = mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let fetches = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(InMemoryStore::new());
    let state = AppState {
        artifacts: store.clone(),
        model_plane_url: model_url,
        model_plane_token: model_token.map(str::to_owned),
        #[cfg(feature = "browser-agent")]
        agent_driver: support::governed_agent_driver(),
        ..support::base_state(
            Arc::new(PageDriver {
                page,
                fetches: fetches.clone(),
            }),
            EventSink::new(tx),
        )
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Edge {
        addr,
        store,
        fetches,
    }
}

/// An edge wired to the model double, with the dev credential.
async fn spawn_edge(double: &Double) -> Edge {
    spawn_edge_with(PAGE, Some(double.uri.clone()), Some(MODEL_TOKEN)).await
}

async fn post(edge: &Edge, path: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{}{path}", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn get(edge: &Edge, path: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .get(format!("http://{}{path}", edge.addr))
        .bearer_auth(TEST_BEARER)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn schema() -> Value {
    json!({
        "type": "object",
        "required": ["title"],
        "additionalProperties": false,
        "properties": { "title": { "type": "string" }, "price": { "type": "integer" } }
    })
}

fn all_four() -> Value {
    json!({
        "url": "https://www.example.com/",
        "formats": [
            "markdown",
            { "type": "json", "schema": schema(), "prompt": "Pull out the shop and the price." },
            { "type": "summary", "max_chars": 500 },
            { "type": "question", "question": "Hva koster en pose?" },
            { "type": "highlights", "query": "pris og levering", "max_highlights": 3 },
        ],
    })
}

// ---- the happy path -------------------------------------------------------

#[tokio::test]
async fn all_four_formats_answer_inline_with_a_report() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let (status, body) = post(&edge, "/v1/scrape", all_four()).await;
    assert_eq!(status, 200, "{body}");
    let data = &body["data"];

    assert_eq!(
        data["json"],
        json!({ "title": "Kaffebrenneriet AS", "price": 199 })
    );
    assert_eq!(data["summary"], "Et kort sammendrag av siden.");
    assert_eq!(data["answer"], "199 kr");

    // Highlights are exact spans of the page: the text is the slice at the char offsets.
    let markdown = data["markdown"].as_str().expect("markdown was asked for");
    let highlights = data["highlights"].as_array().expect("highlights");
    assert_eq!(highlights.len(), 3, "{highlights:?}");
    let mut previous_end = 0;
    for highlight in highlights {
        let (start, end) = (
            highlight["start"].as_u64().unwrap() as usize,
            highlight["end"].as_u64().unwrap() as usize,
        );
        let sliced: String = markdown.chars().skip(start).take(end - start).collect();
        assert_eq!(sliced, highlight["text"].as_str().unwrap());
        assert!(start >= previous_end, "ordered and not overlapping");
        previous_end = end;
    }
    assert!(highlights
        .iter()
        .any(|h| h["text"] == "Æøå er norske bokstaver"));

    let ai = &data["ai"];
    for kind in ["json", "summary", "question", "highlights"] {
        assert_eq!(ai[kind]["status"], "ok", "{kind}: {ai}");
        assert_eq!(ai[kind]["untrusted_source"], true, "{kind}");
    }
    assert_eq!(ai["json"]["schema_valid"], true);
    assert_eq!(ai["json"]["repair"], "none");
    assert_eq!(ai["question"]["found"], true);
    assert_eq!(ai["question"]["grounded"], true);
    assert_eq!(ai["total"]["calls"], 4);
    assert_eq!(ai["total"]["input_tokens"], 1200 + 1000 + 1000 + 1000);

    // What the model saw: the dev credential in both headers, no thread, an alias.
    model.assert_clean();
    let seen = model.seen();
    assert_eq!(seen.len(), 4);
    for call in &seen {
        assert_eq!(
            call.headers["authorization"],
            format!("Bearer {MODEL_TOKEN}")
        );
        assert_eq!(
            call.headers["x-inference-authorization"],
            format!("Bearer {INFERENCE_TOKEN}")
        );
        assert!(call.body.get("thread_id").is_none() && call.body.get("session_key").is_none());
        assert!(
            call.body.get("zdr").is_none(),
            "zdr is sent only when it is true"
        );
        assert!(
            call.user().contains("Prisen for en pose er 199 kr."),
            "the page is in the user message"
        );
    }
    let extract = seen.iter().find(|call| call.task == "extract").unwrap();
    assert_eq!(extract.body["model"], "verevon-balance");
    assert!(
        extract.body["structured_output_schema"].is_string(),
        "the gateway's field is a string"
    );
    assert!(extract
        .system()
        .contains("Pull out the shop and the price."));
    for call in seen.iter().filter(|call| call.task != "extract") {
        assert_eq!(call.body["model"], "verevon-budget");
    }
    assert_eq!(edge.fetches(), 1);

    // The effective options name the formats and nothing the caller wrote.
    let options = data["effective_options"].to_string();
    for needle in [
        "Hva koster",
        "pris og levering",
        "Pull out the shop",
        "additionalProperties",
    ] {
        assert!(
            !options.contains(needle),
            "effective_options echoed {needle:?}: {options}"
        );
    }
}

#[tokio::test]
async fn the_scrape_record_and_the_store_hold_no_ai_output() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let (status, body) = post(&edge, "/v1/scrape", all_four()).await;
    assert_eq!(status, 200, "{body}");
    let scrape_id = body["data"]["scrape_id"]
        .as_str()
        .expect("a record id")
        .to_owned();

    let (status, record) = get(&edge, &format!("/v1/scrape/{scrape_id}")).await;
    assert_eq!(status, 200, "{record}");
    for key in ["json", "summary", "answer", "highlights", "ai"] {
        assert!(
            record["data"].get(key).is_none(),
            "the record carries `{key}`: {record}"
        );
    }
    let text = record.to_string();
    // Only text the model made up: "199 kr" is also on the page, which the record holds.
    for output in [
        "Et kort sammendrag",
        "\"schema_valid\"",
        "\"untrusted_source\"",
        "\"grounded\"",
    ] {
        assert!(
            !text.contains(output),
            "the record carries model output {output:?}"
        );
    }

    // No artifact holds it either: whatever was stored, none of it is AI output.
    let listed = edge
        .store
        .list(ORG, &quarry_core::pagination::ListFilter::default())
        .await
        .unwrap();
    for item in &listed.items {
        let bytes = edge
            .store
            .get(ORG, &item.artifact_id)
            .await
            .expect("stored bytes");
        let stored = String::from_utf8_lossy(&bytes);
        assert!(
            !stored.contains("Et kort sammendrag"),
            "an artifact holds the summary"
        );
        assert!(
            !stored.contains("\"ai\""),
            "an artifact holds the AI report"
        );
    }
}

#[tokio::test]
async fn a_default_scrape_is_unchanged_and_spends_no_model_call() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let (status, body) = post(
        &edge,
        "/v1/scrape",
        json!({ "url": "https://www.example.com/", "formats": ["markdown"] }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    for key in ["json", "summary", "answer", "highlights", "ai"] {
        assert!(
            body["data"].get(key).is_none(),
            "`{key}` appeared on a scrape that asked for none"
        );
    }
    assert!(body["data"]["markdown"].as_str().is_some());
    assert_eq!(model.calls(), 0);
}

// ---- model failures never fail the scrape ---------------------------------

#[tokio::test]
async fn a_model_outage_leaves_the_scrape_a_200_with_the_failure_in_the_report() {
    let model = ai_model_double::start(|_: &Seen| Reply::status(503)).await;
    let edge = spawn_edge(&model).await;
    let (status, body) = post(
        &edge,
        "/v1/scrape",
        json!({
            "url": "https://www.example.com/",
            "formats": ["markdown", { "type": "json", "schema": schema() }, "summary"],
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["data"]["markdown"]
        .as_str()
        .is_some_and(|text| text.contains("Kaffebrenneriet")));
    assert!(body["data"].get("json").is_none() && body["data"].get("summary").is_none());
    for kind in ["json", "summary"] {
        let entry = &body["data"]["ai"][kind];
        assert_eq!(entry["status"], "error", "{kind}: {entry}");
        assert_eq!(entry["code"], "MODEL_UNAVAILABLE", "{kind}: {entry}");
        assert_eq!(entry["reason"], "upstream_unavailable", "{kind}: {entry}");
    }
    assert!(model.calls() >= 2, "each format tried");
}

#[tokio::test]
async fn a_rate_limit_is_reported_with_its_wait_and_nothing_is_ok() {
    let model =
        ai_model_double::start(|_: &Seen| Reply::status(429).header("retry-after", "7")).await;
    let edge = spawn_edge(&model).await;
    let (status, body) = post(&edge, "/v1/scrape", all_four()).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body["data"]["markdown"].as_str().is_some(),
        "the page still comes back"
    );
    let ai = &body["data"]["ai"];
    for kind in ["json", "summary", "question", "highlights"] {
        assert_ne!(ai[kind]["status"], "ok", "{kind}: {ai}");
        assert_eq!(ai[kind]["code"], "MODEL_RATE_LIMITED", "{kind}: {ai}");
    }
    let waits: Vec<_> = ["json", "summary", "question", "highlights"]
        .iter()
        .filter(|kind| ai[**kind]["status"] == "error")
        .map(|kind| ai[*kind]["retry_after_s"].clone())
        .collect();
    assert!(
        !waits.is_empty() && waits.iter().all(|wait| *wait == 7),
        "{waits:?}"
    );
    assert!(body["data"].get("json").is_none() && body["data"].get("answer").is_none());
}

// ---- refusals come before the fetch ---------------------------------------

#[tokio::test]
async fn a_bad_ai_format_is_a_400_with_its_path_and_no_fetch() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    for (formats, path, code) in [
        (
            json!(["markdown", { "type": "json" }]),
            "formats[1].schema",
            "missing_field",
        ),
        (json!(["question"]), "formats[0].question", "missing_field"),
        (json!(["json"]), "formats[0].schema", "missing_field"),
    ] {
        let (status, body) = post(
            &edge,
            "/v1/scrape",
            json!({ "url": "https://www.example.com/", "formats": formats }),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        let fields = body["error"]["details"]["fields"]
            .as_array()
            .expect("fields");
        assert!(
            fields
                .iter()
                .any(|field| field["path"] == path && field["code"] == code),
            "{path}/{code} missing: {body}"
        );
    }
    assert_eq!((edge.fetches(), model.calls()), (0, 0));
}

#[tokio::test]
async fn without_a_model_plane_url_the_answer_is_501_before_the_fetch() {
    let edge = spawn_edge_with(PAGE, None, Some(MODEL_TOKEN)).await;
    let (status, body) = post(&edge, "/v1/scrape", all_four()).await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(
        body["error"]["details"]["reason"],
        "model_plane_unavailable"
    );
    assert_eq!(edge.fetches(), 0, "a refused request spends no fetch");

    // A scrape without AI formats on the same edge is untouched.
    let (status, body) = post(
        &edge,
        "/v1/scrape",
        json!({ "url": "https://www.example.com/", "formats": ["markdown"] }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn with_a_url_and_no_usable_credential_the_answer_is_501_lane_not_granted() {
    let model = ai_model_double::start(standard).await;
    for token in [None, Some("  ")] {
        let edge = spawn_edge_with(PAGE, Some(model.uri.clone()), token).await;
        let (status, body) = post(&edge, "/v1/scrape", all_four()).await;
        assert_eq!(status, 501, "{body}");
        assert_eq!(body["error"]["details"]["reason"], "model_lane_not_granted");
        assert_eq!(edge.fetches(), 0);
    }
    assert_eq!(model.calls(), 0, "nothing was sent without a credential");
}

#[tokio::test]
async fn an_agent_label_is_a_403_before_the_fetch_and_the_same_body_without_it_is_served() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let mut body = json!({
        "url": "https://www.example.com/",
        "formats": ["markdown", "summary"],
        "initiated_by": "agent",
    });
    let (status, refused) = post(&edge, "/v1/scrape", body.clone()).await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(
        refused["error"]["details"]["reason"],
        "extract_ai_scope_required"
    );
    assert_eq!(refused["error"]["details"]["field"], "formats[1]");
    assert_eq!((edge.fetches(), model.calls()), (0, 0));

    // A scrape with no AI format is not touched by the label.
    body["formats"] = json!(["markdown"]);
    let (status, plain) = post(&edge, "/v1/scrape", body.clone()).await;
    assert_eq!(status, 200, "{plain}");

    // Any other label is ignored, and the AI format is served.
    body["formats"] = json!(["markdown", "summary"]);
    body["initiated_by"] = json!("somebody");
    let (status, served) = post(&edge, "/v1/scrape", body).await;
    assert_eq!(status, 200, "{served}");
    assert_eq!(served["data"]["ai"]["summary"]["status"], "ok");
}

// ---- ZDR ------------------------------------------------------------------

#[tokio::test]
async fn zdr_skips_every_format_with_no_model_call_and_stores_nothing() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let mut request = all_four();
    request["zdr"] = json!(true);
    let (status, body) = post(&edge, "/v1/scrape", request).await;
    assert_eq!(status, 200, "{body}");
    let data = &body["data"];
    assert!(
        data["markdown"].as_str().is_some(),
        "the page itself is a normal ZDR scrape"
    );
    for key in ["json", "summary", "answer", "highlights"] {
        assert!(data.get(key).is_none(), "`{key}` under ZDR");
    }
    for kind in ["json", "summary", "question", "highlights"] {
        let entry = &data["ai"][kind];
        assert_eq!(entry["status"], "skipped", "{kind}: {entry}");
        assert_eq!(entry["code"], "ZDR_UNAVAILABLE", "{kind}: {entry}");
        assert!(
            entry.get("model").is_none() && entry.get("usage").is_none(),
            "{kind}"
        );
    }
    assert_eq!(data["ai"]["total"]["calls"], 0);
    assert!(data.get("scrape_id").is_none(), "no record under ZDR");
    assert_eq!(model.calls(), 0, "no page text left for a model");
    let listed = edge
        .store
        .list(ORG, &quarry_core::pagination::ListFilter::default())
        .await
        .unwrap();
    assert!(listed.items.is_empty(), "ZDR wrote an artifact");
}

#[tokio::test]
async fn zdr_is_answered_even_where_no_model_could_be_reached() {
    // ZDR never reaches a model, so whether one is configured is not asked.
    let edge = spawn_edge_with(PAGE, None, None).await;
    let mut request = all_four();
    request["zdr"] = json!(true);
    let (status, body) = post(&edge, "/v1/scrape", request).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["ai"]["json"]["code"], "ZDR_UNAVAILABLE");
    assert_eq!(edge.fetches(), 1);
}

// ---- the input cap --------------------------------------------------------

/// A page whose body is `n` copies of a letter no prompt of ours contains.
fn page_of(n: usize) -> &'static str {
    Box::leak(
        format!("<html><body><main><p>{}</p></main></body></html>", "ж".repeat(n)).into_boxed_str(),
    )
}

/// How many chars of that letter the model was sent in the page of its call.
fn page_chars_sent(seen: &[Seen]) -> Vec<usize> {
    seen.iter()
        .map(|call| call.user().chars().filter(|c| *c == 'ж').count())
        .collect()
}

fn three_formats() -> Value {
    json!({
        "url": "https://www.example.com/",
        "formats": [
            { "type": "json", "schema": schema() },
            { "type": "summary" },
            { "type": "question", "question": "Hva koster en pose?" },
        ],
    })
}

#[tokio::test]
async fn a_page_over_the_input_cap_says_so_and_one_at_the_cap_or_under_does_not() {
    // How long the markdown of `n` letters is, measured on a scrape that asks for no
    // AI format, so the page at the cap is exactly at it.
    let measure = spawn_edge_with(page_of(INPUT_CAP), None, None).await;
    let (status, body) = post(
        &measure,
        "/v1/scrape",
        json!({ "url": "https://www.example.com/", "formats": ["markdown"] }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let at_cap_len = body["data"]["markdown"].as_str().unwrap().chars().count();
    let overhead = at_cap_len as isize - INPUT_CAP as isize;
    let letters_for = |chars: usize| (chars as isize - overhead) as usize;

    for (chars, over) in [
        (INPUT_CAP + 1, true),
        (INPUT_CAP + 500, true),
        (INPUT_CAP, false),
        (INPUT_CAP - 1, false),
        (100, false),
    ] {
        let model = ai_model_double::start(standard).await;
        let edge = spawn_edge_with(
            page_of(letters_for(chars)),
            Some(model.uri.clone()),
            Some(MODEL_TOKEN),
        )
        .await;
        let (status, body) = post(&edge, "/v1/scrape", three_formats()).await;
        assert_eq!(status, 200, "{chars}: {body}");
        for kind in ["json", "summary", "question"] {
            let entry = &body["data"]["ai"][kind];
            assert_eq!(entry["status"], "ok", "{chars} {kind}: {entry}");
            if over {
                assert_eq!(entry["truncated_input"], true, "{chars} {kind}: {entry}");
            } else {
                assert!(
                    entry.get("truncated_input").is_none(),
                    "{chars} {kind}: {entry}"
                );
            }
        }
        // The model never read more than the cap.
        let sent = page_chars_sent(&model.seen());
        assert_eq!(sent.len(), 3, "{chars}");
        let expected = letters_for(chars.min(INPUT_CAP));
        assert!(sent.iter().all(|n| *n == expected), "{chars}: {sent:?}");
    }
}

// ---- the privacy block ----------------------------------------------------

#[tokio::test]
async fn a_privacy_block_that_forbids_a_model_hop_skips_every_format_with_no_model_call() {
    for classification in ["credential_or_secret", "zdr_ephemeral"] {
        let model = ai_model_double::start(standard).await;
        let edge = spawn_edge(&model).await;
        let mut request = all_four();
        request["privacy"] = json!({ "privacy_classification": classification });
        let (status, body) = post(&edge, "/v1/scrape", request).await;
        assert_eq!(status, 200, "{classification}: {body}");
        let data = &body["data"];
        assert!(
            data["markdown"].as_str().is_some(),
            "{classification}: the page itself is still scraped"
        );
        for key in ["json", "summary", "answer", "highlights"] {
            assert!(data.get(key).is_none(), "{classification}: `{key}` came back");
        }
        for kind in ["json", "summary", "question", "highlights"] {
            let entry = &data["ai"][kind];
            assert_eq!(entry["status"], "skipped", "{classification} {kind}: {entry}");
            assert_eq!(entry["code"], "PRIVACY_POLICY", "{classification} {kind}: {entry}");
            assert!(
                entry.get("model").is_none() && entry.get("usage").is_none(),
                "{classification} {kind}"
            );
        }
        assert_eq!(data["ai"]["total"]["calls"], 0, "{classification}");
        assert_eq!(model.calls(), 0, "{classification}: page text left for a model");
        assert_eq!(edge.fetches(), 1, "{classification}");
    }
}

#[tokio::test]
async fn an_unclassified_or_ordinary_privacy_block_still_calls_the_model() {
    for privacy in [
        None,
        Some(json!({})),
        Some(json!({ "privacy_classification": "customer_private" })),
        Some(json!({ "privacy_classification": "personal" })),
        Some(json!({ "privacy_classification": "public_non_personal" })),
    ] {
        let model = ai_model_double::start(standard).await;
        let edge = spawn_edge(&model).await;
        let mut request = three_formats();
        if let Some(privacy) = &privacy {
            request["privacy"] = privacy.clone();
        }
        let (status, body) = post(&edge, "/v1/scrape", request).await;
        assert_eq!(status, 200, "{privacy:?}: {body}");
        for kind in ["json", "summary", "question"] {
            assert_eq!(body["data"]["ai"][kind]["status"], "ok", "{privacy:?} {kind}");
        }
        assert_eq!(model.calls(), 3, "{privacy:?}");
    }
}

// ---- the stream -----------------------------------------------------------

#[tokio::test]
async fn the_stream_route_refuses_ai_formats_before_any_fetch() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    for formats in [
        json!(["markdown", "summary"]),
        json!([{ "type": "json", "schema": schema() }]),
        json!([{ "type": "question", "question": "q" }]),
        json!([{ "type": "highlights", "query": "q" }]),
    ] {
        let response = reqwest::Client::new()
            .post(format!("http://{}/v1/scrape/stream", edge.addr))
            .bearer_auth(TEST_BEARER)
            .json(&json!({ "url": "https://www.example.com/", "formats": formats }))
            .send()
            .await
            .unwrap();
        let text = response.text().await.unwrap();
        assert!(text.contains("event: error"), "{text}");
        assert!(text.contains("unsupported_in_stream"), "{text}");
        assert!(
            text.contains("AI formats are not served on the stream route"),
            "{text}"
        );
    }
    assert_eq!((edge.fetches(), model.calls()), (0, 0));

    // A stream without them still streams.
    let response = reqwest::Client::new()
        .post(format!("http://{}/v1/scrape/stream", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&json!({ "url": "https://www.example.com/", "formats": ["markdown"] }))
        .send()
        .await
        .unwrap();
    let text = response.text().await.unwrap();
    assert!(!text.contains("unsupported_in_stream"), "{text}");
    assert_eq!(edge.fetches(), 1);
}

// ---- logs -----------------------------------------------------------------

const SENTINEL: &str = "ZQSENT";

#[tokio::test]
async fn no_schema_prompt_page_or_model_output_reaches_the_log() {
    let page: &'static str = "<html><head><title>Sentinel page</title></head><body><main>\
<h1>Sentinel page for the log test</h1>\
<p>ZQSENT page line that only the page carries, long enough to read as the content of this page.</p>\
<p>A second paragraph so that the extractor keeps the main content of the page together.</p>\
</main></body></html>";
    let spec = json!({
        "url": "https://www.example.com/",
        "formats": [
            "markdown",
            { "type": "json", "prompt": "ZQSENT prompt", "schema": {
                "type": "object", "required": ["zqsent_field"],
                "properties": { "zqsent_field": { "type": "string" } } } },
            { "type": "summary" },
            { "type": "question", "question": "ZQSENT question?" },
            { "type": "highlights", "query": "ZQSENT query" },
        ],
    });

    // Once with a model that answers, once with one that fails: both paths log.
    let ok = ai_model_double::start(|seen: &Seen| match seen.task.as_str() {
        "extract" => chat(r#"{"zqsent_field":"ZQSENT value"}"#, 100, 5),
        "summarize" => chat("ZQSENT summary", 100, 5),
        "answer" => chat(
            r#"{"found":true,"answer":"ZQSENT answer","quote":"ZQSENT page line that only the page carries"}"#,
            100,
            5,
        ),
        _ => chat(r#"{"quotes":["ZQSENT page line that only the page carries"]}"#, 100, 5),
    })
    .await;
    let edge = spawn_edge_with(page, Some(ok.uri.clone()), Some(MODEL_TOKEN)).await;
    let (status, body) = post(&edge, "/v1/scrape", spec.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["ai"]["json"]["status"], "ok", "{body}");
    assert_eq!(
        body["data"]["answer"], "ZQSENT answer",
        "the sentinel did travel"
    );

    let failing = ai_model_double::start(|_: &Seen| Reply::status(500)).await;
    let edge = spawn_edge_with(page, Some(failing.uri.clone()), Some(MODEL_TOKEN)).await;
    let (status, body) = post(&edge, "/v1/scrape", spec).await;
    assert_eq!(status, 200, "{body}");

    let text = logged();
    assert!(
        !text.is_empty(),
        "the capture saw nothing: the check below would mean nothing"
    );
    assert!(
        text.contains("ai_scrape") || text.contains("quarry"),
        "the capture did not see Quarry's own events"
    );
    assert!(!text.contains(SENTINEL), "content reached the log:\n{text}");
    assert!(
        !text.contains("zqsent_field"),
        "a schema name reached the log"
    );
}

// ---- the page cache (needs Redis) -----------------------------------------

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

async fn try_redis() -> Option<redis::aio::ConnectionManager> {
    let client = redis::Client::open(redis_url()).ok()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.get_connection_manager(),
    )
    .await
    .ok()?
    .ok()
}

/// A request with AI formats neither reads nor writes the page cache. Needs a
/// reachable Redis (`REDIS_URL`); without one it prints a skip notice and passes,
/// unless `REQUIRE_CONTRACT` is set (as `tests/cache_policy.rs`).
#[tokio::test]
async fn an_ai_request_never_reads_or_writes_the_page_cache() {
    use redis::AsyncCommands;
    let Some(mut conn) = try_redis().await else {
        assert!(
            std::env::var_os("REQUIRE_CONTRACT").is_none(),
            "REQUIRE_CONTRACT is set but REDIS_URL is unreachable"
        );
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    init();
    let model = ai_model_double::start(standard).await;
    let (tx, mut rx) = mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let fetches = Arc::new(AtomicUsize::new(0));
    let state = AppState {
        redis: Some(conn.clone()),
        cache: Some(quarry_edge::cache::PageCache::new(
            conn.clone(),
            std::time::Duration::from_secs(3600),
        )),
        model_plane_url: Some(model.uri.clone()),
        model_plane_token: Some(MODEL_TOKEN.to_owned()),
        ..support::base_state(
            Arc::new(PageDriver {
                page: PAGE,
                fetches: fetches.clone(),
            }),
            EventSink::new(tx),
        )
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let edge = Edge {
        addr,
        store: Arc::new(InMemoryStore::new()),
        fetches: fetches.clone(),
    };

    // A default scrape fills the cache; the AI request with the same fingerprint must
    // not be served from it, and must not write its own entry.
    let (status, plain) = post(
        &edge,
        "/v1/scrape",
        json!({ "url": "https://www.example.com/", "cache": { "mode": "read_write" } }),
    )
    .await;
    assert_eq!(status, 200, "{plain}");
    let fingerprint = plain["data"]["fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned();
    let key = format!("quarry:page:{ORG}:{fingerprint}");
    let _: redis::RedisResult<i64> = conn.del(&key).await;
    let before = edge.fetches();

    let mut request = all_four();
    request["cache"] = json!({ "mode": "read_write" });
    request["prev_fingerprint"] = json!(fingerprint);
    let (status, body) = post(&edge, "/v1/scrape", request).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(edge.fetches(), before + 1, "an AI request always fetches");
    assert_eq!(body["data"]["effective_options"]["cache_state"], "bypass");
    let exists: bool = conn.exists(&key).await.unwrap_or(true);
    assert!(!exists, "an AI request wrote the page cache");
    let _: redis::RedisResult<i64> = conn.del(&key).await;
}
