//! The `/v1/scrape` response: a [`NormalizedOutput`] plus what cannot be a
//! stored artifact.
//!
//! Output formats are artifact references, and a Zero-Data-Retention scrape
//! writes no artifacts, so under ZDR the extracted text used to be unreachable:
//! the response named nothing and held nothing. [`ScrapeResponse`] carries the
//! text inline in that case (and only that case, in this version). The
//! `NormalizedOutput` fields stay at the top level, so every existing client
//! keeps working.

use serde::{Deserialize, Serialize};

use crate::ai_spec::{bare_name_error, parse_ai_object, AiKind, AiReport, AiSpec, Highlight};
use crate::capture::{CaptureLimits, CapturePlan, PdfOptions, ScreenshotOptions};
use crate::output::NormalizedOutput;
use crate::scrape_actions::{
    validate_actions, ActionError, ActionLimits, ActionOnError, ActionOutcome, ActionStatus,
    ActionsSummary, ValidatedActions,
};

/// Inline text is capped so a ZDR response stays a reasonable size; the flag
/// [`ScrapeResponse::truncated`] says when the cap applied.
pub const DEFAULT_INLINE_MAX_BYTES: usize = 1_048_576;

/// A non-fatal note about the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct ScrapeWarning {
    /// Stable machine-readable code, e.g. `document_has_no_text_layer`.
    pub code: String,
    pub message: String,
    /// The request field the warning is about (`unknown_field`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ScrapeWarning {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            field: None,
        }
    }

    pub fn for_field(code: &str, field: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            field: Some(field.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResponse {
    #[serde(flatten)]
    pub output: NormalizedOutput,
    /// The extracted text, inline. Present when the caller asked for the
    /// `markdown` format, and by default under ZDR (nothing is stored to read
    /// it from); otherwise read it from `formats.markdown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    /// The document as fetched, inline, when the `html` format was asked for.
    /// The same bytes as `raw_html` until an HTML-cleaning stage exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// The document as fetched, inline, when the `rawHtml` format was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_html: Option<String>,
    /// Inline fields cut at `max_inline_bytes` (ZDR only: there is no stored
    /// copy to point at, so the text is cut on a character boundary).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncated: Vec<String>,
    /// Inline fields left out because they exceed `max_inline_bytes` (not
    /// ZDR): read them from the matching `formats.*` artifact reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ScrapeWarning>,
    /// The options this response was produced with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_options: Option<EffectiveScrapeOptions>,
    /// The id of the stored record of this scrape, for `GET /v1/scrape/{id}`.
    /// Absent under ZDR (nothing is stored) and when records are off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrape_id: Option<String>,
    /// WS13.4b: the screenshot, inline. Under ZDR only (nothing is stored to
    /// point at); otherwise read it from `formats.screenshot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<InlineCapture>,
    /// WS13.4b: the PDF, inline. Under ZDR only; otherwise `formats.pdf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<InlineCapture>,
    /// WS13.4b: what the captures measured, in both modes. Counts and sizes
    /// only, never a URL. Absent when the request captured nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captures: Option<CapturesReport>,
    /// WS13.5: what each scrape-time action did, in request order. Absent (and
    /// not on the wire) when the request carried no actions. Metadata and
    /// artifact references only; under ZDR a capture an action took travels in
    /// the entry's `output.data_base64` under the inline cap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionOutcomeView>,
    /// INNH-1: the `json` AI format's result, inline only. Present only when
    /// `ai.json.status` is `ok`, and then it validates against the caller's
    /// schema. Never stored, never in the scrape record, never cached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,
    /// INNH-1: the `summary` AI format's result (at most `max_chars` characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// INNH-1: the `question` AI format's answer. Present only when the page
    /// held one (`ai.question.found`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// INNH-1: the `highlights` AI format's result: exact spans of the page
    /// text. May be an empty list, an honest "nothing found".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlights: Option<Vec<Highlight>>,
    /// INNH-1: what each AI format did and cost (status, codes, token usage).
    /// Present whenever an AI format was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiReport>,
}

impl ScrapeResponse {
    /// A response with no inline parts (a cache hit, or a non-ZDR run).
    pub fn from_output(output: NormalizedOutput) -> Self {
        Self {
            output,
            markdown: None,
            html: None,
            raw_html: None,
            truncated: Vec::new(),
            omitted: Vec::new(),
            warnings: Vec::new(),
            effective_options: None,
            scrape_id: None,
            screenshot: None,
            pdf: None,
            captures: None,
            actions: Vec::new(),
            json: None,
            summary: None,
            answer: None,
            highlights: None,
            ai: None,
        }
    }
}

/// WS13.5: one scrape-time action's outcome as the wire carries it
/// (`data.actions[]`). The runtime's [`ActionOutcome`] with the capture's
/// stored reference or inline bytes attached. Never a selector or a typed text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutcomeView {
    pub index: usize,
    /// The action's type (`click`, `screenshot`, ...).
    #[serde(rename = "type")]
    pub kind: String,
    pub status: ActionStatus,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ActionError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ActionOutputView>,
}

/// WS13.5: what a `screenshot` or `pdf` action produced. At most one of
/// `artifact_id` (stored, not ZDR) and `data_base64` (inline, ZDR, under the
/// cap) is set; neither means the bytes were over the inline cap and left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutputView {
    /// `screenshot` or `pdf`.
    pub kind: String,
    pub content_type: String,
    /// Size of the decoded bytes.
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
}

impl ActionOutcomeView {
    /// The view of an outcome with no stored reference and no inline bytes yet.
    pub fn from_outcome(outcome: &ActionOutcome) -> Self {
        Self {
            index: outcome.index,
            kind: outcome.kind.clone(),
            status: outcome.status,
            duration_ms: outcome.duration_ms,
            error: outcome.error.clone(),
            output: outcome.output.as_ref().map(|output| ActionOutputView {
                kind: output.kind.clone(),
                content_type: output.content_type.clone(),
                bytes: output.bytes,
                width: output.width,
                height: output.height,
                artifact_id: None,
                data_base64: None,
            }),
        }
    }
}

/// WS13.4b: a capture returned inline (ZDR only). The bytes are base64 under
/// the inline cap; nothing is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineCapture {
    pub content_type: String,
    /// Size of the decoded bytes.
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub data_base64: String,
}

/// WS13.4b: what the captures of one request measured, returned in both modes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CapturesReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<ScreenshotReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<PdfReport>,
    /// Requests the egress policy blocked while the page loaded. A count only.
    pub blocked_requests: u32,
}

/// The measured screenshot: its encoding, size in pixels and bytes, how many
/// tiles it was made from, and whether the page was cut at the limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotReport {
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub tiles: u32,
    pub stitched: bool,
    pub truncated: bool,
}

/// The measured PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfReport {
    pub bytes: u64,
}

/// Cut `text` to at most `max_bytes` on a character boundary. Returns the text
/// and whether anything was removed.
pub fn truncate_on_char_boundary(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

// ---- request options ------------------------------------------------------

/// A field of a request that is not acceptable, with where and why. A model
/// that proposed bad options can read these and correct itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct FieldError {
    /// JSON path of the field, `formats[1]` or `timeout_ms`.
    pub path: String,
    /// Stable machine-readable code: `invalid_type`, `unsupported_format`,
    /// `missing_field`, `out_of_range`, `too_many`, `too_long`, `empty`,
    /// `conflicting_aliases`, `invalid_json`, `reserved_field`, `unknown_field`.
    /// Capture and action codes (`too_many_captures`, `invalid_clip`,
    /// `unknown_action_type`, ...) are listed in the `capture` and `scrape_actions` modules.
    pub code: String,
    pub message: String,
}

impl FieldError {
    pub fn new(path: impl Into<String>, code: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

/// What a scrape can return inline besides the stored artifacts.
///
/// `Links` and `Metadata` are part of every response already; naming them is
/// accepted and changes nothing. `Html` is the HTML the markdown was made from
/// (after the cleaning options); `RawHtml` is the document as fetched.
/// Equality is not total: the capture options hold `f64` values, so
/// `ScrapeFormat` is `PartialEq` only. The wire form is the name (`"markdown"`);
/// the object form (`{"type":"screenshot",...}`) is accepted on input only.
///
/// Not `Copy`: the AI formats carry a schema, a prompt or a question. On the
/// wire (`effective_options.formats`) a format is its name only.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScrapeFormat {
    Markdown,
    Html,
    RawHtml,
    Links,
    Metadata,
    /// A screenshot of the page. The string `screenshot` means the defaults.
    Screenshot(ScreenshotOptions),
    /// A PDF of the page. The string `pdf` means the defaults.
    Pdf(PdfOptions),
    /// INNH-1: a model reads the page (`json`, `summary`, `question`,
    /// `highlights`). The string `summary` means the defaults; the others need
    /// their object form. Off unless `ScrapeLimits::ai_formats_enabled`.
    Ai(AiSpec),
}

impl ScrapeFormat {
    pub const SUPPORTED: [&'static str; 8] = [
        "markdown",
        "html",
        "rawHtml",
        "links",
        "metadata",
        "screenshot",
        "pdf",
        "summary",
    ];

    /// The `type` of every object form, for the error text.
    const OBJECT_TYPES: [&'static str; 6] =
        ["screenshot", "pdf", "json", "summary", "question", "highlights"];

    fn parse(text: &str) -> Option<Self> {
        match text {
            "markdown" => Some(Self::Markdown),
            "html" => Some(Self::Html),
            "rawHtml" | "raw_html" => Some(Self::RawHtml),
            "links" => Some(Self::Links),
            "metadata" => Some(Self::Metadata),
            "screenshot" => Some(Self::Screenshot(ScreenshotOptions::default())),
            "pdf" => Some(Self::Pdf(PdfOptions::default())),
            "summary" => Some(Self::Ai(AiSpec::default_summary())),
            _ => None,
        }
    }

    /// The wire name of the format: the string form, and the object's `type`.
    pub fn wire_name(&self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::RawHtml => "raw_html",
            Self::Links => "links",
            Self::Metadata => "metadata",
            Self::Screenshot(_) => "screenshot",
            Self::Pdf(_) => "pdf",
            Self::Ai(spec) => spec.kind().wire_name(),
        }
    }

    /// The AI spec, when this is an AI format.
    pub fn ai_spec(&self) -> Option<&AiSpec> {
        match self {
            Self::Ai(spec) => Some(spec),
            _ => None,
        }
    }
}

impl Serialize for ScrapeFormat {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.wire_name())
    }
}

