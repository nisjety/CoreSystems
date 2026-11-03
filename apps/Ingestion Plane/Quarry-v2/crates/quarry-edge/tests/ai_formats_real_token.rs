//! INNH-1 U3: who may use the AI formats of `POST /v1/scrape`, on real tokens,
//! and how a ZDR organisation's token floor reaches them.
//!
//! Beside `zdr_claim_floor.rs`, on its real-auth harness (`support/zdr_harness.rs`):
//! no dev bypass, every request runs the real `require_auth` against a JWKS
//! endpoint, and the claim sets are the golden Auth Core minted. `tests/ai_formats.rs`
//! runs on the dev bypass, whose stub identity is a plain user with no `zdr` claim and
//! no scopes, so it cannot show what a service principal or a ZDR organisation gets.
//!
//! These run in a normal `cargo test` (they need no stack and no Chromium). The
//! switch is turned on before the first request; the model is a strict double of
//! the gateway's chat route, and the edge's state carries its URL and a dev
//! credential (what `main.rs` sets only under the dev bypass; here the state is
//! built directly).

mod support;

#[path = "support/zdr_harness.rs"]
mod zdr_harness;

#[path = "support/ai_model_double.rs"]
mod ai_model_double;

use std::sync::Once;

use ai_model_double::{chat, Double, Seen};
use serde_json::{json, Value};
use zdr_harness::{
    golden_token, token_with_scopes, user_in_standard_org, user_in_zdr_org, Harness,
};

const REFUSED: &str = "extract_ai_scope_required";
const SCOPE_EXTRACT_AI: &str = "extract:ai";

fn enable() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("QUARRY_EDGE__AI_FORMATS", "on"));
}

fn model_script(seen: &Seen) -> ai_model_double::Reply {
    match seen.task.as_str() {
        "summarize" => chat("A short summary of the page.", 100, 8),
        _ => chat("{}", 10, 1),
    }
}

async fn start(double: &Double) -> Harness {
    enable();
    let url = double.uri.clone();
    Harness::start_with(move |mut state| {
        state.model_plane_url = Some(url);
        state.model_plane_token = Some("model-dev-token".to_owned());
        state
    })
    .await
}

async fn post(harness: &Harness, bearer: &str, body: Value) -> (u16, Value) {
    let response = harness.post(bearer, "/v1/scrape", body).await;
    let status = response.status().as_u16();
    let text = response.text().await.expect("a response body");
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn body() -> Value {
    json!({
        "url": "https://www.example.com/",
        "formats": ["markdown", "summary"],
    })
}

fn labelled(mut body: Value, label: &str) -> Value {
    body["initiated_by"] = json!(label);
    body
}

#[tokio::test]
async fn a_zdr_org_user_gets_every_format_skipped_even_when_the_body_says_zdr_false() {
    let model = ai_model_double::start(model_script).await;
    let harness = start(&model).await;

    let mut request = body();
    request["zdr"] = json!(false);
    let (status, zdr) = post(&harness, &user_in_zdr_org(), request.clone()).await;
    assert_eq!(status, 200, "{zdr}");
    assert_eq!(zdr["data"]["ai"]["summary"]["status"], "skipped", "{zdr}");
    assert_eq!(
        zdr["data"]["ai"]["summary"]["code"], "ZDR_UNAVAILABLE",
        "{zdr}"
    );
    assert!(zdr["data"].get("summary").is_none());
    assert_eq!(
        zdr["data"]["effective_options"]["zdr"], true,
        "the token floor held"
    );
    assert_eq!(model.calls(), 0, "no page text left for a model");

    // The same body from an organisation without the floor is served.
    let (status, standard) = post(&harness, &user_in_standard_org(), request).await;
    assert_eq!(status, 200, "{standard}");
    assert_eq!(
        standard["data"]["ai"]["summary"]["status"], "ok",
        "{standard}"
    );
    assert_eq!(standard["data"]["summary"], "A short summary of the page.");
    assert_eq!(model.calls(), 1);
    model.assert_clean();
}

#[tokio::test]
async fn a_service_principal_needs_extract_ai_and_the_other_scope_does_not_stand_in() {
    let model = ai_model_double::start(model_script).await;
    let harness = start(&model).await;

    // Both golden service principals reach /v1/scrape; neither holds extract:ai.
    for case in ["service_model_gateway", "service_execution_core"] {
        let (status, refused) = post(&harness, &golden_token(case), body()).await;
        assert_eq!(status, 403, "{case}: {refused}");
        assert_eq!(
            refused["error"]["details"]["reason"], REFUSED,
            "{case}: {refused}"
        );
        assert_eq!(refused["error"]["details"]["field"], "formats[1]", "{case}");
    }
    // scrape:interact is the scope of actions, not of this.
    let interact = token_with_scopes("service_execution_core", &["scrape:interact"]);
    let (status, refused) = post(&harness, &interact, body()).await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(model.calls(), 0);

    // A scrape with no AI format is not touched by the scope.
    let (status, plain) = post(
        &harness,
        &golden_token("service_execution_core"),
        json!({ "url": "https://www.example.com/", "formats": ["markdown"] }),
    )
    .await;
    assert_eq!(status, 200, "{plain}");

    // With the scope the same body is served.
    let granted = token_with_scopes("service_execution_core", &[SCOPE_EXTRACT_AI]);
    let (status, served) = post(&harness, &granted, body()).await;
    assert_eq!(status, 200, "{served}");
    assert_eq!(served["data"]["ai"]["summary"]["status"], "ok", "{served}");
    assert_eq!(model.calls(), 1);
    model.assert_clean();
}

#[tokio::test]
async fn the_agent_label_is_refused_for_a_user_and_for_a_service_that_holds_the_scope() {
    let model = ai_model_double::start(model_script).await;
    let harness = start(&model).await;

    let (status, refused) =
        post(&harness, &user_in_standard_org(), labelled(body(), "agent")).await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(refused["error"]["details"]["reason"], REFUSED);

    let granted = token_with_scopes("service_execution_core", &[SCOPE_EXTRACT_AI]);
    let (status, refused) = post(&harness, &granted, labelled(body(), "agent")).await;
    assert_eq!(status, 403, "the scope does not lift the label: {refused}");
    assert_eq!(refused["error"]["details"]["reason"], REFUSED);

    // The same user body without the label is served; an unknown label is ignored.
    for label in [None, Some("somebody")] {
        let request = match label {
            Some(label) => labelled(body(), label),
            None => body(),
        };
        let (status, served) = post(&harness, &user_in_standard_org(), request).await;
        assert_eq!(status, 200, "{served}");
        assert_eq!(served["data"]["ai"]["summary"]["status"], "ok");
    }
    assert_eq!(
        model.calls(),
        2,
        "only the two served requests reached the model"
    );
}

#[tokio::test]
async fn a_token_that_does_not_verify_never_reaches_the_ai_gate() {
    let model = ai_model_double::start(model_script).await;
    let harness = start(&model).await;
    let (status, _) = post(&harness, "not-a-signed-token", body()).await;
    assert_eq!(status, 401);
    assert_eq!(model.calls(), 0);
}
