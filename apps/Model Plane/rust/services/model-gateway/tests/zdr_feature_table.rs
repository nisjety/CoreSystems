//! What a ZDR turn does with every invoke-request field and feature string.
//!
//! The rows below are the spec, and they run the real functions: the turn's posture
//! decision (`zdr_posture::decide`) on the real `Claims` shape, then the feature
//! classifier, in the order the ZDR stream calls them. A field must have a row that matches its
//! treatment in `zdr_features::FIELD_TREATMENTS`, so a field cannot be classified
//! on paper and left unchecked, and a refused value must be refused with its
//! catalogue code and feature, while a default value must not be refused.

use model_gateway::auth::Claims;
use model_gateway::http_routes::InvokeRequest;
use model_gateway::zdr_features::{self, Treatment, FEATURE_TREATMENTS, FIELD_TREATMENTS};
use model_gateway::zdr_posture::{self, Route};
use mp_contracts::zdr_refusal::{code, Refusal};
use serde_json::{json, Value};

/// What a ZDR turn decides: `Some((code, feature))` for a refusal, else `None`.
type Expect = Option<(&'static str, Option<&'static str>)>;

fn request(body: &Value) -> InvokeRequest {
    serde_json::from_value(body.clone()).expect("a wire body parses")
}

/// A user token that says ZDR, as Auth Core mints it.
fn zdr_token() -> Claims {
    serde_json::from_value(json!({
        "sub": "user-1",
        "iss": "https://auth.test/model",
        "aud": "model-gateway",
        "exp": 4_102_444_800_i64,
        "nbf": 1_700_000_000_i64,
        "org_id": "org-1",
        "user_id": "user-1",
        "principal_type": "user",
        "zdr": true,
    }))
    .expect("a minted token's payload is valid Claims")
}

/// The decision the ZDR stream makes for a turn: the turn's posture first (the
/// resolver's refusal for a connection, as the stream's entry point asks it), then
/// the feature classifier.
fn decided(body: &Value) -> Expect {
    let request = request(body);
    let route = Route::named(
        request.provider.as_deref(),
        request.subscription_connection_id.as_deref(),
        request.zdr,
    );
    match zdr_posture::decide_for(&zdr_token(), &route) {
        Err(refusal) => Some((refusal.code(), refusal.feature())),
        Ok(_) => zdr_features::refusal(&request)
            .map(|refusal: Refusal| (refusal.code(), refusal.feature())),
    }
}

fn refused(feature: &'static str) -> Expect {
    Some((code::FEATURE_NOT_SUPPORTED, Some(feature)))
}

