//! Golden cases of the shared question contract (build plan WS14 S3).
//!
//! The cases are the contract written out as data: which questions and answers are
//! accepted, and the stable `field` and `reason` of each refusal. The model-facing tool
//! (WS14 S4), the session-core store and the card all read this one validator, so a case
//! that changes here changes for every plane at once.

use mp_contracts::user_question::{
    answers_digest, parse_items, validate_answers, Answer, Item, MAX_ID, MAX_ITEMS, MAX_OPTIONS,
    MAX_TEXT,
};
use serde_json::{json, Value};

fn items(value: &Value) -> Vec<Item> {
    parse_items(value).expect("the golden question is valid")
}

fn answer(item_id: &str, options: &[&str], text: &str) -> Answer {
    Answer {
        item_id: item_id.to_owned(),
        option_ids: options.iter().map(|o| (*o).to_owned()).collect(),
        text: text.to_owned(),
    }
}

/// A question that uses every kind, with a free-text alternative on the multi choice.
fn the_ambiguous_task_question() -> Value {
    json!([
        {
            "id": "scope",
            "kind": "single",
            "prompt": "Which branch should the change go on?",
            "options": [
                {"id": "main", "label": "main", "description": "The release branch"},
                {"id": "feature", "label": "A new feature branch"}
            ]
        },
        {
            "id": "checks",
            "kind": "multi",
            "prompt": "Which checks should run first?",
            "options": [
                {"id": "lint", "label": "Lint"},
                {"id": "unit", "label": "Unit tests"},
                {"id": "e2e", "label": "End-to-end tests"}
            ],
            "allow_other": true
        },
        {"id": "notes", "kind": "text", "prompt": "Anything the change must not touch?"}
    ])
}

#[test]
fn user_question_accepted_questions_parse_to_the_same_items_they_were_written_as() {
    let parsed = items(&the_ambiguous_task_question());
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[1].options.len(), 3);
    assert!(parsed[1].allow_other);
    assert_eq!(parsed[0].options[1].description, "");
    let round_trip: Vec<Item> =
        serde_json::from_value(serde_json::to_value(&parsed).expect("serialise")).expect("parse");
    assert_eq!(round_trip, parsed);
}

// One row per refusal keeps the table readable as a single list.
#[allow(clippy::too_many_lines)]
#[test]
fn user_question_refused_questions_name_where_and_why() {
    let long_id = "x".repeat(MAX_ID + 1);
    let too_many_items: Vec<Value> = (0..=MAX_ITEMS)
        .map(|n| json!({"id": format!("i{n}"), "kind": "text", "prompt": "p"}))
        .collect();
    let too_many_options: Vec<Value> = (0..=MAX_OPTIONS)
        .map(|n| json!({"id": format!("o{n}"), "label": "L"}))
        .collect();
    let cases: Vec<(&str, Value, &str, &str)> = vec![
        ("not an array", json!({"id": "q"}), "items", "malformed"),
        ("empty", json!([]), "items", "no_items"),
        (
            "five items",
            Value::Array(too_many_items),
            "items",
            "too_many_items",
        ),
        (
            "a secret kind",
            json!([{"id": "q", "kind": "secret", "prompt": "Your API key?"}]),
            "items[0].kind",
            "unsupported_kind",
        ),
        (
            "a password kind",
            json!([{"id": "q", "kind": "password", "prompt": "Your password?"}]),
            "items[0].kind",
            "unsupported_kind",
        ),
        (
            "an unknown field",
            json!([{"id": "q", "kind": "text", "prompt": "p", "masked": true}]),
            "items",
            "malformed",
        ),
        (
            "an upper-case id",
            json!([{"id": "Q", "kind": "text", "prompt": "p"}]),
            "items[0].id",
            "bad_id",
        ),
        (
            "a too long id",
            json!([{"id": long_id, "kind": "text", "prompt": "p"}]),
            "items[0].id",
            "bad_id",
        ),
        (
            "a repeated item id",
            json!([
                {"id": "q", "kind": "text", "prompt": "p"},
                {"id": "q", "kind": "text", "prompt": "p"}
            ]),
            "items[1].id",
            "duplicate_id",
        ),
        (
            "a blank prompt",
            json!([{"id": "q", "kind": "text", "prompt": " \n "}]),
            "items[0].prompt",
            "empty_text",
        ),
        (
            "a prompt over the bound",
            json!([{"id": "q", "kind": "text", "prompt": "x".repeat(MAX_TEXT + 1)}]),
            "items[0].prompt",
            "text_too_long",
        ),
        (
            "one option",
            json!([{"id": "q", "kind": "single", "prompt": "p", "options": [{"id": "a", "label": "A"}]}]),
            "items[0].options",
            "too_few_options",
        ),
        (
            "seven options",
            json!([{"id": "q", "kind": "multi", "prompt": "p", "options": too_many_options}]),
            "items[0].options",
            "too_many_options",
        ),
        (
            "a repeated option id",
            json!([{"id": "q", "kind": "single", "prompt": "p",
                "options": [{"id": "a", "label": "A"}, {"id": "a", "label": "B"}]}]),
            "items[0].options[1].id",
            "duplicate_id",
        ),
        (
            "an option with no label",
            json!([{"id": "q", "kind": "single", "prompt": "p",
                "options": [{"id": "a", "label": ""}, {"id": "b", "label": "B"}]}]),
            "items[0].options[0].label",
            "empty_text",
        ),
        (
            "options on a text item",
            json!([{"id": "q", "kind": "text", "prompt": "p", "options": [{"id": "a", "label": "A"}]}]),
            "items[0].options",
            "options_on_text",
        ),
        (
            "other on a text item",
            json!([{"id": "q", "kind": "text", "prompt": "p", "allow_other": true}]),
            "items[0].allow_other",
            "other_on_text",
        ),
    ];
    for (name, value, field, reason) in cases {
        let problem = parse_items(&value).expect_err(name);
        assert_eq!(
            (problem.field.as_str(), problem.reason),
            (field, reason),
            "{name}"
        );
    }
}

