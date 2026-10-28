//! Shared pieces of the real-token scrape-actions binaries: `scrape_actions_real_token.rs`
//! runs the `on` mode, `scrape_actions_real_token_user_mode.rs` runs `user`. Each includes this
//! file with `#[path = "support/actions_real_auth.rs"] mod actions_real_auth;`, next to the
//! real-auth harness of `zdr_harness.rs` (no dev bypass, every request through `require_auth`).
//!
//! The browser is a double. These tests are about admission: who may send `actions[]` and
//! what the edge answers. The page content does not matter here, so the double hands back the
//! captures one click would leave behind and nothing else.

#![allow(dead_code)]

use std::sync::{Arc, Once};

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::scrape_actions::{ActionOutcome, ActionStatus};
use quarry_core::QuarryResult;
use quarry_runtime::capture::PageCaptures;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use serde_json::{json, Value};
use url::Url;

use crate::zdr_harness::Harness;

/// The scope a service principal needs before it may send scrape-time actions (decision 3.8).
pub const SCOPE_SCRAPE_INTERACT: &str = "scrape:interact";

/// The environment switch the edge reads once per process (`scrape_actions::ENV_SCRAPE_ACTIONS`).
pub const ENV_SWITCH: &str = "QUARRY_EDGE__SCRAPE_ACTIONS";

const PAGE: &str = "<html><head><title>Actions page</title></head><body><main><h1>After</h1><p>Some body text for the real-token admission test, long enough to be extracted as main content.</p></main></body></html>";

/// A browser double: the captures a real fetch carries after one click on `#swap`.
pub struct ClickBrowser;

#[async_trait]
impl Driver for ClickBrowser {
    fn kind(&self) -> DriverKind {
        DriverKind::Browser
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: PAGE.as_bytes().to_vec(),
            duration_ms: 2,
            served_by: DriverKind::Browser,
            captures: Some(Box::new(PageCaptures {
                actions: vec![ActionOutcome {
                    index: 0,
                    kind: "click".into(),
                    status: ActionStatus::Ok,
                    duration_ms: 3,
                    error: None,
                    output: None,
                }],
                action_outputs: vec![None],
                ..PageCaptures::default()
            })),
        })
    }
}

/// Set the switch for this process, once, before the first request reads it. Each binary
/// uses one mode only, so the single `Once` is enough.
pub fn enable(mode: &str) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var(ENV_SWITCH, mode));
}

/// The real router with the browser double registered beside the static driver.
pub async fn start() -> Harness {
    Harness::start_with(|mut state| {
        state.drivers.register(Arc::new(ClickBrowser));
        state
    })
    .await
}

/// A scrape body for a public URL with `actions` set.
pub fn scrape_body(actions: Value) -> Value {
    json!({
        "url": "https://www.example.com/",
        "formats": ["markdown"],
        "actions": actions,
    })
}

/// One click on `#swap`, the smallest closed list that carries an action.
pub fn click() -> Value {
    json!([{ "type": "click", "selector": "#swap" }])
}

/// The same body with `initiated_by` set to `label`.
pub fn labelled(mut body: Value, label: &str) -> Value {
    body["initiated_by"] = json!(label);
    body
}

/// `POST /v1/scrape` with `bearer`: the status and the parsed body (or the raw text).
pub async fn post(harness: &Harness, bearer: &str, body: Value) -> (u16, Value) {
    let response = harness.post(bearer, "/v1/scrape", body).await;
    let status = response.status().as_u16();
    let text = response.text().await.expect("a response body");
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}
