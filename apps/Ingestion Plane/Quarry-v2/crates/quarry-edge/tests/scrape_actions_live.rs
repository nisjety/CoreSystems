//! WS13.5, ADR-6 (decision 3.8, step 3): the live evidence lane for scrape-time actions.
//!
//! This drives a RUNNING evidence-stack edge over HTTP. The edge is the real router with real
//! auth and a real Chromium, and the fixture host is a real origin the browser loads. For a
//! closed, capped action list the test checks what the contract promises: the markdown is the
//! post-click DOM; a request the page makes off-host after an action is blocked and counted; a
//! refusal names its code; a screenshot is a real PNG; the typed text never comes back; and a
//! second heavy request gets 429 while the first one holds the heavy permit.
//!
//! It is NOT part of `cargo test`. The test is `#[ignore]` and returns at once unless
//! `QUARRY_EVIDENCE_STACK=1` is set. With the flag set, a missing variable or a fixture that is
//! not the expected page FAILS the test; it never skips quietly (the same rule as the
//! `CHROMIUMOXIDE_TEST=1` lanes). It starts no server and no browser.
//!
//! Run on the evidence stack only, and only after the SC1 to SC7 evidence (decision 3.8, step 5):
//!
//!   QUARRY_EVIDENCE_STACK=1 E2E_EDGE=http://127.0.0.1:8082 E2E_TOKEN=<user token> \
//!   E2E_FIXTURE_URL=http://host.docker.internal:8099 \
//!   cargo test -p quarry-edge --test scrape_actions_live -- --ignored --nocapture --test-threads=1
//!
//! Variables: `E2E_EDGE` (the edge base), `E2E_TOKEN` (a USER bearer token: no service token and
//! no agent label), `E2E_FIXTURE_URL` (the fixture base AS THE EDGE CONTAINER REACHES IT).
//! Optional: `E2E_FIXTURE_PROBE_URL` (the same fixture as this process reaches it, read first to
//! check page.html) and `E2E_SERVICE_TOKEN` (a service token WITHOUT scrape:interact, for the 403).
//!
//! The edge must run with `QUARRY_EDGE__SCRAPE_ACTIONS=user` (dev and evidence only; production
//! stays off), `QUARRY_EDGE__SCRAPE_CAPTURE=on` (the screenshot action), and
//! `QUARRY_SCRAPE_HEAVY_PERMITS` at its default of 1 (the 429 check depends on it).
//!
//! The fixture is `page.html` on the fixture host. The checks depend on these parts of it:
//! `<h1 id="heading" data-after="AFTER">BEFORE</h1>`; `<button id="swap">`, whose script sets the heading's text
//! from `data-after="AFTER"` on the heading; `<button id="offhost">`, whose script appends an
//! `<img>` from `http://offhost.example.net/pixel.png` (that host must not be allowed on the
//! stack); `<input id="search">`; and a paragraph long enough to be extracted as main content.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

const FLAG: &str = "QUARRY_EVIDENCE_STACK";
const SECRET: &str = "SECRET_SENTINEL_123";
const FIXTURE_MARKERS: [&str; 5] = [
    r#"id="swap""#,
    r#"data-after="AFTER""#,
    "BEFORE",
    r#"id="offhost""#,
    r#"id="search""#,
];
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[derive(Clone)]
struct Live {
    edge: String,
    token: String,
    fixture: String,
    client: reqwest::Client,
}

fn required(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_owned(),
        _ => panic!("{FLAG}=1 needs {name} set; see the module docs of scrape_actions_live.rs"),
    }
}

impl Live {
    fn from_env() -> Self {
        Self {
            edge: required("E2E_EDGE").trim_end_matches('/').to_owned(),
            token: required("E2E_TOKEN"),
            fixture: required("E2E_FIXTURE_URL").trim_end_matches('/').to_owned(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(130))
                .build()
                .expect("an HTTP client"),
        }
    }

    /// The request body for the fixture page, with `extra` fields added.
    fn page(&self, extra: Value) -> Value {
        let mut body = json!({ "url": format!("{}/page.html", self.fixture) });
        if let (Some(base), Some(more)) = (body.as_object_mut(), extra.as_object()) {
            for (key, value) in more {
                base.insert(key.clone(), value.clone());
            }
        }
        body
    }

