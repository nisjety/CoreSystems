//! Attended auto-continue of a thread goal (Wave 3 EP-7 S4, owner decisions D4 and D5).
//!
//! With "Keep going" on, the goal bar sends the next continuation turn itself after a visible
//! countdown (D4): each such turn is an ordinary user-present request whose bearer is minted for
//! it, and closing the tab stops the goal. The turn says so with `InvokeRequest.goal_auto`. The
//! field grants nothing: it marks the run for audit (`runs.metadata.goal_auto`, stamped by
//! session-core when the goal turn's run is bound, see [`mark`]) and is refused here unless the
//! feature is on and the turn is a Do turn naming its goal. Admission stays session-core's: every
//! goal turn, pump-sent or not, is claimed, and the claim refuses while an approval, question,
//! proposed plan or park waits on the owner, or past 30 claims per goal per hour (D5).
//!
//! The switch is `MODEL_GATEWAY_GOAL_AUTO_CONTINUE`, default off. The goal bar offers the toggle
//! only when `GET /v1/threads/:thread_id/goal` reports `autoContinueAvailable`, which is true only
//! while this switch, the thread goals switch and the goal judge's switch are all on: without a
//! verdict the pump has nothing to continue on.

/// The switch. Opt-in: only the literal `true` enables it.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_GOAL_AUTO_CONTINUE";

/// The refusal of a pump-sent turn while auto-continue is not available.
pub const DISABLED_CODE: &str = "goal_auto_disabled";

/// The gRPC request header session-core reads when it binds a goal turn's run; it stamps the run
/// `goal_auto`. Only the literal `true` counts. Session-core holds the same name.
pub const GOAL_AUTO_HEADER: &str = "x-goal-auto";

/// Whether the switch is on in this process. Read at each call, as the other turn switches are.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether the pump may run on this deployment: auto-continue, thread goals and the judge are all
/// on. This is what the goal read reports as `autoContinueAvailable`.
#[must_use]
pub fn available() -> bool {
    enabled() && crate::thread_goal_routes::enabled() && crate::goal_judge::goal_judge_enabled()
}

/// The typed refusal of a turn that says it was sent by the pump, or `None`. A turn without the
/// mark is never refused here, so every other turn is what it was before the field existed.
#[must_use]
pub fn refusal(
    goal_auto: bool,
    available: bool,
    agentic: bool,
    goal_id: Option<&str>,
) -> Option<(&'static str, &'static str)> {
    if !goal_auto {
        return None;
    }
    if !available {
        return Some((
            DISABLED_CODE,
            "automatic goal continuation is not enabled on this deployment",
        ));
    }
    // A pump turn continues a goal on a Do turn; anything else is a malformed request.
    if !agentic || goal_id.map_or(true, |id| id.trim().is_empty()) {
        return Some(("invalid_request", "goal_auto needs a Do turn naming its goal"));
    }
    None
}

/// Marks a session-core request as sent by the pump. A turn without the mark sends no header, so
/// its request is byte-identical to before.
pub fn mark<T>(request: &mut tonic::Request<T>, goal_auto: bool) {
    if goal_auto {
        request.metadata_mut().insert(
            GOAL_AUTO_HEADER,
            tonic::metadata::MetadataValue::from_static("true"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_auto_refused_when_switch_off() {
        assert_eq!(
            refusal(true, false, true, Some("goal_1")).map(|(code, _)| code),
            Some(DISABLED_CODE)
        );
        // Off, a turn without the mark is never refused, whatever else it carries.
        assert_eq!(refusal(false, false, true, Some("goal_1")), None);
        assert_eq!(refusal(false, false, false, None), None);
    }

    #[test]
    fn goal_auto_needs_a_do_turn_naming_its_goal() {
        assert_eq!(refusal(true, true, true, Some("goal_1")), None);
        for (agentic, goal_id) in [(false, Some("goal_1")), (true, None), (true, Some("  "))] {
            assert_eq!(
                refusal(true, true, agentic, goal_id).map(|(code, _)| code),
                Some("invalid_request"),
                "{agentic} {goal_id:?}"
            );
        }
    }

    #[test]
    fn goal_auto_switch_reads_literal_true_only() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("yes")));
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    #[test]
    fn goal_auto_marks_only_a_pump_turn() {
        let mut plain = tonic::Request::new(());
        mark(&mut plain, false);
        assert!(plain.metadata().get(GOAL_AUTO_HEADER).is_none());
        let mut pumped = tonic::Request::new(());
        mark(&mut pumped, true);
        assert_eq!(
            pumped
                .metadata()
                .get(GOAL_AUTO_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
    }

    #[test]
    fn goal_auto_invoke_field_defaults_off() {
        let plain: crate::http_routes::InvokeRequest =
            serde_json::from_value(serde_json::json!({"content": "hi"})).unwrap();
        assert!(!plain.goal_auto);
        let pumped: crate::http_routes::InvokeRequest = serde_json::from_value(
            serde_json::json!({"content": "hi", "goalId": "goal_1", "goalAuto": true}),
        )
        .unwrap();
        assert!(pumped.goal_auto);
    }

    #[test]
    fn goal_auto_check_sits_before_the_run_is_prepared() {
        let source = include_str!("sse.rs").replace("\r\n", "\n");
        let start = source
            .find("pub async fn invoke_stream_sse(")
            .expect("the invoke handler");
        let rest = &source[start..];
        let body = &rest[..rest.find("\n}\n").expect("the end of the handler")];
        let check = body
            .find("crate::goal_auto::refusal(")
            .expect("the goal_auto check");
        let prepare = body
            .find("prepare_managed_run_authenticated_with_receipt(")
            .expect("the run preparation");
        assert!(check < prepare, "a goal_auto refusal leaves no run behind");
    }
}
