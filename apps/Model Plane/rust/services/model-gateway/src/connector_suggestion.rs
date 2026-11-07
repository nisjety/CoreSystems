//! The connected-app suggestion in the Ask loop (Wave 3 EP-10 S8, decision D6).
//!
//! When a question needs the person's own mail, calendar or files and no connected account is
//! selected, the model may call `suggest_connector` once per turn. The call reads nothing and
//! grants nothing: it becomes one `connector_suggestion` card on the stream, and the turn goes on
//! without that data. The card's two paths (connect, or select for this question) run in the SPA on
//! its existing connect routes, so this module adds no new call to any other plane.
//!
//! The offer, the dispatch arm and the one-per-turn rule live here; the argument contract is
//! `mp_contracts::connector_suggest`. It is declared from `tool_loop.rs` as `connector_reads` is.

use mp_contracts::connector_suggest;
use mp_contracts::model_plane::v1::{ToolCall, ToolDefinition};

use super::connector_reads::ConnectorTurn;
use super::{err_outcome, ToolOutcome};
use crate::moderation::ToolProvenance;
use crate::sse_events::ChatEvent;

/// The switch. Only the literal `true` offers the suggestion.
pub const ENABLED_ENV: &str = "CONNECTOR_SUGGESTIONS_ENABLED";
/// What the model reads after a suggestion was shown.
const SHOWN: &str =
    "Shown to the person. Answer without that data and say what you could do with it.";
/// The tool error of a second suggestion in one turn.
const ALREADY_SUGGESTED: &str = "already suggested: a turn shows one connector suggestion; answer without that data";
const NOT_AVAILABLE: &str = "suggest_connector is not available on this turn";

/// Whether the deployment has switched the suggestion on.
#[must_use]
pub fn parse_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the switch is on in this process, read on every offer so turning it off needs no restart.
#[must_use]
pub fn enabled() -> bool {
    parse_enabled(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether the Ask loop offers the suggestion on this turn. It needs the switch, a turn that is
/// neither zero-retention (a temporary chat is one) nor in a Space, a turn that reads every source,
/// and no connected account selected. The selection names connections, not providers, so any
/// selection holds the card back: a person who chose an account for this turn has already answered
/// the question the card would ask.
#[must_use]
pub fn offered(enabled: bool, zdr: bool, space_scoped: bool, turn: Option<&ConnectorTurn>) -> bool {
    enabled
        && !zdr
        && !space_scoped
        && turn.is_some_and(|turn| turn.all_sources && turn.selection.is_empty())
}

/// The server's own definition of the tool.
#[must_use]
pub fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: connector_suggest::TOOL_NAME.to_owned(),
        description: connector_suggest::TOOL_DESCRIPTION.to_owned(),
        parameters_json: connector_suggest::tool_parameters_json(),
    }
}

/// The suggestion's place in a turn's tool set. A client's copy of the name is always dropped, and
/// the server's definition is added last, so the cached prefix holds and a turn that does not offer
/// the tool lists exactly what it listed before.
#[must_use]
pub fn tools_for_turn(mut tools: Vec<ToolDefinition>, offered: bool) -> Vec<ToolDefinition> {
    tools.retain(|tool| tool.name != connector_suggest::TOOL_NAME);
    if offered {
        tools.push(tool_definition());
    }
    tools
}

/// The dispatch arm. The loop's offered-set check already refused a turn that was not offered the
/// tool; the switch, zero retention and a Space are checked again here for any other caller.
#[must_use]
pub fn outcome(enabled: bool, zdr: bool, space_scoped: bool, call: &ToolCall) -> ToolOutcome {
    if !enabled || zdr || space_scoped {
        return err_outcome(call, NOT_AVAILABLE);
    }
    match connector_suggest::parse_args(&call.arguments_json) {
        Ok(_) => ToolOutcome {
            call_id: call.id.clone(),
            name: call.name.clone(),
            output: SHOWN.to_owned(),
            error: None,
            provenance: ToolProvenance::unscreened(&call.name, SHOWN),
        },
        Err(problem) => err_outcome(call, problem.message()),
    }
}

/// One turn's suggestion state. The calls of a round are dispatched together, so the one-per-turn
/// rule is applied as their outcomes are read back in order, before any result is shown.
#[derive(Debug, Default)]
pub struct SuggestionTurn {
    shown: bool,
}

