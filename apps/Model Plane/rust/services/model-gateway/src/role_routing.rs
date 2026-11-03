//! Whether model-gateway's side tasks ask the platform role table (EP-13 S4).
//!
//! The switch is off unless the env value is the literal `true`, as the other
//! inference switches read it. With it off a side task names the operator's aux
//! model and carries no role fields, the request it sent before the switch existed.
//! With it on a side task names `verevon-role` and carries the turn's own model, and
//! inference-core picks the model from its role table.

use std::sync::OnceLock;

/// The switch for side-task role routing. Read once at startup.
pub const ROLE_ROUTING_ENV: &str = "MODEL_GATEWAY_ROLE_ROUTING";

/// The model id a side task names to ask inference-core's role table. It must match
/// `provider::role::ROLE_MODEL_ID` in inference-core, a separately deployed service.
pub const ROLE_MODEL_ID: &str = "verevon-role";

/// Whether an explicit env value turns role routing on. Pure, so the parser is
/// testable without the process environment.
fn role_routing_enabled_from(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Whether role routing is on for this process.
#[must_use]
pub fn role_routing_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        role_routing_enabled_from(std::env::var(ROLE_ROUTING_ENV).ok().as_deref())
    })
}

/// The model and the role fallback a side task sends (EP-13 S4). With role routing on
/// it names the role table and carries the turn's own model; with it off it names
/// `aux_model` and carries no fallback.
#[must_use]
pub fn side_task_model(enabled: bool, aux_model: &str, turn_model: &str) -> (String, String) {
    if enabled {
        (ROLE_MODEL_ID.to_owned(), turn_model.to_owned())
    } else {
        (aux_model.to_owned(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_literal_true_turns_role_routing_on() {
        assert!(role_routing_enabled_from(Some("true")));
        assert!(role_routing_enabled_from(Some(" true ")));
        for value in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some("on")] {
            assert!(
                !role_routing_enabled_from(value),
                "{value:?} must not turn role routing on"
            );
        }
    }

    #[test]
    fn side_tasks_name_the_aux_model_with_the_switch_off() {
        assert_eq!(
            side_task_model(false, "gpt-4o-mini", "claude-sonnet-5-5"),
            ("gpt-4o-mini".to_owned(), String::new())
        );
    }

    #[test]
    fn side_tasks_name_the_role_id_and_the_turn_model_with_the_switch_on() {
        assert_eq!(
            side_task_model(true, "gpt-4o-mini", "claude-sonnet-5-5"),
            ("verevon-role".to_owned(), "claude-sonnet-5-5".to_owned())
        );
    }
}