/// `(field, a request that uses it, what a ZDR turn decides)`.
fn cases() -> Vec<(&'static str, Value, Expect)> {
    let base = json!({"content": "hello", "model": "gpt-5-mini"});
    let with = |field: &str, value: Value| {
        let mut body = base.clone();
        body[field] = value;
        body
    };
    let subscription = json!({
        "content": "hello",
        "provider": "openai-codex-subscription",
        "subscription_connection_id": "conn-1",
    });
    let refused_connection = Some((code::CONNECTION_NOT_ACKNOWLEDGED, None));
    vec![
        ("content", base.clone(), None),
        ("model", with("model", json!("gpt-5.6-terra")), None),
        ("provider", subscription.clone(), refused_connection),
        ("provider", with("provider", json!("azure-openai")), None),
        (
            "subscription_connection_id",
            subscription,
            refused_connection,
        ),
        ("session_key", with("session_key", json!("s-1")), None),
        ("thread_id", with("thread_id", json!("t-1")), None),
        (
            "space_append_context",
            with("space_append_context", Value::Null),
            None,
        ),
        ("space_context", with("space_context", Value::Null), None),
        (
            "structured_output_schema",
            with("structured_output_schema", json!("{\"type\":\"object\"}")),
            None,
        ),
        (
            "structured_output_schema",
            with("structured_output_schema", json!("  ")),
            None,
        ),
        ("zdr", with("zdr", json!(true)), None),
        ("min_privacy_tier", with("min_privacy_tier", json!(2)), None),
        (
            "browse_web",
            with("browse_web", json!(true)),
            refused("web_search"),
        ),
        ("browse_web", with("browse_web", json!(false)), None),
        (
            "skill_ids",
            with("skill_ids", json!(["skill-1"])),
            refused("skills"),
        ),
        ("skill_ids", with("skill_ids", json!([])), None),
        (
            "deep_research",
            with("deep_research", json!(true)),
            refused("deep_research"),
        ),
        (
            "retrieval_scope",
            with("retrieval_scope", json!("web_only")),
            refused("web_search"),
        ),
        (
            "retrieval_scope",
            with("retrieval_scope", json!("knowledge_only")),
            None,
        ),
        (
            "retrieval_scope",
            with("retrieval_scope", json!("auto")),
            None,
        ),
        (
            "connector_scope",
            with("connector_scope", json!(["conn-1"])),
            refused("connector_read"),
        ),
        (
            "connector_scope",
            with("connector_scope", json!([])),
            None,
        ),
        ("regenerated", with("regenerated", json!(true)), None),
        (
            "pinned_message_ids",
            with("pinned_message_ids", json!(["m-1"])),
            refused("pinned_messages"),
        ),
        (
            "pinned_message_ids",
            with("pinned_message_ids", json!([])),
            None,
        ),
        (
            "edited_resubmit",
            with("edited_resubmit", json!(true)),
            None,
        ),
        (
            "max_cost_usd",
            with("max_cost_usd", json!(0.5)),
            refused("spend_cap"),
        ),
        ("max_tokens", with("max_tokens", json!(256)), None),
        (
            "profile",
            with("profile", json!("deployed_agent")),
            refused("deployed_agent"),
        ),
        ("profile", with("profile", json!("chat")), None),
        ("verbosity", with("verbosity", json!("concise")), None),
        ("effort", with("effort", json!("deep")), None),
        (
            "features",
            with("features", json!(["agentic"])),
            refused("agentic"),
        ),
        (
            "features",
            with(
                "features",
                json!([
                    "usage",
                    "citations",
                    "tools",
                    "artifacts",
                    "reasoning",
                    "steps",
                    "memory"
                ]),
            ),
            None,
        ),
        (
            "idempotency_key",
            with("idempotency_key", json!("key-1")),
            None,
        ),
        (
            "attachments",
            with(
                "attachments",
                json!([{"kind": "image", "url": "https://example.org/a.png"}]),
            ),
            refused("attachments"),
        ),
        // The attachment contract never sees a ZDR turn: an inline image and a named
        // file are refused with the same catalogue code, before any size or kind check.
        (
            "attachments",
            with(
                "attachments",
                json!([{"kind": "image", "mime_type": "image/png", "data_base64": "aGk="}]),
            ),
            refused("attachments"),
        ),
        (
            "attachments",
            with(
                "attachments",
                json!([{"kind": "file", "name": "sales.csv", "data_base64": "aGk="}]),
            ),
            refused("attachments"),
        ),
        (
            "generate_image",
            with("generate_image", json!(true)),
            refused("image_generation"),
        ),
        (
            "plan_mode",
            with("plan_mode", json!(true)),
            refused("plan_mode"),
        ),
        // An execute turn runs an executing loop under a plan grant: no plan or grant
        // exists on a ZDR turn, so it is refused through the `agentic` key, on its own
        // (the request carries no `agentic` feature here) and with it.
        (
            "execute_granted",
            with("execute_granted", json!(true)),
            refused("agentic"),
        ),
        (
            "execute_granted",
            with("executeGranted", json!(true)),
            refused("agentic"),
        ),
        ("execute_granted", with("execute_granted", json!(false)), None),
        (
            "tools",
            with(
                "tools",
                json!([{"name": "lookup", "description": "d", "parameters_json": "{}"}]),
            ),
            refused("client_tools"),
        ),
        ("org_name", with("org_name", json!("Aquatiq")), None),
        ("user_name", with("user_name", json!("Ima")), None),
        ("agent_name", with("agent_name", json!("Planner")), None),
        (
            "agent_system_prompt",
            with("agent_system_prompt", json!("Be brief.")),
            None,
        ),
        (
            "org_instructions",
            with("org_instructions", json!("Write in Norwegian.")),
            None,
        ),
        (
            "space_instructions",
            with("space_instructions", json!("Team notes.")),
            None,
        ),
        (
            "persona_instructions",
            with("persona_instructions", json!("Friendly.")),
            None,
        ),
    ]
}