impl<'de> Deserialize<'de> for ScrapeFormat {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        // The echo of a stored record names an AI format without its spec
        // (`effective_options.formats` is names only), so a bare `json` reads
        // back as a spec with nothing in it. Request parsing never goes this way.
        Self::parse(&text)
            .or_else(|| AiSpec::from_wire_echo(&text).map(Self::Ai))
            .ok_or_else(|| serde::de::Error::custom("unknown scrape format"))
    }
}

#[cfg(feature = "jsonschema")]
impl schemars::JsonSchema for ScrapeFormat {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ScrapeFormat".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "anyOf": [
                {
                    "type": "string",
                    "enum": ["markdown", "html", "rawHtml", "links", "metadata", "screenshot", "pdf", "summary"]
                },
                {
                    "type": "object",
                    "required": ["type"],
                    "properties": { "type": { "enum": ["screenshot", "pdf"] } }
                },
                {
                    "type": "object",
                    "required": ["type", "schema"],
                    "properties": {
                        "type": { "const": "json" },
                        "schema": { "type": "object", "properties": { "type": { "const": "object" } } },
                        "prompt": { "type": "string", "minLength": 1, "maxLength": 10000 }
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["type"],
                    "properties": {
                        "type": { "const": "summary" },
                        "max_chars": { "type": "integer", "minimum": 200, "maximum": 10000 },
                        "language": { "type": "string", "pattern": "^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})?$" }
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["type", "question"],
                    "properties": {
                        "type": { "const": "question" },
                        "question": { "type": "string", "minLength": 1, "maxLength": 10000 }
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["type", "query"],
                    "properties": {
                        "type": { "const": "highlights" },
                        "query": { "type": "string", "minLength": 1, "maxLength": 1000 },
                        "max_highlights": { "type": "integer", "minimum": 1, "maximum": 10 },
                        "max_chars_each": { "type": "integer", "minimum": 40, "maximum": 1000 }
                    },
                    "additionalProperties": false
                }
            ]
        })
    }
}

/// Which content extractor shapes the markdown. `Readability` is the
/// main-content heuristic (`only_main_content`, the default); `CleanFull` keeps
/// the whole body minus scripts and chrome; `Raw` converts the document as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Extractor {
    #[default]
    Readability,
    CleanFull,
    Raw,
}

impl Extractor {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "readability" => Some(Self::Readability),
            "clean_full" | "cleanFull" => Some(Self::CleanFull),
            "raw" => Some(Self::Raw),
            _ => None,
        }
    }
}

/// E3.6-5: whether the text a scrape returns goes through the PII scanner
/// (`quarry_transform::pii`). `Off` is the default. `On` is served under zero data
/// retention only, and never with a capture or an action (see `ScrapeOptions::resolve`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PiiRedaction {
    #[default]
    Off,
    On,
}

/// The options a caller may put at the root of a `/v1/scrape` body, as sent.
/// Every field is optional; `resolve` turns this into what the run uses.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct ScrapeOptions {
    pub formats: Option<Vec<ScrapeFormat>>,
    pub only_main_content: Option<bool>,
    pub include_tags: Option<Vec<String>>,
    pub exclude_tags: Option<Vec<String>>,
    pub remove_base64_images: Option<bool>,
    pub extractor: Option<Extractor>,
    pub timeout_ms: Option<u64>,
    pub max_inline_bytes: Option<u64>,
    /// Actions to run on the page, in order. Checked by WS13.2, run by WS13.5.
    pub actions: Option<Vec<serde_json::Value>>,
    /// `abort` (the default) or `continue`. There is no retry: a replayed click
    /// or type can submit twice.
    pub actions_on_error: Option<ActionOnError>,
    /// INNH-7: serve a stored result no older than this many milliseconds
    /// instead of fetching. Absent: no read of the URL result cache. `0`: never
    /// read it (but still write). Accepted only while `ScrapeLimits::max_age_enabled`.
    pub max_age: Option<u64>,
    /// E3.6-5: `off` (the default) or `on`: pass the returned text through the PII scanner.
    /// `on` is served under zero data retention only, and never with a capture or an action.
    /// Absent and `off` are the same request.
    pub redact_pii: Option<PiiRedaction>,
}

/// The longest `max_age` a request may ask for: 30 days, in milliseconds.
pub const MAX_AGE_CEILING_MS: u64 = 2_592_000_000;

/// Request keys the options own: each field in snake_case and its camelCase
/// alias. A body key that is neither one of these nor a key of the request
/// itself is unknown. A unit test pins this against a fully populated value.
const OPTION_KEYS: [(&str, &str); 12] = [
    ("formats", "formats"),
    ("only_main_content", "onlyMainContent"),
    ("include_tags", "includeTags"),
    ("exclude_tags", "excludeTags"),
    ("remove_base64_images", "removeBase64Images"),
    ("extractor", "extractor"),
    ("timeout_ms", "timeoutMs"),
    ("max_inline_bytes", "maxInlineBytes"),
    ("actions", "actions"),
    ("actions_on_error", "actionsOnError"),
    ("max_age", "maxAge"),
    ("redact_pii", "redactPii"),
];

/// Keys a body may carry that are never a scrape option and never lenient: they
/// would change what the request is allowed to do, so they are refused with a
/// 400 from the first version. There is no TLS toggle, proxy, header or
/// stealth control in this contract, and none is going to be added by
/// accepting a field quietly.
pub const RESERVED_REJECTED: [&str; 7] = [
    "skipTlsVerification",
    "skip_tls_verification",
    "proxy",
    "proxyUrl",
    "headers",
    "mobile",
    "stealth",
];

impl ScrapeOptions {
    /// Every key this type reads, both spellings.
    pub fn field_names() -> Vec<&'static str> {
        let mut names: Vec<&'static str> = OPTION_KEYS
            .iter()
            .flat_map(|(snake, camel)| [*snake, *camel])
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Read the options out of a request body. Keys of the request itself are
    /// ignored here; a value of the wrong type or an unsupported format is a
    /// [`FieldError`] with its path. All errors are collected, not just the first.
    pub fn from_value(root: &serde_json::Value) -> Result<Self, Vec<FieldError>> {
        let Some(object) = root.as_object() else {
            return Err(vec![FieldError::new(
                "$",
                "invalid_type",
                "the request body must be a JSON object",
            )]);
        };
        let mut errors = Vec::new();
        let mut options = Self::default();

        for (snake, camel) in OPTION_KEYS {
            let (key, value) = match (object.get(snake), object.get(camel)) {
                (Some(a), Some(b)) if snake != camel && a != b => {
                    errors.push(FieldError::new(
                        snake,
                        "conflicting_aliases",
                        format!(
                            "`{snake}` and `{camel}` are the same option with different values"
                        ),
                    ));
                    continue;
                }
                (Some(value), _) => (snake, value),
                (None, Some(value)) => (camel, value),
                (None, None) => continue,
            };
            if value.is_null() {
                continue;
            }
            let _ = key;
            match snake {
                "formats" => match parse_formats(value) {
                    Ok(formats) => options.formats = Some(formats),
                    Err(mut found) => errors.append(&mut found),
                },
                "only_main_content" => {
                    read_bool(snake, value, &mut options.only_main_content, &mut errors)
                }
                "remove_base64_images" => {
                    read_bool(snake, value, &mut options.remove_base64_images, &mut errors)
                }
                "include_tags" => {
                    read_strings(snake, value, &mut options.include_tags, &mut errors)
                }
                "exclude_tags" => {
                    read_strings(snake, value, &mut options.exclude_tags, &mut errors)
                }
                "extractor" => match value.as_str().and_then(Extractor::parse) {
                    Some(extractor) => options.extractor = Some(extractor),
                    None => errors.push(FieldError::new(
                        snake,
                        "unsupported_extractor",
                        "extractor must be one of readability, clean_full, raw",
                    )),
                },
                "timeout_ms" => read_u64(snake, value, &mut options.timeout_ms, &mut errors),
                "max_age" => read_u64(snake, value, &mut options.max_age, &mut errors),
                "redact_pii" => match value.as_str() {
                    Some("off") => options.redact_pii = Some(PiiRedaction::Off),
                    Some("on") => options.redact_pii = Some(PiiRedaction::On),
                    _ => errors.push(FieldError::new(
                        snake,
                        "unsupported_redact_pii",
                        "redact_pii must be off or on",
                    )),
                },
                "max_inline_bytes" => {
                    read_u64(snake, value, &mut options.max_inline_bytes, &mut errors)
                }
                "actions" => match value.as_array() {
                    Some(items) if items.is_empty() => options.actions = None,
                    Some(items) => options.actions = Some(items.clone()),
                    None => errors.push(FieldError::new(
                        snake,
                        "invalid_type",
                        "actions must be an array of action objects",
                    )),
                },
                "actions_on_error" => match value.as_str() {
                    Some("abort") => options.actions_on_error = Some(ActionOnError::Abort),
                    Some("continue") => options.actions_on_error = Some(ActionOnError::Continue),
                    _ => errors.push(FieldError::new(
                        snake,
                        "unsupported_on_error",
                        "actionsOnError must be abort or continue",
                    )),
                },
                _ => {}
            }
        }
        if options.actions.is_some()
            && object.get("ingest").and_then(serde_json::Value::as_bool) == Some(true)
        {
            errors.push(FieldError::new(
                "ingest",
                "not_supported_with_actions",
                "ingest cannot be combined with actions",
            ));
        }
        if errors.is_empty() {
            Ok(options)
        } else {
            Err(errors)
        }
    }

