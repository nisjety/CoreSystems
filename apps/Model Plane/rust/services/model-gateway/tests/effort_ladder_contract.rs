//! Guards the reasoning-effort ladder across the gateway's three derivation
//! sites and the wire it shares with inference-core.
//!
//! Before the ladder the same profile was turned into a token budget in three
//! places (the streamed chat turn, the persistence-free ZDR turn and the
//! source-validation check), and each adapter then guessed what the number
//! meant, which is how `deep` came to equal `quick` on the default deploy. The
//! ladder has one derivation, `thinking::plan`, and these tests fail if a call
//! site goes back to deriving its own.
//!
//! # Why some of this reads source text
//!
//! The behaviour of `thinking` is public and is tested by calling it. The call
//! sites are private functions deep inside the streaming handler, which no test
//! can reach without a running provider, so their wiring is asserted on the
//! source of record, the way `cross_service_loop_contract.rs` does for the two
//! loops. The proto and inference-core are read the same way because model-
//! gateway does not depend on inference-core: neither side is widened for test
//! visibility.

use std::path::PathBuf;

use model_gateway::thinking::{
    allowance_for_wire, effort_level, parse_ladder, plan, raised_ceiling, reasoning_allowance,
    source_review_plan, Plan,
};
use mp_contracts::model_plane::v1::ReasoningEffort;

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read {} ({error}). This file holds one half of the effort-ladder \
                 contract: if it moved, re-point the path here, do not delete the assertion.",
                path.display()
            )
        })
        // A Windows checkout may carry CRLF line ends.
        .replace("\r\n", "\n")
}

/// The body of `fn name(` up to the first line that closes it at column 0 or 4.
fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found"));
    let tail = &source[start..];
    let end = tail
        .find("\n}\n")
        .or_else(|| tail.find("\n    }\n"))
        .unwrap_or_else(|| panic!("`{signature}` has no end"));
    &tail[..end]
}

#[test]
fn profile_maps_to_level() {
    for (profile, level) in [
        ("quick", ReasoningEffort::Low),
        ("brief", ReasoningEffort::Low),
        ("deep", ReasoningEffort::High),
        ("extended", ReasoningEffort::High),
        ("thorough", ReasoningEffort::High),
        ("standard", ReasoningEffort::Unspecified),
        ("", ReasoningEffort::Unspecified),
    ] {
        assert_eq!(effort_level(profile), level, "{profile:?}");
    }
}

#[test]
fn canonical_names_pass_through() {
    for (name, level) in [
        ("minimal", ReasoningEffort::Minimal),
        ("low", ReasoningEffort::Low),
        ("medium", ReasoningEffort::Medium),
        ("high", ReasoningEffort::High),
        ("xhigh", ReasoningEffort::Xhigh),
        ("max", ReasoningEffort::Max),
    ] {
        assert_eq!(effort_level(name), level, "{name}");
    }
}

#[test]
fn unknown_effort_is_standard_and_counted() {
    assert_eq!(effort_level("wat"), ReasoningEffort::Unspecified);
    let thinking = read("src/thinking.rs");
    let body = function_body(&thinking, "pub fn effort_level(");
    let unknown_arm = body
        .split("_ => {")
        .nth(1)
        .expect("`effort_level` has a catch-all arm");
    assert!(
        unknown_arm.contains("gateway_effort_unknown_total") && unknown_arm.contains("warn!"),
        "an unrecognised profile must be counted and logged, not silently dropped"
    );
}

#[test]
fn flag_off_sends_legacy_budget_only() {
    for profile in ["quick", "deep", "standard", "high", "wat"] {
        for ceiling in [0, 2047, 4096, 8192] {
            let legacy = model_gateway::thinking::budget_tokens(profile, ceiling);
            let planned = plan(profile, ceiling, false);
            assert_eq!(planned.budget_tokens, legacy, "{profile} at {ceiling}");
            assert_eq!(planned.effort, ReasoningEffort::Unspecified);
            assert_eq!(planned.effort_wire(), 0);
            assert_eq!(planned.allowance_tokens, 0);
        }
    }
    assert_eq!(
        source_review_plan(false),
        Plan {
            budget_tokens: 4_096,
            ..Plan::NONE
        }
    );
}

