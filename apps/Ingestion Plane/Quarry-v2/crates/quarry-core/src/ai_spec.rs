//! The AI formats of `POST /v1/scrape` (INNH-1): the request specs and the
//! response report. No I/O and no model call; the engine is in `quarry-runtime`
//! and the wiring in `quarry-edge`.
//!
//! Four formats ask a model about the page: `json` (extract by a schema),
//! `summary`, `question` and `highlights`. They are object formats on
//! `formats[]` (`summary` also as a bare string) and are parsed here with the
//! same rules as the capture formats: every bad field is a [`FieldError`] with
//! its path and a stable code, and a value is never echoed. The design is in
//! `.plan-work/quarry-w1w2/INNH-1-design.md`; the contract is in
//! `docs/CONTRACTS.md` ("Scrape AI formats") and `docs/openapi.yaml`.
//!
//! Model output is untrusted. These types carry what the caller asked for and
//! what Quarry reports; the checks on the output (schema, verbatim quotes) are
//! the engine's, and a failed model call never fails the scrape: it is an
//! `error` or `skipped` entry in [`AiReport`] inside a 200.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::scrape::FieldError;

// ---- limits ---------------------------------------------------------------

/// Serialized size of a `json` schema, as sent.
pub const MAX_SCHEMA_BYTES: usize = 65_536;
/// Nesting of schema nodes (`properties` and `items` each add one); the root is 1.
pub const MAX_SCHEMA_DEPTH: usize = 10;
/// `properties` entries in the whole schema.
pub const MAX_SCHEMA_PROPERTIES: usize = 200;
/// Field errors reported for one schema (the walk stops there).
const MAX_SCHEMA_ERRORS: usize = 20;
/// A `json.prompt` or `question.question`, in characters.
pub const MAX_PROMPT_CHARS: usize = 10_000;
/// A `highlights.query`, in characters.
pub const MAX_QUERY_CHARS: usize = 1_000;
pub const SUMMARY_MIN_CHARS: u32 = 200;
pub const SUMMARY_MAX_CHARS: u32 = 10_000;
pub const SUMMARY_DEFAULT_CHARS: u32 = 2_000;
pub const HIGHLIGHTS_MAX_COUNT: u32 = 10;
pub const HIGHLIGHTS_DEFAULT_COUNT: u32 = 3;
pub const HIGHLIGHT_MIN_CHARS_EACH: u32 = 40;
pub const HIGHLIGHT_MAX_CHARS_EACH: u32 = 1_000;
pub const HIGHLIGHT_DEFAULT_CHARS_EACH: u32 = 300;
/// The shortest quote Quarry returns as a highlight (shorter ones are too
/// easy to find by chance and say nothing).
pub const HIGHLIGHT_MIN_QUOTE_CHARS: usize = 8;
/// The most a model's JSON output may serialize to.
pub const MAX_AI_JSON_OUTPUT_BYTES: usize = 256 * 1024;
/// The most an `answer` may hold, in characters.
pub const MAX_ANSWER_CHARS: usize = 10_000;
/// Schema issues listed in a report (paths and codes only).
pub const MAX_SCHEMA_ISSUES: usize = 20;

/// Schema keywords the validator in [`crate::json_schema`] checks. Anything
/// else it would ignore silently, which would make `schema_valid: true` a lie,
/// so a schema that uses one is refused.
const ALLOWED_KEYWORDS: [&str; 14] = [
    "type",
    "properties",
    "required",
    "items",
    "enum",
    "minItems",
    "maxItems",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "additionalProperties",
    "description",
    "title",
];
/// Keywords dropped without complaint: they name or annotate the schema, not the data.
const STRIPPED_KEYWORDS: [&str; 3] = ["$schema", "$id", "$comment"];
const SCHEMA_TYPES: [&str; 7] = [
    "object", "array", "string", "integer", "number", "boolean", "null",
];

// ---- the specs ------------------------------------------------------------

/// Which AI format. The declaration order is the order the specs are listed in
/// (`ai_specs`, the cache key, the report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AiKind {
    Json,
    Summary,
    Question,
    Highlights,
}

impl AiKind {
    pub const ALL: [AiKind; 4] = [Self::Json, Self::Summary, Self::Question, Self::Highlights];

    /// The wire name: a format's `type`, and its name in `effective_options.formats`.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Summary => "summary",
            Self::Question => "question",
            Self::Highlights => "highlights",
        }
    }

    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "json" => Some(Self::Json),
            "summary" => Some(Self::Summary),
            "question" => Some(Self::Question),
            "highlights" => Some(Self::Highlights),
            _ => None,
        }
    }

    /// The field a bare name of this kind lacks (`None` for `summary`, which has defaults).
    pub const fn required_field(self) -> Option<&'static str> {
        match self {
            Self::Json => Some("schema"),
            Self::Summary => None,
            Self::Question => Some("question"),
            Self::Highlights => Some("query"),
        }
    }
}

/// `{"type":"json","schema":{...},"prompt":"..."}`.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonSpec {
    /// The schema as the caller wrote it, minus `$schema`, `$id` and `$comment`.
    pub schema: Value,
    pub prompt: Option<String>,
}

/// `{"type":"summary","max_chars":2000,"language":"nb"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummarySpec {
    pub max_chars: u32,
    /// BCP-47-ish tag; `None` means the page's own language.
    pub language: Option<String>,
}

impl Default for SummarySpec {
    fn default() -> Self {
        Self {
            max_chars: SUMMARY_DEFAULT_CHARS,
            language: None,
        }
    }
}

/// `{"type":"question","question":"..."}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionSpec {
    pub question: String,
}