    /// Apply defaults and limits. `zdr` selects the default for `formats`
    /// (markdown inline under ZDR, since nothing is stored to point at).
    pub fn resolve(
        self,
        zdr: bool,
        limits: &ScrapeLimits,
    ) -> Result<ResolvedScrapeOptions, Vec<FieldError>> {
        let mut errors = Vec::new();

        // INNH-1: the AI formats, checked against the request's own indices
        // (parse_formats pushes one entry per item, so the position is the path).
        let named = self.formats.as_deref().unwrap_or_default();
        let mut seen_kinds: Vec<AiKind> = Vec::new();
        for (index, format) in named.iter().enumerate() {
            let Some(spec) = format.ai_spec() else { continue };
            let path = format!("formats[{index}]");
            if !limits.ai_formats_enabled {
                errors.push(FieldError::new(
                    path,
                    "feature_disabled",
                    format!(
                        "the {} format is not enabled on this endpoint",
                        spec.kind().wire_name()
                    ),
                ));
            } else if seen_kinds.contains(&spec.kind()) {
                errors.push(FieldError::new(
                    path,
                    "too_many",
                    format!("at most one {} format per request", spec.kind().wire_name()),
                ));
            } else {
                seen_kinds.push(spec.kind());
            }
        }

        let mut formats = Vec::new();
        for format in self.formats.clone().unwrap_or_default() {
            if !formats.contains(&format) {
                formats.push(format);
            }
        }
        let requested_formats = !formats.is_empty();
        if formats.is_empty() && zdr {
            formats.push(ScrapeFormat::Markdown);
        }

        let mut check_tags = |name: &str, tags: &Option<Vec<String>>| {
            let Some(tags) = tags else { return };
            if tags.len() > limits.max_selectors {
                errors.push(FieldError::new(
                    name,
                    "too_many",
                    format!("at most {} selectors", limits.max_selectors),
                ));
            }
            for (index, tag) in tags.iter().enumerate() {
                if tag.trim().is_empty() {
                    errors.push(FieldError::new(
                        format!("{name}[{index}]"),
                        "empty",
                        "a selector cannot be empty",
                    ));
                } else if tag.chars().count() > limits.max_selector_len {
                    errors.push(FieldError::new(
                        format!("{name}[{index}]"),
                        "too_long",
                        format!(
                            "a selector is at most {} characters",
                            limits.max_selector_len
                        ),
                    ));
                }
            }
        };
        check_tags("include_tags", &self.include_tags);
        check_tags("exclude_tags", &self.exclude_tags);

        let timeout_ms = match self.timeout_ms {
            None => limits.max_timeout_ms,
            Some(value) if (limits.min_timeout_ms..=limits.max_timeout_ms).contains(&value) => {
                value
            }
            Some(_) => {
                errors.push(FieldError::new(
                    "timeout_ms",
                    "out_of_range",
                    format!(
                        "timeout_ms must be between {} and {}",
                        limits.min_timeout_ms, limits.max_timeout_ms
                    ),
                ));
                limits.max_timeout_ms
            }
        };
        let max_inline_bytes = match self.max_inline_bytes {
            None => limits.default_inline_bytes,
            Some(value) if (1..=limits.max_inline_bytes).contains(&value) => value,
            Some(_) => {
                errors.push(FieldError::new(
                    "max_inline_bytes",
                    "out_of_range",
                    format!(
                        "max_inline_bytes must be between 1 and {}",
                        limits.max_inline_bytes
                    ),
                ));
                limits.default_inline_bytes
            }
        };
        // INNH-7: `max_age` is a read-freshness bound in milliseconds. Off by default:
        // a request for it is `feature_disabled`, never a 200 that silently refetches.
        let max_age_ms = match self.max_age {
            None => None,
            Some(_) if !limits.max_age_enabled => {
                errors.push(FieldError::new(
                    "max_age",
                    "feature_disabled",
                    "max_age is not enabled on this endpoint",
                ));
                None
            }
            Some(value) if value <= MAX_AGE_CEILING_MS => Some(value),
            Some(_) => {
                errors.push(FieldError::new(
                    "max_age",
                    "out_of_range",
                    format!("max_age must be between 0 and {MAX_AGE_CEILING_MS} milliseconds"),
                ));
                None
            }
        };
        let wants_capture = formats
            .iter()
            .any(|format| matches!(format, ScrapeFormat::Screenshot(_) | ScrapeFormat::Pdf(_)));
        if wants_capture && !limits.captures_enabled {
            errors.push(FieldError::new(
                "formats",
                "feature_disabled",
                "screenshot and pdf formats are not enabled on this endpoint",
            ));
        }
        if self.actions.is_some() && !limits.actions_enabled {
            errors.push(FieldError::new(
                "actions",
                "feature_disabled",
                "actions are not enabled on this endpoint",
            ));
        }
        // E3.6-5: a redacted scrape returns text only. An image or a PDF cannot be
        // redacted and an action can take one, so neither is served with redaction.
        // Outside ZDR a stored artifact, the local index and ingest would hold the text
        // unredacted (the pipeline stage is E3.6-4a/4b), so the request is refused there
        // instead of stored.
        let redact_pii = self.redact_pii.unwrap_or_default();
        if redact_pii == PiiRedaction::On {
            if !zdr {
                errors.push(FieldError::new(
                    "redact_pii",
                    "requires_zdr",
                    "redact_pii is served under zero data retention only, until stored artifacts are redacted",
                ));
            }
            if wants_capture {
                errors.push(FieldError::new(
                    "formats",
                    "not_supported_with_redact_pii",
                    "screenshot and pdf cannot be redacted, so they are not served with redact_pii",
                ));
            }
            if self.actions.is_some() {
                errors.push(FieldError::new(
                    "actions",
                    "not_supported_with_redact_pii",
                    "actions are not served with redact_pii",
                ));
            }
        }
        // Captures: at most one screenshot and one PDF among the formats.
        let mut capture = CapturePlan::default();
        for format in &formats {
            match format {
                ScrapeFormat::Screenshot(options) => {
                    if capture.screenshot.is_some() {
                        errors.push(FieldError::new(
                            "formats",
                            "too_many",
                            "at most one screenshot format per request",
                        ));
                    }
                    capture.screenshot = Some(*options);
                }
                ScrapeFormat::Pdf(options) => {
                    if capture.pdf.is_some() {
                        errors.push(FieldError::new(
                            "formats",
                            "too_many",
                            "at most one pdf format per request",
                        ));
                    }
                    capture.pdf = Some(*options);
                }
                _ => {}
            }
        }
        let actions = match self.actions.clone() {
            None => None,
            Some(_) if !limits.actions_enabled => None,
            Some(raw) => match validate_actions(
                raw,
                self.actions_on_error.unwrap_or_default(),
                &limits.actions,
                &limits.captures,
            ) {
                Ok(validated) => Some(validated),
                Err(mut found) => {
                    errors.append(&mut found);
                    None
                }
            },
        };
        let capture_count = capture.count() + actions.as_ref().map_or(0, |a| a.capture_count);
        if capture_count > limits.captures.max_captures {
            errors.push(FieldError::new(
                "formats",
                "too_many_captures",
                format!(
                    "at most {} captures per request, formats and actions together",
                    limits.captures.max_captures
                ),
            ));
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        // `only_main_content` and `extractor` are two ways to say one thing: the
        // heuristic (readability) or the whole body (clean_full, raw). They must
        // agree; an unset one follows the other.
        let only_main_content = match (self.only_main_content, self.extractor) {
            (Some(value), _) => value,
            (None, Some(extractor)) => extractor == Extractor::Readability,
            (None, None) => true,
        };
        let extractor = self.extractor.unwrap_or(if only_main_content {
            Extractor::Readability
        } else {
            Extractor::CleanFull
        });
        if only_main_content != (extractor == Extractor::Readability) {
            return Err(vec![FieldError::new(
                "extractor",
                "conflicts_with_only_main_content",
                "extractor readability goes with only_main_content true; clean_full and raw go with only_main_content false",
            )]);
        }

        Ok(ResolvedScrapeOptions {
            formats,
            requested_formats,
            only_main_content,
            include_tags: self.include_tags.unwrap_or_default(),
            exclude_tags: self.exclude_tags.unwrap_or_default(),
            remove_base64_images: self.remove_base64_images.unwrap_or(false),
            extractor,
            timeout_ms,
            max_inline_bytes: usize::try_from(max_inline_bytes).unwrap_or(usize::MAX),
            ignored_fields: Vec::new(),
            capture,
            actions,
            max_age_ms,
            redact_pii,
        })
    }
}

fn parse_formats(value: &serde_json::Value) -> Result<Vec<ScrapeFormat>, Vec<FieldError>> {
    let Some(items) = value.as_array() else {
        return Err(vec![FieldError::new(
            "formats",
            "invalid_type",
            "formats must be an array of strings",
        )]);
    };
    let mut formats = Vec::new();
    let mut errors = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let path = format!("formats[{index}]");
        if let Some(object) = item.as_object() {
            match parse_object_format(object, &path) {
                Ok(format) => formats.push(format),
                Err(mut found) => errors.append(&mut found),
            }
            continue;
        }
        let Some(text) = item.as_str() else {
            errors.push(FieldError::new(
                path,
                "unsupported_format",
                format!(
                    "a format is a string or an object; supported strings: {}; object types: {}",
                    ScrapeFormat::SUPPORTED.join(", "),
                    ScrapeFormat::OBJECT_TYPES.join(", ")
                ),
            ));
            continue;
        };
        match ScrapeFormat::parse(text) {
            Some(format) => formats.push(format),
            // INNH-1: json, question and highlights each need an input, so the
            // bare name cannot be honoured; the error names the field it lacks.
            None if AiKind::from_wire(text)
                .and_then(|kind| bare_name_error(kind, &path))
                .is_some() =>
            {
                errors.extend(AiKind::from_wire(text).and_then(|kind| bare_name_error(kind, &path)));
            }
            None => errors.push(FieldError::new(
                path,
                "unsupported_format",
                format!(
                    "unknown format; supported: {}",
                    ScrapeFormat::SUPPORTED.join(", ")
                ),
            )),
        }
    }
    if errors.is_empty() {
        Ok(formats)
    } else {
        Err(errors)
    }
}

/// A format given as an object: `{"type":"screenshot", ...}` or `{"type":"pdf", ...}`.
fn parse_object_format(
    object: &serde_json::Map<String, serde_json::Value>,
    path: &str,
) -> Result<ScrapeFormat, Vec<FieldError>> {
    let kind = object.get("type").and_then(serde_json::Value::as_str);
    let mut body = object.clone();
    body.remove("type");
    let body = serde_json::Value::Object(body);
    match kind {
        Some("screenshot") => {
            let options: ScreenshotOptions =
                serde_json::from_value(body).map_err(|error| vec![option_decode_error(path, &error)])?;
            options.validate(path).map_err(|error| vec![error])?;
            Ok(ScrapeFormat::Screenshot(options))
        }
        Some("pdf") => {
            let options: PdfOptions =
                serde_json::from_value(body).map_err(|error| vec![option_decode_error(path, &error)])?;
            options.validate(path).map_err(|error| vec![error])?;
            Ok(ScrapeFormat::Pdf(options))
        }
        Some(name) if AiKind::from_wire(name).is_some() => {
            let kind = AiKind::from_wire(name).expect("checked");
            let Some(body) = body.as_object() else {
                unreachable!("the body was built from an object");
            };
            parse_ai_object(kind, body, path).map(ScrapeFormat::Ai)
        }
        _ => Err(vec![FieldError::new(
            path,
            "unsupported_format",
            format!(
                "a format object needs a type: {}",
                ScrapeFormat::OBJECT_TYPES.join(", ")
            ),
        )]),
    }
}