#[test]
fn flag_on_sends_level_and_zero_budget() {
    for (profile, level) in [
        ("quick", ReasoningEffort::Low),
        ("deep", ReasoningEffort::High),
        ("max", ReasoningEffort::Max),
    ] {
        for ceiling in [2047, 4096, 8192] {
            let planned = plan(profile, ceiling, true);
            assert_eq!(planned.effort, level, "{profile}");
            assert_eq!(planned.effort_wire(), level as i32);
            assert_eq!(planned.budget_tokens, 0, "the level wins; no budget beside it");
        }
    }
    assert_eq!(plan("standard", 4096, true), Plan::NONE);
    let review = source_review_plan(true);
    assert_eq!(review.effort, ReasoningEffort::High);
    assert_eq!(review.budget_tokens, 0);
}

#[test]
fn only_the_literal_true_enables_the_ladder() {
    assert!(parse_ladder(Some("true")));
    for raw in [None, Some(""), Some("1"), Some("on"), Some("TRUE"), Some("enabled")] {
        assert!(!parse_ladder(raw), "{raw:?}");
    }
    let thinking = read("src/thinking.rs");
    assert!(
        thinking.contains("pub const LADDER_ENV: &str = \"MODEL_GATEWAY_EFFORT_LADDER\";"),
        "the switch is named by the plan"
    );
}

/// The streamed turn, the ZDR turn and the source check all derive their
/// reasoning request through `thinking`, and none derives a budget itself.
#[test]
fn normal_and_zdr_paths_derive_the_same_level() {
    let sse = read("src/sse.rs");
    let normal = sse
        .find("let thinking_plan = crate::thinking::plan(")
        .map(|at| &sse[at..at + 400])
        .expect("the streamed turn derives its plan through `thinking::plan`");
    assert!(normal.contains("req.effort"), "{normal}");

    let zdr = function_body(&sse, "fn zdr_turn_settings_with(");
    assert!(
        zdr.contains("crate::thinking::plan(") && zdr.contains("req.effort"),
        "the ZDR turn must use the same `thinking::plan` on the same request field"
    );

    let validation = read("src/source_validation.rs");
    assert!(
        validation.contains("crate::thinking::source_review_plan("),
        "the source check derives its reasoning request through `thinking`"
    );

    for (name, source) in [
        ("sse.rs", &sse),
        ("source_validation.rs", &validation),
        ("http_routes.rs", &read("src/http_routes.rs")),
    ] {
        assert!(
            !source.contains("thinking::budget_tokens("),
            "{name} derives a token budget itself instead of calling `thinking::plan`"
        );
    }

    // Both turns put the plan on the wire through the same two fields.
    assert!(sse.contains("reasoning_effort: thinking_plan.effort_wire(),"));
    assert!(sse.contains("reasoning_effort: thinking.effort_wire(),"));
    assert!(read("src/http_routes.rs").contains("reasoning_effort: settings.thinking.effort_wire(),"));
}