/// `{"type":"highlights","query":"...","max_highlights":3,"max_chars_each":300}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightsSpec {
    pub query: String,
    pub max_highlights: u32,
    pub max_chars_each: u32,
}

/// One checked AI format of a request.
#[derive(Debug, Clone, PartialEq)]
pub enum AiSpec {
    Json(JsonSpec),
    Summary(SummarySpec),
    Question(QuestionSpec),
    Highlights(HighlightsSpec),
}

impl AiSpec {
    pub fn kind(&self) -> AiKind {
        match self {
            Self::Json(_) => AiKind::Json,
            Self::Summary(_) => AiKind::Summary,
            Self::Question(_) => AiKind::Question,
            Self::Highlights(_) => AiKind::Highlights,
        }
    }

    /// The spec a bare `"summary"` stands for.
    pub fn default_summary() -> Self {
        Self::Summary(SummarySpec::default())
    }

    /// The kind alone, as a spec with nothing in it. For reading the echo of a
    /// stored record back (`effective_options.formats` holds the name only);
    /// never used to run anything. `None` for a name that is not an AI kind.
    pub fn from_wire_echo(name: &str) -> Option<Self> {
        Some(match AiKind::from_wire(name)? {
            AiKind::Json => Self::Json(JsonSpec {
                schema: serde_json::json!({ "type": "object" }),
                prompt: None,
            }),
            AiKind::Summary => Self::default_summary(),
            AiKind::Question => Self::Question(QuestionSpec {
                question: String::new(),
            }),
            AiKind::Highlights => Self::Highlights(HighlightsSpec {
                query: String::new(),
                max_highlights: HIGHLIGHTS_DEFAULT_COUNT,
                max_chars_each: HIGHLIGHT_DEFAULT_CHARS_EACH,
            }),
        })
    }

    /// The spec as canonical JSON: object keys sorted at every depth, the
    /// defaults filled in. Two requests that mean the same thing give the same text.
    pub fn canonical_json(&self) -> String {
        let value = match self {
            Self::Json(spec) => serde_json::json!({
                "kind": "json",
                "schema": spec.schema,
                "prompt": spec.prompt,
            }),
            Self::Summary(spec) => serde_json::json!({
                "kind": "summary",
                "max_chars": spec.max_chars,
                "language": spec.language,
            }),
            Self::Question(spec) => serde_json::json!({
                "kind": "question",
                "question": spec.question,
            }),
            Self::Highlights(spec) => serde_json::json!({
                "kind": "highlights",
                "query": spec.query,
                "max_highlights": spec.max_highlights,
                "max_chars_each": spec.max_chars_each,
            }),
        };
        let mut out = String::new();
        write_canonical(&value, &mut out);
        out
    }

    /// The first 16 hex digits of blake3 over [`Self::canonical_json`]. A key
    /// part for anything that must tell two specs apart without holding their
    /// content (the schema, prompt, question and query never appear in a key).
    pub fn canonical_hash16(&self) -> String {
        let mut hex = blake3::hash(self.canonical_json().as_bytes()).to_hex().to_string();
        hex.truncate(16);
        hex
    }
}

/// JSON with object keys in sorted order at every depth (serde_json's own map
/// may keep insertion order, so the order is made here).
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

// ---- parsing --------------------------------------------------------------

/// Parse the object form of an AI format. `body` is the object without its
/// `type`; `path` is `formats[i]`. All errors found are returned, not the first.
pub fn parse_ai_object(
    kind: AiKind,
    body: &Map<String, Value>,
    path: &str,
) -> Result<AiSpec, Vec<FieldError>> {
    let mut errors = Vec::new();
    let known: &[&str] = match kind {
        AiKind::Json => &["schema", "prompt"],
        AiKind::Summary => &["max_chars", "language"],
        AiKind::Question => &["question"],
        AiKind::Highlights => &["query", "max_highlights", "max_chars_each"],
    };
    for key in body.keys() {
        if !known.contains(&key.as_str()) {
            errors.push(FieldError::new(
                path,
                "unknown_field",
                format!(
                    "unknown option `{}` for the {} format; it takes: {}",
                    clip(key, 40),
                    kind.wire_name(),
                    known.join(", ")
                ),
            ));
        }
    }
    let spec = match kind {
        AiKind::Json => {
            let schema = parse_schema(body.get("schema"), &format!("{path}.schema"), &mut errors);
            let prompt = optional_text(
                body.get("prompt"),
                &format!("{path}.prompt"),
                MAX_PROMPT_CHARS,
                &mut errors,
            );
            schema.map(|schema| AiSpec::Json(JsonSpec { schema, prompt }))
        }
        AiKind::Summary => {
            let max_chars = optional_range(
                body.get("max_chars"),
                &format!("{path}.max_chars"),
                (SUMMARY_MIN_CHARS, SUMMARY_MAX_CHARS),
                &mut errors,
            );
            let language = parse_language(body.get("language"), &format!("{path}.language"), &mut errors);
            Some(AiSpec::Summary(SummarySpec {
                max_chars: max_chars.unwrap_or(SUMMARY_DEFAULT_CHARS),
                language,
            }))
        }
        AiKind::Question => required_text(
            body.get("question"),
            &format!("{path}.question"),
            MAX_PROMPT_CHARS,
            &mut errors,
        )
        .map(|question| AiSpec::Question(QuestionSpec { question })),
        AiKind::Highlights => {
            let query = required_text(
                body.get("query"),
                &format!("{path}.query"),
                MAX_QUERY_CHARS,
                &mut errors,
            );
            let count = optional_range(
                body.get("max_highlights"),
                &format!("{path}.max_highlights"),
                (1, HIGHLIGHTS_MAX_COUNT),
                &mut errors,
            );
            let each = optional_range(
                body.get("max_chars_each"),
                &format!("{path}.max_chars_each"),
                (HIGHLIGHT_MIN_CHARS_EACH, HIGHLIGHT_MAX_CHARS_EACH),
                &mut errors,
            );
            query.map(|query| {
                AiSpec::Highlights(HighlightsSpec {
                    query,
                    max_highlights: count.unwrap_or(HIGHLIGHTS_DEFAULT_COUNT),
                    max_chars_each: each.unwrap_or(HIGHLIGHT_DEFAULT_CHARS_EACH),
                })
            })
        }
    };
    match spec {
        Some(spec) if errors.is_empty() => Ok(spec),
        _ => Err(errors),
    }
}