#[test]
fn user_question_accepted_answers_cover_every_kind() {
    let questions = items(&the_ambiguous_task_question());
    let full = [
        answer("scope", &["main"], ""),
        answer("checks", &["lint", "unit"], "and the type check"),
        answer("notes", &[], "the migrations"),
    ];
    assert_eq!(validate_answers(&questions, &full, false), Ok(()));
    let other_only = [
        answer("scope", &["feature"], ""),
        answer("checks", &[], "only the type check"),
        answer("notes", &[], "n/a"),
    ];
    assert_eq!(validate_answers(&questions, &other_only, false), Ok(()));
    assert_eq!(validate_answers(&questions, &[], true), Ok(()));
}

#[test]
fn user_question_refused_answers_name_where_and_why() {
    let questions = items(&the_ambiguous_task_question());
    let ok_notes = answer("notes", &[], "n/a");
    let cases: Vec<(&str, Vec<Answer>, bool, &str, &str)> = vec![
        ("nothing", vec![], false, "answers[scope]", "missing_answer"),
        (
            "an unknown item",
            vec![answer("ghost", &["a"], "")],
            false,
            "answers[0].item_id",
            "unknown_item",
        ),
        (
            "an unknown option",
            vec![
                answer("scope", &["ghost"], ""),
                answer("checks", &["lint"], ""),
                ok_notes.clone(),
            ],
            false,
            "answers[0].option_ids[0]",
            "unknown_option",
        ),
        (
            "two choices on a single",
            vec![
                answer("scope", &["main", "feature"], ""),
                answer("checks", &["lint"], ""),
                ok_notes.clone(),
            ],
            false,
            "answers[0].option_ids",
            "too_many_choices",
        ),
        (
            "other text where none is allowed",
            vec![
                answer("scope", &[], "somewhere else"),
                answer("checks", &["lint"], ""),
                ok_notes.clone(),
            ],
            false,
            "answers[0].text",
            "other_not_allowed",
        ),
        (
            "no choice on a multi",
            vec![
                answer("scope", &["main"], ""),
                answer("checks", &[], ""),
                ok_notes.clone(),
            ],
            false,
            "answers[1].option_ids",
            "no_choice",
        ),
        (
            "an empty text answer",
            vec![
                answer("scope", &["main"], ""),
                answer("checks", &["lint"], ""),
                answer("notes", &[], ""),
            ],
            false,
            "answers[2].text",
            "empty_text",
        ),
        (
            "answers with a skip",
            vec![ok_notes],
            true,
            "answers",
            "skip_with_answers",
        ),
    ];
    for (name, given, skip, field, reason) in cases {
        let problem = validate_answers(&questions, &given, skip).expect_err(name);
        assert_eq!(
            (problem.field.as_str(), problem.reason),
            (field, reason),
            "{name}"
        );
    }
}

/// The digest tells a retry from a different second answer, so its bytes are part of the
/// contract: a changed canonical form would make every stored answer look different.
#[test]
fn user_question_the_answer_digest_is_pinned() {
    let answers = [answer("scope", &["main"], ""), answer("notes", &[], "n/a")];
    let reordered = [answer("notes", &[], "n/a"), answer("scope", &["main"], "")];
    let digest = answers_digest(&answers, false);
    assert_eq!(digest, answers_digest(&reordered, false));
    assert_eq!(
        digest,
        "6cc9caf703a92f5ed3536691c168d02cc0e11d2374c47fa139a0841719ff17bf"
    );
}