/// A Do or Plan run's level is the chat turn's derivation applied to the same clamped
/// profile: the launch reads the field the org cap has already clamped, through
/// `thinking::run_level`, and sse.rs names no profile of its own.
#[test]
fn do_run_level_uses_the_one_derivation() {
    let sse = read("src/sse.rs");
    let clamp = sse
        .find("admitted.clamp_effort(&mut req.effort")
        .expect("the org cap clamps the request's effort");
    let launch = sse
        .find("return agentic_run_stream(")
        .expect("the agentic run is launched from the invoke handler");
    assert!(
        clamp < launch,
        "the run must read the clamped effort, not the one the client sent"
    );
    let call = &sse[launch..];
    let call = &call[..call.find("\n        );\n").expect("the launch call ends")];
    let call: String = call.split_whitespace().collect();
    assert!(
        call.contains(
            "crate::thinking::run_level(req.effort.as_deref().unwrap_or_default(),crate::thinking::ladder_enabled(),)"
        ),
        "the launch carries the level from `thinking::run_level`"
    );
    // A call is `effort_level(` standing alone, not the tail of a longer name: sse.rs has a
    // test called `the_effort_level()`, and that is not a derivation.
    let called = sse.match_indices("effort_level(").any(|(at, _)| {
        !sse[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    });
    assert!(
        !sse.contains("\"deep\" =>") && !called,
        "sse.rs names no profile: the mapping lives in `thinking` alone"
    );
}

/// A derived request that clears the budget must clear the level too, or a
/// repair round inherits the author's reasoning.
#[test]
fn a_repair_round_clears_the_level_with_the_budget() {
    let validation = read("src/source_validation.rs");
    assert!(
        validation.contains("repair.thinking_budget_tokens = 0;\n        repair.reasoning_effort = 0;"),
        "the private repair request must not inherit the author's effort level"
    );
}

#[test]
fn truncation_budget_includes_the_allowance() {
    assert_eq!(raised_ceiling(4096, 0), 4096);
    assert_eq!(raised_ceiling(4096, reasoning_allowance(ReasoningEffort::High)), 8192);
    assert_eq!(raised_ceiling(30_000, 16_384), 32_768, "the total cap bounds the raise");
    assert_eq!(raised_ceiling(40_000, 4_096), 40_000, "a caller's own ceiling is never lowered");

    let sse = read("src/sse.rs");
    let body = function_body(&sse, "fn truncation_budget(");
    assert!(body.contains("crate::thinking::raised_ceiling("), "{body}");
    // Every call site passes an allowance, so a Deep answer's reasoning tokens
    // cannot score it as truncated.
    for line in sse.lines().filter(|line| line.contains("truncation_budget(")) {
        let arguments = line.split("truncation_budget(").nth(1).unwrap_or_default();
        assert!(
            arguments.contains(", "),
            "a `truncation_budget` call without an allowance: {line}"
        );
    }
    assert_eq!(allowance_for_wire(ReasoningEffort::Xhigh as i32), 8_192);
    assert_eq!(allowance_for_wire(0), 0);
    assert_eq!(allowance_for_wire(99), 0);
}

/// The gateway's allowance table is a copy of inference-core's, because the
/// gateway does not depend on it. If either moves, this is the alarm.
#[test]
fn allowance_table_matches_inference_core() {
    let effort = read("../inference-core/src/provider/effort.rs");
    let body = function_body(&effort, "pub fn allowance(");
    let rows: Vec<(&str, i32)> = body
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (level, value) = line.strip_prefix("Level::")?.split_once(" => ")?;
            let value = value.trim_end_matches(',').replace('_', "").parse().ok()?;
            Some((level, value))
        })
        .collect();
    assert_eq!(rows.len(), 6, "inference-core lists six levels: {rows:?}");
    for (level, tokens) in rows {
        let ours = match level {
            "Minimal" => ReasoningEffort::Minimal,
            "Low" => ReasoningEffort::Low,
            "Medium" => ReasoningEffort::Medium,
            "High" => ReasoningEffort::High,
            "XHigh" => ReasoningEffort::Xhigh,
            "Max" => ReasoningEffort::Max,
            other => panic!("inference-core has a level the gateway does not know: {other}"),
        };
        assert_eq!(reasoning_allowance(ours), tokens, "{level}");
    }
    assert!(
        effort.contains("pub const TOTAL_TOKEN_CAP: i32 = 32_768;"),
        "the gateway's total cap mirrors inference-core's"
    );
}

#[test]
fn list_models_carries_effort_levels() {
    let routes = read("src/http_routes.rs");
    let descriptor = function_body(&routes, "struct ModelDescriptor {");
    assert!(
        descriptor.contains("effort_levels: Vec<String>,")
            && descriptor.contains("skip_serializing_if = \"Vec::is_empty\""),
        "`/v1/models` lists the levels, and only when the model has any"
    );
    assert!(
        routes.contains("effort_levels: m.effort_levels,"),
        "the descriptor is filled from inference-core's ModelInfo"
    );
}

/// The record fields reach `/v1/models` the way the effort levels do: the descriptor
/// carries each one, leaves it out when unknown, and is filled from inference-core's
/// `ModelInfo`. The tags are the ones the plan reserves for `ModelInfo` (EP-13 S2).
#[test]
fn list_models_carries_context_and_lifecycle() {
    let routes = read("src/http_routes.rs");
    let descriptor = function_body(&routes, "struct ModelDescriptor {");
    assert!(
        descriptor.contains("context_window_tokens: Option<u32>,")
            && descriptor.contains("max_output_tokens: Option<u32>,")
            && descriptor.contains("lifecycle: String,")
            && descriptor
                .matches("skip_serializing_if = \"Option::is_none\"")
                .count()
                >= 2
            && descriptor.contains("skip_serializing_if = \"String::is_empty\""),
        "`/v1/models` lists the record fields, and leaves each one out when it is unknown"
    );
    assert!(
        routes.contains("context_window_tokens: known_tokens(m.context_window_tokens),")
            && routes.contains("max_output_tokens: known_tokens(m.max_output_tokens),")
            && routes.contains("lifecycle: m.lifecycle,"),
        "the descriptor is filled from inference-core's ModelInfo, with zero read as unknown"
    );
    let proto = read("../../../proto/model_plane/v1/inference.proto");
    for declaration in [
        "uint32 context_window_tokens = 9;",
        "uint32 max_output_tokens = 10;",
        "string lifecycle = 11;",
    ] {
        assert!(proto.contains(declaration), "ModelInfo must declare `{declaration}`");
    }
}

