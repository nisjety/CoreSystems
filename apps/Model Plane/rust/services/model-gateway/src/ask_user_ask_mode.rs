//! `ask_user` in the Ask loop (build plan WS14 S6): the one way an Ask-mode turn puts a question to the
//! person who sent it.
//!
//! The Do loop already has the tool (WS14 S4, `execution-core/src/ask.rs`). This is the chat tool loop's
//! copy of it, and it reuses what that slice built instead of making a second store: the question is a
//! `next_turn` row of session-core's `run_questions` (WS14 S3), checked by the shared
//! `mp_contracts::user_question` bounds, and it reaches the browser as the same `user_question` event. A
//! question is information the person owes the turn, never an approval, so an answer cannot read as one
//! (decision DEC-05, option b).
//!
//! A call ends the turn it is made in: the loop shows the question card, the answer to the turn is one
//! short line, and the person's answer arrives as their next message. One question is stored per turn;
//! a second call in the same turn is answered with a refusal and writes nothing.
//!
//! # Switch and scope
//!
//! Off unless `MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED` is the literal `true`, read once per process. The
//! tool is offered only to a turn that keeps its content (not zero-retention, not waived), is not
//! Space-scoped, and is not a plan turn (a plan turn is a Do-loop turn and never reaches this loop). With
//! the switch off, or on any other turn, the tool is absent from the definition list and a call that
//! arrives anyway is refused before anything is written.
//!
//! # Privacy
//!
//! Logs carry a question id and counts, never a prompt, an option label or an answer.

use mp_contracts::model_plane::v1::{
    CreateRunQuestionRequest, ListRunQuestionsRequest, QuestionItem, QuestionOption, QuestionState,
    RunQuestion, ToolCall, ToolDefinition,
};
use mp_contracts::user_question::{self, Item, DELIVERY_NEXT_TURN, NEXT_TURN_TTL_SECS};
use serde_json::{json, Value};

use crate::sse_events::ChatEvent;
use crate::state::AppState;
use crate::tool_loop::{err_outcome, with_authorization, ToolOutcome};

/// The tool's name, as the model calls it.
pub const ASK_TOOL: &str = "ask_user";

/// The switch that offers the tool to the Ask loop. Read strictly: only the literal `true` enables it.
pub const SWITCH_ENV: &str = "MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED";

/// What a refused call says, for a deployment, turn or run that is not offered the tool.
pub const NOT_ENABLED: &str = "ask_user is not enabled on this turn";

/// The closing instruction the loop gives the model once a question is on the person's screen.
pub const END_TURN_NOTE: &str = "The person has been shown your question. End this turn now with one short sentence saying you are waiting for their answer. Do not repeat the questions and do not call another tool.";

/// The refusal for a second call in one turn.
const ALREADY_ASKED: &str =
    "ask_user: this turn has already asked its question. End the turn with one short sentence saying you are waiting for the answer.";

/// Whether the deployment offers the tool to the Ask loop, read once per process so a turn cannot see it
/// change. Only the literal `true` enables it, and the default is off.
#[must_use]
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| switch(std::env::var(SWITCH_ENV).ok().as_deref()))
}

/// The strict reading of [`SWITCH_ENV`]: only the literal `true` enables it.
fn switch(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether `name` is the question tool. Exact: a spelling variant is some other tool.
#[must_use]
pub fn is_ask_tool(name: &str) -> bool {
    name == ASK_TOOL
}

/// Add the tool's definition to `defs` when `on`. Off, the list is untouched. A definition the client
/// declared under the same name gives way to the server's own.
pub(crate) fn offer(defs: &mut Vec<ToolDefinition>, on: bool) {
    if on {
        defs.retain(|tool| !is_ask_tool(&tool.name));
        defs.push(tool_definition());
    }
}

/// The tool as the model is offered it. Copied from the Do loop's definition (`execution-core/src/ask.rs`)
/// so the two loops describe the same question in the same words and bounds.
fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: ASK_TOOL.to_owned(),
        description: "Ask the person who started this run a question, but only when you cannot go on \
            without their choice or a fact only they know, and cannot find the answer yourself. Ask one \
            to four questions, each ready to answer: give the choices you would accept, and say whether \
            they may write their own answer. Asking ends this turn: their answer arrives as their next \
            message, and your next turn works from it. Never ask for a password, a secret, a card number \
            or any other credential. At most three questions in a run."
            .to_owned(),
        parameters_json: schema().to_string(),
    }
}