/// The error for a bare `"json"`, `"question"` or `"highlights"`: the format
/// needs its object form because it has a required input.
pub fn bare_name_error(kind: AiKind, path: &str) -> Option<FieldError> {
    let field = kind.required_field()?;
    Some(FieldError::new(
        format!("{path}.{field}"),
        "missing_field",
        format!(
            "the {} format needs an object with a `{field}`: {{\"type\":\"{}\",\"{field}\":...}}",
            kind.wire_name(),
            kind.wire_name()
        ),
    ))
}

fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A text option: a string of 1..=`max` characters, or absent (or null).
fn optional_text(
    value: Option<&Value>,
    path: &str,
    max: usize,
    errors: &mut Vec<FieldError>,
) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(value) => check_text(value, path, max, errors),
    }
}

/// A required text option; absent (or null) is `missing_field`.
fn required_text(
    value: Option<&Value>,
    path: &str,
    max: usize,
    errors: &mut Vec<FieldError>,
) -> Option<String> {
    match value {
        None | Some(Value::Null) => {
            errors.push(FieldError::new(path, "missing_field", "this field is required"));
            None
        }
        Some(value) => check_text(value, path, max, errors),
    }
}

fn check_text(value: &Value, path: &str, max: usize, errors: &mut Vec<FieldError>) -> Option<String> {
    let Some(text) = value.as_str() else {
        errors.push(FieldError::new(path, "invalid_type", "expected a string"));
        return None;
    };
    if text.trim().is_empty() {
        errors.push(FieldError::new(path, "empty", "this field cannot be empty"));
        return None;
    }
    if text.chars().count() > max {
        errors.push(FieldError::new(
            path,
            "text_too_long",
            format!("at most {max} characters"),
        ));
        return None;
    }
    Some(text.to_owned())
}

/// An integer option within an inclusive range, or absent (or null).
fn optional_range(
    value: Option<&Value>,
    path: &str,
    (min, max): (u32, u32),
    errors: &mut Vec<FieldError>,
) -> Option<u32> {
    match value {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_u64() {
            Some(number) if (u64::from(min)..=u64::from(max)).contains(&number) => {
                u32::try_from(number).ok()
            }
            Some(_) => {
                errors.push(FieldError::new(
                    path,
                    "out_of_range",
                    format!("must be between {min} and {max}"),
                ));
                None
            }
            None => {
                errors.push(FieldError::new(path, "invalid_type", "expected a non-negative integer"));
                None
            }
        },
    }
}

/// `[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})?`.
fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let primary = parts.next().unwrap_or("");
    let region = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let primary_ok = (2..=3).contains(&primary.len()) && primary.chars().all(|c| c.is_ascii_alphabetic());
    let region_ok = region.is_none_or(|region| {
        (2..=8).contains(&region.len()) && region.chars().all(|c| c.is_ascii_alphanumeric())
    });
    primary_ok && region_ok
}

fn parse_language(value: Option<&Value>, path: &str, errors: &mut Vec<FieldError>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_str() {
            Some(tag) if is_language_tag(tag) => Some(tag.to_owned()),
            Some(_) => {
                errors.push(FieldError::new(
                    path,
                    "invalid_value",
                    "expected a language tag such as `en` or `nb-NO`",
                ));
                None
            }
            None => {
                errors.push(FieldError::new(path, "invalid_type", "expected a string"));
                None
            }
        },
    }
}

// ---- the json schema ------------------------------------------------------

/// Check a `json.schema` and return it normalised (`$schema`, `$id` and
/// `$comment` removed at every schema node). Errors have paths like
/// `formats[1].schema.properties.title.pattern`; a value is never echoed.
fn parse_schema(value: Option<&Value>, path: &str, errors: &mut Vec<FieldError>) -> Option<Value> {
    let schema = match value {
        None | Some(Value::Null) => {
            errors.push(FieldError::new(path, "missing_field", "this field is required"));
            return None;
        }
        Some(schema) => schema,
    };
    if !schema.is_object() {
        errors.push(FieldError::new(path, "invalid_type", "expected a JSON Schema object"));
        return None;
    }
    let size = serde_json::to_vec(schema).map(|bytes| bytes.len()).unwrap_or(usize::MAX);
    if size > MAX_SCHEMA_BYTES {
        errors.push(FieldError::new(
            path,
            "schema_too_large",
            format!("the schema is at most {MAX_SCHEMA_BYTES} bytes"),
        ));
        return None;
    }
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        errors.push(FieldError::new(
            path,
            "schema_root_not_object",
            "the schema's root must have \"type\": \"object\"",
        ));
        return None;
    }
    let mut walk = SchemaWalk {
        errors: Vec::new(),
        properties: 0,
        too_complex: false,
    };
    let normalised = walk.node(schema, path, 1);
    errors.append(&mut walk.errors);
    normalised
}

