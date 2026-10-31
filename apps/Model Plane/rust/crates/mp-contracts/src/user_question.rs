//! The one contract of a question put to a user (build plan WS14 S3).
//!
//! A run may ask its owner for information it cannot find out itself. The question is
//! a small closed shape: a few items, each a single choice, a multiple choice or a line
//! of text. Every plane that creates, stores or answers one reads the same bounds from
//! here, so the model-facing tool (WS14 S4), the durable store (session-core) and the
//! card that renders it cannot drift apart.
//!
//! The kind set is closed on purpose. There is no kind for a secret, a password or any
//! other credential: a question is information, never a credential channel and never an
//! authority grant, so an unknown kind is refused instead of being passed through.

use serde::{Deserialize, Serialize};

/// Most items one question may carry.
pub const MAX_ITEMS: usize = 4;
/// Most options one single- or multi-choice item may carry.
pub const MAX_OPTIONS: usize = 6;
/// Fewest options a choice item needs to be a choice at all.
pub const MIN_OPTIONS: usize = 2;
/// Most characters of any free text: a prompt, an option description, an answer.
pub const MAX_TEXT: usize = 2000;
/// Most characters of an option label.
pub const MAX_LABEL: usize = 200;
/// Most characters of an item or option id.
pub const MAX_ID: usize = 64;
/// Most questions one run may ever ask, whatever their state.
pub const MAX_QUESTIONS_PER_RUN: usize = 3;

/// The answer is the owner's next message.
pub const DELIVERY_NEXT_TURN: &str = "next_turn";
/// The answer continues the same run. Not offered until the resume slice ships.
pub const DELIVERY_RESUME: &str = "resume";

/// Lifetime ceiling of a `next_turn` question.
pub const NEXT_TURN_TTL_SECS: u32 = 24 * 60 * 60;
/// Lifetime ceiling of a `resume` question, which holds a run open.
pub const RESUME_TTL_SECS: u32 = 60 * 60;

/// The lifetime ceiling of `delivery`, or `None` for a delivery that does not exist.
#[must_use]
pub fn delivery_ttl_secs(delivery: &str) -> Option<u32> {
    match delivery {
        DELIVERY_NEXT_TURN => Some(NEXT_TURN_TTL_SECS),
        DELIVERY_RESUME => Some(RESUME_TTL_SECS),
        _ => None,
    }
}

/// The closed set of item kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Single,
    Multi,
    Text,
}

impl ItemKind {
    /// The wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Multi => "multi",
            Self::Text => "text",
        }
    }

    /// Parse a wire token. Exact match only, so no spelling of a credential kind passes.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        [Self::Single, Self::Multi, Self::Text]
            .into_iter()
            .find(|kind| kind.as_str() == token)
    }
}

/// One choice of a single- or multi-choice item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
}

/// One thing asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<OptionSpec>,
    #[serde(default)]
    pub allow_other: bool,
}

/// The owner's answer to one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub item_id: String,
    #[serde(default)]
    pub option_ids: Vec<String>,
    #[serde(default)]
    pub text: String,
}

/// Why a question or an answer was refused: where, and a stable machine token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// A path into the refused value, such as `items[1].options`.
    pub field: String,
    /// A stable `snake_case` token (`too_many_items`, `unsupported_kind`, ...).
    pub reason: &'static str,
}