/// A capture option the decoder refused: an unknown key, or a value of the wrong type.
fn option_decode_error(path: &str, error: &serde_json::Error) -> FieldError {
    let text = error.to_string();
    if text.starts_with("unknown field") {
        FieldError::new(path, "unknown_field", text)
    } else {
        FieldError::new(path, "invalid_type", "a capture option has the wrong type")
    }
}

fn read_bool(
    name: &str,
    value: &serde_json::Value,
    slot: &mut Option<bool>,
    errors: &mut Vec<FieldError>,
) {
    match value.as_bool() {
        Some(flag) => *slot = Some(flag),
        None => errors.push(FieldError::new(
            name,
            "invalid_type",
            "expected true or false",
        )),
    }
}

fn read_u64(
    name: &str,
    value: &serde_json::Value,
    slot: &mut Option<u64>,
    errors: &mut Vec<FieldError>,
) {
    match value.as_u64() {
        Some(number) => *slot = Some(number),
        None => errors.push(FieldError::new(
            name,
            "invalid_type",
            "expected a non-negative integer",
        )),
    }
}

fn read_strings(
    name: &str,
    value: &serde_json::Value,
    slot: &mut Option<Vec<String>>,
    errors: &mut Vec<FieldError>,
) {
    let Some(items) = value.as_array() else {
        errors.push(FieldError::new(
            name,
            "invalid_type",
            "expected an array of strings",
        ));
        return;
    };
    let mut strings = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item.as_str() {
            Some(text) => strings.push(text.to_owned()),
            None => errors.push(FieldError::new(
                format!("{name}[{index}]"),
                "invalid_type",
                "expected a string",
            )),
        }
    }
    *slot = Some(strings);
}

/// The bounds on what a request may ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeLimits {
    pub max_selectors: usize,
    pub max_selector_len: usize,
    pub min_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub default_inline_bytes: u64,
    pub max_inline_bytes: u64,
    /// Bounds on the actions of a request.
    pub actions: ActionLimits,
    /// Bounds on the captures of a request (lowerable by environment, never raised).
    pub captures: CaptureLimits,
    /// The capture formats (`screenshot`, `pdf`) are accepted only when this is
    /// set. Off by default: a request for them is `feature_disabled`, never a
    /// 200 that silently lacks the capture. WS13.4b sets it from the environment.
    pub captures_enabled: bool,
    /// Same for `actions`. Off by default; WS13.5 sets it from the environment.
    pub actions_enabled: bool,
    /// The AI formats (`json`, `summary`, `question`, `highlights`) are accepted
    /// only when this is set. Off by default: a request for one is
    /// `feature_disabled` at `formats[i]`. INNH-1 sets it from `QUARRY_EDGE__AI_FORMATS`.
    pub ai_formats_enabled: bool,
    /// `max_age` is accepted only when this is set. Off by default: a request for
    /// it is `feature_disabled` at `max_age`. INNH-7 sets it from `QUARRY_EDGE__SCRAPE_MAX_AGE`.
    pub max_age_enabled: bool,
}

impl Default for ScrapeLimits {
    fn default() -> Self {
        Self {
            max_selectors: 32,
            max_selector_len: 256,
            min_timeout_ms: 1_000,
            // Under the edge router's 60 s limit, so a typed Timeout wins.
            max_timeout_ms: 55_000,
            default_inline_bytes: DEFAULT_INLINE_MAX_BYTES as u64,
            max_inline_bytes: 8 * 1024 * 1024,
            actions: ActionLimits::default(),
            captures: CaptureLimits::default(),
            captures_enabled: false,
            actions_enabled: false,
            ai_formats_enabled: false,
            max_age_enabled: false,
        }
    }
}

/// The options a run uses, after defaults and limits.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedScrapeOptions {
    pub formats: Vec<ScrapeFormat>,
    /// The caller named formats (as opposed to the ZDR default filling them in).
    pub requested_formats: bool,
    pub only_main_content: bool,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub remove_base64_images: bool,
    pub extractor: Extractor,
    pub timeout_ms: u64,
    pub max_inline_bytes: usize,
    /// Accepted options this version does not apply (none today; the field stays
    /// so a later option can be accepted before it is applied).
    pub ignored_fields: Vec<String>,
    /// What the request captures from the page: at most one screenshot and one PDF.
    pub capture: CapturePlan,
    /// The actions, checked. `None` when the request has none.
    pub actions: Option<ValidatedActions>,
    /// INNH-7: how old, in milliseconds, a stored result may be and still be served.
    /// `None`: the request did not ask for the URL result cache. `Some(0)`: it must
    /// not be read. It is never part of [`Self::canonical_options_key`]: the key names
    /// what a result IS, `max_age` only says how stale a reader tolerates.
    pub max_age_ms: Option<u64>,
    /// E3.6-5: `On` when the returned text goes through the PII scanner. A redacted result
    /// is not the page's text, so it neither reads nor writes a cache (see [`Self::is_default`]).
    pub redact_pii: PiiRedaction,
}

impl ResolvedScrapeOptions {
    pub fn wants(&self, format: ScrapeFormat) -> bool {
        self.formats.contains(&format)
    }

    /// INNH-1: the AI formats of the request, at most one per kind, in kind
    /// order (`json`, `summary`, `question`, `highlights`) whatever order the
    /// request named them in.
    pub fn ai_specs(&self) -> Vec<&AiSpec> {
        let mut specs: Vec<&AiSpec> = self.formats.iter().filter_map(ScrapeFormat::ai_spec).collect();
        specs.sort_by_key(|spec| spec.kind());
        specs
    }

    /// Nothing that changes what a cached result would hold: a request with
    /// default options may read and write the page cache; any other must not.
    /// The timeout and the inline cap shape delivery, not the result.
    pub fn is_default(&self) -> bool {
        !self.requested_formats
            && self.only_main_content
            && self.include_tags.is_empty()
            && self.exclude_tags.is_empty()
            && !self.remove_base64_images
            && self.extractor == Extractor::Readability
            && self.capture.is_empty()
            && self.actions.is_none()
            && self.redact_pii == PiiRedaction::Off
    }

    /// Whether the page cache may serve or store this request. Only a default
    /// request may: a capture or an action changes what the page holds.
    pub fn cache_admissible(&self) -> bool {
        self.is_default()
    }

    /// INNH-7: whether the URL result cache (`quarry:scrape:{org}:...`, read by `max_age`)
    /// may serve or store this request. Wider than [`Self::cache_admissible`]: named
    /// formats, selectors and an extractor are fine, because [`Self::canonical_options_key`]
    /// is part of the key and each distinct option set is its own entry. A capture or
    /// an action changes what the page holds (and needs the browser), and an AI format
    /// answers from a model: none of those is ever cached. The caller adds what the
    /// options cannot see: ZDR, the driver plan, the web policy and the privacy block.
    pub fn result_cache_admissible(&self) -> bool {
        self.capture.is_empty()
            && self.actions.is_none()
            && self.ai_specs().is_empty()
            && self.redact_pii == PiiRedaction::Off
    }

    /// A stable key for the options that change the result, independent of the
    /// key order in the request, of `zdr`, `timeout_ms`, `max_inline_bytes` and
    /// `max_age`. For the URL-keyed result cache: `quarry:scrape:{org}:{blake3(url, key)}`.
    pub fn canonical_options_key(&self) -> String {
        let mut formats: Vec<&str> = self
            .formats
            .iter()
            .map(ScrapeFormat::wire_name)
            .collect();
        formats.sort_unstable();
        let mut include = self.include_tags.clone();
        include.sort_unstable();
        let mut exclude = self.exclude_tags.clone();
        exclude.sort_unstable();
        let mut canonical = serde_json::json!({
            "formats": formats,
            "only_main_content": self.only_main_content,
            "include_tags": include,
            "exclude_tags": exclude,
            "remove_base64_images": self.remove_base64_images,
            "extractor": self.extractor,
        });
        // Options that are absent keep the key the default request has, so the
        // page cache key of an existing request does not change.
        if !self.capture.is_empty() {
            canonical["capture"] = serde_json::json!({
                "screenshot": self.capture.screenshot,
                "pdf": self.capture.pdf,
            });
        }
        if let Some(actions) = &self.actions {
            canonical["actions"] = serde_json::json!(actions.actions);
            canonical["actions_on_error"] = serde_json::json!(actions.on_error);
        }
        // INNH-1: one `ai:<kind>:<hash16>` per AI format, in kind order. The
        // schema, prompt, question and query are in the hash, never in the key.
        let ai: Vec<String> = self
            .ai_specs()
            .into_iter()
            .map(|spec| format!("ai:{}:{}", spec.kind().wire_name(), spec.canonical_hash16()))
            .collect();
        if !ai.is_empty() {
            canonical["ai"] = serde_json::json!(ai);
        }
        // E3.6-5: a redacted result is another result. Absent for the default request, so
        // the key of every existing request is unchanged.
        if self.redact_pii == PiiRedaction::On {
            canonical["redact_pii"] = serde_json::json!("on");
        }
        blake3::hash(canonical.to_string().as_bytes())
            .to_hex()
            .to_string()
    }
}

/// Whether the page cache was consulted for this response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CacheState {
    Hit,
    Miss,
    /// Not used: ZDR, non-default options or an explicit `cache: off`.
    Bypass,
}