    /// `POST /v1/scrape` with `token`: the status and the parsed body (or the raw text).
    async fn scrape(&self, token: &str, body: &Value) -> (u16, Value) {
        let response = self
            .client
            .post(format!("{}/v1/scrape", self.edge))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .expect("the edge answers");
        let status = response.status().as_u16();
        let text = response.text().await.expect("a response body");
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
}

/// The page's markdown. Under ZDR it travels inline in `data.markdown`.
fn markdown_of(body: &Value) -> String {
    body["data"]["markdown"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Whether the error body carries a field error with this `code`.
fn has_field_code(body: &Value, code: &str) -> bool {
    body["error"]["details"]["fields"]
        .as_array()
        .is_some_and(|fields| fields.iter().any(|field| field["code"] == code))
}

fn pass(checks: &mut u32, what: &str) {
    *checks += 1;
    eprintln!("ok   {what}");
}

#[tokio::test]
#[ignore = "WS13.5 ADR-6 evidence lane: needs a running evidence stack (set QUARRY_EVIDENCE_STACK=1, see the module docs)"]
async fn scrape_actions_live() {
    if std::env::var(FLAG).ok().as_deref() != Some("1") {
        eprintln!("skipping: set {FLAG}=1 on the evidence stack to run scrape_actions_live");
        return;
    }
    let live = Arc::new(Live::from_env());
    let mut checks = 0;

    // 0. The fixture is the page the checks expect, read from a host this process can reach.
    match std::env::var("E2E_FIXTURE_PROBE_URL") {
        Ok(probe) if !probe.trim().is_empty() => {
            let page = live
                .client
                .get(format!("{}/page.html", probe.trim().trim_end_matches('/')))
                .send()
                .await
                .expect("the fixture probe answers");
            assert_eq!(
                page.status().as_u16(),
                200,
                "page.html is not served at the probe URL"
            );
            let html = page.text().await.expect("page.html");
            for marker in FIXTURE_MARKERS {
                assert!(
                    html.contains(marker),
                    "page.html lacks {marker}: the fixture is not the expected page"
                );
            }
            pass(
                &mut checks,
                "fixture: page.html has the swap, the off-host button, the search box and BEFORE",
            );
        }
        _ => eprintln!("skip fixture probe: E2E_FIXTURE_PROBE_URL is not set"),
    }

    // 1. Post-click DOM: the markdown shows what the click left on the page.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": [{ "type": "click", "selector": "#swap" }, { "type": "wait", "milliseconds": 300 }],
                "formats": ["markdown"],
                "zdr": true,
            })),
        )
        .await;
    assert_eq!(
        status, 200,
        "a click then a wait must be served; is the edge in user mode? {body}"
    );
    assert_eq!(
        body["data"]["effective_options"]["actions"]["count"], 2,
        "{body}"
    );
    assert_eq!(body["data"]["actions"][0]["status"], "ok", "{body}");
    assert_eq!(body["data"]["actions"][1]["status"], "ok", "{body}");
    let markdown = markdown_of(&body);
    assert!(
        markdown.contains("AFTER") && !markdown.contains("BEFORE"),
        "the markdown is not the post-click DOM: {markdown:.200}"
    );
    pass(
        &mut checks,
        "post-click DOM: click then wait, the markdown shows AFTER and not BEFORE",
    );

    // 2. Without the click the page is still BEFORE: the change came from the action.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": [{ "type": "wait", "milliseconds": 100 }],
                "formats": ["markdown"],
                "zdr": true,
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let markdown = markdown_of(&body);
    assert!(
        markdown.contains("BEFORE") && !markdown.contains("AFTER"),
        "without the click: {markdown:.200}"
    );
    pass(
        &mut checks,
        "no click: the markdown shows BEFORE and not AFTER",
    );

    // 3. Abort (the default): a missing selector fails the run, with the ordered outcomes.
    let missing_then_swap = json!([
        { "type": "click", "selector": "#missing" },
        { "type": "click", "selector": "#swap" },
    ]);
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({ "actions": missing_then_swap, "zdr": true })),
        )
        .await;
    assert_eq!(status, 502, "{body}");
    assert_eq!(
        body["error"]["details"]["reason"], "action_failed",
        "{body}"
    );
    assert_eq!(
        body["error"]["details"]["actions"][0]["status"], "failed",
        "{body}"
    );
    assert_eq!(
        body["error"]["details"]["actions"][1]["status"], "skipped",
        "{body}"
    );
    pass(
        &mut checks,
        "abort: a missing selector is 502 action_failed, and the later action is skipped",
    );

    // 4. Continue: the run goes on, the failure is a warning, and the later click still happens.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": missing_then_swap,
                "actionsOnError": "continue",
                "formats": ["markdown"],
                "zdr": true,
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let warned = body["data"]["warnings"].as_array().is_some_and(|warnings| {
        warnings
            .iter()
            .any(|warning| warning["code"] == "action_failed")
    });
    assert!(
        warned,
        "continue must keep the failure as an action_failed warning: {body}"
    );
    assert!(
        markdown_of(&body).contains("AFTER"),
        "continue must run the later click: {body}"
    );
    pass(
        &mut checks,
        "continue: 200 with an action_failed warning, and the later click still ran",
    );

    // 5. The closed list: 51 actions are too many, and a denied type is refused by name.
    let fifty_one: Vec<Value> = (0..51)
        .map(|_| json!({ "type": "wait", "milliseconds": 10 }))
        .collect();
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({ "actions": fifty_one, "zdr": true })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert!(has_field_code(&body, "too_many_actions"), "{body}");
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({ "actions": [{ "type": "evaluate", "script": "1" }] })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert!(has_field_code(&body, "action_denied"), "{body}");
    pass(
        &mut checks,
        "closed list: 51 actions are too_many_actions, and evaluate is action_denied",
    );

    // 6. The agent label: a user token cannot send actions labelled as a Model action.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": [{ "type": "wait", "milliseconds": 10 }],
                "initiated_by": "agent",
            })),
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(
        body["error"]["details"]["reason"], "scrape_interact_scope_required",
        "{body}"
    );
    pass(
        &mut checks,
        "agent label: a user token with initiated_by agent is 403 scrape_interact_scope_required",
    );

    // 7. Ingest with actions is a field error, never a silent drop.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(
                json!({ "actions": [{ "type": "wait", "milliseconds": 10 }], "ingest": true }),
            ),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert!(
        has_field_code(&body, "not_supported_with_actions"),
        "{body}"
    );
    pass(
        &mut checks,
        "ingest with actions: 400 not_supported_with_actions",
    );

    // 8. Off-host denial: after the click the page asks for an off-host image. The egress policy
    //    blocks it, and the response counts it. The response carries a count, never the URL.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": [{ "type": "click", "selector": "#offhost" }, { "type": "wait", "milliseconds": 500 }],
                "formats": ["markdown"],
                "zdr": true,
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let blocked = body["data"]["captures"]["blocked_requests"]
        .as_u64()
        .unwrap_or(0);
    assert!(
        blocked >= 1,
        "the off-host request after the click was not blocked and counted: {body}"
    );
    assert!(
        !body.to_string().contains("offhost.example.net"),
        "a blocked URL came back: {body}"
    );
    pass(&mut checks, "off-host denial: the request after the click is blocked and counted, and its URL never comes back");

    // 9. A screenshot after the click is a real PNG, inline under ZDR and never stored.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": [{ "type": "click", "selector": "#swap" }, { "type": "screenshot" }],
                "zdr": true,
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let output = &body["data"]["actions"][1]["output"];
    assert_eq!(output["content_type"], "image/png", "{body}");
    assert!(
        output.get("artifact_id").is_none(),
        "ZDR stores nothing, so there is no artifact id: {body}"
    );
    let data = output["data_base64"]
        .as_str()
        .expect("an inline screenshot under ZDR");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("base64 screenshot");
    assert!(
        bytes.starts_with(&PNG_SIGNATURE),
        "the inline screenshot is not a PNG"
    );
    pass(
        &mut checks,
        "screenshot action: an inline PNG under ZDR, with no artifact id",
    );

    // 10. The typed text never comes back in a response. The operator also greps the edge's logs
    //     for it (the progress file has the command); this test cannot read the logs.
    let (status, body) = live
        .scrape(
            &live.token,
            &live.page(json!({
                "actions": [
                    { "type": "write", "selector": "#search", "text": SECRET },
                    { "type": "press", "key": "Tab" },
                ],
                "zdr": true,
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        !body.to_string().contains(SECRET),
        "the typed text came back in a response: {body}"
    );
    pass(
        &mut checks,
        "typed text: the write's text is in no response",
    );

    // 11. Heavy permit: while one actions request holds the single permit, a second one gets
    //     429 heavy_browser_busy at once, a plain scrape (no actions) still answers 200, and the
    //     first request completes with 200.
    let slow = {
        let live = live.clone();
        let body = live
            .page(json!({ "actions": [{ "type": "wait", "milliseconds": 6000 }], "zdr": true }));
        tokio::spawn(async move { live.scrape(&live.token, &body).await })
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let (status, body) = live
        .scrape(
            &live.token,
            &live
                .page(json!({ "actions": [{ "type": "wait", "milliseconds": 100 }], "zdr": true })),
        )
        .await;
    assert_eq!(
        status, 429,
        "a second actions request while the permit is held: {body}"
    );
    assert_eq!(
        body["error"]["details"]["reason"], "heavy_browser_busy",
        "{body}"
    );
    let (status, body) = live
        .scrape(&live.token, &live.page(json!({ "formats": ["markdown"] })))
        .await;
    assert_eq!(
        status, 200,
        "a plain scrape is not heavy and must answer during the held permit: {body}"
    );
    let (status, body) = slow.await.expect("the first actions request finishes");
    assert_eq!(status, 200, "the first actions request completes: {body}");
    pass(&mut checks, "heavy permit: a second actions request is 429 heavy_browser_busy, a plain scrape is 200, the first is 200");

    // 12. Optional: a service token without scrape:interact is refused when it sends actions.
    match std::env::var("E2E_SERVICE_TOKEN") {
        Ok(token) if !token.trim().is_empty() => {
            let (status, body) = live
                .scrape(
                    token.trim(),
                    &live.page(json!({ "actions": [{ "type": "wait", "milliseconds": 10 }] })),
                )
                .await;
            assert_eq!(status, 403, "{body}");
            assert_eq!(
                body["error"]["details"]["reason"], "scrape_interact_scope_required",
                "{body}"
            );
            pass(
                &mut checks,
                "service token without scrape:interact: 403 scrape_interact_scope_required",
            );
        }
        _ => eprintln!("skip service token check: E2E_SERVICE_TOKEN is not set"),
    }

    eprintln!("all {checks} checks passed");
}