/// Additivity of `inference.proto`, proved by tag number rather than by
/// `buf breaking` (vacuous until its baseline is committed): the new fields sit
/// at the tags the plan reserves, and every field that existed before keeps
/// its tag.
#[test]
fn proto_is_additive_at_the_reserved_tags() {
    let proto = read("../../../proto/model_plane/v1/inference.proto");
    let fields = |message: &str| -> Vec<(String, u32)> {
        let start = proto
            .find(&format!("\nmessage {message} {{"))
            .unwrap_or_else(|| panic!("message {message} not found"));
        let tail = &proto[start + 1..];
        let end = tail.find("\n}\n").expect("message end");
        tail[..end]
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.starts_with("//") || !line.ends_with(';') {
                    return None;
                }
                let (declaration, tag) = line.trim_end_matches(';').rsplit_once(" = ")?;
                let name = declaration.split_whitespace().last()?;
                Some((name.to_owned(), tag.trim().parse().ok()?))
            })
            .collect()
    };

    let infer = fields("InferRequest");
    let tag_of = |set: &[(String, u32)], name: &str| {
        set.iter()
            .find(|(field, _)| field == name)
            .map(|(_, tag)| *tag)
            .unwrap_or_else(|| panic!("field {name} missing"))
    };
    assert_eq!(tag_of(&infer, "reasoning_effort"), 17);
    for (name, tag) in [
        ("request_id", 1),
        ("org_id", 2),
        ("model", 3),
        ("provider_hint", 4),
        ("messages", 5),
        ("temperature", 6),
        ("max_tokens", 7),
        ("structured_output_schema", 8),
        ("zdr", 9),
        ("tools", 10),
        ("tool_choice", 11),
        ("min_residency", 12),
        ("thinking_budget_tokens", 13),
        ("min_privacy_tier", 14),
        ("subscription_connection_id", 15),
        ("prefer_priority_service_tier", 16),
    ] {
        assert_eq!(tag_of(&infer, name), tag, "InferRequest.{name} moved");
    }

    let model = fields("ModelInfo");
    assert_eq!(tag_of(&model, "effort_levels"), 8);
    for (name, tag) in [
        ("id", 1),
        ("provider", 2),
        ("modality", 3),
        ("streaming", 4),
        ("features", 5),
        ("privacy_tier", 6),
        ("residency", 7),
    ] {
        assert_eq!(tag_of(&model, name), tag, "ModelInfo.{name} moved");
    }

    // The enum numbers are the wire: UNSPECIFIED must stay 0 and the ladder
    // must stay weakest-first.
    for (value, number) in [
        (ReasoningEffort::Unspecified, 0),
        (ReasoningEffort::Minimal, 1),
        (ReasoningEffort::Low, 2),
        (ReasoningEffort::Medium, 3),
        (ReasoningEffort::High, 4),
        (ReasoningEffort::Xhigh, 5),
        (ReasoningEffort::Max, 6),
    ] {
        assert_eq!(value as i32, number);
    }
}

/// inference-core refuses a level it cannot interpret instead of weakening the
/// request; the gateway side of that is that it never sends one.
#[test]
fn inference_core_maps_the_level_and_refuses_unknown_values() {
    let grpc = read("../inference-core/src/grpc.rs");
    let mapper = function_body(&grpc, "fn effort_from_wire(");
    assert!(mapper.contains("Status::invalid_argument("), "{mapper}");
    assert!(
        grpc.contains("reasoning_effort: effort_from_wire(req.reasoning_effort)?,"),
        "the request conversion must carry the level"
    );
    assert!(
        grpc.contains("effort_levels: model"),
        "the catalogue conversion must carry the advertised levels"
    );
}