/// The options a response was produced with, so a caller (or a model) can see
/// what was applied and what was not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct EffectiveScrapeOptions {
    pub formats: Vec<ScrapeFormat>,
    pub only_main_content: bool,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub remove_base64_images: bool,
    pub extractor: Extractor,
    pub timeout_ms: u64,
    pub max_inline_bytes: usize,
    pub zdr: bool,
    /// Accepted options this version does not apply, and unknown request keys
    /// that were ignored.
    pub ignored_fields: Vec<String>,
    pub cache_state: CacheState,
    pub doc_routing: String,
    /// Unknown keys were rejected (strict) rather than ignored (lenient).
    pub strict: bool,
    /// The actions as counts only: never a text or a selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<ActionsSummary>,
    /// INNH-7: the `max_age` the request asked for, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<u64>,
    /// INNH-7: on a `max_age` hit, when the served result was fetched (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_at: Option<String>,
    /// INNH-7: on a `max_age` hit, how old the served result was, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_age_ms: Option<u64>,
    /// E3.6-5: `on` when the returned text went through the PII scanner, `off` (the default)
    /// when it did not. Always reported, so a caller can see what was applied.
    #[serde(default)]
    pub redact_pii: PiiRedaction,
}

impl EffectiveScrapeOptions {
    /// Mark this as the echo of a result served from the URL result cache.
    pub fn served_from_cache(mut self, cached_at: chrono::DateTime<chrono::Utc>, age_ms: u64) -> Self {
        self.cache_state = CacheState::Hit;
        self.cached_at = Some(cached_at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true));
        self.cache_age_ms = Some(age_ms);
        self
    }

    pub fn new(
        resolved: &ResolvedScrapeOptions,
        zdr: bool,
        cache_state: CacheState,
        doc_routing: &str,
        strict: bool,
        unknown_keys: &[String],
    ) -> Self {
        let mut ignored_fields = resolved.ignored_fields.clone();
        ignored_fields.extend(unknown_keys.iter().cloned());
        Self {
            formats: resolved.formats.clone(),
            only_main_content: resolved.only_main_content,
            include_tags: resolved.include_tags.clone(),
            exclude_tags: resolved.exclude_tags.clone(),
            remove_base64_images: resolved.remove_base64_images,
            extractor: resolved.extractor,
            timeout_ms: resolved.timeout_ms,
            max_inline_bytes: resolved.max_inline_bytes,
            zdr,
            ignored_fields,
            cache_state,
            doc_routing: doc_routing.to_owned(),
            strict,
            actions: resolved.actions.as_ref().map(ValidatedActions::summary),
            max_age: resolved.max_age_ms,
            cached_at: None,
            cache_age_ms: None,
            redact_pii: resolved.redact_pii,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_char_boundaries() {
        let (cut, truncated) = truncate_on_char_boundary("aæøå", 3);
        assert!(truncated);
        assert_eq!(cut, "aæ");
        let (whole, truncated) = truncate_on_char_boundary("abc", 3);
        assert!(!truncated);
        assert_eq!(whole, "abc");
        let (empty, truncated) = truncate_on_char_boundary("ææ", 1);
        assert!(truncated);
        assert_eq!(empty, "");
    }
    use serde_json::json;

    fn resolve(
        body: serde_json::Value,
        zdr: bool,
    ) -> Result<ResolvedScrapeOptions, Vec<FieldError>> {
        ScrapeOptions::from_value(&body)?.resolve(zdr, &enabled_limits())
    }

    /// The limits with the capture and action flags on, as the dev environment sets them.
    fn enabled_limits() -> ScrapeLimits {
        ScrapeLimits {
            captures_enabled: true,
            actions_enabled: true,
            ai_formats_enabled: true,
            ..ScrapeLimits::default()
        }
    }

    #[test]
    fn captures_and_actions_are_refused_while_their_flags_are_off() {
        let off = ScrapeLimits::default();
        let errors = ScrapeOptions::from_value(&json!({ "formats": ["screenshot"] }))
            .unwrap()
            .resolve(false, &off)
            .unwrap_err();
        assert_eq!(errors[0].code, "feature_disabled");
        assert_eq!(errors[0].path, "formats");
        let errors = ScrapeOptions::from_value(&json!({ "actions": [{ "type": "press", "key": "Enter" }] }))
            .unwrap()
            .resolve(false, &off)
            .unwrap_err();
        assert_eq!(errors[0].code, "feature_disabled");
        assert_eq!(errors[0].path, "actions");
        // A plain request is not affected by the flags.
        assert!(ScrapeOptions::from_value(&json!({ "formats": ["markdown"] }))
            .unwrap()
            .resolve(false, &off)
            .is_ok());
    }

    #[test]
    fn defaults_are_what_the_endpoint_did_before_options_existed() {
        let resolved = resolve(json!({ "url": "https://example.com" }), false).unwrap();
        assert!(resolved.formats.is_empty());
        assert!(resolved.is_default());
        assert!(resolved.only_main_content);
        assert_eq!(resolved.extractor, Extractor::Readability);
        assert_eq!(resolved.timeout_ms, 55_000);
        assert_eq!(resolved.max_inline_bytes, DEFAULT_INLINE_MAX_BYTES);
        assert!(resolved.ignored_fields.is_empty());
    }

    #[test]
    fn zdr_with_no_formats_defaults_to_inline_markdown() {
        let resolved = resolve(json!({}), true).unwrap();
        assert_eq!(resolved.formats, vec![ScrapeFormat::Markdown]);
        // The caller did not ask: the options are still the defaults.
        assert!(!resolved.requested_formats);
        assert!(resolved.is_default());
        // Naming formats overrides the default.
        let resolved = resolve(json!({ "formats": ["links"] }), true).unwrap();
        assert_eq!(resolved.formats, vec![ScrapeFormat::Links]);
        assert!(!resolved.is_default());
    }

    #[test]
    fn camel_case_aliases_are_accepted_and_conflicts_are_not() {
        let resolved = resolve(
            json!({ "formats": ["rawHtml", "markdown"], "onlyMainContent": false, "timeoutMs": 2000, "maxInlineBytes": 4096 }),
            false,
        )
        .unwrap();
        assert_eq!(
            resolved.formats,
            vec![ScrapeFormat::RawHtml, ScrapeFormat::Markdown]
        );
        assert!(!resolved.only_main_content);
        assert_eq!(resolved.timeout_ms, 2000);
        assert_eq!(resolved.max_inline_bytes, 4096);
        // Same value under both spellings is fine; different values are an error.
        assert!(resolve(json!({ "timeout_ms": 2000, "timeoutMs": 2000 }), false).is_ok());
        let errors = resolve(json!({ "timeout_ms": 2000, "timeoutMs": 3000 }), false).unwrap_err();
        assert_eq!(errors[0].code, "conflicting_aliases");
    }

    #[test]
    fn bad_values_are_field_errors_with_a_path_and_all_of_them_are_reported() {
        let errors = resolve(
            json!({
                "formats": ["markdown", "xml", "json", 7],
                "timeout_ms": 5,
                "max_inline_bytes": 99_999_999_u64,
                "include_tags": ["", "ok"],
                "only_main_content": "yes",
            }),
            false,
        )
        .unwrap_err();
        let at = |path: &str| {
            errors
                .iter()
                .find(|error| error.path == path)
                .map(|error| error.code.as_str())
        };
        assert_eq!(at("formats[1]"), Some("unsupported_format"));
        // json needs its object form: the error names the field it lacks.
        assert_eq!(at("formats[2].schema"), Some("missing_field"));
        assert_eq!(at("formats[3]"), Some("unsupported_format"));
        assert_eq!(at("only_main_content"), Some("invalid_type"));
        // from_value stopped at the type errors; range errors come from resolve.
        let errors = resolve(
            json!({ "timeout_ms": 5, "max_inline_bytes": 99_999_999_u64, "include_tags": ["", "ok"] }),
            false,
        )
        .unwrap_err();
        let at = |path: &str| {
            errors
                .iter()
                .find(|error| error.path == path)
                .map(|error| error.code.as_str())
        };
        assert_eq!(at("timeout_ms"), Some("out_of_range"));
        assert_eq!(at("max_inline_bytes"), Some("out_of_range"));
        assert_eq!(at("include_tags[0]"), Some("empty"));
        // The supported list is in the message so the caller can correct itself.
        let errors = resolve(json!({ "formats": ["xml"] }), false).unwrap_err();
        assert!(errors[0]
            .message
            .contains("markdown, html, rawHtml, links, metadata"));
    }

    #[test]
    fn selector_limits_are_enforced() {
        let many: Vec<String> = (0..33).map(|index| format!(".c{index}")).collect();
        let errors = resolve(json!({ "exclude_tags": many }), false).unwrap_err();
        assert_eq!(errors[0].code, "too_many");
        let long = "a".repeat(257);
        let errors = resolve(json!({ "include_tags": [long] }), false).unwrap_err();
        assert_eq!(errors[0].code, "too_long");
        // 32 selectors of 256 characters is the limit and is fine.
        let ok: Vec<String> = (0..32).map(|_| "a".repeat(256)).collect();
        assert!(resolve(json!({ "include_tags": ok }), false).is_ok());
    }

    #[test]
    fn extractor_and_only_main_content_follow_each_other_and_must_agree() {
        // Unset follows the other.
        let r = resolve(json!({ "only_main_content": false }), false).unwrap();
        assert_eq!((r.only_main_content, r.extractor), (false, Extractor::CleanFull));
        let r = resolve(json!({ "extractor": "raw" }), false).unwrap();
        assert_eq!((r.only_main_content, r.extractor), (false, Extractor::Raw));
        let r = resolve(json!({ "extractor": "readability" }), false).unwrap();
        assert_eq!((r.only_main_content, r.extractor), (true, Extractor::Readability));
        let r = resolve(json!({ "only_main_content": false, "extractor": "clean_full" }), false).unwrap();
        assert_eq!(r.extractor, Extractor::CleanFull);
        // Disagreeing is a field error on extractor, both ways round.
        for body in [
            json!({ "only_main_content": true, "extractor": "raw" }),
            json!({ "only_main_content": false, "extractor": "readability" }),
        ] {
            let errors = resolve(body, false).unwrap_err();
            assert_eq!(errors[0].path, "extractor");
            assert_eq!(errors[0].code, "conflicts_with_only_main_content");
        }
        // Nothing is left unapplied now.
        let r = resolve(json!({ "exclude_tags": [".nav"], "remove_base64_images": true }), false).unwrap();
        assert!(r.ignored_fields.is_empty());
        assert!(!r.is_default());
    }

    #[test]
    fn field_names_match_a_fully_populated_serialization() {
        // A fully populated value serializes every field; its keys are the snake_case
        // names, and field_names() must hold each of them plus its camelCase alias.
        let full = ScrapeOptions {
            formats: Some(vec![]),
            only_main_content: Some(true),
            include_tags: Some(vec![]),
            exclude_tags: Some(vec![]),
            remove_base64_images: Some(true),
            extractor: Some(Extractor::Raw),
            timeout_ms: Some(1),
            max_inline_bytes: Some(1),
            actions: Some(vec![]),
            actions_on_error: Some(ActionOnError::Continue),
            max_age: Some(1),
            redact_pii: Some(PiiRedaction::On),
        };
        let value = serde_json::to_value(&full).unwrap();
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        let names = ScrapeOptions::field_names();
        for key in &keys {
            assert!(
                names.contains(&key.as_str()),
                "{key} missing from field_names"
            );
        }
        assert_eq!(
            names.len(),
            keys.len() * 2 - 3,
            "snake and camel spellings; formats, extractor and actions are spelled the same"
        );
        assert!(names.contains(&"onlyMainContent") && names.contains(&"maxInlineBytes"));
        assert!(names.contains(&"actionsOnError") && names.contains(&"actions"));
        assert!(names.contains(&"maxAge") && names.contains(&"max_age"));
        assert!(names.contains(&"redactPii") && names.contains(&"redact_pii"));
        assert_eq!(OPTION_KEYS.len(), 12);
    }

    // ---- E3.6-5: redact_pii

    #[test]
    fn redact_pii_parses_off_and_on_in_both_spellings() {
        let option = |body: serde_json::Value| ScrapeOptions::from_value(&body).unwrap().redact_pii;
        assert_eq!(option(json!({ "redact_pii": "on" })), Some(PiiRedaction::On));
        assert_eq!(option(json!({ "redactPii": "off" })), Some(PiiRedaction::Off));
        assert_eq!(option(json!({ "redact_pii": "on", "redactPii": "on" })), Some(PiiRedaction::On));
        // Absent and null are both "not asked".
        assert_eq!(option(json!({})), None);
        assert_eq!(option(json!({ "redact_pii": null })), None);
        let errors =
            ScrapeOptions::from_value(&json!({ "redact_pii": "on", "redactPii": "off" })).unwrap_err();
        assert_eq!(errors[0].code, "conflicting_aliases");
        assert_eq!(errors[0].path, "redact_pii");
    }

    #[test]
    fn redact_pii_accepts_only_the_words_off_and_on() {
        for bad in [json!(true), json!("yes"), json!("ON"), json!(1), json!(["on"])] {
            let errors = ScrapeOptions::from_value(&json!({ "redact_pii": bad })).unwrap_err();
            assert_eq!(errors[0].code, "unsupported_redact_pii", "{bad}");
            assert_eq!(errors[0].path, "redact_pii", "{bad}");
        }
    }

    #[test]
    fn redact_pii_defaults_to_off_and_an_explicit_off_is_the_same_request() {
        let plain = resolve(json!({}), false).unwrap();
        assert_eq!(plain.redact_pii, PiiRedaction::Off);
        assert!(plain.is_default());
        let named = resolve(json!({ "formats": ["markdown"], "redact_pii": "off" }), false).unwrap();
        assert_eq!(named.redact_pii, PiiRedaction::Off);
        let without = resolve(json!({ "formats": ["markdown"] }), false).unwrap();
        assert_eq!(named.canonical_options_key(), without.canonical_options_key());
    }

    #[test]
    fn redact_pii_outside_zdr_is_refused_because_stored_copies_are_not_redacted() {
        let errors = resolve(json!({ "redact_pii": "on" }), false).unwrap_err();
        assert_eq!(errors[0].path, "redact_pii");
        assert_eq!(errors[0].code, "requires_zdr");
    }

    #[test]
    fn redact_pii_under_zdr_bypasses_both_caches_and_keys_apart() {
        let redacted = resolve(json!({ "redact_pii": "on" }), true).unwrap();
        assert_eq!(redacted.redact_pii, PiiRedaction::On);
        assert!(!redacted.is_default());
        assert!(!redacted.cache_admissible());
        assert!(!redacted.result_cache_admissible());
        let plain = resolve(json!({}), true).unwrap();
        assert_ne!(redacted.canonical_options_key(), plain.canonical_options_key());
    }

    #[test]
    fn redact_pii_is_not_served_with_a_capture_or_an_action() {
        let errors = resolve(json!({ "redact_pii": "on", "formats": ["screenshot"] }), true).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.path == "formats" && e.code == "not_supported_with_redact_pii"),
            "{errors:?}"
        );
        let errors = resolve(
            json!({ "redact_pii": "on", "actions": [{ "type": "press", "key": "Enter" }] }),
            true,
        )
        .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.path == "actions" && e.code == "not_supported_with_redact_pii"),
            "{errors:?}"
        );
        // Text formats are served.
        assert!(resolve(json!({ "redact_pii": "on", "formats": ["markdown", "html"] }), true).is_ok());
    }

    #[test]
    fn effective_options_report_redact_pii_on_and_off() {
        let reported = |body: serde_json::Value, zdr: bool| {
            let resolved = resolve(body, zdr).unwrap();
            let effective =
                EffectiveScrapeOptions::new(&resolved, zdr, CacheState::Bypass, "shadow", false, &[]);
            serde_json::to_value(&effective).unwrap()["redact_pii"].clone()
        };
        assert_eq!(reported(json!({ "redact_pii": "on" }), true), json!("on"));
        assert_eq!(reported(json!({}), true), json!("off"));
        assert_eq!(reported(json!({}), false), json!("off"));
    }

    // ---- INNH-7: max_age

    fn max_age_limits() -> ScrapeLimits {
        ScrapeLimits { max_age_enabled: true, ..enabled_limits() }
    }

    fn resolve_max_age(body: serde_json::Value) -> Result<ResolvedScrapeOptions, Vec<FieldError>> {
        ScrapeOptions::from_value(&body)?.resolve(false, &max_age_limits())
    }

    #[test]
    fn max_age_parses_from_snake_and_camel_case() {
        assert_eq!(ScrapeOptions::from_value(&json!({ "max_age": 5000 })).unwrap().max_age, Some(5000));
        assert_eq!(ScrapeOptions::from_value(&json!({ "maxAge": 5000 })).unwrap().max_age, Some(5000));
        // The same value under both spellings is fine; different values conflict.
        assert_eq!(
            ScrapeOptions::from_value(&json!({ "max_age": 7, "maxAge": 7 })).unwrap().max_age,
            Some(7)
        );
        let errors = ScrapeOptions::from_value(&json!({ "max_age": 1, "maxAge": 2 })).unwrap_err();
        assert_eq!(errors[0].code, "conflicting_aliases");
        assert_eq!(errors[0].path, "max_age");
        // Absent and null are both "not asked".
        assert_eq!(ScrapeOptions::from_value(&json!({})).unwrap().max_age, None);
        assert_eq!(ScrapeOptions::from_value(&json!({ "max_age": null })).unwrap().max_age, None);
    }

    #[test]
    fn max_age_must_be_a_non_negative_integer() {
        for bad in [json!("5000"), json!(-1), json!(1.5), json!(true), json!([1])] {
            let errors = ScrapeOptions::from_value(&json!({ "max_age": bad })).unwrap_err();
            assert_eq!(errors[0].code, "invalid_type", "{bad}");
            assert_eq!(errors[0].path, "max_age");
        }
    }

    #[test]
    fn max_age_is_bounded_to_thirty_days() {
        assert_eq!(MAX_AGE_CEILING_MS, 30 * 24 * 60 * 60 * 1000);
        assert_eq!(resolve_max_age(json!({ "max_age": 0 })).unwrap().max_age_ms, Some(0));
        assert_eq!(
            resolve_max_age(json!({ "max_age": MAX_AGE_CEILING_MS })).unwrap().max_age_ms,
            Some(MAX_AGE_CEILING_MS)
        );
        let errors = resolve_max_age(json!({ "max_age": MAX_AGE_CEILING_MS + 1 })).unwrap_err();
        assert_eq!(errors[0].code, "out_of_range");
        assert_eq!(errors[0].path, "max_age");
    }

    #[test]
    fn max_age_is_feature_disabled_while_its_flag_is_off() {
        let errors = ScrapeOptions::from_value(&json!({ "max_age": 1000 }))
            .unwrap()
            .resolve(false, &ScrapeLimits::default())
            .unwrap_err();
        assert_eq!(errors[0].code, "feature_disabled");
        assert_eq!(errors[0].path, "max_age");
        // A request without it is not affected by the flag.
        let resolved = ScrapeOptions::from_value(&json!({ "formats": ["markdown"] }))
            .unwrap()
            .resolve(false, &ScrapeLimits::default())
            .unwrap();
        assert_eq!(resolved.max_age_ms, None);
    }

    #[test]
    fn max_age_never_rotates_the_options_key() {
        let without = resolve_max_age(json!({ "formats": ["markdown"], "include_tags": ["main"] })).unwrap();
        let with = resolve_max_age(json!({ "formats": ["markdown"], "include_tags": ["main"], "max_age": 60000 })).unwrap();
        let zero = resolve_max_age(json!({ "formats": ["markdown"], "include_tags": ["main"], "max_age": 0 })).unwrap();
        assert_eq!(without.canonical_options_key(), with.canonical_options_key());
        assert_eq!(without.canonical_options_key(), zero.canonical_options_key());
        // ...and it does not make a default request non-default.
        assert!(resolve_max_age(json!({ "max_age": 1 })).unwrap().is_default());
    }

    #[test]
    fn the_result_cache_admits_options_but_not_captures_actions_or_ai() {
        let admissible = |body: serde_json::Value| resolve_max_age(body).unwrap().result_cache_admissible();
        assert!(admissible(json!({})));
        assert!(admissible(json!({ "formats": ["markdown", "html"] })));
        assert!(admissible(json!({ "include_tags": ["main"], "exclude_tags": [".nav"] })));
        assert!(admissible(json!({ "extractor": "raw" })));
        assert!(!admissible(json!({ "formats": ["screenshot"] })));
        assert!(!admissible(json!({ "formats": [{ "type": "pdf" }] })));
        assert!(!admissible(json!({ "actions": [{ "type": "press", "key": "Enter" }] })));
        assert!(!admissible(json!({ "formats": ["markdown", { "type": "summary" }] })));
        assert!(!admissible(json!({ "formats": [{ "type": "question", "question": "Who?" }] })));
    }

    #[test]
    fn effective_options_echo_max_age_and_a_cache_hit() {
        let resolved = resolve_max_age(json!({ "max_age": 1500 })).unwrap();
        let miss = EffectiveScrapeOptions::new(&resolved, false, CacheState::Miss, "shadow", false, &[]);
        let value = serde_json::to_value(&miss).unwrap();
        assert_eq!(value["max_age"], 1500);
        assert_eq!(value["cache_state"], "miss");
        assert!(value.get("cached_at").is_none() && value.get("cache_age_ms").is_none());
        let when = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let hit = miss.served_from_cache(when, 900);
        let value = serde_json::to_value(&hit).unwrap();
        assert_eq!(value["cache_state"], "hit");
        assert_eq!(value["cached_at"], "2026-10-01T12:00:00Z");
        assert_eq!(value["cache_age_ms"], 900);
        // A request without max_age echoes none.
        let plain = resolve_max_age(json!({})).unwrap();
        let value = serde_json::to_value(EffectiveScrapeOptions::new(&plain, false, CacheState::Bypass, "shadow", false, &[])).unwrap();
        assert!(value.get("max_age").is_none());
    }

    #[test]
    fn the_reserved_rejected_list_never_overlaps_an_option() {
        let names = ScrapeOptions::field_names();
        for reserved in RESERVED_REJECTED {
            assert!(!names.contains(&reserved), "{reserved}");
        }
        // The contract has no way to turn off TLS verification or to bring a proxy.
        assert!(RESERVED_REJECTED.contains(&"skipTlsVerification"));
        assert!(RESERVED_REJECTED.contains(&"proxy"));
    }

    #[test]
    fn canonical_options_key_ignores_order_zdr_timeout_and_inline_cap() {
        let a = resolve(
            json!({ "formats": ["markdown", "html"], "include_tags": ["b", "a"] }),
            false,
        )
        .unwrap();
        let b = resolve(
            json!({ "include_tags": ["a", "b"], "formats": ["html", "markdown"], "timeout_ms": 2000, "max_inline_bytes": 10 }),
            false,
        )
        .unwrap();
        let c = resolve(
            json!({ "formats": ["markdown", "html"], "include_tags": ["a", "b"] }),
            true,
        )
        .unwrap();
        assert_eq!(a.canonical_options_key(), b.canonical_options_key());
        assert_eq!(a.canonical_options_key(), c.canonical_options_key());
        // A different result-shaping option changes it.
        let d = resolve(
            json!({ "formats": ["markdown"], "include_tags": ["a", "b"] }),
            false,
        )
        .unwrap();
        assert_ne!(a.canonical_options_key(), d.canonical_options_key());
        assert_eq!(a.canonical_options_key().len(), 64);
    }

    #[test]
    fn effective_options_carry_what_was_ignored_and_the_cache_state() {
        let resolved = resolve(json!({ "exclude_tags": [".nav"] }), false).unwrap();
        let effective = EffectiveScrapeOptions::new(
            &resolved,
            false,
            CacheState::Bypass,
            "shadow",
            false,
            &["maxPages".to_owned()],
        );
        assert_eq!(effective.ignored_fields, vec!["maxPages"]);
        let value = serde_json::to_value(&effective).unwrap();
        assert_eq!(value["cache_state"], "bypass");
        assert_eq!(value["doc_routing"], "shadow");
        assert_eq!(value["strict"], false);
    }

    #[test]
    fn string_formats_screenshot_and_pdf_parse_to_default_options() {
        let resolved = resolve(json!({ "formats": ["screenshot", "pdf"] }), false).unwrap();
        assert_eq!(
            resolved.formats,
            vec![
                ScrapeFormat::Screenshot(ScreenshotOptions::default()),
                ScrapeFormat::Pdf(PdfOptions::default()),
            ]
        );
        assert_eq!(resolved.capture.screenshot, Some(ScreenshotOptions::default()));
        assert_eq!(resolved.capture.pdf, Some(PdfOptions::default()));
        assert!(!resolved.is_default());
        assert!(!resolved.cache_admissible());
        // The object form carries options; a bad one is a field error at its index.
        let shot = resolve(
            json!({ "formats": ["markdown", { "type": "screenshot", "fullPage": true, "quality": 80 }] }),
            false,
        )
        .unwrap();
        let expected = ScreenshotOptions {
            full_page: true,
            quality: Some(80),
            ..ScreenshotOptions::default()
        };
        assert_eq!(shot.capture.screenshot, Some(expected));
        assert_eq!(expected.format(), crate::capture::ImageFormat::Jpeg);
        let errors = resolve(json!({ "formats": [{ "type": "pdf", "maxPages": 500 }] }), false)
            .unwrap_err();
        assert_eq!(errors[0].path, "formats[0].maxPages");
        assert_eq!(errors[0].code, "invalid_max_pages");
        let unknown = resolve(
            json!({ "formats": [{ "type": "screenshot", "fullpage": true }] }),
            false,
        )
        .unwrap_err();
        assert_eq!(unknown[0].code, "unknown_field");
        assert_eq!(unknown[0].path, "formats[0]");
    }

    #[test]
    fn two_screenshot_formats_are_too_many() {
        let errors = resolve(
            json!({ "formats": ["screenshot", { "type": "screenshot", "fullPage": true }] }),
            false,
        )
        .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.code == "too_many" && error.path == "formats"));
    }

    #[test]
    fn actions_are_checked_resolved_and_make_the_request_not_default() {
        let resolved = resolve(
            json!({
                "formats": ["markdown"],
                "actions": [{ "type": "wait", "milliseconds": 100 }],
                "actionsOnError": "continue"
            }),
            false,
        )
        .unwrap();
        let actions = resolved.actions.as_ref().expect("actions are present");
        assert_eq!(actions.actions.len(), 1);
        assert_eq!(actions.on_error, ActionOnError::Continue);
        assert_eq!(actions.total_wait_ms, 100);
        assert!(!resolved.is_default());
        assert!(!resolved.cache_admissible());
        // An empty list is no actions at all.
        let none = resolve(json!({ "actions": [] }), false).unwrap();
        assert!(none.actions.is_none());
        assert!(none.is_default());
        // A denied action is a field error with its entry path.
        let errors = resolve(json!({ "actions": [{ "type": "evaluate", "script": "1" }] }), false)
            .unwrap_err();
        assert_eq!(errors[0].code, "action_denied");
        assert_eq!(errors[0].path, "actions[0].type");
        // Only abort or continue: there is no retry.
        let retry = ScrapeOptions::from_value(&json!({ "actionsOnError": "retry" })).unwrap_err();
        assert_eq!(retry[0].code, "unsupported_on_error");
    }

    #[test]
    fn actions_with_ingest_true_is_a_field_error() {
        let errors = ScrapeOptions::from_value(&json!({
            "actions": [{ "type": "press", "key": "Enter" }],
            "ingest": true
        }))
        .unwrap_err();
        assert_eq!(errors[0].code, "not_supported_with_actions");
        assert_eq!(errors[0].path, "ingest");
        // Without actions, ingest is not this option's concern.
        assert!(ScrapeOptions::from_value(&json!({ "ingest": true })).is_ok());
    }

    #[test]
    fn canonical_options_key_changes_with_actions_and_ignores_zdr() {
        let a = resolve(
            json!({ "formats": ["markdown"], "actions": [{ "type": "wait", "milliseconds": 10 }] }),
            false,
        )
        .unwrap();
        let with_zdr = resolve(
            json!({ "formats": ["markdown"], "actions": [{ "type": "wait", "milliseconds": 10 }] }),
            true,
        )
        .unwrap();
        let other_wait = resolve(
            json!({ "formats": ["markdown"], "actions": [{ "type": "wait", "milliseconds": 20 }] }),
            false,
        )
        .unwrap();
        let no_actions = resolve(json!({ "formats": ["markdown"] }), false).unwrap();
        assert_eq!(a.canonical_options_key(), with_zdr.canonical_options_key());
        assert_ne!(a.canonical_options_key(), other_wait.canonical_options_key());
        assert_ne!(a.canonical_options_key(), no_actions.canonical_options_key());
        // Screenshot options change the key as well.
        let jpeg = resolve(json!({ "formats": [{ "type": "screenshot", "quality": 80 }] }), false)
            .unwrap();
        let png = resolve(json!({ "formats": ["screenshot"] }), false).unwrap();
        assert_ne!(jpeg.canonical_options_key(), png.canonical_options_key());
    }

    #[test]
    fn write_text_is_absent_from_effective_options_and_errors() {
        const TEXT: &str = "SECRET-TYPED-VALUE-7731";
        let resolved = resolve(
            json!({
                "formats": ["markdown"],
                "actions": [{ "type": "write", "selector": "#note", "text": TEXT }]
            }),
            false,
        )
        .unwrap();
        let effective = EffectiveScrapeOptions::new(
            &resolved,
            false,
            CacheState::Bypass,
            "shadow",
            false,
            &[],
        );
        let echoed = serde_json::to_string(&effective).unwrap();
        assert!(!echoed.contains(TEXT));
        assert!(!echoed.contains("#note"));
        assert!(echoed.contains("\"write\""));
        // Over the cap: the error names the path and the rule, never the text.
        let long = format!("{TEXT}{}", "x".repeat(5000));
        let errors = resolve(
            json!({ "actions": [{ "type": "write", "selector": "#note", "text": long }] }),
            false,
        )
        .unwrap_err();
        let shown = serde_json::to_string(&errors).unwrap();
        assert!(!shown.contains(TEXT));
        assert_eq!(errors[0].code, "text_too_long");
    }
    // ---- INNH-1: the AI formats -------------------------------------------

    fn schema_body() -> serde_json::Value {
        json!({ "type": "object", "required": ["title"], "properties": { "title": { "type": "string" } } })
    }

    #[test]
    fn each_ai_object_form_parses_and_a_string_summary_means_the_defaults() {
        let resolved = resolve(
            json!({ "formats": [
                "markdown",
                { "type": "json", "schema": schema_body(), "prompt": "Be exact." },
                "summary",
                { "type": "question", "question": "Who owns it?" },
                { "type": "highlights", "query": "price", "max_highlights": 5, "max_chars_each": 120 },
            ] }),
            false,
        )
        .unwrap();
        let specs = resolved.ai_specs();
        assert_eq!(specs.len(), 4);
        assert_eq!(
            specs.iter().map(|spec| spec.kind()).collect::<Vec<_>>(),
            AiKind::ALL.to_vec()
        );
        assert_eq!(specs[1], &AiSpec::default_summary());
        let AiSpec::Highlights(highlights) = specs[3] else { panic!() };
        assert_eq!((highlights.max_highlights, highlights.max_chars_each), (5, 120));
        assert!(resolved.wants(ScrapeFormat::Markdown));
        // Naming an AI format is a named format: the request is not default.
        assert!(!resolved.is_default());
        assert!(!resolved.cache_admissible());
        // The object summary is the same spec as the string.
        let object = resolve(json!({ "formats": [{ "type": "summary" }] }), false).unwrap();
        assert_eq!(object.ai_specs(), vec![&AiSpec::default_summary()]);
    }

    #[test]
    fn bare_json_question_and_highlights_are_missing_field_at_the_field_they_lack() {
        for (name, path) in [
            ("json", "formats[1].schema"),
            ("question", "formats[1].question"),
            ("highlights", "formats[1].query"),
        ] {
            let errors = resolve(json!({ "formats": ["markdown", name] }), false).unwrap_err();
            assert_eq!(errors.len(), 1, "{name}");
            assert_eq!((errors[0].path.as_str(), errors[0].code.as_str()), (path, "missing_field"));
        }
        // reserved_format is gone for good.
        let errors = resolve(json!({ "formats": ["json", "xml"] }), false).unwrap_err();
        assert!(errors.iter().all(|error| error.code != "reserved_format"));
    }

    #[test]
    fn an_ai_error_inside_an_object_carries_the_request_index() {
        let errors = resolve(
            json!({ "formats": [
                "markdown",
                "links",
                { "type": "json", "schema": { "type": "object", "pattern": "x" } },
            ] }),
            false,
        )
        .unwrap_err();
        assert_eq!(errors[0].path, "formats[2].schema.pattern");
        assert_eq!(errors[0].code, "schema_keyword_not_supported");
        // Several bad formats are all reported.
        let errors = resolve(
            json!({ "formats": [
                { "type": "summary", "max_chars": 5 },
                { "type": "question" },
                { "type": "tweet" },
            ] }),
            false,
        )
        .unwrap_err();
        let at = |path: &str| errors.iter().find(|e| e.path == path).map(|e| e.code.as_str());
        assert_eq!(at("formats[0].max_chars"), Some("out_of_range"));
        assert_eq!(at("formats[1].question"), Some("missing_field"));
        assert_eq!(at("formats[2]"), Some("unsupported_format"));
        // The type list is in the message so the caller can correct itself.
        assert!(errors
            .iter()
            .find(|e| e.path == "formats[2]")
            .unwrap()
            .message
            .contains("json, summary, question, highlights"));
    }

    #[test]
    fn only_one_ai_format_of_each_kind() {
        let errors = resolve(
            json!({ "formats": [
                { "type": "question", "question": "a" },
                "markdown",
                { "type": "question", "question": "b" },
            ] }),
            false,
        )
        .unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!((errors[0].path.as_str(), errors[0].code.as_str()), ("formats[2]", "too_many"));
        // The same spec twice is just as many.
        let errors = resolve(json!({ "formats": ["summary", "summary"] }), false).unwrap_err();
        assert_eq!(errors[0].code, "too_many");
        assert_eq!(errors[0].path, "formats[1]");
        // One of each kind is fine.
        assert!(resolve(
            json!({ "formats": [
                { "type": "json", "schema": schema_body() },
                "summary",
                { "type": "question", "question": "q" },
                { "type": "highlights", "query": "h" },
            ] }),
            false
        )
        .is_ok());
    }

    #[test]
    fn the_ai_formats_are_feature_disabled_at_their_index_while_the_flag_is_off() {
        let off = ScrapeLimits::default();
        assert!(!off.ai_formats_enabled);
        let body = json!({ "formats": [
            "markdown",
            { "type": "json", "schema": schema_body() },
            "links",
            "summary",
        ] });
        let errors = ScrapeOptions::from_value(&body).unwrap().resolve(false, &off).unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!((errors[0].path.as_str(), errors[0].code.as_str()), ("formats[1]", "feature_disabled"));
        assert_eq!((errors[1].path.as_str(), errors[1].code.as_str()), ("formats[3]", "feature_disabled"));
        // A plain request is untouched by the flag.
        assert!(ScrapeOptions::from_value(&json!({ "formats": ["markdown", "links"] }))
            .unwrap()
            .resolve(false, &off)
            .is_ok());
        // On, the same body is accepted.
        let on = ScrapeLimits { ai_formats_enabled: true, ..ScrapeLimits::default() };
        assert!(ScrapeOptions::from_value(&body).unwrap().resolve(false, &on).is_ok());
        // Under ZDR the formats still resolve: the engine, not the contract, answers ZDR_UNAVAILABLE.
        assert!(ScrapeOptions::from_value(&body).unwrap().resolve(true, &on).is_ok());
    }

    #[test]
    fn an_ai_format_with_unknown_keys_is_unknown_field() {
        let errors = resolve(json!({ "formats": [{ "type": "summary", "length": 3 }] }), false).unwrap_err();
        assert_eq!((errors[0].path.as_str(), errors[0].code.as_str()), ("formats[0]", "unknown_field"));
    }

    #[test]
    fn effective_options_name_an_ai_format_and_nothing_of_what_it_asked() {
        const SECRET: &str = "SECRET-AI-INPUT-5530";
        let resolved = resolve(
            json!({ "formats": [
                "markdown",
                { "type": "json", "schema": { "type": "object", "description": SECRET }, "prompt": SECRET },
                { "type": "question", "question": SECRET },
                { "type": "highlights", "query": SECRET },
                "summary",
            ] }),
            false,
        )
        .unwrap();
        let effective = EffectiveScrapeOptions::new(&resolved, false, CacheState::Bypass, "shadow", false, &[]);
        let value = serde_json::to_value(&effective).unwrap();
        assert_eq!(
            value["formats"],
            json!(["markdown", "json", "question", "highlights", "summary"])
        );
        assert!(!value.to_string().contains(SECRET));
        // The stored echo reads back (GET /v1/scrape/{id} deserializes it).
        let back: EffectiveScrapeOptions = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap()["formats"], value["formats"]);
        // A name read back is not a request: it holds none of the input.
        let AiSpec::Question(question) = back.formats[2].ai_spec().unwrap().clone() else { panic!() };
        assert!(question.question.is_empty());
    }

    #[test]
    fn the_canonical_key_follows_the_ai_specs_and_leaves_other_keys_alone() {
        let key = |formats: serde_json::Value| {
            resolve(json!({ "formats": formats }), false).unwrap().canonical_options_key()
        };
        let plain = key(json!(["markdown"]));
        let with_ai = key(json!(["markdown", { "type": "question", "question": "a" }]));
        assert_ne!(plain, with_ai);
        assert_ne!(with_ai, key(json!(["markdown", { "type": "question", "question": "b" }])));
        let schema_a = json!({ "type": "object", "properties": { "a": { "type": "string" } } });
        let schema_b = json!({ "type": "object", "properties": { "b": { "type": "string" } } });
        let json_a = key(json!([{ "type": "json", "schema": schema_a }]));
        assert_ne!(json_a, key(json!([{ "type": "json", "schema": schema_b }])));
        assert_ne!(json_a, key(json!([{ "type": "json", "schema": schema_a, "prompt": "p" }])));
        // Order-stable: formats in another order, a schema with keys in another order.
        let reordered = json!({ "properties": { "a": { "type": "string" } }, "type": "object" });
        assert_eq!(
            key(json!([{ "type": "json", "schema": schema_a }, "summary"])),
            key(json!(["summary", { "type": "json", "schema": reordered }]))
        );
        assert_ne!(key(json!(["summary"])), key(json!([{ "type": "summary", "language": "nb" }])));
        assert_ne!(
            key(json!([{ "type": "highlights", "query": "q" }])),
            key(json!([{ "type": "highlights", "query": "q", "max_highlights": 4 }]))
        );
        // No content in the key: it is a 64-hex digest.
        assert_eq!(with_ai.len(), 64);
    }

    fn sample_output() -> NormalizedOutput {
        use crate::output::{
            ChangeInfo, ChangeStatus, DriverInfo, DriverKind, OutputFormats, PageMetadata, UrlTriple,
        };
        NormalizedOutput {
            run_id: crate::ids::Id::new(),
            url: UrlTriple {
                requested: "https://example.com/".into(),
                final_url: "https://example.com/".into(),
                canonical: None,
            },
            status: 200,
            fetched_at: chrono::Utc::now(),
            fingerprint: "abc".into(),
            formats: OutputFormats::default(),
            change: ChangeInfo { status: ChangeStatus::New, prev_fingerprint: None },
            metadata: PageMetadata::default(),
            driver: DriverInfo {
                kind: DriverKind::Static,
                duration_ms: 1,
                profile: None,
                version: None,
                session_id: None,
                live_view_url: None,
                recording_id: None,
            },
            determinism: None,
            privacy: None,
            branding: None,
            etag: None,
            last_modified: None,
            extraction: None,
            text_fp: None,
        }
    }

    #[test]
    fn a_response_without_ai_fields_has_none_of_them_on_the_wire() {
        let response = ScrapeResponse::from_output(sample_output());
        let value = serde_json::to_value(&response).unwrap();
        for key in ["json", "summary", "answer", "highlights", "ai"] {
            assert!(value.get(key).is_none(), "{key} is on the wire without being asked for");
        }
        // The default wire shape: the output's own keys and nothing added.
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["change", "driver", "fetched_at", "fingerprint", "formats", "metadata", "run_id", "status", "url"]
        );
        // With them set they are there, under the names the contract gives.
        let mut asked = response.clone();
        asked.json = Some(json!({ "title": "T" }));
        asked.summary = Some("S".into());
        asked.answer = Some("A".into());
        asked.highlights = Some(vec![Highlight { text: "tilbud".into(), start: 3, end: 9 }]);
        asked.ai = Some(AiReport::default());
        let value = serde_json::to_value(&asked).unwrap();
        assert_eq!(value["json"]["title"], "T");
        assert_eq!(value["summary"], "S");
        assert_eq!(value["answer"], "A");
        assert_eq!(value["highlights"][0]["end"], 9);
        assert_eq!(value["ai"]["total"]["calls"], 0);
        let back: ScrapeResponse = serde_json::from_value(value).unwrap();
        assert_eq!(back.summary.as_deref(), Some("S"));
        assert!(back.ai.is_some());
    }
}
