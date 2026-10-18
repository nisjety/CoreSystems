//! Effect classification and the ledger calls around a dispatch (build plan WS03 S5).
//!
//! The ledger records what a call may change, and the loop that resumes a run
//! decides whether it may run a call again from that class. The class is derived
//! from the same per-call classifier the autonomy ladder already uses, never from a
//! second table: two tables would let a call be an effect to one gate and a read
//! to the other, and that only shows up as a duplicated side effect. WS05 will
//! replace this with declared tool classes; until then this is the stub.
//!
//! The ledger is off unless `EXECUTION_EFFECT_LEDGER_ENABLED` is `true`. When it is
//! on, an effectful call is reserved before it is dispatched and finalized after,
//! and a call that cannot be reserved is not run: a call whose effect cannot be
//! recorded ahead of time is not one to run blind.

use mp_contracts::effect_ledger::EffectClass;
use mp_contracts::model_plane::v1::{self as pb, session_core_client::SessionCoreClient, AutonomyRung};
use tonic::transport::Channel;

const LEDGER_ENV: &str = "EXECUTION_EFFECT_LEDGER_ENABLED";

#[cfg(test)]
thread_local! {
    /// A test's override of the switch. `#[tokio::test]` runs its future on the
    /// test's own thread, so a thread-local cannot leak into a parallel test the
    /// way a process-wide environment variable would.
    static TEST_SWITCH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Holds the switch at a fixed value for the current test thread, then restores it.
#[cfg(test)]
pub(crate) struct LedgerSwitch;

#[cfg(test)]
impl LedgerSwitch {
    pub(crate) fn set(on: bool) -> Self {
        TEST_SWITCH.with(|cell| cell.set(Some(on)));
        Self
    }
}

#[cfg(test)]
impl Drop for LedgerSwitch {
    fn drop(&mut self) {
        TEST_SWITCH.with(|cell| cell.set(None));
    }
}

/// Whether the effect ledger is switched on. Off by default.
pub(crate) fn ledger_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_SWITCH.with(std::cell::Cell::get) {
        return forced;
    }
    std::env::var(LEDGER_ENV).is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

/// The effect class of one call, from the tool and its actual arguments.
///
/// The rung a call needs says how far it reaches: the widest rung is a
/// destructive or outbound action, which can leave the platform (`External`); the
/// middle rung is a durable write inside the run's own context (`Write`); anything
/// else is a read. A call the classifier cannot place is already treated as the
/// widest rung, so an unrecognised tool is never mistaken for a read.
pub(crate) fn effect_class_of(tool_name: &str, tool_input: &str) -> EffectClass {
    match crate::permission::rung_required_for(tool_name, tool_input) {
        AutonomyRung::DangerFullAccess => EffectClass::External,
        AutonomyRung::WorkspaceWrite => EffectClass::Write,
        _ => EffectClass::Read,
    }
}

/// The digest the ledger stores for a call's arguments, or `None` when they are not
/// JSON. Only the digest is stored; the arguments never leave the loop.
pub(crate) fn args_digest_of(tool_input: &str) -> Option<String> {
    mp_contracts::effect_ledger::args_digest(tool_input)
}

/// The ledger's action id for a step: the step id itself when the ledger will accept
/// it, otherwise a digest of it. A step id embeds the model's call id, which the
/// ledger does not bound the way it bounds its own tokens.
pub(crate) fn action_id_for(step_id: &str) -> String {
    let plain = !step_id.is_empty()
        && step_id.len() <= 128
        && step_id == step_id.trim()
        && !step_id.chars().any(char::is_control);
    if plain {
        step_id.to_owned()
    } else {
        format!("act-{}", &blake3::hash(step_id.as_bytes()).to_hex()[..40])
    }
}

/// The outcome the ledger records for a step that ended with `status`.
///
/// A call the gate stopped before its body ran did not run. A call that finished
/// reports its own result. Anything else, a timeout above all, is an outcome nobody
/// observed: the honest record is that it is unknown.
pub(crate) fn final_outcome_for(status: &str) -> &'static str {
    match status {
        "completed" => "completed",
        "failed" => "failed",
        "awaiting_approval" | "permission_denied" => "not_run",
        _ => "unknown_outcome",
    }
}

