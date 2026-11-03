//! An MCP elicitation form, mapped onto the run-question shape and back (build plan EP-4 S4, D8).
//!
//! An MCP server may ask the user for a few values in the middle of a tool call
//! (`elicitation/create`, protocol revision 2025-06-18). Its `requestedSchema` is a flat object of
//! primitive properties. This module turns such a schema into [`user_question::Item`]s, so the
//! owner answers it with the same closed card as any run question, and turns the owner's answers
//! back into the `content` object the server expects.
//!
//! The mapping is deliberately narrow: `string` becomes a text item, `string` with an `enum` of 2
//! to 6 values a single choice, `boolean` a Ja/Nei choice, `number`/`integer` a text item checked
//! as a number when answered. Anything else (nested objects, arrays, more than
//! [`user_question::MAX_ITEMS`] fields, a field that looks like a credential) is declined: the
//! specification forbids servers to ask for sensitive data and the card has no secret kind, so
//! such a form is refused instead of being shown. The gateway and execution-core both read this
//! one mapping, so the form the owner saw and the content the server receives cannot drift.

use serde_json::{Map, Value};

use crate::user_question::{
    self, Answer, Item, ItemKind, OptionSpec, Problem, MAX_ID, MAX_ITEMS, MAX_LABEL, MAX_OPTIONS,
    MAX_TEXT, MIN_OPTIONS,
};

/// Option id of the "yes" choice a boolean field becomes.
pub const BOOLEAN_YES: &str = "ja";
/// Option id of the "no" choice a boolean field becomes.
pub const BOOLEAN_NO: &str = "nei";

/// Credential-like words matched anywhere in a field's name or title, once case and separators
/// are dropped (`api_key`, `apiKey` and `API Key` all read `apikey`).
const SENSITIVE_FRAGMENTS: &[&str] = &[
    "password",
    "passwd",
    "passord",
    "secret",
    "token",
    "apikey",
    "personnummer",
    "fødselsnummer",
    "fodselsnummer",
    "creditcard",
    "cardnumber",
    "kortnummer",
    "cvv",
    "cvc",
];

/// Credential-like words matched only as a whole word, because as fragments they sit inside
/// ordinary names (`shipping` holds `pin`, `discard` holds `card`).
const SENSITIVE_WORDS: &[&str] = &["pin", "card", "kort"];

/// Why a form was declined to the server. Stable `snake_case` tokens; the model reads
/// "form declined: not supported", never the form itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decline {
    pub reason: &'static str,
}

impl Decline {
    const fn because(reason: &'static str) -> Self {
        Self { reason }
    }
}

impl std::fmt::Display for Decline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "form declined: {}", self.reason)
    }
}

impl std::error::Error for Decline {}

/// Whether a field's name or title looks like it asks for a credential or a national id.
#[must_use]
pub fn is_sensitive_field(text: &str) -> bool {
    let squashed: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    if SENSITIVE_FRAGMENTS
        .iter()
        .any(|fragment| squashed.contains(fragment))
    {
        return true;
    }
    words(text)
        .iter()
        .any(|word| SENSITIVE_WORDS.contains(&word.as_str()))
}

/// Lower-case words of `text`, split on anything not alphanumeric and on a camelCase boundary.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if c.is_uppercase() && previous_lower && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        previous_lower = c.is_lowercase() || c.is_ascii_digit();
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What one schema property is, once accepted.
enum FieldKind {
    Text {
        format: Option<Format>,
        min_length: Option<u64>,
        max_length: Option<u64>,
    },
    Choice {
        values: Vec<String>,
        labels: Vec<String>,
    },
    Boolean,
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
}

/// The string formats the specification names; anything else is declined.
#[derive(Clone, Copy)]
enum Format {
    Email,
    Uri,
    Date,
    DateTime,
}

impl Format {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "email" => Some(Self::Email),
            "uri" => Some(Self::Uri),
            "date" => Some(Self::Date),
            "date-time" => Some(Self::DateTime),
            _ => None,
        }
    }

    fn accepts(self, text: &str) -> bool {
        if text.chars().any(char::is_whitespace) {
            return false;
        }
        match self {
            Self::Email => text.split_once('@').is_some_and(|(local, domain)| {
                !local.is_empty()
                    && !domain.contains('@')
                    && domain.contains('.')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
            }),
            Self::Uri => text.split_once(':').is_some_and(|(scheme, rest)| {
                !rest.is_empty()
                    && scheme
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic())
                    && scheme
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
            }),
            Self::Date => is_date(text),
            Self::DateTime => text
                .split_once(['T', 't'])
                .is_some_and(|(date, time)| is_date(date) && time.len() >= 8),
        }
    }
}