impl SuggestionTurn {
    /// The card event for the first accepted suggestion of the turn. A later one is turned into a
    /// tool error, so the model is told and the person sees one card. Any other outcome is left
    /// alone. `call` is the call the outcome answers; the arguments are parsed again from it so the
    /// event carries exactly what the validator accepted.
    pub fn settle(&mut self, outcome: &mut ToolOutcome, call: Option<&ToolCall>) -> Option<ChatEvent> {
        if outcome.name != connector_suggest::TOOL_NAME || outcome.error.is_some() {
            return None;
        }
        if self.shown {
            refuse(outcome, ALREADY_SUGGESTED);
            return None;
        }
        let Some(args) = call.and_then(|call| connector_suggest::parse_args(&call.arguments_json).ok())
        else {
            refuse(outcome, NOT_AVAILABLE);
            return None;
        };
        self.shown = true;
        Some(ChatEvent::ConnectorSuggestion {
            suggestion_id: format!("sug_{}", mp_ids::new_ulid()),
            provider: args.provider.to_owned(),
            reason: args.reason,
        })
    }
}

fn refuse(outcome: &mut ToolOutcome, message: &str) {
    outcome.output.clear();
    outcome.error = Some(message.to_owned());
    outcome.provenance = ToolProvenance::unscreened(&outcome.name, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(selection: &[&str], all_sources: bool) -> ConnectorTurn {
        ConnectorTurn::new(
            selection.iter().map(|id| (*id).to_owned()).collect(),
            all_sources,
        )
    }

    fn call(id: &str, provider: &str, reason: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: connector_suggest::TOOL_NAME.to_owned(),
            arguments_json: serde_json::json!({ "provider": provider, "reason": reason }).to_string(),
        }
    }

    fn other_tool() -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_owned(),
            description: String::new(),
            parameters_json: "{}".to_owned(),
        }
    }

    #[test]
    fn only_the_literal_true_switches_it_on() {
        assert!(parse_enabled(Some("true")));
        for off in [None, Some(""), Some("TRUE"), Some("1"), Some("yes"), Some(" true")] {
            assert!(!parse_enabled(off), "{off:?}");
        }
    }

    #[test]
    fn suggestion_not_offered_without_flag() {
        assert!(!offered(false, false, false, Some(&turn(&[], true))));
        assert!(offered(true, false, false, Some(&turn(&[], true))));
        let tools = tools_for_turn(vec![other_tool()], false);
        assert!(tools.iter().all(|tool| tool.name != connector_suggest::TOOL_NAME));
    }

    #[test]
    fn suggestion_not_offered_in_zdr() {
        assert!(!offered(true, true, false, Some(&turn(&[], true))));
        assert_eq!(
            outcome(true, true, false, &call("c1", "google", "Your mail would help.")).error.as_deref(),
            Some(NOT_AVAILABLE)
        );
    }

    /// The ZDR golden of the design's section 5 (`e_zdr_turn_offers_no_connector_suggestion`), kept
    /// as a gateway test because the posture matrix is keyed on resolver inputs, not feature offers:
    /// a zero-retention turn is offered no suggestion tool, whatever else holds, and no card exists.
    #[test]
    fn connector_suggestion_absent_under_zdr() {
        for space_scoped in [false, true] {
            for all_sources in [false, true] {
                for selection in [&[][..], &["conn_1"][..]] {
                    let state = turn(selection, all_sources);
                    assert!(!offered(true, true, space_scoped, Some(&state)));
                    let tools = tools_for_turn(
                        vec![other_tool(), tool_definition()],
                        offered(true, true, space_scoped, Some(&state)),
                    );
                    assert!(tools.iter().all(|tool| tool.name != connector_suggest::TOOL_NAME));
                }
            }
        }
        let mut refused = outcome(true, true, false, &call("c1", "google", "Your mail would help."));
        let suggestion = call("c1", "google", "Your mail would help.");
        assert!(SuggestionTurn::default().settle(&mut refused, Some(&suggestion)).is_none());
    }

    #[test]
    fn suggestion_not_offered_in_space() {
        assert!(!offered(true, false, true, Some(&turn(&[], true))));
        assert_eq!(
            outcome(true, false, true, &call("c1", "google", "Your mail would help.")).error.as_deref(),
            Some(NOT_AVAILABLE)
        );
    }

    #[test]
    fn suggestion_not_offered_when_provider_already_selected() {
        assert!(!offered(true, false, false, Some(&turn(&["conn_gmail"], true))));
    }

    #[test]
    fn suggestion_not_offered_outside_auto_scope_or_without_a_live_stream() {
        assert!(!offered(true, false, false, Some(&turn(&[], false))));
        assert!(!offered(true, false, false, None));
    }

    #[test]
    fn the_offered_tool_replaces_a_client_copy_and_comes_last() {
        let mut client_copy = tool_definition();
        client_copy.description = "client text".to_owned();
        let tools = tools_for_turn(vec![client_copy, other_tool()], true);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "web_search");
        assert_eq!(tools[1], tool_definition());
        let closed = tools_for_turn(vec![tool_definition(), other_tool()], false);
        assert_eq!(closed, vec![other_tool()]);
    }

    #[test]
    fn suggestion_emits_one_event_per_turn() {
        let mut state = SuggestionTurn::default();
        let first = call("c1", "google", "Your mail would answer this.");
        let mut first_outcome = outcome(true, false, false, &first);
        assert_eq!(first_outcome.output, SHOWN);
        let event = state.settle(&mut first_outcome, Some(&first)).expect("the first is shown");
        let ChatEvent::ConnectorSuggestion { suggestion_id, provider, reason } = event else {
            panic!("not a connector suggestion");
        };
        assert!(suggestion_id.starts_with("sug_"));
        assert_eq!(provider, "google");
        assert_eq!(reason, "Your mail would answer this.");
        assert!(first_outcome.error.is_none());

        let second = call("c2", "microsoft", "Your calendar would help.");
        let mut second_outcome = outcome(true, false, false, &second);
        assert!(state.settle(&mut second_outcome, Some(&second)).is_none());
        assert_eq!(second_outcome.error.as_deref(), Some(ALREADY_SUGGESTED));
        assert!(second_outcome.output.is_empty());
    }

    #[test]
    fn a_refused_suggestion_does_not_use_up_the_turn() {
        let mut state = SuggestionTurn::default();
        let bad = call("c1", "slack", "Your messages would help.");
        let mut bad_outcome = outcome(true, false, false, &bad);
        assert!(state.settle(&mut bad_outcome, Some(&bad)).is_none());
        let good = call("c2", "microsoft", "Your calendar would help.");
        let mut good_outcome = outcome(true, false, false, &good);
        assert!(state.settle(&mut good_outcome, Some(&good)).is_some());
    }

    #[test]
    fn other_tools_are_left_alone() {
        let mut state = SuggestionTurn::default();
        let mut other = ToolOutcome {
            call_id: "c1".to_owned(),
            name: "web_search".to_owned(),
            output: "hits".to_owned(),
            error: None,
            provenance: ToolProvenance::unscreened("web_search", "hits"),
        };
        let before = other.clone();
        assert!(state.settle(&mut other, None).is_none());
        assert_eq!(other, before);
    }

    #[test]
    fn unknown_provider_is_a_tool_error() {
        let refused = outcome(true, false, false, &call("c1", "notion", "Your pages would help."));
        assert_eq!(
            refused.error.as_deref(),
            Some(connector_suggest::SuggestProblem::UnknownProvider.message())
        );
        assert!(refused.output.is_empty());
    }

    #[test]
    fn reason_is_scrubbed_and_bounded() {
        let mut state = SuggestionTurn::default();
        let messy = call("c1", "google", "Mail from\nkari@example.com\twould   help");
        let mut shown = outcome(true, false, false, &messy);
        let Some(ChatEvent::ConnectorSuggestion { reason, .. }) = state.settle(&mut shown, Some(&messy)) else {
            panic!("the suggestion is shown");
        };
        assert_eq!(reason, "Mail from would help");
        let long = call("c2", "google", &"a".repeat(connector_suggest::MAX_REASON_CHARS + 1));
        assert_eq!(
            outcome(true, false, false, &long).error.as_deref(),
            Some(connector_suggest::SuggestProblem::ReasonTooLong.message())
        );
    }
}