/// What the ledger records about one call: ids and a class, never arguments.
pub(crate) struct LedgerEntry<'a> {
    pub run_id: &'a str,
    pub action_id: String,
    pub tool: &'a str,
    pub data_category: &'a str,
    pub zdr: bool,
    pub class: EffectClass,
    pub args_digest: Option<String>,
}

/// What a reservation did. The ledger answers `created: false` for an exact
/// replay of an earlier reservation, and that answer is not "go ahead": it says an
/// earlier attempt, possibly of a loop that has since died, already claimed this
/// action and may have started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReserveOutcome {
    /// This call made the reservation: nothing has started the action before.
    Created,
    /// The reservation already existed. An earlier attempt may have run the call,
    /// so it is never dispatched again; it is closed as an unknown outcome.
    AlreadyReserved,
}

impl ReserveOutcome {
    /// From the response's `created` flag.
    pub(crate) const fn from_created(created: bool) -> Self {
        if created {
            Self::Created
        } else {
            Self::AlreadyReserved
        }
    }
}

/// Reserve a call before it is dispatched. The error is the reason, for the log.
pub(crate) async fn reserve(
    channel: &Channel,
    bearer: Option<&str>,
    entry: &LedgerEntry<'_>,
) -> Result<ReserveOutcome, String> {
    let bearer = bearer.ok_or_else(|| "no verified session credential".to_owned())?;
    let request = super::authenticated_session_request(
        pb::ReserveToolActionRequest {
            run_id: entry.run_id.to_owned(),
            action_id: entry.action_id.clone(),
            // The run is the request: a run's agent loop has no per-call gateway request.
            request_id: entry.run_id.to_owned(),
            tool: entry.tool.to_owned(),
            data_category: entry.data_category.to_owned(),
            zdr: entry.zdr,
            args_digest: entry.args_digest.clone().unwrap_or_default(),
            effect_class: entry.class.as_str().to_owned(),
        },
        bearer,
    )?;
    SessionCoreClient::new(channel.clone())
        .reserve_tool_action(request)
        .await
        .map(|response| ReserveOutcome::from_created(response.into_inner().created))
        .map_err(|status| status.message().to_owned())
}

