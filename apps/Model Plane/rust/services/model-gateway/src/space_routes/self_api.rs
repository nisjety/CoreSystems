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

/// The feature the gateway grants for the room canvas tools (`domains/spaces/agent_canvas_tools.rs`, W3-DEC-CANVAS).
pub(crate) const SPACE_CANVAS_TOOLS_FEATURE: &str = "space_canvas_tools";

/// The `RunAgentRequest.tool_profile` marker for the room canvas rows (`space.canvas.read`, `space.canvas.write`). It
/// is offered only under this marker, and never under the self-API marker alone.
pub(crate) const SPACE_CANVAS_TOOLS_TOOL_PROFILE: &str = "space_canvas_tools";

/// Joins the markers of one run's profile (`space_self_api+space_canvas_tools`). Execution-core reads the same grammar
/// (`space_self_api::profile_markers`), and any other value offers no Space tool.
pub(crate) const PROFILE_JOIN: &str = "+";

/// Model Gateway's own switch for honouring the feature. Off by default and in production; read as the
/// `switch_value_is_on` spellings (`1`, `true`, `on`, `yes`).
const SPACE_SELF_API_VAR: &str = "MODEL_GATEWAY_SPACE_SELF_API";

pub(crate) fn space_self_api_enabled() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(
        std::env::var(SPACE_SELF_API_VAR).ok().as_deref(),
    )
}

/// Model Gateway's own switch for honouring the canvas feature (W3-DEC-CANVAS). Off by default and in production;
/// read as the same `switch_value_is_on` spellings as the self-API switch.
const SPACE_CANVAS_TOOLS_VAR: &str = "MODEL_GATEWAY_SPACE_CANVAS_TOOLS";

pub(crate) fn space_canvas_tools_enabled() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(
        std::env::var(SPACE_CANVAS_TOOLS_VAR).ok().as_deref(),
    )
}

/// The tool profile of one run (pure). Empty unless the turn is agentic and Space-scoped and the run is neither
/// zero-retention nor waived. Each marker is added only when its own switch is on and the gateway granted its feature:
/// `space_self_api` for the thread rows, `space_canvas_tools` for the canvas rows. Both give the joined profile, and
/// execution-core offers each row set only under its own marker.
pub(crate) fn tool_profile_for_run(
    self_api_switch_on: bool,
    canvas_tools_switch_on: bool,
    features: &[String],
    space_id: &str,
    zdr: bool,
    waived: bool,
) -> String {
    let agentic = features.iter().any(|feature| feature == "agentic");
    let granted = |name: &str| features.iter().any(|feature| feature == name);
    if !agentic || zdr || waived || space_id.trim().is_empty() {
        return String::new();
    }
    let mut markers: Vec<&str> = Vec::new();
    if self_api_switch_on && granted(SPACE_SELF_API_FEATURE) {
        markers.push(SPACE_SELF_API_TOOL_PROFILE);
    }
    if canvas_tools_switch_on && granted(SPACE_CANVAS_TOOLS_FEATURE) {
        markers.push(SPACE_CANVAS_TOOLS_TOOL_PROFILE);
    }
    markers.join(PROFILE_JOIN)
}