fn schema() -> Value {
    let identifier = "A short id, unique within the call: lowercase letters, digits, _ or -";
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "questions": {
                "type": "array",
                "minItems": 1,
                "maxItems": user_question::MAX_ITEMS,
                "description": "One to four questions, asked together and answered in one reply",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "id": {"type": "string", "description": identifier},
                        "kind": {
                            "type": "string",
                            "enum": ["single", "multi", "text"],
                            "description": "single: one choice; multi: several choices; text: a free-text answer"
                        },
                        "prompt": {"type": "string", "description": "The question, as the person reads it"},
                        "options": {
                            "type": "array",
                            "minItems": user_question::MIN_OPTIONS,
                            "maxItems": user_question::MAX_OPTIONS,
                            "description": "single and multi only: two to six choices",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "id": {"type": "string", "description": identifier},
                                    "label": {"type": "string"},
                                    "description": {"type": "string"}
                                },
                                "required": ["id", "label"]
                            }
                        },
                        "allow_other": {
                            "type": "boolean",
                            "description": "single and multi only: whether the person may write their own answer"
                        }
                    },
                    "required": ["id", "kind", "prompt"]
                }
            }
        },
        "required": ["questions"]
    })
}

/// The questions a call carries, checked against the shared bounds: the same check session-core makes
/// when it stores them.
///
/// # Errors
/// The first refusal, as a message the model reads: malformed input, or a bound the contract breaks.
fn parse(arguments_json: &str) -> Result<Vec<Item>, String> {
    #[derive(serde::Deserialize)]
    struct Input {
        questions: Value,
    }
    let input: Input = serde_json::from_str(arguments_json)
        .map_err(|error| format!("invalid {ASK_TOOL} input: {error}"))?;
    user_question::parse_items(&input.questions).map_err(|problem| format!("{ASK_TOOL}: {problem}"))
}

/// The wire form of one checked question, for session-core.
fn to_wire_item(item: &Item) -> QuestionItem {
    QuestionItem {
        id: item.id.clone(),
        kind: item.kind.clone(),
        prompt: item.prompt.clone(),
        options: item
            .options
            .iter()
            .map(|option| QuestionOption {
                id: option.id.clone(),
                label: option.label.clone(),
                description: option.description.clone(),
            })
            .collect(),
        allow_other: item.allow_other,
    }
}

/// `ask_user` for the Ask loop: checks the arguments, then stores the question for the turn's run as one
/// `next_turn` question the model authored. The result names the question and tells the model to end the
/// turn; the loop turns it into the card ([`AskTurn::settle`]).
///
/// `on` is [`enabled`]. `retained` is true only for a turn that keeps its content: not zero-retention and
/// not waived. A call on any other turn, or in a Space, is refused before anything is read or written.
#[allow(clippy::too_many_arguments)]
pub async fn ask_outcome(
    state: &AppState,
    on: bool,
    retained: bool,
    space_scoped: bool,
    run_id: &str,
    org_id: &str,
    session_bearer: &str,
    call: &ToolCall,
) -> ToolOutcome {
    if !on || !retained || space_scoped {
        return err_outcome(call, NOT_ENABLED);
    }
    if session_bearer.is_empty() || run_id.is_empty() || org_id.is_empty() {
        return err_outcome(call, "ask_user requires a verified session credential");
    }
    let items = match parse(&call.arguments_json) {
        Ok(items) => items,
        Err(problem) => return err_outcome(call, problem),
    };
    // One key per turn: a second call of the same turn resolves to the question already stored.
    let request = CreateRunQuestionRequest {
        run_id: run_id.to_owned(),
        step_id: call.id.clone(),
        org_id: org_id.to_owned(),
        delivery: DELIVERY_NEXT_TURN.to_owned(),
        items: items.iter().map(to_wire_item).collect(),
        expires_in_seconds: NEXT_TURN_TTL_SECS,
        idempotency_key: format!("{run_id}:{ASK_TOOL}"),
        // The turn's own model asks here; only an MCP elicitation names another origin.
        origin: String::new(),
        origin_label: String::new(),
    };
    match state
        .orchestration_client
        .clone()
        .create_run_question(with_authorization(request, session_bearer))
        .await
    {
        Ok(response) => {
            let question_id = response
                .into_inner()
                .question
                .map(|question| question.id)
                .unwrap_or_default();
            tracing::info!(%run_id, %question_id, items = items.len(), "ask_user: question stored for the turn");
            let output = json!({
                "question_id": question_id,
                "state": "requested",
                "delivery": DELIVERY_NEXT_TURN,
                "next": END_TURN_NOTE,
            })
            .to_string();
            ToolOutcome {
                call_id: call.id.clone(),
                name: call.name.clone(),
                provenance: crate::moderation::ToolProvenance::unscreened(&call.name, &output),
                output,
                error: None,
            }
        }
        Err(status) => {
            tracing::warn!(code = ?status.code(), %run_id, "ask_user: question could not be stored");
            err_outcome(call, format!("{ASK_TOOL} failed: {}", status.message()))
        }
    }
}

