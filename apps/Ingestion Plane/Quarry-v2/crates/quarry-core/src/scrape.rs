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

use crate::capture::{CaptureLimits, CapturePlan, PdfOptions, ScreenshotOptions};
use crate::output::NormalizedOutput;
use crate::scrape_actions::{
    validate_actions, ActionLimits, ActionOnError, ActionsSummary, ValidatedActions,
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
        }
    }
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
    /// `reserved_format`, `out_of_range`, `too_many`, `too_long`, `empty`,
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

impl ScrapeFormat {
    pub const SUPPORTED: [&'static str; 7] = [
        "markdown",
        "html",
        "rawHtml",
        "links",
        "metadata",
        "screenshot",
        "pdf",
    ];

    /// Formats the contract reserves for a later slice (typed objects there).
    /// Named in the error so a caller knows they are not a typo.
    const RESERVED: [&'static str; 4] = ["json", "summary", "question", "highlights"];

    fn parse(text: &str) -> Option<Self> {
        match text {
            "markdown" => Some(Self::Markdown),
            "html" => Some(Self::Html),
            "rawHtml" | "raw_html" => Some(Self::RawHtml),
            "links" => Some(Self::Links),
            "metadata" => Some(Self::Metadata),
            "screenshot" => Some(Self::Screenshot(ScreenshotOptions::default())),
            "pdf" => Some(Self::Pdf(PdfOptions::default())),
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
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("unknown scrape format"))
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
                    "enum": ["markdown", "html", "rawHtml", "links", "metadata", "screenshot", "pdf"]
                },
                {
                    "type": "object",
                    "required": ["type"],
                    "properties": { "type": { "enum": ["screenshot", "pdf"] } }
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
}

/// Request keys the options own: each field in snake_case and its camelCase
/// alias. A body key that is neither one of these nor a key of the request
/// itself is unknown. A unit test pins this against a fully populated value.
const OPTION_KEYS: [(&str, &str); 10] = [
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
                    "a format is a string or an object; supported: {}",
                    ScrapeFormat::SUPPORTED.join(", ")
                ),
            ));
            continue;
        };
        match ScrapeFormat::parse(text) {
            Some(format) => formats.push(format),
            None if ScrapeFormat::RESERVED.contains(&text) => errors.push(FieldError::new(
                path,
                "reserved_format",
                format!(
                    "`{text}` is reserved and not available yet; supported: {}",
                    ScrapeFormat::SUPPORTED.join(", ")
                ),
            )),
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
        _ => Err(vec![FieldError::new(
            path,
            "unsupported_format",
            "a format object needs type screenshot or pdf",
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
}

impl ResolvedScrapeOptions {
    pub fn wants(&self, format: ScrapeFormat) -> bool {
        self.formats.contains(&format)
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
    }

    /// Whether the page cache may serve or store this request. Only a default
    /// request may: a capture or an action changes what the page holds.
    pub fn cache_admissible(&self) -> bool {
        self.is_default()
    }

    /// A stable key for the options that change the result, independent of the
    /// key order in the request, of `zdr`, `timeout_ms` and `max_inline_bytes`.
    /// For a URL-keyed result cache: `quarry:scrape:{org}:{blake3(url, key)}`.
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
}

impl EffectiveScrapeOptions {
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
        assert_eq!(at("formats[2]"), Some("reserved_format"));
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
}