/// `YYYY-MM-DD` with a month of 1-12 and a day of 1-31.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    if !(digits(*year, 4) && digits(*month, 2) && digits(*day, 2)) {
        return false;
    }
    let (Ok(month), Ok(day)) = (month.parse::<u8>(), day.parse::<u8>()) else {
        return false;
    };
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// One accepted property: its item id, its schema name and what it is.
struct Field<'a> {
    item_id: String,
    name: &'a str,
    property: &'a Map<String, Value>,
    kind: FieldKind,
}

fn id_is_well_formed(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The item id of the property at `index`: its own name when that is a valid item id, otherwise a
/// positional one. Both directions of the mapping derive it the same way.
fn item_id(index: usize, name: &str) -> String {
    if id_is_well_formed(name) {
        name.to_owned()
    } else {
        format!("field_{index}")
    }
}

fn string_of<'a>(property: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    property
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// Read and check every property of a flat elicitation schema.
fn fields(schema: &Value) -> Result<Vec<Field<'_>>, Decline> {
    let Some(schema) = schema.as_object() else {
        return Err(Decline::because("schema_not_an_object"));
    };
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(Decline::because("schema_not_an_object"));
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Err(Decline::because("no_fields"));
    };
    if properties.is_empty() {
        return Err(Decline::because("no_fields"));
    }
    if properties.len() > MAX_ITEMS {
        return Err(Decline::because("too_many_fields"));
    }
    let mut out = Vec::with_capacity(properties.len());
    for (index, (name, property)) in properties.iter().enumerate() {
        let Some(property) = property.as_object() else {
            return Err(Decline::because("malformed_field"));
        };
        if is_sensitive_field(name) || string_of(property, "title").is_some_and(is_sensitive_field)
        {
            return Err(Decline::because("sensitive_field"));
        }
        let kind = field_kind(property)?;
        out.push(Field {
            item_id: item_id(index, name),
            name,
            property,
            kind,
        });
    }
    Ok(out)
}

fn field_kind(property: &Map<String, Value>) -> Result<FieldKind, Decline> {
    let number = |key: &str| property.get(key).and_then(Value::as_f64);
    match property.get("type").and_then(Value::as_str) {
        Some("string") => {
            if let Some(values) = property.get("enum") {
                return choice_kind(property, values);
            }
            let format = match string_of(property, "format") {
                None => None,
                Some(token) => {
                    Some(Format::parse(token).ok_or(Decline::because("unsupported_format"))?)
                }
            };
            Ok(FieldKind::Text {
                format,
                min_length: property.get("minLength").and_then(Value::as_u64),
                max_length: property.get("maxLength").and_then(Value::as_u64),
            })
        }
        Some("boolean") => Ok(FieldKind::Boolean),
        Some(kind @ ("number" | "integer")) => Ok(FieldKind::Number {
            integer: kind == "integer",
            minimum: number("minimum"),
            maximum: number("maximum"),
        }),
        Some("object" | "array") => Err(Decline::because("nested_or_array")),
        _ => Err(Decline::because("unsupported_type")),
    }
}

fn choice_kind(property: &Map<String, Value>, values: &Value) -> Result<FieldKind, Decline> {
    let Some(values) = values.as_array() else {
        return Err(Decline::because("malformed_enum"));
    };
    if values.len() < MIN_OPTIONS || values.len() > MAX_OPTIONS {
        return Err(Decline::because("enum_size"));
    }
    let values: Vec<String> = values
        .iter()
        .map(|value| value.as_str().map(ToOwned::to_owned))
        .collect::<Option<_>>()
        .ok_or(Decline::because("malformed_enum"))?;
    // `enumNames` labels the values only when it labels every one of them.
    let labels = property
        .get("enumNames")
        .and_then(Value::as_array)
        .filter(|names| names.len() == values.len())
        .and_then(|names| {
            names
                .iter()
                .map(|name| name.as_str().map(ToOwned::to_owned))
                .collect::<Option<Vec<_>>>()
        })
        .unwrap_or_else(|| values.clone());
    Ok(FieldKind::Choice { values, labels })
}