struct SchemaWalk {
    errors: Vec<FieldError>,
    properties: usize,
    /// `schema_too_complex` is reported once.
    too_complex: bool,
}

impl SchemaWalk {
    fn push(&mut self, path: &str, code: &str, message: &str) {
        if self.errors.len() < MAX_SCHEMA_ERRORS {
            self.errors.push(FieldError::new(path, code, message));
        }
    }

    fn complex(&mut self, path: &str, message: &str) {
        if !self.too_complex {
            self.too_complex = true;
            self.push(path, "schema_too_complex", message);
        }
    }

    /// One schema node. Returns its normalised copy.
    fn node(&mut self, node: &Value, path: &str, depth: usize) -> Option<Value> {
        let Some(object) = node.as_object() else {
            self.push(path, "invalid_type", "expected a schema object");
            return None;
        };
        if depth > MAX_SCHEMA_DEPTH {
            self.complex(
                path,
                &format!("the schema nests at most {MAX_SCHEMA_DEPTH} levels"),
            );
            return None;
        }
        let mut out = Map::new();
        for (key, value) in object {
            if STRIPPED_KEYWORDS.contains(&key.as_str()) {
                continue;
            }
            let at = format!("{path}.{}", clip(key, 64));
            if !ALLOWED_KEYWORDS.contains(&key.as_str()) {
                self.push(
                    &at,
                    "schema_keyword_not_supported",
                    "this keyword is not validated and so not accepted",
                );
                continue;
            }
            if let Some(kept) = self.keyword(key, value, &at, depth) {
                out.insert(key.clone(), kept);
            }
        }
        Some(Value::Object(out))
    }

    fn keyword(&mut self, key: &str, value: &Value, at: &str, depth: usize) -> Option<Value> {
        match key {
            "type" => {
                let ok = match value {
                    Value::String(name) => SCHEMA_TYPES.contains(&name.as_str()),
                    Value::Array(names) => {
                        !names.is_empty()
                            && names
                                .iter()
                                .all(|name| name.as_str().is_some_and(|name| SCHEMA_TYPES.contains(&name)))
                    }
                    _ => false,
                };
                if !ok {
                    self.push(at, "invalid_value", "expected a JSON Schema type name or a list of them");
                    return None;
                }
                Some(value.clone())
            }
            "required" => {
                let ok = value
                    .as_array()
                    .is_some_and(|names| names.iter().all(Value::is_string));
                if !ok {
                    self.push(at, "invalid_type", "expected an array of property names");
                    return None;
                }
                Some(value.clone())
            }
            "enum" => {
                if !value.is_array() {
                    self.push(at, "invalid_type", "expected an array");
                    return None;
                }
                Some(value.clone())
            }
            "minItems" | "maxItems" | "minLength" | "maxLength" => {
                if value.as_u64().is_none() {
                    self.push(at, "invalid_type", "expected a non-negative integer");
                    return None;
                }
                Some(value.clone())
            }
            "minimum" | "maximum" => {
                if !value.is_number() {
                    self.push(at, "invalid_type", "expected a number");
                    return None;
                }
                Some(value.clone())
            }
            "description" | "title" => {
                if !value.is_string() {
                    self.push(at, "invalid_type", "expected a string");
                    return None;
                }
                Some(value.clone())
            }
            "additionalProperties" => match value {
                Value::Bool(_) => Some(value.clone()),
                Value::Object(_) => {
                    self.push(
                        at,
                        "schema_keyword_not_supported",
                        "additionalProperties is accepted as true or false only",
                    );
                    None
                }
                _ => {
                    self.push(at, "invalid_type", "expected true or false");
                    None
                }
            },
            "items" => match value {
                Value::Object(_) => self.node(value, at, depth + 1),
                Value::Array(_) => {
                    self.push(
                        at,
                        "schema_keyword_not_supported",
                        "items is accepted as one schema, not a list",
                    );
                    None
                }
                _ => {
                    self.push(at, "invalid_type", "expected a schema object");
                    None
                }
            },
            "properties" => {
                let Some(map) = value.as_object() else {
                    self.push(at, "invalid_type", "expected an object of property schemas");
                    return None;
                };
                self.properties += map.len();
                if self.properties > MAX_SCHEMA_PROPERTIES {
                    self.complex(
                        at,
                        &format!("the schema has at most {MAX_SCHEMA_PROPERTIES} properties in all"),
                    );
                    return None;
                }
                let mut out = Map::new();
                for (name, schema) in map {
                    let child = format!("{at}.{}", clip(name, 64));
                    if let Some(kept) = self.node(schema, &child, depth + 1) {
                        out.insert(name.clone(), kept);
                    }
                }
                Some(Value::Object(out))
            }
            _ => None,
        }
    }
}

// ---- the report -----------------------------------------------------------

/// How one AI format ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiStatus {
    Ok,
    /// A model call was made and failed.
    Error,
    /// No model call was made for this format.
    Skipped,
}

/// Why a format is not `ok`. These are strings inside a 200 body, not wire
/// error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiCode {
    ModelUnavailable,
    ModelRateLimited,
    SchemaInvalid,
    ZdrUnavailable,
    BudgetExhausted,
    EmptySource,
    /// The request's privacy block forbids a model hop (`credential_or_secret`,
    /// `zdr_ephemeral`). Skipped, no model call.
    PrivacyPolicy,
}

/// The detail under a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiReason {
    ModelLaneDenied,
    ModelLaneUnauthorized,
    ModelRejectedRequest,
    /// Quarry could not mint the lane's two org-bound tokens (WS16.S3).
    /// Nothing was sent.
    ModelLaneUnavailable,
    UpstreamUnavailable,
    Timeout,
    Transport,
    BadReply,
    NotJson,
    TooLarge,
    Deadline,
}