/// Record how a reserved call ended. A failure leaves the row `reserved`, which the
/// session-core sweep closes as `unknown_outcome`: honest, and never a replay.
pub(crate) async fn finalize(
    channel: &Channel,
    bearer: Option<&str>,
    run_id: &str,
    action_id: &str,
    outcome: &str,
) -> Result<(), String> {
    let bearer = bearer.ok_or_else(|| "no verified session credential".to_owned())?;
    let request = super::authenticated_session_request(
        pb::FinalizeToolActionRequest {
            run_id: run_id.to_owned(),
            action_id: action_id.to_owned(),
            outcome: outcome.to_owned(),
            receipt_id: String::new(),
        },
        bearer,
    )?;
    SessionCoreClient::new(channel.clone())
        .finalize_tool_action(request)
        .await
        .map(|_| ())
        .map_err(|status| status.message().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destructive_or_outbound_call_is_external() {
        for tool in ["delete_records", "send_email", "execute_provider_action", "browser_agent"] {
            assert_eq!(effect_class_of(tool, "{}"), EffectClass::External, "{tool}");
        }
    }

    #[test]
    fn a_provider_read_is_not_external_but_a_provider_write_is() {
        // `execute_provider_action` is classified by its operation, not its name.
        assert_eq!(
            effect_class_of("execute_provider_action", r#"{"operation":"pages.list"}"#),
            EffectClass::Read,
            "a catalogued read is a read"
        );
        assert_eq!(
            effect_class_of("execute_provider_action", r#"{"operation":"pages.post"}"#),
            EffectClass::External,
            "a catalogued write reaches outside the platform"
        );
        assert_eq!(
            effect_class_of("execute_provider_action", "{}"),
            EffectClass::External,
            "a call with no operation fails safe"
        );
    }

    #[test]
    fn a_memory_write_is_a_write_and_a_lookup_is_a_read() {
        assert_eq!(effect_class_of("save_memory", "{}"), EffectClass::Write);
        assert_eq!(effect_class_of("knowledge_search", "{}"), EffectClass::Read);
        assert_eq!(effect_class_of("recall_memory", "{}"), EffectClass::Read);
    }

    #[test]
    fn an_external_tool_is_never_mistaken_for_a_read() {
        assert_eq!(
            effect_class_of("mcp__crm__lookup_contact", "{}"),
            EffectClass::External
        );
    }

    #[test]
    fn the_digest_is_stable_and_only_exists_for_json_arguments() {
        let digest = args_digest_of(r#"{"b":1,"a":2}"#).expect("digest");
        assert_eq!(args_digest_of(r#"{"a":2,"b":1}"#).as_deref(), Some(digest.as_str()));
        assert_eq!(args_digest_of("not json"), None);
    }

    #[test]
    fn a_replayed_reservation_is_not_a_fresh_one() {
        assert_eq!(ReserveOutcome::from_created(true), ReserveOutcome::Created);
        assert_eq!(
            ReserveOutcome::from_created(false),
            ReserveOutcome::AlreadyReserved,
            "created: false means an earlier attempt already claimed the action"
        );
    }

    #[test]
    fn a_step_id_the_ledger_accepts_is_used_as_it_is() {
        assert_eq!(action_id_for("tool_1_call-1"), "tool_1_call-1");
    }

    #[test]
    fn a_step_id_the_ledger_would_refuse_becomes_a_digest_of_itself() {
        for hostile in [
            "x".repeat(200),
            " padded ".to_owned(),
            "has\nnewline".to_owned(),
            String::new(),
        ] {
            let id = action_id_for(&hostile);
            assert!(id.starts_with("act-"), "{hostile:?}");
            assert!(id.len() <= 128);
            assert!(!id.chars().any(char::is_control));
            // The same step id always maps to the same action id.
            assert_eq!(id, action_id_for(&hostile));
        }
        assert_ne!(action_id_for(&"a".repeat(200)), action_id_for(&"b".repeat(200)));
    }

    #[test]
    fn a_step_status_maps_to_the_honest_ledger_outcome() {
        assert_eq!(final_outcome_for("completed"), "completed");
        assert_eq!(final_outcome_for("failed"), "failed");
        // Stopped by a gate before its body ran: it did not run.
        assert_eq!(final_outcome_for("awaiting_approval"), "not_run");
        assert_eq!(final_outcome_for("permission_denied"), "not_run");
        // Nobody saw how these ended, so the record says so.
        for status in ["timed_out", "resource_exhausted", "anything_else", ""] {
            assert_eq!(final_outcome_for(status), "unknown_outcome", "{status:?}");
        }
    }

    #[test]
    fn every_outcome_the_loop_records_is_one_the_ledger_accepts() {
        for status in [
            "completed",
            "failed",
            "awaiting_approval",
            "permission_denied",
            "timed_out",
            "unknown",
        ] {
            assert!(mp_contracts::effect_ledger::is_final_outcome(final_outcome_for(status)));
        }
    }

    #[test]
    fn the_switch_follows_the_environment_unless_a_test_forces_it() {
        {
            let _on = LedgerSwitch::set(true);
            assert!(ledger_enabled());
        }
        {
            let _off = LedgerSwitch::set(false);
            assert!(!ledger_enabled());
        }
    }
}
