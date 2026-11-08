//! `ask_user` (build plan WS14 S4): the Do loop's one way to put a question to the run's owner.
//!
//! A question is information the person owes the run, never an approval. It is stored in
//! session-core's `run_questions` table (WS14 S3), and an answer does not read as an authority
//! grant. A call ends the turn it is made in, and the answer arrives as the person's next message,
//! which the run's next turn works from. A client that predates the question card reads the same
//! question as plain prose, and that prose is the answer the turn ends with.
//!
//! Offering is governed by `AGENT_ASK_USER_ENABLED`, read strictly: only the literal `true`
//! enables it, and the default is off. With the switch off the tool is never offered and a call to
//! it is refused, so the run is what it was before the tool existed.
//!
//! The file lives at the crate root; `runtime_loop` declares it with a `#[path]`, as it does for
//! `owner_reads.rs`.

use std::sync::atomic::{AtomicU32, Ordering};

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::user_question::{self, Item, ItemKind, MAX_QUESTIONS_PER_RUN};

/// The tool's name, as the model calls it.
pub const ASK_TOOL: &str = "ask_user";

/// The switch that offers the tool. Execution-core and model-gateway both read it, strictly.
pub const SWITCH_ENV: &str = "AGENT_ASK_USER_ENABLED";

/// Whether `tool_name` is the question tool. Exact: a spelling variant is some other tool.
#[must_use]
pub fn is_ask_tool(tool_name: &str) -> bool {
    tool_name == ASK_TOOL
}