/// How a `json` output was made valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRepair {
    None,
    /// Quarry fixed it without a model call (a fence, a trailing comma).
    Local,
    /// One extra model call, carrying the invalid output and the schema, never the page.
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// A place the output broke the schema: the path and a code, never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaIssue {
    pub path: String,
    pub code: String,
}

/// What one AI format did (`data.ai.<kind>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiFormatReport {
    pub status: AiStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<AiCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<AiReason>,
    /// `MODEL_RATE_LIMITED` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_s: Option<u32>,
    /// The model the gateway answered with (a routing alias, or what it resolved to).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AiUsage>,
    /// The page text was cut to the input budget before it was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_input: Option<bool>,
    /// True on every `ok` entry: the output was derived from page text.
    pub untrusted_source: bool,
    // json
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<AiRepair>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_issues: Vec<SchemaIssue>,
    // question
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<bool>,
    /// The model's supporting quote is verbatim on the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grounded: Option<bool>,
    // highlights
    /// Model quotes not found on the page, dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<u32>,
}

impl AiFormatReport {
    fn with_status(status: AiStatus) -> Self {
        Self {
            status,
            code: None,
            reason: None,
            retry_after_s: None,
            model: None,
            usage: None,
            truncated_input: None,
            untrusted_source: false,
            schema_valid: None,
            repair: None,
            schema_issues: Vec::new(),
            found: None,
            grounded: None,
            dropped: None,
        }
    }

    /// A finished format. `untrusted_source` is set.
    pub fn ok() -> Self {
        Self {
            untrusted_source: true,
            ..Self::with_status(AiStatus::Ok)
        }
    }

    /// No model call was made for this format.
    pub fn skipped(code: AiCode) -> Self {
        Self {
            code: Some(code),
            ..Self::with_status(AiStatus::Skipped)
        }
    }

    /// A model call was made and failed.
    pub fn failed(code: AiCode, reason: Option<AiReason>) -> Self {
        Self {
            code: Some(code),
            reason,
            ..Self::with_status(AiStatus::Error)
        }
    }
}

/// Totals over the whole request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiTotal {
    pub calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// `data.ai`: one entry per requested kind, and the total.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AiReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<AiFormatReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<AiFormatReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<AiFormatReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlights: Option<AiFormatReport>,
    pub total: AiTotal,
}

impl AiReport {
    pub fn set(&mut self, kind: AiKind, report: AiFormatReport) {
        let slot = match kind {
            AiKind::Json => &mut self.json,
            AiKind::Summary => &mut self.summary,
            AiKind::Question => &mut self.question,
            AiKind::Highlights => &mut self.highlights,
        };
        *slot = Some(report);
    }

    pub fn get(&self, kind: AiKind) -> Option<&AiFormatReport> {
        match kind {
            AiKind::Json => self.json.as_ref(),
            AiKind::Summary => self.summary.as_ref(),
            AiKind::Question => self.question.as_ref(),
            AiKind::Highlights => self.highlights.as_ref(),
        }
    }
}