impl Problem {
    fn at(field: impl Into<String>, reason: &'static str) -> Self {
        Self {
            field: field.into(),
            reason,
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

impl std::error::Error for Problem {}

fn id_is_well_formed(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// A text a person reads: bounded, and free of control characters other than a newline or tab.
fn check_text(field: &str, text: &str, max: usize, allow_empty: bool) -> Result<(), Problem> {
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Problem::at(field, "control_character"));
    }
    if !allow_empty && text.trim().is_empty() {
        return Err(Problem::at(field, "empty_text"));
    }
    if text.chars().count() > max {
        return Err(Problem::at(field, "text_too_long"));
    }
    Ok(())
}

/// Parse the JSON array of items a model or a client sent, then validate it.
///
/// # Errors
/// [`Problem`] with reason `malformed` when the value is not an array of well-typed items, or
/// the first reason [`validate_items`] finds.
pub fn parse_items(value: &serde_json::Value) -> Result<Vec<Item>, Problem> {
    let items: Vec<Item> =
        serde_json::from_value(value.clone()).map_err(|_| Problem::at("items", "malformed"))?;
    validate_items(&items)?;
    Ok(items)
}

/// Check a question's items against the bounds.
///
/// # Errors
/// The first [`Problem`] found, in item order.
pub fn validate_items(items: &[Item]) -> Result<(), Problem> {
    if items.is_empty() {
        return Err(Problem::at("items", "no_items"));
    }
    if items.len() > MAX_ITEMS {
        return Err(Problem::at("items", "too_many_items"));
    }
    for (index, item) in items.iter().enumerate() {
        let at = format!("items[{index}]");
        if !id_is_well_formed(&item.id) {
            return Err(Problem::at(format!("{at}.id"), "bad_id"));
        }
        if items[..index].iter().any(|earlier| earlier.id == item.id) {
            return Err(Problem::at(format!("{at}.id"), "duplicate_id"));
        }
        let Some(kind) = ItemKind::parse(&item.kind) else {
            return Err(Problem::at(format!("{at}.kind"), "unsupported_kind"));
        };
        check_text(&format!("{at}.prompt"), &item.prompt, MAX_TEXT, false)?;
        match kind {
            ItemKind::Text => {
                if !item.options.is_empty() {
                    return Err(Problem::at(format!("{at}.options"), "options_on_text"));
                }
                if item.allow_other {
                    return Err(Problem::at(format!("{at}.allow_other"), "other_on_text"));
                }
            }
            ItemKind::Single | ItemKind::Multi => {
                if item.options.len() < MIN_OPTIONS {
                    return Err(Problem::at(format!("{at}.options"), "too_few_options"));
                }
                if item.options.len() > MAX_OPTIONS {
                    return Err(Problem::at(format!("{at}.options"), "too_many_options"));
                }
                validate_options(&at, &item.options)?;
            }
        }
    }
    Ok(())
}

fn validate_options(at: &str, options: &[OptionSpec]) -> Result<(), Problem> {
    for (index, option) in options.iter().enumerate() {
        let here = format!("{at}.options[{index}]");
        if !id_is_well_formed(&option.id) {
            return Err(Problem::at(format!("{here}.id"), "bad_id"));
        }
        if options[..index]
            .iter()
            .any(|earlier| earlier.id == option.id)
        {
            return Err(Problem::at(format!("{here}.id"), "duplicate_id"));
        }
        check_text(&format!("{here}.label"), &option.label, MAX_LABEL, false)?;
        check_text(
            &format!("{here}.description"),
            &option.description,
            MAX_TEXT,
            true,
        )?;
    }
    Ok(())
}

/// Check an answer against the question it answers. `skip` dismisses the question and carries
/// no answers; otherwise every item is answered exactly once.
///
/// # Errors
/// The first [`Problem`] found.
pub fn validate_answers(items: &[Item], answers: &[Answer], skip: bool) -> Result<(), Problem> {
    if skip {
        return if answers.is_empty() {
            Ok(())
        } else {
            Err(Problem::at("answers", "skip_with_answers"))
        };
    }
    for (index, answer) in answers.iter().enumerate() {
        let at = format!("answers[{index}]");
        if !items.iter().any(|item| item.id == answer.item_id) {
            return Err(Problem::at(format!("{at}.item_id"), "unknown_item"));
        }
        if answers[..index]
            .iter()
            .any(|earlier| earlier.item_id == answer.item_id)
        {
            return Err(Problem::at(format!("{at}.item_id"), "duplicate_answer"));
        }
    }
    for item in items {
        let Some((index, answer)) = answers
            .iter()
            .enumerate()
            .find(|(_, answer)| answer.item_id == item.id)
        else {
            return Err(Problem::at(
                format!("answers[{}]", item.id),
                "missing_answer",
            ));
        };
        validate_answer(&format!("answers[{index}]"), item, answer)?;
    }
    Ok(())
}

fn validate_answer(at: &str, item: &Item, answer: &Answer) -> Result<(), Problem> {
    // An item that passed `validate_items` always has a known kind; anything else is not answerable.
    let Some(kind) = ItemKind::parse(&item.kind) else {
        return Err(Problem::at(format!("{at}.item_id"), "unsupported_kind"));
    };
    let has_text = !answer.text.is_empty();
    if has_text {
        check_text(&format!("{at}.text"), &answer.text, MAX_TEXT, false)?;
    }
    for (index, option_id) in answer.option_ids.iter().enumerate() {
        if !item.options.iter().any(|option| option.id == *option_id) {
            return Err(Problem::at(
                format!("{at}.option_ids[{index}]"),
                "unknown_option",
            ));
        }
        if answer.option_ids[..index].contains(option_id) {
            return Err(Problem::at(
                format!("{at}.option_ids[{index}]"),
                "duplicate_option",
            ));
        }
    }
    match kind {
        ItemKind::Text => {
            if !answer.option_ids.is_empty() {
                return Err(Problem::at(format!("{at}.option_ids"), "options_on_text"));
            }
            if !has_text {
                return Err(Problem::at(format!("{at}.text"), "empty_text"));
            }
        }
        ItemKind::Single | ItemKind::Multi => {
            if has_text && !item.allow_other {
                return Err(Problem::at(format!("{at}.text"), "other_not_allowed"));
            }
            let chosen = answer.option_ids.len() + usize::from(has_text);
            if chosen == 0 {
                return Err(Problem::at(format!("{at}.option_ids"), "no_choice"));
            }
            if kind == ItemKind::Single && chosen > 1 {
                return Err(Problem::at(format!("{at}.option_ids"), "too_many_choices"));
            }
        }
    }
    Ok(())
}

/// A digest of an answer that does not depend on the order it was written in, so an identical
/// retry is told apart from a different second answer. Hex blake3 over the canonical JSON.
#[must_use]
pub fn answers_digest(answers: &[Answer], skip: bool) -> String {
    let mut canonical: Vec<Answer> = answers
        .iter()
        .map(|answer| {
            let mut answer = answer.clone();
            answer.option_ids.sort();
            answer
        })
        .collect();
    canonical.sort_by(|a, b| a.item_id.cmp(&b.item_id));
    let body = serde_json::json!({ "skip": skip, "answers": canonical });
    blake3::hash(body.to_string().as_bytes())
        .to_hex()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: &str) -> OptionSpec {
        OptionSpec {
            id: id.to_owned(),
            label: format!("Label {id}"),
            description: String::new(),
        }
    }

    fn single(id: &str) -> Item {
        Item {
            id: id.to_owned(),
            kind: "single".to_owned(),
            prompt: "Which one?".to_owned(),
            options: vec![option("a"), option("b")],
            allow_other: false,
        }
    }

    fn answer(item_id: &str, options: &[&str], text: &str) -> Answer {
        Answer {
            item_id: item_id.to_owned(),
            option_ids: options.iter().map(|o| (*o).to_owned()).collect(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn the_bounds_are_the_ones_the_plan_names() {
        assert_eq!(MAX_ITEMS, 4);
        assert_eq!(MAX_OPTIONS, 6);
        assert_eq!(MAX_TEXT, 2000);
        assert_eq!(MAX_QUESTIONS_PER_RUN, 3);
        assert_eq!(delivery_ttl_secs(DELIVERY_NEXT_TURN), Some(86_400));
        assert_eq!(delivery_ttl_secs(DELIVERY_RESUME), Some(3_600));
        assert_eq!(delivery_ttl_secs("later"), None);
    }

    #[test]
    fn a_question_at_every_bound_is_accepted_and_one_past_it_is_not() {
        let mut items: Vec<Item> = (0..MAX_ITEMS).map(|n| single(&format!("i{n}"))).collect();
        assert_eq!(validate_items(&items), Ok(()));
        items.push(single("one-more"));
        assert_eq!(validate_items(&items).unwrap_err().reason, "too_many_items");

        let mut item = single("q");
        item.options = (0..MAX_OPTIONS).map(|n| option(&format!("o{n}"))).collect();
        assert_eq!(validate_items(std::slice::from_ref(&item)), Ok(()));
        item.options.push(option("o-extra"));
        assert_eq!(
            validate_items(std::slice::from_ref(&item))
                .unwrap_err()
                .reason,
            "too_many_options"
        );
        item.options.truncate(1);
        assert_eq!(
            validate_items(std::slice::from_ref(&item))
                .unwrap_err()
                .reason,
            "too_few_options"
        );

        let mut text = single("t");
        text.kind = "text".to_owned();
        text.options.clear();
        text.prompt = "x".repeat(MAX_TEXT);
        assert_eq!(validate_items(std::slice::from_ref(&text)), Ok(()));
        text.prompt.push('x');
        assert_eq!(
            validate_items(std::slice::from_ref(&text))
                .unwrap_err()
                .reason,
            "text_too_long"
        );
    }

    #[test]
    fn no_credential_kind_exists() {
        for kind in [
            "secret",
            "password",
            "credential",
            "token",
            "Single",
            " single",
            "single ",
            "",
        ] {
            let mut item = single("q");
            item.kind = kind.to_owned();
            assert_eq!(
                validate_items(&[item]).unwrap_err().reason,
                "unsupported_kind",
                "{kind:?}"
            );
        }
    }

    #[test]
    fn ids_must_be_well_formed_and_unique() {
        for bad in ["", "Upper", "has space", "dot.id", &"x".repeat(MAX_ID + 1)] {
            assert_eq!(
                validate_items(&[single(bad)]).unwrap_err().reason,
                "bad_id",
                "{bad:?}"
            );
        }
        assert_eq!(
            validate_items(&[single("same"), single("same")])
                .unwrap_err()
                .reason,
            "duplicate_id"
        );
        let mut item = single("q");
        item.options = vec![option("a"), option("a")];
        assert_eq!(validate_items(&[item]).unwrap_err().reason, "duplicate_id");
    }

    #[test]
    fn a_text_item_takes_no_options_and_no_other() {
        let mut item = single("t");
        item.kind = "text".to_owned();
        assert_eq!(
            validate_items(std::slice::from_ref(&item))
                .unwrap_err()
                .reason,
            "options_on_text"
        );
        item.options.clear();
        item.allow_other = true;
        assert_eq!(
            validate_items(std::slice::from_ref(&item))
                .unwrap_err()
                .reason,
            "other_on_text"
        );
    }

    #[test]
    fn control_characters_and_blank_text_are_refused() {
        let mut item = single("q");
        item.prompt = "line one\nline two\tend".to_owned();
        assert_eq!(validate_items(std::slice::from_ref(&item)), Ok(()));
        item.prompt = "bell\u{7}".to_owned();
        assert_eq!(
            validate_items(std::slice::from_ref(&item))
                .unwrap_err()
                .reason,
            "control_character"
        );
        item.prompt = "   ".to_owned();
        assert_eq!(
            validate_items(std::slice::from_ref(&item))
                .unwrap_err()
                .reason,
            "empty_text"
        );
    }

    #[test]
    fn parse_items_refuses_unknown_fields_and_wrong_types() {
        let ok = serde_json::json!([{"id":"q","kind":"single","prompt":"p",
            "options":[{"id":"a","label":"A"},{"id":"b","label":"B"}]}]);
        assert_eq!(parse_items(&ok).expect("valid").len(), 1);
        for bad in [
            serde_json::json!({"id":"q"}),
            serde_json::json!([{"id":"q","kind":"text","prompt":"p","secret":true}]),
            serde_json::json!([{"id":1,"kind":"text","prompt":"p"}]),
        ] {
            assert_eq!(parse_items(&bad).unwrap_err().reason, "malformed", "{bad}");
        }
    }

    #[test]
    fn a_single_item_takes_exactly_one_choice() {
        let mut item = single("q");
        item.allow_other = true;
        let items = [item];
        assert_eq!(
            validate_answers(&items, &[answer("q", &["a"], "")], false),
            Ok(())
        );
        assert_eq!(
            validate_answers(&items, &[answer("q", &[], "something else")], false),
            Ok(())
        );
        for (given, reason) in [
            (answer("q", &[], ""), "no_choice"),
            (answer("q", &["a", "b"], ""), "too_many_choices"),
            (answer("q", &["a"], "and more"), "too_many_choices"),
            (answer("q", &["z"], ""), "unknown_option"),
        ] {
            assert_eq!(
                validate_answers(&items, &[given], false)
                    .unwrap_err()
                    .reason,
                reason
            );
        }
    }

    #[test]
    fn other_text_needs_the_item_to_allow_it() {
        let items = [single("q")];
        assert_eq!(
            validate_answers(&items, &[answer("q", &[], "free")], false)
                .unwrap_err()
                .reason,
            "other_not_allowed"
        );
    }

    #[test]
    fn a_multi_item_takes_one_or_more_distinct_choices() {
        let mut item = single("q");
        item.kind = "multi".to_owned();
        let items = [item];
        assert_eq!(
            validate_answers(&items, &[answer("q", &["a", "b"], "")], false),
            Ok(())
        );
        assert_eq!(
            validate_answers(&items, &[answer("q", &["a", "a"], "")], false)
                .unwrap_err()
                .reason,
            "duplicate_option"
        );
        assert_eq!(
            validate_answers(&items, &[answer("q", &[], "")], false)
                .unwrap_err()
                .reason,
            "no_choice"
        );
    }

    #[test]
    fn a_text_item_needs_text_and_no_options() {
        let mut item = single("t");
        item.kind = "text".to_owned();
        item.options.clear();
        let items = [item];
        assert_eq!(
            validate_answers(&items, &[answer("t", &[], "hello")], false),
            Ok(())
        );
        assert_eq!(
            validate_answers(&items, &[answer("t", &[], "")], false)
                .unwrap_err()
                .reason,
            "empty_text"
        );
        assert_eq!(
            validate_answers(&items, &[answer("t", &["a"], "hello")], false)
                .unwrap_err()
                .reason,
            "unknown_option"
        );
        assert_eq!(
            validate_answers(
                &items,
                &[answer("t", &[], &"x".repeat(MAX_TEXT + 1))],
                false
            )
            .unwrap_err()
            .reason,
            "text_too_long"
        );
    }

    #[test]
    fn every_item_is_answered_once_and_a_skip_carries_no_answers() {
        let items = [single("a"), single("b")];
        assert_eq!(
            validate_answers(&items, &[answer("a", &["a"], "")], false)
                .unwrap_err()
                .reason,
            "missing_answer"
        );
        assert_eq!(
            validate_answers(
                &items,
                &[answer("a", &["a"], ""), answer("z", &["a"], "")],
                false
            )
            .unwrap_err()
            .reason,
            "unknown_item"
        );
        assert_eq!(
            validate_answers(
                &items,
                &[answer("a", &["a"], ""), answer("a", &["b"], "")],
                false
            )
            .unwrap_err()
            .reason,
            "duplicate_answer"
        );
        assert_eq!(validate_answers(&items, &[], true), Ok(()));
        assert_eq!(
            validate_answers(&items, &[answer("a", &["a"], "")], true)
                .unwrap_err()
                .reason,
            "skip_with_answers"
        );
    }

    #[test]
    fn the_digest_ignores_order_and_tells_answers_apart() {
        let one = [answer("a", &["x", "y"], ""), answer("b", &[], "t")];
        let reordered = [answer("b", &[], "t"), answer("a", &["y", "x"], "")];
        let other = [answer("a", &["x"], ""), answer("b", &[], "t")];
        assert_eq!(
            answers_digest(&one, false),
            answers_digest(&reordered, false)
        );
        assert_ne!(answers_digest(&one, false), answers_digest(&other, false));
        assert_ne!(answers_digest(&[], true), answers_digest(&[], false));
        assert_eq!(answers_digest(&one, false).len(), 64);
    }
}