/// What the turn has asked so far: at most one question, and the call that stored it ends the turn.
#[derive(Debug, Default)]
pub(crate) struct AskTurn {
    asked: bool,
}

impl AskTurn {
    /// Whether the turn has put its question, so the loop ends the turn.
    pub(crate) fn asked(&self) -> bool {
        self.asked
    }

    /// Settles one dispatched outcome, before its result is shown. The first successful `ask_user` call is
    /// the turn's question: its id is returned, for the card. A later successful call is turned into a
    /// refusal, so the model is told the turn has asked. Any other outcome is left alone.
    pub(crate) fn settle(&mut self, outcome: &mut ToolOutcome) -> Option<String> {
        if !is_ask_tool(&outcome.name) || outcome.error.is_some() {
            return None;
        }
        if self.asked {
            outcome.output = String::new();
            outcome.error = Some(ALREADY_ASKED.to_owned());
            return None;
        }
        self.asked = true;
        let id = serde_json::from_str::<Value>(&outcome.output)
            .ok()?
            .get("question_id")?
            .as_str()?
            .to_owned();
        Some(id)
    }
}

/// The card's event for the question a call stored: the question as the browser reads it, the same frame
/// the Do loop sends. Best-effort: a failed lookup sends no card, and the turn still ends with its closing
/// line.
pub(crate) async fn card_event(
    state: &AppState,
    run_id: &str,
    org_id: &str,
    session_bearer: &str,
    question_id: &str,
) -> Option<ChatEvent> {
    let question = requested_question(state, run_id, org_id, session_bearer, question_id).await?;
    Some(card(&question))
}

/// The stored question the call made, read back from the run's list. Best-effort: a failed lookup sends no
/// card.
async fn requested_question(
    state: &AppState,
    run_id: &str,
    org_id: &str,
    session_bearer: &str,
    question_id: &str,
) -> Option<RunQuestion> {
    let request = with_authorization(
        ListRunQuestionsRequest {
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
        },
        session_bearer,
    );
    let questions = match state
        .orchestration_client
        .clone()
        .list_run_questions(request)
        .await
    {
        Ok(response) => response.into_inner().questions,
        Err(status) => {
            tracing::warn!(code = ?status.code(), %run_id, %question_id, "ask_user: question card lookup failed");
            return None;
        }
    };
    questions.into_iter().find(|question| {
        question.id == question_id && question.state == QuestionState::Requested as i32
    })
}

/// The card's event: the question as the browser reads it, the same frame the Do loop sends.
fn card(question: &RunQuestion) -> ChatEvent {
    let fields = crate::http_routes::run_question_value(question);
    ChatEvent::UserQuestion {
        question_id: question.id.clone(),
        delivery: question.delivery.clone(),
        expires_at: fields["expires_at"].as_str().map(str::to_owned),
        items: fields["items"].clone(),
        origin: fields["origin"].as_str().map(str::to_owned),
        origin_label: fields["origin_label"].as_str().map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_literal_true_enables_the_tool() {
        assert!(switch(Some("true")));
        for raw in [
            None,
            Some(""),
            Some("TRUE"),
            Some("1"),
            Some("yes"),
            Some("true "),
        ] {
            assert!(!switch(raw), "{raw:?} must not enable the tool");
        }
    }

    #[test]
    fn offer_adds_nothing_while_the_switch_is_off() {
        let mut defs = Vec::new();
        offer(&mut defs, false);
        assert!(defs.is_empty());
    }

    #[test]
    fn offer_adds_exactly_one_ask_user_while_the_switch_is_on() {
        let mut defs = Vec::new();
        offer(&mut defs, true);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, ASK_TOOL);
        offer(&mut defs, true);
        assert_eq!(defs.len(), 1, "a second offer replaces the first");
    }
}