/// One highlight: an exact span of the page text. `start` and `end` are Unicode
/// scalar (char) offsets, end-exclusive, into the AI source text; `text` is the
/// slice, so a client never needs to cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    pub text: String,
    pub start: u64,
    pub end: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(kind: AiKind, body: Value) -> Result<AiSpec, Vec<FieldError>> {
        parse_ai_object(kind, body.as_object().unwrap(), "formats[1]")
    }

    fn codes(errors: &[FieldError]) -> Vec<(&str, &str)> {
        errors.iter().map(|e| (e.path.as_str(), e.code.as_str())).collect()
    }

    fn json_with(schema: Value) -> Result<AiSpec, Vec<FieldError>> {
        parse(AiKind::Json, json!({ "schema": schema }))
    }

    #[test]
    fn language_tags() {
        for ok in ["en", "nb", "nb-NO", "zh-Hans", "eng", "pt-BR"] {
            assert!(is_language_tag(ok), "{ok}");
        }
        for bad in ["", "e", "english", "nb_NO", "nb-", "nb-N", "nb-NO-x", "n1", "-nb"] {
            assert!(!is_language_tag(bad), "{bad}");
        }
    }

    #[test]
    fn kinds_round_trip_their_wire_names() {
        for kind in AiKind::ALL {
            assert_eq!(AiKind::from_wire(kind.wire_name()), Some(kind));
        }
        assert_eq!(AiKind::from_wire("markdown"), None);
        assert!(AiKind::Json < AiKind::Summary && AiKind::Question < AiKind::Highlights);
    }

    #[test]
    fn a_schema_with_every_allowed_keyword_is_accepted_and_normalised() {
        let spec = json_with(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "$id": "x",
            "$comment": "c",
            "type": "object",
            "title": "T",
            "description": "d",
            "required": ["name"],
            "additionalProperties": false,
            "properties": {
                "$id": { "type": "string", "$comment": "dropped" },
                "name": { "type": "string", "minLength": 1, "maxLength": 9 },
                "tags": { "type": "array", "minItems": 0, "maxItems": 3, "items": { "type": "string", "enum": ["a", "b"] } },
                "n": { "type": ["integer", "null"], "minimum": 0, "maximum": 5.5 }
            }
        }))
        .expect("accepted");
        let AiSpec::Json(spec) = spec else { panic!() };
        assert!(spec.schema.get("$schema").is_none() && spec.schema.get("$id").is_none());
        assert!(spec.schema.get("$comment").is_none());
        // A property called $id is a name, not a keyword, and stays.
        assert!(spec.schema["properties"].get("$id").is_some());
        assert!(spec.schema["properties"]["$id"].get("$comment").is_none());
        assert_eq!(spec.schema["required"], json!(["name"]));
    }

    #[test]
    fn the_object_forms_parse_with_their_defaults() {
        let AiSpec::Summary(summary) = parse(AiKind::Summary, json!({})).unwrap() else { panic!() };
        assert_eq!(summary, SummarySpec { max_chars: 2000, language: None });
        let AiSpec::Summary(summary) =
            parse(AiKind::Summary, json!({ "max_chars": 4000, "language": "nb" })).unwrap() else { panic!() };
        assert_eq!((summary.max_chars, summary.language.as_deref()), (4000, Some("nb")));
        let AiSpec::Question(question) = parse(AiKind::Question, json!({ "question": "Who?" })).unwrap() else {
            panic!()
        };
        assert_eq!(question.question, "Who?");
        let AiSpec::Highlights(highlights) =
            parse(AiKind::Highlights, json!({ "query": "price" })).unwrap() else { panic!() };
        assert_eq!((highlights.max_highlights, highlights.max_chars_each), (3, 300));
        let AiSpec::Json(json_spec) =
            parse(AiKind::Json, json!({ "schema": { "type": "object" }, "prompt": "Be exact." })).unwrap() else {
                panic!()
            };
        assert_eq!(json_spec.prompt.as_deref(), Some("Be exact."));
        // A null optional is absent.
        assert!(parse(AiKind::Summary, json!({ "max_chars": null, "language": null })).is_ok());
    }

    #[test]
    fn required_inputs_are_missing_field_with_their_path() {
        let errors = parse(AiKind::Json, json!({})).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].schema", "missing_field")]);
        let errors = parse(AiKind::Question, json!({})).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].question", "missing_field")]);
        let errors = parse(AiKind::Highlights, json!({})).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].query", "missing_field")]);
        // The bare names say the same, at the same paths.
        assert_eq!(bare_name_error(AiKind::Json, "formats[0]").unwrap().path, "formats[0].schema");
        assert_eq!(bare_name_error(AiKind::Question, "formats[0]").unwrap().path, "formats[0].question");
        assert_eq!(bare_name_error(AiKind::Highlights, "formats[0]").unwrap().path, "formats[0].query");
        assert!(bare_name_error(AiKind::Summary, "formats[0]").is_none());
    }

    #[test]
    fn schema_limits() {
        // Root must be an object schema.
        let errors = json_with(json!({ "type": "array", "items": { "type": "string" } })).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].schema", "schema_root_not_object")]);
        let errors = json_with(json!({ "properties": {} })).unwrap_err();
        assert_eq!(errors[0].code, "schema_root_not_object");
        let errors = parse(AiKind::Json, json!({ "schema": "object" })).unwrap_err();
        assert_eq!(errors[0].code, "invalid_type");
        // Size: exactly at the limit is fine, one byte over is not.
        let sized = |bytes: usize| {
            let mut schema = json!({ "type": "object", "description": "" });
            let base = serde_json::to_vec(&schema).unwrap().len();
            schema["description"] = Value::String("x".repeat(bytes - base));
            assert_eq!(serde_json::to_vec(&schema).unwrap().len(), bytes);
            schema
        };
        assert!(json_with(sized(MAX_SCHEMA_BYTES)).is_ok());
        let errors = json_with(sized(MAX_SCHEMA_BYTES + 1)).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].schema", "schema_too_large")]);
        // Depth: ten levels is fine, eleven is not.
        let nest = |levels: usize| {
            let mut node = json!({ "type": "string" });
            for _ in 1..levels {
                node = json!({ "type": "object", "properties": { "a": node } });
            }
            // `levels` counts the schema nodes down the chain.
            node
        };
        // nest(n): n nodes in a chain; its root is an object for n >= 2.
        assert!(json_with(json!({ "type": "object", "properties": { "a": nest(9) } })).is_ok()); // 10 nodes
        let errors = json_with(json!({ "type": "object", "properties": { "a": nest(10) } })).unwrap_err(); // 11 nodes
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "schema_too_complex");
        // Properties in all: 200 is fine, 201 is not.
        let wide = |count: usize| {
            let props: Map<String, Value> =
                (0..count).map(|i| (format!("p{i}"), json!({ "type": "string" }))).collect();
            json!({ "type": "object", "properties": props })
        };
        assert!(json_with(wide(200)).is_ok());
        let errors = json_with(wide(201)).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "schema_too_complex");
        // Counted across levels.
        let split = json!({ "type": "object", "properties": {
            "a": { "type": "object", "properties": wide(120)["properties"].clone() },
            "b": { "type": "object", "properties": wide(120)["properties"].clone() },
        } });
        assert_eq!(json_with(split).unwrap_err()[0].code, "schema_too_complex");
    }

    #[test]
    fn every_keyword_the_validator_ignores_is_refused_with_its_path() {
        for keyword in [
            "$ref", "anyOf", "oneOf", "allOf", "not", "const", "pattern", "format", "if", "then", "else",
            "patternProperties", "dependentRequired", "dependentSchemas", "default", "examples", "uniqueItems",
        ] {
            let errors = json_with(json!({
                "type": "object",
                "properties": { "a": { "type": "string", keyword: "x" } }
            }))
            .unwrap_err();
            let expected = format!("formats[1].schema.properties.a.{keyword}");
            assert_eq!(
                codes(&errors),
                [(expected.as_str(), "schema_keyword_not_supported")],
                "{keyword}"
            );
        }
        // At the root, and inside items.
        let errors = json_with(json!({ "type": "object", "pattern": "x" })).unwrap_err();
        assert_eq!(errors[0].path, "formats[1].schema.pattern");
        let errors = json_with(json!({
            "type": "object",
            "properties": { "l": { "type": "array", "items": { "type": "string", "format": "email" } } }
        }))
        .unwrap_err();
        assert_eq!(errors[0].path, "formats[1].schema.properties.l.items.format");
        // additionalProperties is a boolean only; a schema there is not validated.
        let errors = json_with(json!({ "type": "object", "additionalProperties": { "type": "string" } })).unwrap_err();
        assert_eq!(errors[0].code, "schema_keyword_not_supported");
        // items as a list (tuple form) is not validated either.
        let errors = json_with(json!({
            "type": "object",
            "properties": { "l": { "type": "array", "items": [{ "type": "string" }] } }
        }))
        .unwrap_err();
        assert_eq!(errors[0].code, "schema_keyword_not_supported");
        // Wrong value types are named.
        let errors = json_with(json!({
            "type": "object",
            "required": "name",
            "properties": { "a": { "type": "text", "minLength": -1 } }
        }))
        .unwrap_err();
        let found: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
        assert!(found.contains(&"invalid_type") && found.contains(&"invalid_value"), "{found:?}");
        // All the errors are reported, up to a cap.
        let many: Map<String, Value> = (0..60).map(|i| (format!("k{i}"), json!(1))).collect();
        let mut root = many;
        root.insert("type".into(), json!("object"));
        let errors = json_with(Value::Object(root)).unwrap_err();
        assert_eq!(errors.len(), MAX_SCHEMA_ERRORS);
    }

    #[test]
    fn text_and_number_limits() {
        let long = "x".repeat(MAX_PROMPT_CHARS + 1);
        let ok = "x".repeat(MAX_PROMPT_CHARS);
        let schema = json!({ "type": "object" });
        let errors = parse(AiKind::Json, json!({ "schema": schema, "prompt": long })).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].prompt", "text_too_long")]);
        assert!(parse(AiKind::Json, json!({ "schema": schema, "prompt": ok })).is_ok());
        // Characters, not bytes: 10,000 two-byte characters are fine.
        assert!(parse(AiKind::Question, json!({ "question": "æ".repeat(MAX_PROMPT_CHARS) })).is_ok());
        let errors = parse(AiKind::Question, json!({ "question": "æ".repeat(MAX_PROMPT_CHARS + 1) })).unwrap_err();
        assert_eq!(errors[0].code, "text_too_long");
        let errors = parse(AiKind::Question, json!({ "question": "   " })).unwrap_err();
        assert_eq!(errors[0].code, "empty");
        let errors = parse(AiKind::Question, json!({ "question": 7 })).unwrap_err();
        assert_eq!(errors[0].code, "invalid_type");
        let errors = parse(AiKind::Highlights, json!({ "query": "q".repeat(MAX_QUERY_CHARS + 1) })).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].query", "text_too_long")]);
        assert!(parse(AiKind::Highlights, json!({ "query": "q".repeat(MAX_QUERY_CHARS) })).is_ok());

        for (kind, field, low, high) in [
            (AiKind::Highlights, "max_highlights", 1, 10),
            (AiKind::Highlights, "max_chars_each", 40, 1000),
            (AiKind::Summary, "max_chars", 200, 10_000),
        ] {
            let body = |value: i64| {
                let mut body = serde_json::Map::new();
                if kind == AiKind::Highlights {
                    body.insert("query".into(), json!("q"));
                }
                body.insert(field.into(), json!(value));
                Value::Object(body)
            };
            assert!(parse(kind, body(low)).is_ok(), "{field} low");
            assert!(parse(kind, body(high)).is_ok(), "{field} high");
            for bad in [low - 1, high + 1] {
                let errors = parse(kind, body(bad)).unwrap_err();
                assert_eq!(
                    codes(&errors),
                    [(format!("formats[1].{field}").as_str(), "out_of_range")],
                    "{field} {bad}"
                );
            }
        }
        let errors = parse(AiKind::Summary, json!({ "max_chars": "many" })).unwrap_err();
        assert_eq!(errors[0].code, "invalid_type");
        let errors = parse(AiKind::Summary, json!({ "max_chars": -5 })).unwrap_err();
        assert_eq!(errors[0].code, "invalid_type");
        let errors = parse(AiKind::Summary, json!({ "language": "not a tag" })).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1].language", "invalid_value")]);
    }

    #[test]
    fn an_unknown_key_in_an_object_is_unknown_field_at_the_format() {
        let errors = parse(AiKind::Summary, json!({ "max_char": 300 })).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1]", "unknown_field")]);
        let errors = parse(AiKind::Question, json!({ "question": "q", "prompt": "p" })).unwrap_err();
        assert_eq!(codes(&errors), [("formats[1]", "unknown_field")]);
        // The key is named, a value never is.
        let errors = parse(AiKind::Question, json!({ "question": "q", "extra": "SECRET-VALUE-9" })).unwrap_err();
        assert!(!serde_json::to_string(&errors).unwrap().contains("SECRET-VALUE-9"));
    }

    #[test]
    fn errors_never_echo_a_value() {
        const SECRET: &str = "SECRET-PROMPT-TEXT-4471";
        let errors = parse(
            AiKind::Json,
            json!({
                "schema": { "type": "object", "properties": { "a": { "type": "string", "pattern": SECRET } } },
                "prompt": format!("{SECRET}{}", "x".repeat(MAX_PROMPT_CHARS)),
            }),
        )
        .unwrap_err();
        assert!(errors.len() >= 2);
        assert!(!serde_json::to_string(&errors).unwrap().contains(SECRET));
    }

    #[test]
    fn the_canonical_hash_tells_specs_apart_and_ignores_key_order() {
        let hash = |spec: Result<AiSpec, Vec<FieldError>>| spec.unwrap().canonical_hash16();
        let a = hash(json_with(json!({ "type": "object", "properties": { "x": { "type": "string" }, "y": { "type": "integer" } } })));
        let reordered = hash(json_with(json!({ "properties": { "y": { "type": "integer" }, "x": { "type": "string" } }, "type": "object" })));
        assert_eq!(a, reordered);
        assert_eq!(a.len(), 16);
        let other = hash(json_with(json!({ "type": "object", "properties": { "x": { "type": "string" } } })));
        assert_ne!(a, other);
        // $schema and $id do not count.
        let with_meta = hash(json_with(json!({ "$schema": "s", "type": "object", "properties": { "y": { "type": "integer" }, "x": { "type": "string" } } })));
        assert_eq!(a, with_meta);
        // The prompt does.
        let prompted = hash(parse(AiKind::Json, json!({ "schema": { "type": "object" }, "prompt": "p" })));
        let unprompted = hash(parse(AiKind::Json, json!({ "schema": { "type": "object" } })));
        assert_ne!(prompted, unprompted);
        // Defaults are filled in: spelling them out changes nothing.
        assert_eq!(
            hash(parse(AiKind::Summary, json!({}))),
            hash(parse(AiKind::Summary, json!({ "max_chars": 2000 })))
        );
        assert_ne!(
            hash(parse(AiKind::Summary, json!({}))),
            hash(parse(AiKind::Summary, json!({ "language": "nb" })))
        );
        assert_ne!(
            hash(parse(AiKind::Question, json!({ "question": "a" }))),
            hash(parse(AiKind::Question, json!({ "question": "b" })))
        );
        assert_ne!(
            hash(parse(AiKind::Highlights, json!({ "query": "a" }))),
            hash(parse(AiKind::Highlights, json!({ "query": "a", "max_highlights": 4 })))
        );
        // The content is not in the text a hash is made of its kind plus a digest: the hash holds none of it.
        assert!(!a.contains('x'));
    }

    #[test]
    fn a_report_round_trips_and_keeps_its_wire_spelling() {
        let mut report = AiReport::default();
        let mut json_report = AiFormatReport::ok();
        json_report.model = Some("verevon-balance".into());
        json_report.usage = Some(AiUsage { input_tokens: 1834, output_tokens: 22 });
        json_report.schema_valid = Some(true);
        json_report.repair = Some(AiRepair::None);
        report.set(AiKind::Json, json_report);
        let mut limited = AiFormatReport::failed(AiCode::ModelRateLimited, None);
        limited.retry_after_s = Some(7);
        report.set(AiKind::Summary, limited);
        report.set(AiKind::Question, AiFormatReport::skipped(AiCode::ZdrUnavailable));
        let mut broken = AiFormatReport::failed(AiCode::SchemaInvalid, Some(AiReason::NotJson));
        broken.schema_issues = vec![SchemaIssue { path: "$.title".into(), code: "missing_required".into() }];
        report.set(AiKind::Highlights, broken);
        report.total = AiTotal { calls: 1, input_tokens: 1834, output_tokens: 22 };

        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["json"]["status"], "ok");
        assert_eq!(value["json"]["untrusted_source"], true);
        assert_eq!(value["json"]["repair"], "none");
        assert_eq!(value["json"]["usage"]["input_tokens"], 1834);
        assert_eq!(value["summary"]["code"], "MODEL_RATE_LIMITED");
        assert_eq!(value["summary"]["retry_after_s"], 7);
        assert_eq!(value["summary"]["status"], "error");
        assert_eq!(value["question"]["status"], "skipped");
        assert_eq!(value["question"]["code"], "ZDR_UNAVAILABLE");
        assert_eq!(value["question"]["untrusted_source"], false);
        assert_eq!(value["highlights"]["reason"], "not_json");
        assert_eq!(value["highlights"]["schema_issues"][0]["path"], "$.title");
        assert_eq!(value["total"]["calls"], 1);
        // Absent means absent.
        assert!(value["json"].get("code").is_none() && value["json"].get("schema_issues").is_none());
        let back: AiReport = serde_json::from_value(value).unwrap();
        assert_eq!(back, report);
        assert_eq!(report.get(AiKind::Json).map(|r| r.status), Some(AiStatus::Ok));

        for (code, text) in [
            (AiCode::ModelUnavailable, "MODEL_UNAVAILABLE"),
            (AiCode::SchemaInvalid, "SCHEMA_INVALID"),
            (AiCode::BudgetExhausted, "BUDGET_EXHAUSTED"),
            (AiCode::EmptySource, "EMPTY_SOURCE"),
            (AiCode::PrivacyPolicy, "PRIVACY_POLICY"),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), text);
        }
        for (reason, text) in [
            (AiReason::ModelLaneDenied, "model_lane_denied"),
            (AiReason::ModelLaneUnauthorized, "model_lane_unauthorized"),
            (AiReason::ModelRejectedRequest, "model_rejected_request"),
            (AiReason::UpstreamUnavailable, "upstream_unavailable"),
            (AiReason::BadReply, "bad_reply"),
            (AiReason::TooLarge, "too_large"),
            (AiReason::Deadline, "deadline"),
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), text);
        }
    }

    #[test]
    fn a_highlight_serializes_as_text_and_offsets() {
        let highlight = Highlight { text: "tilbud".into(), start: 12, end: 18 };
        assert_eq!(
            serde_json::to_value(&highlight).unwrap(),
            json!({ "text": "tilbud", "start": 12, "end": 18 })
        );
    }
}