/// Only the literal `true` enables a switch, as every switch of this service reads it.
#[must_use]
pub fn switch_on(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the deployment offers the tool, read once per process so a run cannot see it change.
#[must_use]
pub fn enabled() -> bool {
    #[cfg(test)]
    {
        if let Some(forced) = test_switch::forced() {
            return forced;
        }
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| switch_on(std::env::var(SWITCH_ENV).ok().as_deref()))
}

/// The most questions this run may put to its owner: zero when the tool is not offered to it. It is
/// offered only to a Do run that may ask: not zero-retention, not waived, not a plan, not a
/// continuation (which runs the tool set it paused with) and not in a Space.
#[must_use]
pub fn budget(req: &pb::RunAgentRequest, waived: bool) -> u32 {
    budget_when(enabled(), req, waived)
}

/// [`budget`] with the switch given, so the rule is testable without the environment.
#[must_use]
pub fn budget_when(on: bool, req: &pb::RunAgentRequest, waived: bool) -> u32 {
    let may_ask = on
        && !req.zdr
        && !waived
        && !req.plan_mode
        && req.resume.is_none()
        && req.space_id.trim().is_empty();
    if !may_ask {
        return 0;
    }
    let shared_limit = u32::try_from(MAX_QUESTIONS_PER_RUN).unwrap_or(0);
    req.max_questions.min(shared_limit)
}

/// Admits one call to the tool, or refuses it with the words the model reads. `asked` counts every
/// call admitted in the run so far, so the cap holds however many calls one round makes.
///
/// # Errors
/// The refusal text, for a subagent, for a run the tool is not offered to, and for a call past the cap.
pub fn admit(depth: u32, budget: u32, asked: &AtomicU32) -> Result<(), String> {
    if depth > 0 {
        return Err(main_agent_only_refusal());
    }
    if budget == 0 {
        return Err(format!(
            "{ASK_TOOL} is not available in this run. Answer from what you have."
        ));
    }
    if asked.fetch_add(1, Ordering::SeqCst) >= budget {
        return Err(format!(
            "{ASK_TOOL}: this run may ask at most {budget} question(s), and it has asked them. \
             Answer from what you have."
        ));
    }
    Ok(())
}

/// The refusal for a delegated subagent that calls the tool. A question goes to the person who
/// started the run, so only the main agent may put one.
#[must_use]
pub fn main_agent_only_refusal() -> String {
    format!(
        "{ASK_TOOL} is only available to the main agent: a question goes to the person who started \
         the run, and you are a delegated subagent that only investigates. Report what you need to \
         the agent that delegated to you instead."
    )
}

/// The tool's definition, offered to a run whose [`budget`] is above zero.
#[must_use]
pub fn tool_def() -> pb::ToolDefinition {
    pb::ToolDefinition {
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

fn schema() -> serde_json::Value {
    let identifier = "A short id, unique within the call: lowercase letters, digits, _ or -";
    serde_json::json!({
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

/// The questions a call carries, checked against the shared bounds: the same check session-core
/// makes when it stores them.
///
/// # Errors
/// The first refusal, as a message the model reads: malformed input, or a bound the contract breaks.
pub fn parse(tool_input: &str) -> Result<Vec<Item>, String> {
    #[derive(serde::Deserialize)]
    struct Input {
        questions: serde_json::Value,
    }
    let input: Input = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid {ASK_TOOL} input: {error}"))?;
    user_question::parse_items(&input.questions).map_err(|problem| format!("{ASK_TOOL}: {problem}"))
}

/// The wire form of one checked question, for session-core.
#[must_use]
pub fn to_wire_item(item: &Item) -> pb::QuestionItem {
    pb::QuestionItem {
        id: item.id.clone(),
        kind: item.kind.clone(),
        prompt: item.prompt.clone(),
        options: item
            .options
            .iter()
            .map(|option| pb::QuestionOption {
                id: option.id.clone(),
                label: option.label.clone(),
                description: option.description.clone(),
            })
            .collect(),
        allow_other: item.allow_other,
    }
}

/// The questions as plain prose: the answer a turn that asked ends with, which a client that
/// predates the question card shows as it stands.
#[must_use]
pub fn prose(items: &[Item]) -> String {
    let mut lines = vec!["I need an answer before I can continue.".to_owned()];
    for (number, item) in items.iter().enumerate() {
        lines.push(if items.len() > 1 {
            format!("{}. {}", number + 1, item.prompt)
        } else {
            item.prompt.clone()
        });
        for option in &item.options {
            lines.push(format!("- {}", option.label));
        }
        if item.allow_other {
            lines.push("(You can also write your own answer.)".to_owned());
        }
        if ItemKind::parse(&item.kind) == Some(ItemKind::Text) {
            lines.push("(Answer in your own words.)".to_owned());
        }
    }
    lines.join("\n")
}

/// The prose of the questions a round's successful calls put to the owner, in call order, or `None`
/// when none succeeded and the loop goes on. `arguments` are those calls' input JSON.
#[must_use]
pub fn asked_prose<'a>(arguments: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let blocks: Vec<String> = arguments
        .into_iter()
        .filter_map(|input| parse(input).ok())
        .map(|items| prose(&items))
        .collect();
    (!blocks.is_empty()).then(|| blocks.join("\n\n"))
}

#[cfg(test)]
pub(crate) mod test_switch {
    use std::cell::Cell;

    thread_local! {
        static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
    }

    /// Forces the switch on this thread until the guard drops.
    pub(crate) fn force(on: bool) -> Guard {
        FORCED.with(|forced| forced.set(Some(on)));
        Guard
    }

    pub(super) fn forced() -> Option<bool> {
        FORCED.with(Cell::get)
    }

    pub(crate) struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            FORCED.with(|forced| forced.set(None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(max_questions: u32) -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            max_questions,
            ..Default::default()
        }
    }

    fn carrier_call() -> String {
        serde_json::json!({
            "questions": [{
                "id": "carrier",
                "kind": "single",
                "prompt": "Which carrier?",
                "options": [
                    {"id": "bring", "label": "Bring"},
                    {"id": "postnord", "label": "PostNord"}
                ],
                "allow_other": true
            }]
        })
        .to_string()
    }

    #[test]
    fn the_tool_name_is_exact() {
        assert!(is_ask_tool("ask_user"));
        assert!(!is_ask_tool("ask_user "));
        assert!(!is_ask_tool("ask"));
        assert!(!is_ask_tool("Ask_User"));
    }

    #[test]
    fn the_switch_reads_only_the_literal_true() {
        assert!(switch_on(Some("true")));
        for raw in [None, Some(""), Some("TRUE"), Some("1"), Some("yes"), Some(" true")] {
            assert!(!switch_on(raw), "{raw:?} must not enable the tool");
        }
    }

    #[test]
    fn the_budget_is_zero_unless_every_condition_holds() {
        let asking = request(3);
        assert_eq!(budget_when(true, &asking, false), 3);
        assert_eq!(budget_when(false, &asking, false), 0, "the switch off offers nothing");
        assert_eq!(
            budget_when(true, &request(0), false),
            0,
            "a zero cap from the gateway offers nothing"
        );
        assert_eq!(budget_when(true, &asking, true), 0, "a waived run is never asked");
        let zdr = pb::RunAgentRequest { zdr: true, ..request(3) };
        assert_eq!(budget_when(true, &zdr, false), 0, "a zero-retention run is never asked");
        let planning = pb::RunAgentRequest { plan_mode: true, ..request(3) };
        assert_eq!(budget_when(true, &planning, false), 0, "a plan run is never asked");
        let in_space = pb::RunAgentRequest { space_id: "space-1".to_owned(), ..request(3) };
        assert_eq!(budget_when(true, &in_space, false), 0, "a Space run is never asked");
        let continuation = pb::RunAgentRequest {
            resume: Some(pb::AgentResume::default()),
            ..request(3)
        };
        assert_eq!(
            budget_when(true, &continuation, false),
            0,
            "a continuation runs the tool set it paused with"
        );
    }

    #[test]
    fn the_budget_never_exceeds_the_shared_limit() {
        let shared = u32::try_from(MAX_QUESTIONS_PER_RUN).expect("a small limit");
        assert_eq!(budget_when(true, &request(9), false), shared);
    }

    #[test]
    fn a_subagent_and_an_unoffered_run_are_refused_and_the_cap_binds() {
        let asked = AtomicU32::new(0);
        assert!(admit(1, 3, &asked)
            .unwrap_err()
            .contains("only available to the main agent"));
        assert!(admit(0, 0, &asked).is_err());
        assert!(admit(0, 3, &asked).is_ok());
        assert!(admit(0, 3, &asked).is_ok());
        assert!(admit(0, 3, &asked).is_ok());
        let refusal = admit(0, 3, &asked).unwrap_err();
        assert!(refusal.contains("at most 3"), "{refusal}");
    }

    #[test]
    fn the_tool_definition_asks_for_one_to_four_questions_of_three_kinds() {
        let def = tool_def();
        assert_eq!(def.name, ASK_TOOL);
        let schema: serde_json::Value =
            serde_json::from_str(&def.parameters_json).expect("the schema is JSON");
        let questions = &schema["properties"]["questions"];
        assert_eq!(questions["minItems"], 1);
        assert_eq!(questions["maxItems"], 4);
        assert_eq!(
            questions["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["single", "multi", "text"])
        );
        assert_eq!(schema["required"], serde_json::json!(["questions"]));
    }

    #[test]
    fn a_well_formed_question_parses_and_a_credential_kind_is_refused() {
        let items = parse(&carrier_call()).expect("a well-formed question");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].options.len(), 2);
        let refused =
            parse(r#"{"questions":[{"id":"pin","kind":"password","prompt":"Your PIN?"}]}"#)
                .unwrap_err();
        assert!(refused.contains("unsupported_kind"), "{refused}");
    }

    #[test]
    fn more_than_four_questions_and_a_lone_option_are_refused() {
        let one = |id: &str| {
            serde_json::json!({"id": id, "kind": "text", "prompt": "Anything else?"})
        };
        let five = serde_json::json!({
            "questions": [one("a"), one("b"), one("c"), one("d"), one("e")]
        })
        .to_string();
        assert!(parse(&five).unwrap_err().contains("too_many_items"));
        let lone = r#"{"questions":[{"id":"x","kind":"single","prompt":"Pick?","options":[{"id":"a","label":"A"}]}]}"#;
        assert!(parse(lone).unwrap_err().contains("too_few_options"));
        assert!(parse("not json").unwrap_err().starts_with("invalid ask_user input"));
    }

    #[test]
    fn to_wire_item_carries_the_choices_across() {
        let items = parse(&carrier_call()).expect("a well-formed question");
        let wire = to_wire_item(&items[0]);
        assert_eq!(wire.id, "carrier");
        assert_eq!(wire.kind, "single");
        assert_eq!(wire.options.len(), 2);
        assert_eq!(wire.options[1].label, "PostNord");
        assert!(wire.allow_other);
    }

    #[test]
    fn prose_lists_each_question_with_its_choices() {
        let items = parse(&carrier_call()).expect("a well-formed question");
        let text = prose(&items);
        assert!(text.starts_with("I need an answer before I can continue."), "{text}");
        assert!(text.contains("Which carrier?"), "{text}");
        assert!(text.contains("- Bring"), "{text}");
        assert!(text.contains("write your own answer"), "{text}");
    }

    #[test]
    fn asked_prose_is_none_until_a_call_succeeds_with_a_question() {
        assert!(asked_prose(std::iter::empty()).is_none());
        assert!(asked_prose(["not json"]).is_none());
        let call = carrier_call();
        assert!(asked_prose([call.as_str()]).is_some());
    }
}