#[test]
fn every_field_row_decides_what_its_treatment_says() {
    for (field, body, expect) in cases() {
        assert_eq!(decided(&body), expect, "{field}: {body}");
    }
}

#[test]
fn every_field_has_a_row_that_matches_its_treatment() {
    let cases = cases();
    for (field, treatment) in FIELD_TREATMENTS {
        let rows: Vec<&Expect> = cases
            .iter()
            .filter(|(name, _, _)| name == field)
            .map(|(_, _, expect)| expect)
            .collect();
        assert!(!rows.is_empty(), "{field} has no row in the ZDR spec");
        match treatment {
            Treatment::Refused { code, feature } => assert!(
                rows.contains(&&Some((*code, *feature))),
                "{field} is refused as {code} {feature:?} but no row shows that refusal"
            ),
            Treatment::Honored | Treatment::Inert(_) => assert!(
                rows.contains(&&None),
                "{field} is honored or inert but no row shows a turn that is not refused"
            ),
            Treatment::PerMember => assert!(
                rows.iter().any(|expect| expect.is_some())
                    && rows.iter().any(|expect| expect.is_none()),
                "{field} is classified per member: its rows must show both a refusal and a pass"
            ),
        }
    }
}

#[test]
fn the_guard_names_every_invoke_field_once_in_declaration_order() {
    let names = zdr_features::invoke_field_names(&request(&json!({"content": "x"})));
    let table: Vec<&str> = FIELD_TREATMENTS.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names, table,
        "FIELD_TREATMENTS must list the fields the guard names"
    );
}

#[test]
fn every_feature_string_decides_what_its_treatment_says() {
    let mut names: Vec<&str> = FEATURE_TREATMENTS.iter().map(|(name, _)| *name).collect();
    names.push("a_feature_nobody_reads");
    for name in names {
        let body = json!({"content": "x", "features": [name]});
        let expect = match zdr_features::feature_treatment(name) {
            Treatment::Refused { code, feature } => Some((code, feature)),
            _ => None,
        };
        assert_eq!(decided(&body), expect, "feature {name}");
    }
}

#[test]
fn every_refusal_the_spec_names_is_a_catalogue_refusal() {
    for (field, treatment) in FIELD_TREATMENTS.iter().chain(FEATURE_TREATMENTS.iter()) {
        if let Treatment::Refused { code, feature } = treatment {
            assert!(
                Refusal::new(code, *feature).is_some(),
                "{field} refuses with {code} {feature:?}, which the catalogue does not define"
            );
        }
    }
}

#[test]
fn every_inert_field_states_its_reason() {
    for (field, treatment) in FIELD_TREATMENTS.iter().chain(FEATURE_TREATMENTS.iter()) {
        if let Treatment::Inert(reason) = treatment {
            assert!(
                reason.trim().len() > 20,
                "{field} is inert without a stated reason"
            );
        }
    }
}

#[test]
fn a_request_asking_for_several_things_is_refused_for_the_first_in_order() {
    let body = json!({
        "content": "x",
        "attachments": [{"url": "https://example.org/a.png"}],
        "plan_mode": true,
        "browse_web": true,
    });
    assert_eq!(decided(&body), refused("attachments"));
}

#[test]
fn web_only_is_not_refused_inside_a_conversation_only_turn() {
    let body = json!({
        "content": "x",
        "retrieval_scope": "web_only",
        "features": ["conversation_only"],
    });
    assert_eq!(
        decided(&body),
        None,
        "conversation-only turns force the scope to auto"
    );
}

/// Until the ZDR stream asks the classifier, its old four-feature list still
/// decides, and most of the table above is not enforced on a live turn.
#[test]
fn the_zdr_stream_asks_the_classifier_and_no_longer_keeps_its_own_list() {
    let sse = include_str!("../src/sse.rs");
    assert!(
        sse.contains("zdr_features::refusal("),
        "the ZDR branch of invoke_stream_sse must ask zdr_features::refusal"
    );
    assert!(
        !sse.contains("ZDR currently supports plain chat and read-only grounding only"),
        "the four-feature refusal with no feature name is still in sse.rs"
    );
}