fn bounded(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The prompt the owner reads for one field: its title (or name) and its description.
fn prompt_of(field: &Field<'_>) -> String {
    let title = string_of(field.property, "title").unwrap_or(field.name);
    let prompt = match string_of(field.property, "description") {
        Some(description) if description != title => format!("{title} - {description}"),
        _ => title.to_owned(),
    };
    bounded(&prompt, MAX_TEXT)
}

fn option(id: String, label: &str) -> OptionSpec {
    OptionSpec {
        id,
        label: bounded(label, MAX_LABEL),
        description: String::new(),
    }
}

/// Map a server's `requestedSchema` onto question items.
///
/// # Errors
/// [`Decline`] when the form is not a flat object of at most [`MAX_ITEMS`] supported primitive
/// properties, when a field looks sensitive, or when the mapped items fail the question bounds.
pub fn items_from_schema(schema: &Value) -> Result<Vec<Item>, Decline> {
    let fields = fields(schema)?;
    let items: Vec<Item> = fields
        .iter()
        .map(|field| {
            let (kind, options) = match &field.kind {
                FieldKind::Text { .. } | FieldKind::Number { .. } => (ItemKind::Text, Vec::new()),
                FieldKind::Choice { labels, .. } => (
                    ItemKind::Single,
                    labels
                        .iter()
                        .enumerate()
                        .map(|(index, label)| option(format!("o{index}"), label))
                        .collect(),
                ),
                FieldKind::Boolean => (
                    ItemKind::Single,
                    vec![
                        option(BOOLEAN_YES.to_owned(), "Ja"),
                        option(BOOLEAN_NO.to_owned(), "Nei"),
                    ],
                ),
            };
            Item {
                id: field.item_id.clone(),
                kind: kind.as_str().to_owned(),
                prompt: prompt_of(field),
                options,
                allow_other: false,
            }
        })
        .collect();
    // A server's text with control characters, or two names that map to one id, is not a form
    // the card can show faithfully.
    user_question::validate_items(&items).map_err(|_| Decline::because("invalid_form"))?;
    Ok(items)
}

fn problem(item_id: &str, reason: &'static str) -> Problem {
    Problem {
        field: format!("answers[{item_id}]"),
        reason,
    }
}

/// Turn the owner's answers into the `content` object of an `accept` result.
///
/// `items` are the ones [`items_from_schema`] produced for `schema`; every item must be answered.
///
/// # Errors
/// [`Problem`] when the answers do not fit the items ([`user_question::validate_answers`]), the
/// schema no longer maps (`unsupported_schema`), or a value breaks its field: `not_a_number`,
/// `out_of_range`, `bad_format`, `text_length`, `unknown_option`.
pub fn content_from_answers(
    schema: &Value,
    items: &[Item],
    answers: &[Answer],
) -> Result<Value, Problem> {
    user_question::validate_answers(items, answers, false)?;
    let fields = fields(schema).map_err(|_| Problem {
        field: "schema".to_owned(),
        reason: "unsupported_schema",
    })?;
    let mut content = Map::new();
    for field in &fields {
        let Some(answer) = answers.iter().find(|answer| answer.item_id == field.item_id) else {
            return Err(problem(&field.item_id, "missing_answer"));
        };
        let value = field_value(field, answer)?;
        content.insert(field.name.to_owned(), value);
    }
    Ok(Value::Object(content))
}

fn field_value(field: &Field<'_>, answer: &Answer) -> Result<Value, Problem> {
    let id = field.item_id.as_str();
    let chosen = || answer.option_ids.first().map(String::as_str);
    match &field.kind {
        FieldKind::Text {
            format,
            min_length,
            max_length,
        } => {
            let text = answer.text.trim();
            let length = text.chars().count() as u64;
            if min_length.is_some_and(|min| length < min) || max_length.is_some_and(|max| length > max)
            {
                return Err(problem(id, "text_length"));
            }
            if format.is_some_and(|format| !format.accepts(text)) {
                return Err(problem(id, "bad_format"));
            }
            Ok(Value::String(text.to_owned()))
        }
        FieldKind::Choice { values, .. } => chosen()
            .and_then(|option_id| option_id.strip_prefix('o'))
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| values.get(index))
            .map(|value| Value::String(value.clone()))
            .ok_or_else(|| problem(id, "unknown_option")),
        FieldKind::Boolean => match chosen() {
            Some(BOOLEAN_YES) => Ok(Value::Bool(true)),
            Some(BOOLEAN_NO) => Ok(Value::Bool(false)),
            _ => Err(problem(id, "unknown_option")),
        },
        FieldKind::Number {
            integer,
            minimum,
            maximum,
        } => {
            let text = answer.text.trim();
            let (value, as_f64) = if *integer {
                let parsed = text
                    .parse::<i64>()
                    .map_err(|_| problem(id, "not_a_number"))?;
                // Only compared against the schema's bounds, which are JSON numbers already.
                #[allow(clippy::cast_precision_loss)]
                let as_f64 = parsed as f64;
                (Value::from(parsed), as_f64)
            } else {
                let parsed = text
                    .parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| problem(id, "not_a_number"))?;
                let number = serde_json::Number::from_f64(parsed)
                    .ok_or_else(|| problem(id, "not_a_number"))?;
                (Value::Number(number), parsed)
            };
            if minimum.is_some_and(|min| as_f64 < min) || maximum.is_some_and(|max| as_f64 > max) {
                return Err(problem(id, "out_of_range"));
            }
            Ok(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project_form() -> Value {
        json!({
            "type": "object",
            "properties": {
                "project": {
                    "type": "string",
                    "title": "Prosjekt",
                    "enum": ["alpha", "beta", "gamma"],
                    "enumNames": ["Alpha", "Beta", "Gamma"]
                },
                "notify": { "type": "boolean", "title": "Varsle teamet" },
                "hours": { "type": "number", "minimum": 0, "maximum": 24 },
                "note": { "type": "string", "description": "Kort merknad", "maxLength": 20 }
            },
            "required": ["project"]
        })
    }

    fn item<'a>(items: &'a [Item], id: &str) -> &'a Item {
        items.iter().find(|item| item.id == id).expect("item")
    }

    fn answer(item_id: &str, options: &[&str], text: &str) -> Answer {
        Answer {
            item_id: item_id.to_owned(),
            option_ids: options.iter().map(|o| (*o).to_owned()).collect(),
            text: text.to_owned(),
        }
    }

    fn full_answers(hours: &str, notify: &str) -> Vec<Answer> {
        vec![
            answer("project", &["o1"], ""),
            answer("notify", &[notify], ""),
            answer("hours", &[], hours),
            answer("note", &[], "ok"),
        ]
    }

    #[test]
    fn string_enum_boolean_number_map_to_items() {
        let items = items_from_schema(&project_form()).expect("maps");
        assert_eq!(items.len(), 4);

        let project = item(&items, "project");
        assert_eq!(project.kind, "single");
        assert_eq!(project.prompt, "Prosjekt");
        let labels: Vec<&str> = project.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Alpha", "Beta", "Gamma"]);

        let notify = item(&items, "notify");
        assert_eq!(notify.kind, "single");
        let ids: Vec<&str> = notify.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, [BOOLEAN_YES, BOOLEAN_NO]);

        assert_eq!(item(&items, "hours").kind, "text");
        let note = item(&items, "note");
        assert_eq!(note.kind, "text");
        assert_eq!(note.prompt, "note - Kort merknad");
        assert!(items.iter().all(|item| !item.allow_other));
    }

    #[test]
    fn a_name_that_is_not_an_item_id_gets_a_positional_one_both_ways() {
        let schema = json!({
            "type": "object",
            "properties": { "Project Name": { "type": "string" } }
        });
        let items = items_from_schema(&schema).expect("maps");
        assert_eq!(items[0].id, "field_0");
        let content =
            content_from_answers(&schema, &items, &[answer("field_0", &[], "Apollo")]).expect("ok");
        assert_eq!(content, json!({ "Project Name": "Apollo" }));
    }

    #[test]
    fn nested_or_array_schema_is_declined() {
        for nested in [
            json!({ "type": "object", "properties": { "x": { "type": "string" } } }),
            json!({ "type": "array", "items": { "type": "string" } }),
        ] {
            let schema = json!({ "type": "object", "properties": { "inner": nested } });
            assert_eq!(
                items_from_schema(&schema),
                Err(Decline::because("nested_or_array"))
            );
        }
        assert_eq!(
            items_from_schema(&json!({ "type": "array" })),
            Err(Decline::because("schema_not_an_object"))
        );
    }

    #[test]
    fn more_than_four_fields_is_declined() {
        let mut properties = Map::new();
        for name in ["a", "b", "c", "d", "e"] {
            properties.insert(name.to_owned(), json!({ "type": "string" }));
        }
        let schema = json!({ "type": "object", "properties": properties });
        assert_eq!(
            items_from_schema(&schema),
            Err(Decline::because("too_many_fields"))
        );
    }

    #[test]
    fn an_enum_outside_two_to_six_values_is_declined() {
        let schema = json!({
            "type": "object",
            "properties": { "only": { "type": "string", "enum": ["one"] } }
        });
        assert_eq!(items_from_schema(&schema), Err(Decline::because("enum_size")));
    }

    #[test]
    fn sensitive_field_names_are_declined() {
        for name in [
            "password",
            "Passord",
            "api_key",
            "apiKey",
            "client_secret",
            "accessToken",
            "pin",
            "card_cvv",
            "personnummer",
            "Fødselsnummer",
        ] {
            let schema = json!({
                "type": "object",
                "properties": { name: { "type": "string" } }
            });
            assert_eq!(
                items_from_schema(&schema),
                Err(Decline::because("sensitive_field")),
                "{name}"
            );
        }
        // A title can make an innocent name sensitive.
        let titled = json!({
            "type": "object",
            "properties": { "value": { "type": "string", "title": "Your PIN" } }
        });
        assert_eq!(
            items_from_schema(&titled),
            Err(Decline::because("sensitive_field"))
        );
        // Short words count only as whole words.
        for innocent in ["shipping_method", "discard_draft", "spinner"] {
            assert!(!is_sensitive_field(innocent), "{innocent}");
        }
    }

    #[test]
    fn number_answer_must_parse() {
        let schema = project_form();
        let items = items_from_schema(&schema).expect("maps");
        let content = content_from_answers(&schema, &items, &full_answers("7.5", BOOLEAN_YES))
            .expect("accepted");
        assert_eq!(content["hours"], json!(7.5));
        assert_eq!(content["project"], json!("beta"));

        let refused = content_from_answers(&schema, &items, &full_answers("sju", BOOLEAN_YES))
            .expect_err("not a number");
        assert_eq!(refused.reason, "not_a_number");
        let out_of_range = content_from_answers(&schema, &items, &full_answers("25", BOOLEAN_YES))
            .expect_err("above the maximum");
        assert_eq!(out_of_range.reason, "out_of_range");
    }

    #[test]
    fn integer_answer_rejects_a_fraction() {
        let schema = json!({
            "type": "object",
            "properties": { "count": { "type": "integer" } }
        });
        let items = items_from_schema(&schema).expect("maps");
        let refused = content_from_answers(&schema, &items, &[answer("count", &[], "2.5")])
            .expect_err("a fraction");
        assert_eq!(refused.reason, "not_a_number");
        let content =
            content_from_answers(&schema, &items, &[answer("count", &[], "3")]).expect("ok");
        assert_eq!(content, json!({ "count": 3 }));
    }

    #[test]
    fn boolean_answer_maps_from_option_id() {
        let schema = project_form();
        let items = items_from_schema(&schema).expect("maps");
        let yes = content_from_answers(&schema, &items, &full_answers("1", BOOLEAN_YES))
            .expect("accepted");
        assert_eq!(yes["notify"], json!(true));
        let no = content_from_answers(&schema, &items, &full_answers("1", BOOLEAN_NO))
            .expect("accepted");
        assert_eq!(no["notify"], json!(false));
    }

    #[test]
    fn formats_and_lengths_are_checked_at_answer_time() {
        let schema = json!({
            "type": "object",
            "properties": {
                "email": { "type": "string", "format": "email" },
                "day": { "type": "string", "format": "date" }
            }
        });
        let items = items_from_schema(&schema).expect("maps");
        let good = [
            answer("email", &[], "ola@example.no"),
            answer("day", &[], "2026-11-03"),
        ];
        assert!(content_from_answers(&schema, &items, &good).is_ok());
        let bad = [
            answer("email", &[], "ola at example"),
            answer("day", &[], "2026-11-03"),
        ];
        assert_eq!(
            content_from_answers(&schema, &items, &bad)
                .expect_err("bad email")
                .reason,
            "bad_format"
        );
        let unknown_format = json!({
            "type": "object",
            "properties": { "x": { "type": "string", "format": "ipv4" } }
        });
        assert_eq!(
            items_from_schema(&unknown_format),
            Err(Decline::because("unsupported_format"))
        );
    }

    #[test]
    fn every_item_must_be_answered() {
        let schema = project_form();
        let items = items_from_schema(&schema).expect("maps");
        let partial = [answer("project", &["o0"], "")];
        assert_eq!(
            content_from_answers(&schema, &items, &partial)
                .expect_err("incomplete")
                .reason,
            "missing_answer"
        );
    }
}
