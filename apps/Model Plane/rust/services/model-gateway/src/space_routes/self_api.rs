//! WS18-S5: the model-gateway side of the room agent's self-API switch.
//!
//! The gateway adds the `space_self_api` feature to a mention turn only when the binding's switch is on, the
//! flag is on and the organization is not zero-retention, and always with `agentic` (the gateway's
//! `domains/spaces/self_api.rs`). This module turns that feature into the run's `tool_profile`, which
//! execution-core reads (`RunAgentRequest.tool_profile`, field 28). `space_self_api` offers only the
//! server-resolved Space tools, never the agent's client, MCP or built-in tools.
//!
//! The profile is honoured only for a Space-scoped agentic turn, never for a zero-retention or waived run, and
//! only while `MODEL_GATEWAY_SPACE_SELF_API` is on. Off, or for any other turn, the request is what it was.
//! This switch is never an authorization input: Control's run-action decision decides each action.

/// The feature the gateway grants (`domains/spaces/self_api.rs`).
pub(crate) const SPACE_SELF_API_FEATURE: &str = "space_self_api";

/// The `RunAgentRequest.tool_profile` value execution-core reads for a Space self-API run.
pub(crate) const SPACE_SELF_API_TOOL_PROFILE: &str = "space_self_api";

/// Model Gateway's own switch for honouring the feature. Off by default and in production; read as the
/// `switch_value_is_on` spellings (`1`, `true`, `on`, `yes`).
const SPACE_SELF_API_VAR: &str = "MODEL_GATEWAY_SPACE_SELF_API";

pub(crate) fn space_self_api_enabled() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(
        std::env::var(SPACE_SELF_API_VAR).ok().as_deref(),
    )
}

/// The tool profile of one run (pure). Empty unless the switch is on, the turn is agentic and Space-scoped, the
/// gateway granted the feature, and the run is neither zero-retention nor waived.
pub(crate) fn tool_profile_for_run(
    switch_on: bool,
    features: &[String],
    space_id: &str,
    zdr: bool,
    waived: bool,
) -> String {
    let agentic = features.iter().any(|feature| feature == "agentic");
    let granted = features.iter().any(|feature| feature == SPACE_SELF_API_FEATURE);
    if switch_on && agentic && granted && !zdr && !waived && !space_id.trim().is_empty() {
        SPACE_SELF_API_TOOL_PROFILE.to_owned()
    } else {
        String::new()
    }
}
