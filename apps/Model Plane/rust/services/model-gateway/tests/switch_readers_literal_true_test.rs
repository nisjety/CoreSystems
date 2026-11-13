//! Verification plan 3.4, the rest of model-gateway's W2-W5 switches (coding-complete section 6) that
//! `switch_literal_true_test.rs` does not read: each is "Only the literal true". Read through the reader the
//! production path calls, each one turns on for `true`, stays off for every other spelling (another case, `1`, `on`,
//! `yes`, empty), and is off again once the variable is removed, so turning a switch off needs no restart and leaves
//! the feature where it was before the switch existed. This binary sets the variables itself and runs its one test
//! alone, so no other test sees them.
//!
//! `MODEL_GATEWAY_TEACH_MODE_ENABLED` and `MODEL_GATEWAY_REVIEW_RUN_ENABLED` are registered as "the literal true, in
//! any case" and are not read here; `MODEL_GATEWAY_MCP_PROBE` is read once per process and its parser is pinned below
//! without the environment.

use model_gateway::{
    budget, goal_judge, image_tool, mcp_credential_ref, mcp_probe, normalize, retrieval_tools,
    web_bundle, zdr_posture,
};

fn cap_min() -> bool {
    budget::cap_min_enabled(&|key| std::env::var(key).ok())
}

#[test]
fn only_the_literal_true_turns_these_switches_on_and_removing_it_turns_them_off() {
    let readers: [(&str, fn() -> bool); 8] = [
        ("MODEL_GATEWAY_CAP_MIN", cap_min),
        (
            "MODEL_GATEWAY_GOAL_JUDGE_ENABLED",
            goal_judge::goal_judge_enabled,
        ),
        ("MODEL_GATEWAY_IMAGE_TOOL", image_tool::enabled),
        ("MCP_SECRET_REF_AUTH_ENABLED", mcp_credential_ref::enabled),
        ("MODEL_GATEWAY_BYO_GATEWAY", normalize::byo_gateway_enabled),
        (
            "MODEL_GATEWAY_DATA_SOURCE_CATALOGUE",
            retrieval_tools::enabled,
        ),
        ("MODEL_GATEWAY_WEB_BUNDLE_ARTIFACTS", web_bundle::enabled),
        (
            "ZDR_TYPED_REFUSALS_ENABLED",
            zdr_posture::typed_refusals_enabled,
        ),
    ];
    let mut wrong = Vec::new();
    for (name, enabled) in readers {
        std::env::remove_var(name);
        if enabled() {
            wrong.push(format!("{name} unset is on"));
        }
        std::env::set_var(name, "true");
        if !enabled() {
            wrong.push(format!("{name}=true is off"));
        }
        for value in ["TRUE", "True", "1", "on", "yes", "enabled", "", "false"] {
            std::env::set_var(name, value);
            if enabled() {
                wrong.push(format!("{name}={value:?} is on"));
            }
        }
        std::env::set_var(name, "true");
        std::env::remove_var(name);
        if enabled() {
            wrong.push(format!("{name} removed after true is still on"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn the_mcp_probe_parser_reads_only_the_literal_true() {
    assert!(mcp_probe::enabled_from_env_value(Some("true")));
    for value in [
        None,
        Some(""),
        Some("TRUE"),
        Some("True"),
        Some("1"),
        Some("on"),
        Some("yes"),
    ] {
        assert!(!mcp_probe::enabled_from_env_value(value), "{value:?}");
    }
}
