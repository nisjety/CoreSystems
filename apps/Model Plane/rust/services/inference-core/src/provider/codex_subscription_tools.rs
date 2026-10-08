//! Constrained proposals only. Integration Core never executes these tools;
//! Model Gateway still validates and authorizes every action before dispatch.
use super::{ChatMessage, InferRequest, ProviderError, ToolCall};
use serde::Deserialize;
use serde_json::{error::Category, json, Value};
use std::fmt::Write;
use tracing::{info, warn};

fn invalid(message: &str) -> ProviderError {
    ProviderError::InvalidResponse(message.to_owned())
}

pub(super) fn prepare(
    req: &InferRequest,
) -> Result<(Vec<ChatMessage>, Option<Value>), ProviderError> {
    let mut messages = req.messages.clone();
    if req.tools.is_empty() {
        let schema = req
            .structured_output_schema
            .as_ref()
            .map(|raw| {
                let value: Value = serde_json::from_str(raw)
                    .map_err(|_| invalid("invalid subscription output schema"))?;
                if value["type"] != "object" {
                    return Err(invalid(
                        "subscription output schema must describe an object",
                    ));
                }
                Ok(value)
            })
            .transpose()?;
        return Ok((messages, schema));
    }
    if req.structured_output_schema.is_some() {
        return Err(invalid(
            "subscription tools and a separate output schema cannot be combined",
        ));
    }
    let choice = req.tool_choice.as_str();
    if !matches!(choice, "" | "auto" | "none" | "required")
        && !req.tools.iter().any(|tool| tool.name == choice)
    {
        return Err(invalid("subscription tool choice is not offered"));
    }
    let definitions = req
        .tools
        .iter()
        .map(|tool| {
            let parameters: Value = serde_json::from_str(&tool.parameters_json)
                .map_err(|_| invalid("invalid subscription tool parameters"))?;
            Ok(json!({"name":tool.name,"description":tool.description,"parameters":parameters}))
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    // Arguments are encoded as JSON text because the supplied tool schemas may
    // contain optional fields or arbitrary objects unsupported by strict output
    // schemas. The normal gateway dispatch validates their actual contracts.
    messages.push(ChatMessage { role: "system".into(), content: format!(
        "Verevon tool-proposal protocol: return only the constrained JSON object. Do not execute native Codex tools. toolCalls are proposals for Verevon's existing action dispatcher, not claims that work has run. Each arguments field must be a JSON-encoded object matching that tool's parameters. Use content for ordinary response text, usually empty when proposing tools. tool_choice={choice:?}: required means at least one proposal; none means none; a tool name means only that tool; auto permits either. Respect dependencies: do not propose operations that depend on results you have not received. Allowed definitions: {}", Value::Array(definitions)
    ), name: String::new(), compaction_summary: String::new() });
    let names: Vec<_> = if matches!(choice, "" | "auto" | "none" | "required") {
        req.tools.iter().map(|tool| tool.name.as_str()).collect()
    } else {
        vec![choice]
    };
    let minimum_calls = if matches!(choice, "" | "auto" | "none") { 0 } else { 1 };
    let maximum_calls = if choice == "none" { 0 } else { 16 };
    Ok((
        messages,
        Some(json!({
            "type":"object", "additionalProperties":false, "required":["content","toolCalls"],
            "properties": {
                "content":{"type":"string"},
                "toolCalls":{"type":"array","minItems":minimum_calls,"maxItems":maximum_calls,"items":{
                    "type":"object","additionalProperties":false,"required":["name","arguments"],
                    "properties":{"name":{"type":"string","enum":names},"arguments":{"type":"string"}}
                }}
            }
        })),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Proposal {
    content: String,
    tool_calls: Vec<Call>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    name: String,
    arguments: String,
}

pub(super) fn parse(
    req: &InferRequest,
    content: &str,
) -> Result<(String, Vec<ToolCall>), ProviderError> {
    if req.tools.is_empty() {
        return Ok((content.to_owned(), Vec::new()));
    }
    let proposal: Proposal = serde_json::from_str(content)
        .map_err(|_| invalid("invalid subscription tool proposal envelope"))?;
    let choice = req.tool_choice.as_str();
    if proposal.tool_calls.len() > 16
        || (choice == "none" && !proposal.tool_calls.is_empty())
        || (!matches!(choice, "" | "auto" | "none") && proposal.tool_calls.is_empty())
    {
        return Err(invalid(
            "subscription proposal violates tool choice or call limit",
        ));
    }
    let mut calls = Vec::new();
    for call in proposal.tool_calls {
        if !req.tools.iter().any(|tool| tool.name == call.name)
            || (!matches!(choice, "" | "auto" | "none" | "required") && call.name != choice)
        {
            return Err(invalid(
                "subscription proposed a tool outside the allowed selection",
            ));
        }
        let args = parse_arguments(&call.arguments)?;
        if !args.is_object() {
            return Err(invalid("subscription tool arguments must be an object"));
        }
        calls.push(ToolCall {
            id: format!("codex-{}", mp_ids::new_ulid()),
            name: call.name,
            arguments_json: args.to_string(),
        });
    }
    Ok((proposal.content, calls))
}

/// Tool arguments arrive as JSON text nested inside the broker envelope. The
/// strict parse is always tried first and its result is used unchanged when it
/// succeeds. Only a strict failure gives `repair` one chance to fix the few
/// unambiguous damage classes; the repaired text must then parse strictly too,
/// and every later check (object shape, tool allow-list) applies to the result.
/// Nothing about the arguments' content is ever logged: only lengths, offsets,
/// error categories and booleans.
fn parse_arguments(raw: &str) -> Result<Value, ProviderError> {
    parse_arguments_with(raw, repair_tool_arguments)
}

fn parse_arguments_with(
    raw: &str,
    repair: impl FnOnce(&str) -> Option<Repaired>,
) -> Result<Value, ProviderError> {
    let strict_error = match serde_json::from_str::<Value>(raw) {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };
    let mut attempted = RepairKinds::default();
    let mut repaired_still_invalid = false;
    if let Some(repaired) = repair(raw) {
        attempted = repaired.kinds;
        if let Ok(value) = serde_json::from_str::<Value>(&repaired.text) {
            info!(
                repair_kinds = %repaired.kinds.label(),
                args_bytes = raw.len(),
                "subscription tool arguments repaired"
            );
            return Ok(value);
        }
        repaired_still_invalid = true;
    }
    let shape = describe_shape(raw);
    warn!(
        error_category = error_category(&strict_error),
        line = strict_error.line(),
        column = strict_error.column(),
        args_bytes = raw.len(),
        ends_in_open_string = shape.ends_in_open_string,
        max_depth = shape.max_depth,
        raw_control_chars = shape.raw_control_chars,
        repair_kinds = %attempted.label(),
        repaired_still_invalid,
        "invalid subscription tool arguments"
    );
    Err(invalid("invalid subscription tool arguments"))
}

fn error_category(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        Category::Io => "io",
        Category::Syntax => "syntax",
        Category::Data => "data",
        Category::Eof => "eof",
    }
}

/// The unambiguous damage classes `repair_tool_arguments` may fix. A flag is
/// set only when that class was actually present, so the flags double as a
/// content-free description of what was wrong with the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RepairKinds {
    /// A raw U+0000..U+001F inside a string literal, now escaped.
    control_chars: bool,
    /// `\X` with X not a JSON escape letter: `\'` is now `'`, any other `\X`
    /// now a literal backslash followed by X.
    invalid_escapes: bool,
    /// A byte-order mark, or whitespace JSON itself does not allow, around the
    /// text. Plain JSON whitespace (space, tab, CR, LF) is not counted: the
    /// strict parser already accepts it.
    bom_or_whitespace: bool,
    /// A comma directly before a closing `}` or `]`.
    trailing_commas: bool,
}

impl RepairKinds {
    fn any(self) -> bool {
        self.control_chars || self.invalid_escapes || self.bom_or_whitespace || self.trailing_commas
    }

    fn label(self) -> String {
        let names: Vec<&str> = [
            (self.control_chars, "control_chars"),
            (self.invalid_escapes, "invalid_escapes"),
            (self.bom_or_whitespace, "bom_or_whitespace"),
            (self.trailing_commas, "trailing_commas"),
        ]
        .into_iter()
        .filter_map(|(present, name)| present.then_some(name))
        .collect();
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(",")
        }
    }
}

struct Repaired {
    text: String,
    kinds: RepairKinds,
}

/// Strips leading BOMs and whitespace, and trailing whitespace. `kinds` notes
/// whether anything was removed that the strict parser would have rejected.
fn trim_edges<'a>(raw: &'a str, kinds: &mut RepairKinds) -> &'a str {
    let after_lead = raw.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let body = after_lead.trim_end_matches(char::is_whitespace);
    let lead = &raw[..raw.len() - after_lead.len()];
    let trail = &after_lead[body.len()..];
    if lead
        .chars()
        .chain(trail.chars())
        .any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
    {
        kinds.bom_or_whitespace = true;
    }
    body
}

/// Appends one character of a string literal, escaping raw control characters.
fn push_string_char(out: &mut String, c: char, kinds: &mut RepairKinds) {
    match c {
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        c if u32::from(c) < 0x20 => {
            let _ = write!(out, "\\u{:04x}", u32::from(c));
        }
        c => {
            out.push(c);
            return;
        }
    }
    kinds.control_chars = true;
}

/// Fixes only damage whose intended meaning is unambiguous, in one pass that
/// tracks whether it is inside a string literal:
///
/// - inside a string, a raw control character (U+0000..U+001F) becomes its
///   escape (`\n`, `\r`, `\t`, otherwise `\u00xx`);
/// - inside a string, `\X` with X not one of `" \ / b f n r t u` is an invalid
///   escape. `\'` (the JavaScript-style escape) becomes just `'`. For every
///   other X the backslash is kept as literal text: the output is an escaped
///   backslash (`\\`) followed by X, so `\*`, `C:\Users` or `\alpha` decode to
///   exactly what was written, and X then goes through the control-character
///   rule above (a backslash before a raw newline decodes to a backslash and
///   a newline). A `\u` not followed by four hex digits, or a backslash that
///   ends the text, is not repaired at all;
/// - leading BOMs and surrounding whitespace are stripped;
/// - outside strings, a comma that follows a value and directly precedes a
///   closing `}` or `]` (JSON whitespace may sit between) is removed.
///
/// It never adds, closes or invents structure and never alters a character
/// that was already valid, so an unbalanced or truncated document, or one that
/// ends inside a string, stays invalid. Returns `None` when nothing needed
/// fixing or the damage is not of these kinds; the caller re-parses the text.
fn repair_tool_arguments(raw: &str) -> Option<Repaired> {
    let mut kinds = RepairKinds::default();
    let body = trim_edges(raw, &mut kinds);
    if body.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(body.len() + 16);
    let mut in_string = false;
    // Offset in `out` of a comma that is trailing if the next significant
    // character closes its container.
    let mut pending_comma: Option<usize> = None;
    // Last significant character outside strings; a closing quote counts.
    let mut prev: Option<char> = None;
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if in_string {
            match c {
                '"' => {
                    in_string = false;
                    prev = Some('"');
                    out.push('"');
                }
                '\\' => match chars.next()? {
                    escaped @ ('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't') => {
                        out.push('\\');
                        out.push(escaped);
                    }
                    'u' => {
                        let rest = chars.as_str();
                        let hex = rest
                            .get(..4)
                            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))?;
                        out.push_str("\\u");
                        out.push_str(hex);
                        chars = rest[4..].chars();
                    }
                    other => {
                        kinds.invalid_escapes = true;
                        // `\'` is the one invalid escape whose backslash is
                        // noise. Anywhere else it is text the model meant
                        // literally (a Markdown escape, a path, a LaTeX
                        // command), so it survives as an escaped backslash.
                        if other != '\'' {
                            out.push_str("\\\\");
                        }
                        push_string_char(&mut out, other, &mut kinds);
                    }
                },
                _ => push_string_char(&mut out, c, &mut kinds),
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                pending_comma = None;
                out.push('"');
            }
            ' ' | '\t' | '\n' | '\r' => out.push(c),
            ',' => {
                pending_comma = if matches!(prev, None | Some('[' | '{' | ',' | ':')) {
                    None
                } else {
                    Some(out.len())
                };
                prev = Some(',');
                out.push(',');
            }
            '}' | ']' => {
                if let Some(at) = pending_comma.take() {
                    out.remove(at);
                    kinds.trailing_commas = true;
                }
                prev = Some(c);
                out.push(c);
            }
            _ => {
                pending_comma = None;
                prev = Some(c);
                out.push(c);
            }
        }
    }
    if in_string || !kinds.any() {
        return None;
    }
    Some(Repaired { text: out, kinds })
}

/// Content-free shape of text that failed to parse, for the warning log.
struct Shape {
    ends_in_open_string: bool,
    max_depth: usize,
    /// A raw U+0000..U+001F inside a string literal.
    raw_control_chars: bool,
}

fn describe_shape(text: &str) -> Shape {
    let mut shape = Shape { ends_in_open_string: false, max_depth: 0, raw_control_chars: false };
    let (mut in_string, mut escaped, mut depth) = (false, false, 0_usize);
    for c in text.chars() {
        if in_string {
            if u32::from(c) < 0x20 {
                shape.raw_control_chars = true;
            }
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => {
                depth += 1;
                shape.max_depth = shape.max_depth.max(depth);
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    shape.ends_in_open_string = in_string;
    shape
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ToolDefinition;
    fn request() -> InferRequest {
        InferRequest {
            tools: vec![ToolDefinition {
                name: "create_artifact".into(),
                description: "Draft a document".into(),
                parameters_json: r#"{"type":"object"}"#.into(),
            }],
            tool_choice: "required".into(),
            ..Default::default()
        }
    }
    #[test]
    fn proposals_are_data_and_unknown_or_malformed_actions_fail_closed() {
        let req = request();
        let (messages, schema) = prepare(&req).unwrap();
        assert!(messages
            .last()
            .unwrap()
            .content
            .contains("Do not execute native Codex tools"));
        assert_eq!(
            schema.unwrap()["properties"]["toolCalls"]["items"]["properties"]["name"]["enum"],
            json!(["create_artifact"])
        );
        let (_, schema) = prepare(&req).unwrap();
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["minItems"], 1);
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["maxItems"], 16);
        let (_, schema) = prepare(&InferRequest { tool_choice: "none".into(), ..req.clone() }).unwrap();
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["maxItems"], 0);
        let mut named = req.clone();
        named.tools.push(ToolDefinition { name: "finish_tool_phase".into(), description: "Finish".into(), parameters_json: r#"{"type":"object"}"#.into() });
        named.tool_choice = "create_artifact".into();
        let (_, schema) = prepare(&named).unwrap();
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["items"]["properties"]["name"]["enum"], json!(["create_artifact"]));
        for value in [
            json!({"content":"","toolCalls":[]}),
            json!({"content":"","toolCalls":[{"name":"shell","arguments":"{}"}]}),
            json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":"[]"}]}),
            json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":"incomplete"}]}),
        ] {
            assert!(parse(&req, &value.to_string()).is_err());
        }
        let raw=json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":"{\"content\":\"Hei\"}"}]}).to_string();
        let (_, calls) = parse(&req, &raw).unwrap();
        assert_eq!(calls[0].name, "create_artifact");
        assert_eq!(
            serde_json::from_str::<Value>(&calls[0].arguments_json).unwrap()["content"],
            "Hei"
        );
        assert_ne!(calls[0].id, parse(&req, &raw).unwrap().1[0].id);
        assert!(parse(
            &InferRequest {
                tool_choice: "none".into(),
                ..req
            },
            &raw
        )
        .is_err());
    }
    #[test]
    fn structured_review_and_plain_text_have_distinct_protocols() {
        assert!(prepare(&InferRequest::default()).unwrap().1.is_none());
        let req = InferRequest {
            structured_output_schema: Some(
                r#"{"type":"object","properties":{"accepted":{"type":"boolean"}}}"#.into(),
            ),
            ..Default::default()
        };
        assert_eq!(
            prepare(&req).unwrap().1.unwrap()["properties"]["accepted"]["type"],
            "boolean"
        );
        assert!(prepare(&InferRequest {
            structured_output_schema: req.structured_output_schema,
            ..request()
        })
        .is_err());
    }

    // ---- tool-argument repair ---------------------------------------------

    const NONE: RepairKinds = RepairKinds {
        control_chars: false,
        invalid_escapes: false,
        bom_or_whitespace: false,
        trailing_commas: false,
    };
    const CONTROL: RepairKinds = RepairKinds { control_chars: true, ..NONE };
    const ESCAPES: RepairKinds = RepairKinds { invalid_escapes: true, ..NONE };
    const BOM_WS: RepairKinds = RepairKinds { bom_or_whitespace: true, ..NONE };
    const COMMAS: RepairKinds = RepairKinds { trailing_commas: true, ..NONE };
    const INVALID: &str = "invalid subscription tool arguments";
    const NOT_OBJECT: &str = "subscription tool arguments must be an object";

    /// A Norwegian Markdown body with quotes, backslashes, a literal `\u00e5`,
    /// tabs, CRLF, an emoji and a `,]` that sits inside the text.
    const NORWEGIAN: &str = "# Årsrapport 2026\n\nDette er en **test** av «norske» tegn: æ ø å Æ Ø Å.\n\n- Punkt én: \"sitat\" og 'apostrof'\n- Sti: C:\\Users\\Ola\\Dokumenter og \\u00e5 skrevet ut\n\tinnrykk med tab\r\nWindows-linjeskift\n\nRegex \\d+\\.\\d{2} og JSON {\"a\": [1, 2,]}\n\nSlutt 🙂";

    fn envelope(arguments: &str) -> String {
        json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":arguments}]})
            .to_string()
    }

    /// The single call's decoded arguments, or the InvalidResponse message.
    fn decoded(arguments: &str) -> Result<Value, String> {
        match parse(&request(), &envelope(arguments)) {
            Ok((_, calls)) => Ok(serde_json::from_str(&calls[0].arguments_json).unwrap()),
            Err(ProviderError::InvalidResponse(message)) => Err(message),
            Err(_) => Err("unexpected error variant".to_owned()),
        }
    }

    fn repaired(raw: &str) -> Option<(String, RepairKinds)> {
        repair_tool_arguments(raw).map(|r| (r.text, r.kinds))
    }

    /// How a model damages a document: quotes and backslashes are escaped
    /// properly but newlines, tabs and carriage returns are left raw.
    fn escape_quotes_and_backslashes(text: &str) -> String {
        text.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn norwegian_arguments(prefix: &str, trailing_comma: bool, suffix: &str) -> String {
        format!(
            "{prefix}{{\"title\":\"Årsrapport\",\"content\":\"{}\"{}}}{suffix}",
            escape_quotes_and_backslashes(NORWEGIAN),
            if trailing_comma { "," } else { "" }
        )
    }

    #[test]
    fn repair_escapes_raw_control_characters_inside_strings_only() {
        let raw = "{\n\"content\":\"a\nb\rc\td\u{1}e\u{8}f\u{1f}g\"\n}";
        assert!(serde_json::from_str::<Value>(raw).is_err());
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, "{\n\"content\":\"a\\nb\\rc\\td\\u0001e\\u0008f\\u001fg\"\n}");
        assert_eq!(kinds, CONTROL);
        assert_eq!(
            decoded(raw).unwrap(),
            json!({"content":"a\nb\rc\td\u{1}e\u{8}f\u{1f}g"})
        );
    }

    #[test]
    fn repair_keeps_the_backslash_of_invalid_escapes_except_before_an_apostrophe() {
        let raw = r#"{"content":"it\'s \ a \* \å end"}"#;
        assert!(serde_json::from_str::<Value>(raw).is_err());
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, r#"{"content":"it's \\ a \\* \\å end"}"#);
        assert_eq!(kinds, ESCAPES);
        assert_eq!(decoded(raw).unwrap(), json!({"content":"it's \\ a \\* \\å end"}));

        // Valid escapes beside an invalid one are left exactly as they were,
        // and `\\` followed by a quote character is a pair, not an escape.
        let raw = r#"{"content":"\' \\ \" \/ \u00e5 \n","other":"a\\'b\'"}"#;
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, r#"{"content":"' \\ \" \/ \u00e5 \n","other":"a\\'b'"}"#);
        assert_eq!(kinds, ESCAPES);
        assert_eq!(
            decoded(raw).unwrap(),
            json!({"content":"' \\ \" / å \n","other":"a\\'b'"})
        );

        // A backslash before a raw newline keeps the backslash and the newline
        // is then escaped too: the decoded text is a backslash and a newline.
        let raw = "{\"content\":\"a\\\nb\"}";
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, "{\"content\":\"a\\\\\\nb\"}");
        assert_eq!(kinds, RepairKinds { control_chars: true, invalid_escapes: true, ..NONE });
        assert_eq!(decoded(raw).unwrap(), json!({"content":"a\\\nb"}));
    }

    /// `{"content":"<body>"}` with `body` placed verbatim inside the quotes.
    fn content_document(body: &str) -> String {
        format!("{{\"content\":\"{body}\"}}")
    }

    #[test]
    fn markdown_escapes_keep_their_backslash() {
        // The eight Markdown escapes models write most often come first, then
        // the rest of ASCII punctuation that is neither a JSON escape (`"`, `\`,
        // `/`) nor the apostrophe, which has a rule of its own.
        const PUNCTUATION: &str = "*_#[()!-]{}<>.,:;+=?@$%&^|~`";
        for mark in PUNCTUATION.chars() {
            let raw = content_document(&format!("a \\{mark} b"));
            assert!(serde_json::from_str::<Value>(&raw).is_err(), "{mark}");
            let (text, kinds) = repaired(&raw).unwrap();
            assert_eq!(text, content_document(&format!("a \\\\{mark} b")), "{mark}");
            assert_eq!(kinds, ESCAPES, "{mark}");
            assert_eq!(
                decoded(&raw).unwrap(),
                json!({"content": format!("a \\{mark} b")}),
                "{mark}"
            );
        }
        // Several in one document: every backslash survives, in place.
        let body = r"\# Tittel \*ikke kursiv\* snake\_case \[lenke\]\(url\) \! \-";
        assert_eq!(decoded(&content_document(body)).unwrap(), json!({"content": body}));
    }

    #[test]
    fn paths_latex_and_non_ascii_after_a_backslash_decode_as_written() {
        let raw = content_document(r"C:\Users\me");
        assert!(serde_json::from_str::<Value>(&raw).is_err());
        let (text, kinds) = repaired(&raw).unwrap();
        assert_eq!(text, content_document(r"C:\\Users\\me"));
        assert_eq!(kinds, ESCAPES);
        assert_eq!(decoded(&raw).unwrap(), json!({"content": r"C:\Users\me"}));

        // `\a` and `\g` are not JSON escapes (unlike `\b` and `\f`), so a LaTeX
        // command written with them keeps its backslash.
        let raw = content_document(r"\alpha + \gamma");
        assert_eq!(decoded(&raw).unwrap(), json!({"content": r"\alpha + \gamma"}));

        let raw = content_document(r"\å og \Å og \🙂");
        assert_eq!(repaired(&raw).unwrap().1, ESCAPES);
        assert_eq!(decoded(&raw).unwrap(), json!({"content": r"\å og \Å og \🙂"}));

        // A Python-style `\U0001F600` is an invalid escape (the JSON one is a
        // lowercase `\u`), so it stays the ten literal characters.
        let raw = content_document(r"\U0001F600");
        let (text, kinds) = repaired(&raw).unwrap();
        assert_eq!(text, content_document(r"\\U0001F600"));
        assert_eq!(kinds, ESCAPES);
        assert_eq!(decoded(&raw).unwrap(), json!({"content": r"\U0001F600"}));

        // Escapes that are valid are never reinterpreted, even where they look
        // like a path separator: the `\n` below stays a newline.
        let raw = content_document(r"C:\Users\me\notes");
        assert_eq!(decoded(&raw).unwrap(), json!({"content": "C:\\Users\\me\notes"}));
    }

    #[test]
    fn a_backslash_before_a_raw_newline_decodes_to_a_backslash_and_a_newline() {
        // A CommonMark hard line break written as a backslash at the end of a
        // line, once with LF and once with CRLF line endings.
        let raw = "{\"content\":\"first\\\nsecond\\\r\nthird\"}";
        assert!(serde_json::from_str::<Value>(raw).is_err());
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, "{\"content\":\"first\\\\\\nsecond\\\\\\r\\nthird\"}");
        assert_eq!(kinds, RepairKinds { control_chars: true, invalid_escapes: true, ..NONE });
        assert_eq!(decoded(raw).unwrap(), json!({"content": "first\\\nsecond\\\r\nthird"}));

        // The same holds for any other raw control character after a backslash.
        let raw = "{\"content\":\"a\\\tb\\\u{1}c\"}";
        assert_eq!(decoded(raw).unwrap(), json!({"content": "a\\\tb\\\u{1}c"}));
    }

    #[test]
    fn the_javascript_apostrophe_escape_still_drops_its_backslash() {
        let raw = content_document(r"it\'s \'quoted\' and don\'t");
        let (text, kinds) = repaired(&raw).unwrap();
        assert_eq!(text, content_document("it's 'quoted' and don't"));
        assert_eq!(kinds, ESCAPES);
        assert_eq!(decoded(&raw).unwrap(), json!({"content": "it's 'quoted' and don't"}));

        // A valid `\\` before an apostrophe is a pair followed by a plain
        // apostrophe: nothing to repair, and nothing touched beside real damage.
        let raw = content_document(r"x\\'y");
        assert!(serde_json::from_str::<Value>(&raw).is_ok());
        assert!(repaired(&raw).is_none());
        assert_eq!(decoded(&raw).unwrap(), json!({"content": r"x\'y"}));
        let raw = content_document("x\\\\'y\n");
        let (text, kinds) = repaired(&raw).unwrap();
        assert_eq!(text, content_document(r"x\\'y\n"));
        assert_eq!(kinds, CONTROL);
        assert_eq!(decoded(&raw).unwrap(), json!({"content": "x\\'y\n"}));

        // A pair, then an apostrophe escape: one backslash and one apostrophe.
        let raw = content_document(r"\\\'");
        let (text, kinds) = repaired(&raw).unwrap();
        assert_eq!(text, content_document(r"\\'"));
        assert_eq!(kinds, ESCAPES);
        assert_eq!(decoded(&raw).unwrap(), json!({"content": r"\'"}));
    }

    #[test]
    fn invalid_escapes_are_reported_whichever_way_they_are_repaired() {
        // The apostrophe branch, the keep-the-backslash branch and both at once.
        for raw in [r#"{"a":"it\'s"}"#, r#"{"a":"\*"}"#, r#"{"a":"\'\*"}"#] {
            let (_, kinds) = repaired(raw).unwrap();
            assert_eq!(kinds, ESCAPES, "{raw}");
            assert_eq!(kinds.label(), "invalid_escapes", "{raw}");
        }
        // Valid pairs and escaped quotes beside `*` and `'` are not invalid
        // escapes, so nothing is reported and repair has nothing to do.
        assert!(repaired(r#"{"a":"\\*\\'","b":"\""}"#).is_none());
        // Reported together with the other classes, in the fixed label order.
        let (_, kinds) = repaired("{\"a\":\"\\*\n\",}").unwrap();
        assert_eq!(kinds.label(), "control_chars,invalid_escapes,trailing_commas");
    }

    #[test]
    fn malformed_unicode_escapes_are_never_repaired() {
        for bad in [r"\u", r"\u1", r"\u12", r"\u00e", r"\u12G4", r"\uZZZZ", r#"\u12""#] {
            // A raw newline sits in front, so the text is otherwise repairable.
            let raw = format!("{{\"content\":\"a\n{bad}\"}}");
            assert!(repaired(&raw).is_none(), "{bad}");
            assert_eq!(decoded(&raw), Err(INVALID.to_owned()), "{bad}");
        }
        // Well-formed hex is copied through; a lone surrogate is then rejected
        // by the parser, not silently replaced.
        let raw = "{\"content\":\"a\n\\ud800\"}";
        assert!(repaired(raw).is_some());
        assert_eq!(decoded(raw), Err(INVALID.to_owned()));
        // A well-formed pair still decodes.
        let raw = "{\"content\":\"a\n\\ud83d\\ude42 \\u00e5\"}";
        assert_eq!(decoded(raw).unwrap(), json!({"content":"a\n🙂 å"}));
    }

    #[test]
    fn repair_strips_a_leading_bom_and_surrounding_whitespace() {
        for raw in [
            "\u{feff}{\"a\":1}",
            "\u{feff}  \n{\"a\":1}\r\n",
            "\u{a0}{\"a\":1}\u{2003}",
            "\u{feff}\u{feff}{\"a\":1}",
            "  \u{feff}{\"a\":1}",
        ] {
            assert!(serde_json::from_str::<Value>(raw).is_err());
            assert_eq!(repaired(raw), Some(("{\"a\":1}".to_owned(), BOM_WS)));
            assert_eq!(decoded(raw).unwrap(), json!({"a":1}));
        }
        // Plain JSON whitespace is not damage, so it is not reported as such.
        assert_eq!(
            repaired("\n {\"a\":1,}\n"),
            Some(("{\"a\":1}".to_owned(), COMMAS))
        );
        // Only a leading BOM is stripped; one anywhere else stays and fails.
        assert!(repaired("{\"a\":1}\u{feff}").is_none());
        assert_eq!(decoded("{\"a\":1}\u{feff}"), Err(INVALID.to_owned()));
        assert_eq!(decoded("{\"a\":\"\u{feff}x\"}").unwrap(), json!({"a":"\u{feff}x"}));
    }

    #[test]
    fn repair_removes_commas_before_closers_outside_strings_only() {
        // No multi-line literals: the source checkout uses CRLF line endings.
        let raw = "{\"a\":[1,2,],\"b\":{\"c\":\"x,}\",},\"d\":[ ],\"e\":[[1],],\"f\":[true, null,\n ],\"g\":\"y\",\n}";
        assert!(serde_json::from_str::<Value>(raw).is_err());
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(kinds, COMMAS);
        assert_eq!(
            text,
            "{\"a\":[1,2],\"b\":{\"c\":\"x,}\"},\"d\":[ ],\"e\":[[1]],\"f\":[true, null\n ],\"g\":\"y\"\n}"
        );
        assert_eq!(
            decoded(raw).unwrap(),
            json!({"a":[1,2],"b":{"c":"x,}"},"d":[],"e":[[1]],"f":[true,null],"g":"y"})
        );
        // A comma that does not follow a value is never dropped.
        for raw in [
            "[,]",
            "{,}",
            r#"{"a":1,,}"#,
            r#"{"a":,}"#,
            r#"{"a":[,]}"#,
            r#"{"a":1 "b":2,}"#,
            r#"{"a":1,} {"b":2,}"#,
        ] {
            assert_eq!(decoded(raw), Err(INVALID.to_owned()), "{raw}");
        }
        // Repaired to a valid array: the object check then applies unchanged.
        assert_eq!(decoded("[1,]"), Err(NOT_OBJECT.to_owned()));
    }

    #[test]
    fn norwegian_document_with_raw_newlines_round_trips_exactly() {
        let raw = norwegian_arguments("", false, "");
        assert!(serde_json::from_str::<Value>(&raw).is_err());
        let (_, kinds) = repaired(&raw).unwrap();
        assert_eq!(kinds, CONTROL);
        let args = decoded(&raw).unwrap();
        assert_eq!(args["content"].as_str().unwrap().as_bytes(), NORWEGIAN.as_bytes());
        assert_eq!(args["title"], "Årsrapport");

        // Every class at once, as the repair pass would see a badly damaged reply.
        let raw = norwegian_arguments("\u{feff}\n", true, "\n");
        let (_, kinds) = repaired(&raw).unwrap();
        assert_eq!(
            kinds,
            RepairKinds { control_chars: true, bom_or_whitespace: true, trailing_commas: true, ..NONE }
        );
        let args = decoded(&raw).unwrap();
        assert_eq!(args["content"].as_str().unwrap().as_bytes(), NORWEGIAN.as_bytes());

        // The same body encoded properly takes the strict path untouched.
        let strict = json!({"title":"Årsrapport","content":NORWEGIAN}).to_string();
        assert!(repaired(&strict).is_none());
        assert_eq!(decoded(&strict).unwrap()["content"], NORWEGIAN);
    }

    /// What a model meant: Markdown escapes, a Windows path, a UNC path, a
    /// LaTeX command, `\å`, a hard line break, backslash text that looks like
    /// a JSON escape (`\n`, `\u00e5`, `\'`), quotes, apostrophes, tabs, CRLF,
    /// Norwegian letters and an emoji.
    const NORWEGIAN_MARKDOWN: &str = "# Årsrapport 2026\n\nHusk at 2 \\* 3 ikke er kursiv, og at snake\\_case\\_navn beholder understrekene. Skriv \\# for en ekte skigard, \\[lenke\\]\\(https://eksempel.no\\) \\! og \\- som tekst.\n\nSti: C:\\Users\\Ola\\Dokumenter\\Årsrapport.docx, UNC \\\\server\\delt og LaTeX \\alpha + \\gamma \\å.\n\nEkte tegn: \\n, \\u00e5 og \\' skal bli stående som tekst.\n\nEt hardt linjeskift\\\nmidt i et avsnitt.\n\n- Punkt én: \"sitat\" og 'apostrof' og æ ø å Æ Ø Å\n\tinnrykk med tab\r\nWindows-linjeskift\n\nSlutt 🙂";

    /// How an inconsistent model writes `text` as the inside of a JSON string.
    /// It escapes quotes and tabs, writes every apostrophe as `\'`, and writes
    /// `Å` and the emoji as `\u` escapes. It doubles a backslash only where a
    /// single one would be read as a JSON escape (so `\n` stays literal text),
    /// and otherwise writes it once: `\*`, `C:\Users`, `\alpha`. Newlines are
    /// either escaped or left raw.
    fn model_encode(text: &str, raw_newlines: bool) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        for (at, &c) in chars.iter().enumerate() {
            match c {
                '"' => out.push_str("\\\""),
                '\'' => out.push_str("\\'"),
                '\t' => out.push_str("\\t"),
                '\n' if !raw_newlines => out.push_str("\\n"),
                '\r' if !raw_newlines => out.push_str("\\r"),
                'Å' => out.push_str("\\u00c5"),
                '🙂' => out.push_str("\\ud83d\\ude42"),
                '\\' => {
                    let read_as_escape = match chars.get(at + 1) {
                        None => true,
                        Some('\n' | '\r') => !raw_newlines,
                        Some(next) => "\"\\/'bfnrtu\tÅ🙂".contains(*next),
                    };
                    out.push_str(if read_as_escape { "\\\\" } else { "\\" });
                }
                c => out.push(c),
            }
        }
        out
    }

    /// NORWEGIAN_MARKDOWN as model_encode writes it, wrapped in an argument
    /// object; optionally with a prefix (a BOM) and a trailing comma.
    fn markdown_arguments(prefix: &str, raw_newlines: bool, trailing_comma: bool) -> String {
        let body = model_encode(NORWEGIAN_MARKDOWN, raw_newlines);
        format!(
            "{prefix}{{\"title\":\"Årsrapport\",\"content\":\"{body}\"{}}}",
            if trailing_comma { "," } else { "" }
        )
    }

    #[test]
    fn norwegian_document_with_markdown_and_valid_escapes_round_trips_exactly() {
        for raw_newlines in [false, true] {
            let body = model_encode(NORWEGIAN_MARKDOWN, raw_newlines);
            // The encoder must really produce the damage under test: single
            // backslashes before Markdown punctuation, a path, LaTeX and `\å`,
            // beside doubled ones and valid `\u`, `\"`, `\t` and `\'` escapes.
            for expected in [
                "2 \\* 3",
                "snake\\_case\\_navn",
                "\\# for",
                "\\[lenke\\]\\(https",
                "C:\\Users\\Ola\\Dokumenter\\\\\\u00c5rsrapport",
                "UNC \\\\\\server\\delt",
                "\\alpha + \\gamma \\å.",
                "Ekte tegn: \\\\n, \\\\u00e5 og \\\\\\' skal",
                "\\\"sitat\\\" og \\'apostrof\\'",
                "\\ud83d\\ude42",
                "\\tinnrykk med tab",
            ] {
                assert!(body.contains(expected), "{raw_newlines}: {expected}");
            }
            assert_eq!(body.contains('\n'), raw_newlines);

            let raw = format!("{{\"title\":\"Årsrapport\",\"content\":\"{body}\"}}");
            assert!(serde_json::from_str::<Value>(&raw).is_err());
            let (_, kinds) = repaired(&raw).unwrap();
            assert_eq!(
                kinds,
                RepairKinds { control_chars: raw_newlines, invalid_escapes: true, ..NONE }
            );
            let args = decoded(&raw).unwrap();
            assert_eq!(args["content"].as_str().unwrap().as_bytes(), NORWEGIAN_MARKDOWN.as_bytes());
            assert_eq!(args["title"], "Årsrapport");

            // Every class at once, as a badly damaged reply would arrive.
            let raw = format!("\u{feff}\n{{\"title\":\"Årsrapport\",\"content\":\"{body}\",}}\n");
            let (_, kinds) = repaired(&raw).unwrap();
            assert_eq!(
                kinds,
                RepairKinds {
                    control_chars: raw_newlines,
                    invalid_escapes: true,
                    bom_or_whitespace: true,
                    trailing_commas: true,
                }
            );
            let args = decoded(&raw).unwrap();
            assert_eq!(args["content"].as_str().unwrap().as_bytes(), NORWEGIAN_MARKDOWN.as_bytes());
        }
    }

    #[test]
    fn valid_arguments_take_the_strict_path_and_never_reach_repair() {
        // Each of these would be changed by a blanket "fix commas, strip
        // whitespace, unescape quotes" pass, so a repair call would show.
        for raw in [
            "  {\"content\":\"a\\\\'b \\n \\u00e5\"}\n",
            r#"{"a":"x,}","b":"\\'","c":"[1,]","d":"\\\\u12","e":"å"}"#,
            "{\"a\":\"\u{feff}\u{a0}\"}",
            r#"{"a":[1,2],"b":{}}"#,
        ] {
            let expected: Value = serde_json::from_str(raw).unwrap();
            let got = parse_arguments_with(raw, |_| panic!("repair ran on valid arguments"));
            assert_eq!(got.unwrap(), expected);
            assert!(repair_tool_arguments(raw).is_none());
            assert_eq!(decoded(raw).unwrap(), expected);
        }
        // An invalid document does reach repair, exactly once.
        let calls = std::cell::Cell::new(0);
        let got = parse_arguments_with("{\"a\":1,}", |raw| {
            calls.set(calls.get() + 1);
            repair_tool_arguments(raw)
        });
        assert_eq!(got.unwrap(), json!({"a":1}));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn unrepairable_arguments_still_fail_with_the_same_errors() {
        for raw in [
            "",
            "   ",
            r#"{"content":"abc""#,
            r#"{"content":"abc"#,
            "{\"content\":\"abc\ndef",
            "{\"content\":\"a\n",
            "\u{feff}{\"content\":\"abc",
            r#"{"a":[1,2"#,
            r#"{"a":{"b":1}"#,
            r#"{"a":[1,2}"#,
            r#"{"a":1}}"#,
            r#"{"a":1,"#,
            r#"{"a":1,,}"#,
            r#"{"content":"a\"#,
            r#"{"content":"a\"}"#,
            "{\"a\":1,}\n{\"b\":2,}",
            r#"{"a":"b" "c":"d",}"#,
            r"{'a':1}",
            "{\"a\":1} trailing",
        ] {
            assert_eq!(decoded(raw), Err(INVALID.to_owned()), "{raw:?}");
        }
        for raw in ["[]", "null", r#""text""#, "42", "true", "[1,]", "\u{feff}[1]", "\"a\nb\""] {
            assert_eq!(decoded(raw), Err(NOT_OBJECT.to_owned()), "{raw:?}");
        }
    }

    #[test]
    fn repair_never_completes_a_truncated_document() {
        let strict = json!({"title":"Årsrapport","content":NORWEGIAN,"n":[1,{"k":[]}]}).to_string();
        // Invalid escapes are repaired in place (kept backslashes, apostrophe
        // escapes, a backslash before a raw newline), so cutting a document that
        // holds them must be refused exactly like cutting any other.
        for doc in [
            strict,
            norwegian_arguments("", false, ""),
            markdown_arguments("", false, false),
            markdown_arguments("", true, false),
            markdown_arguments("\u{feff}", true, true),
            norwegian_arguments("\u{feff}", true, "\n"),
        ] {
            let last_closer = doc.rfind('}').unwrap();
            for cut in doc.char_indices().map(|(at, _)| at).filter(|&at| at <= last_closer) {
                assert!(
                    parse_arguments(&doc[..cut]).is_err(),
                    "a truncated document was accepted at byte {cut}"
                );
            }
        }
    }

    #[test]
    fn repair_itself_refuses_open_strings_and_dangling_escapes() {
        // Each text carries damage that repair could fix (a raw newline, an
        // invalid escape, a BOM) yet ends inside a string or an escape, so the
        // document is truncated. Repair must refuse it outright instead of
        // relying on the follow-up parse to notice.
        for raw in [
            "{\"content\":\"abc\ndef",
            "{\"content\":\"abc\\'def",
            "\u{feff}{\"content\":\"abc\ndef",
            "{\"content\":\"abc\n\\",
            "{\"content\":\"abc\n\\u00e",
            "{\"a\":\"x\",\"content\":\"abc\n",
            "{\"content\":[\"abc\n",
            // The same when the invalid escape keeps its backslash: a Markdown
            // escape or a path in a string that never closes, a hard line break
            // (backslash, raw newline) inside one, and a backslash that ends the
            // text after a kept one.
            "{\"content\":\"abc\\*def",
            "{\"content\":\"C:\\Users\\me",
            "{\"content\":\"abc\\\ndef",
            "{\"content\":\"abc\\\n",
            "{\"content\":\"abc\\*\\",
        ] {
            assert!(repaired(raw).is_none(), "{raw:?}");
            assert_eq!(decoded(raw), Err(INVALID.to_owned()), "{raw:?}");
        }
        // The same damage in a closed string is repaired, so the refusal above
        // is about the truncation and not about the damage.
        assert!(repaired("{\"content\":\"abc\ndef\"}").is_some());
        assert!(repaired("{\"content\":\"abc\\*def\"}").is_some());
        assert!(repaired("{\"content\":\"abc\\\ndef\"}").is_some());
    }

    #[test]
    fn repair_leaves_valid_characters_and_escape_pairs_exactly_as_written() {
        // DEL, NBSP, BOM, U+2028, an emoji and an escaped surrogate pair sit
        // beside one raw newline; only the newline may change.
        let raw = "{\"a\":\"\u{7f}\u{a0}\u{feff}\u{2028}🙂\\ud83d\\ude42\n\"}";
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, "{\"a\":\"\u{7f}\u{a0}\u{feff}\u{2028}🙂\\ud83d\\ude42\\n\"}");
        assert_eq!(kinds, CONTROL);
        assert_eq!(decoded(raw).unwrap(), json!({"a":"\u{7f}\u{a0}\u{feff}\u{2028}🙂🙂\n"}));

        // A backslash pair right before a closing quote is a pair, not an
        // escaped quote, so the string still closes and the next raw newline
        // is inside a string of its own.
        let raw = "{\"path\":\"C:\\\\dir\\\\\",\"note\":\"a\nb\",\"last\":\"\\\\\"}";
        let (text, kinds) = repaired(raw).unwrap();
        assert_eq!(text, "{\"path\":\"C:\\\\dir\\\\\",\"note\":\"a\\nb\",\"last\":\"\\\\\"}");
        assert_eq!(kinds, CONTROL);
        assert_eq!(
            decoded(raw).unwrap(),
            json!({"path":"C:\\dir\\","note":"a\nb","last":"\\"})
        );
        let raw = "{\"q\":\"say \\\"hi\\\"\\\\\",\"n\":\"a\nb\"}";
        assert_eq!(decoded(raw).unwrap(), json!({"q":"say \"hi\"\\","n":"a\nb"}));
    }

    #[test]
    fn a_large_damaged_document_repairs_exactly() {
        let body = NORWEGIAN.repeat(1200);
        let raw = format!("{{\"content\":\"{}\"}}", escape_quotes_and_backslashes(&body));
        assert!(raw.len() > 200_000);
        let (_, kinds) = repaired(&raw).unwrap();
        assert_eq!(kinds, CONTROL);
        let args = decoded(&raw).unwrap();
        assert_eq!(args["content"].as_str().unwrap().as_bytes(), body.as_bytes());
        // Cut inside the last line: still refused at this size.
        assert!(parse_arguments(&raw[..raw.rfind("Slutt").unwrap() + "Slutt".len()]).is_err());
    }

    #[test]
    fn a_large_document_full_of_invalid_escapes_repairs_exactly() {
        // Over 1 MB in which every kind of invalid escape recurs: kept
        // backslashes before ASCII, before a non-ASCII letter and before a raw
        // newline, apostrophe escapes, and a valid pair right after them.
        const WRITTEN: &str = "Sti C:\\Users\\me \\* \\_ it\\'s \\å\\\n\\\\ end ";
        const MEANT: &str = "Sti C:\\Users\\me \\* \\_ it's \\å\\\n\\ end ";
        let raw = format!("{{\"content\":\"{}\"}}", WRITTEN.repeat(30_000));
        assert!(raw.len() > 1_000_000);
        let (_, kinds) = repaired(&raw).unwrap();
        assert_eq!(kinds, RepairKinds { control_chars: true, invalid_escapes: true, ..NONE });
        let args = decoded(&raw).unwrap();
        assert_eq!(args["content"].as_str().unwrap().as_bytes(), MEANT.repeat(30_000).as_bytes());
        // Cut inside the last unit: still refused at this size.
        assert!(parse_arguments(&raw[..raw.rfind("end").unwrap() + "end".len()]).is_err());
    }

    #[test]
    fn shape_and_labels_describe_failures_without_content() {
        let shape = describe_shape("{\"a\":[[\"x\ny");
        assert!(shape.ends_in_open_string && shape.raw_control_chars);
        assert_eq!(shape.max_depth, 3);
        let shape = describe_shape("{\"a\":\"q\\\"[[[[\",\"b\":[1]}");
        assert!(!shape.ends_in_open_string && !shape.raw_control_chars);
        assert_eq!(shape.max_depth, 2);
        assert_eq!(NONE.label(), "none");
        assert_eq!(
            RepairKinds { control_chars: true, trailing_commas: true, ..NONE }.label(),
            "control_chars,trailing_commas"
        );
    }

    type Logged = (tracing::Level, Vec<(String, String)>);

    struct FieldCollector(Vec<(String, String)>);
    impl tracing::field::Visit for FieldCollector {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}").trim_matches('"').to_owned()));
        }
    }

    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<Logged>>>);
    impl tracing::Subscriber for Recorder {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut fields = FieldCollector(Vec::new());
            event.record(&mut fields);
            self.0.lock().unwrap().push((*event.metadata().level(), fields.0));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// The events `parse_arguments(raw)` logs. Callsite interest is cached
    /// process-wide and every other test in the crate reaches the same
    /// callsites without a subscriber; a first-time registration that overlaps
    /// this subscriber's can leave a stale "never" behind, and the event is
    /// then silently dropped. Each attempt installs a fresh subscriber, which
    /// rebuilds that cache, and the first attempt that logged anything is
    /// returned, so the recording no longer depends on that race. Text that
    /// logs nothing is still reported as logging nothing.
    fn events_of(raw: &str) -> Vec<Logged> {
        for _ in 0..5 {
            let sink = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let _ = tracing::subscriber::with_default(Recorder(sink.clone()), || parse_arguments(raw));
            let events = sink.lock().unwrap().clone();
            if !events.is_empty() {
                return events;
            }
        }
        Vec::new()
    }

    fn field<'a>(event: &'a Logged, name: &str) -> &'a str {
        event.1.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()).unwrap()
    }

    #[test]
    fn repairs_and_failures_are_logged_without_content() {
        const MARKER: &str = "SECRETMARKER";
        // Valid arguments: no log at all.
        assert!(events_of(&format!("{{\"content\":\"{MARKER}\"}}")).is_empty());

        // Repaired: one INFO with the kinds and the byte length only.
        let raw = format!("{{\"content\":\"{MARKER}\nline\"}}");
        let events = events_of(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, tracing::Level::INFO);
        assert_eq!(field(&events[0], "message"), "subscription tool arguments repaired");
        assert_eq!(field(&events[0], "repair_kinds"), "control_chars");
        assert_eq!(field(&events[0], "args_bytes"), raw.len().to_string());
        assert_eq!(events[0].1.len(), 3);

        // Invalid escapes are logged as such in both branches: the apostrophe
        // escape that loses its backslash and the ones that keep it. This
        // stays in this test because a second test installing its own
        // subscriber would race this one on tracing's shared callsite cache.
        for raw in [
            format!("{{\"content\":\"{MARKER}\\'s\"}}"),
            format!("{{\"content\":\"{MARKER}\\*\"}}"),
            format!("{{\"content\":\"{MARKER}\\'\\*\"}}"),
        ] {
            let events = events_of(&raw);
            assert_eq!(events.len(), 1, "{raw}");
            assert_eq!(events[0].0, tracing::Level::INFO, "{raw}");
            assert_eq!(field(&events[0], "repair_kinds"), "invalid_escapes", "{raw}");
            assert_eq!(field(&events[0], "args_bytes"), raw.len().to_string(), "{raw}");
            assert_eq!(events[0].1.len(), 3, "{raw}");
        }

        // Unrepairable and truncated: one WARN with the diagnostic fields.
        let raw = format!("{{\"content\":\"{MARKER}\nline");
        let events = events_of(&raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.0, tracing::Level::WARN);
        assert_eq!(field(event, "message"), INVALID);
        assert!(matches!(field(event, "error_category"), "syntax" | "eof"));
        assert_eq!(field(event, "args_bytes"), raw.len().to_string());
        assert_eq!(field(event, "ends_in_open_string"), "true");
        assert_eq!(field(event, "raw_control_chars"), "true");
        assert_eq!(field(event, "max_depth"), "1");
        assert_eq!(field(event, "repair_kinds"), "none");
        assert_eq!(field(event, "repaired_still_invalid"), "false");
        assert!(field(event, "line").parse::<usize>().unwrap() >= 1);
        assert!(field(event, "column").parse::<usize>().is_ok());

        // Repair produced text that still does not parse: reported as such.
        let raw = format!("{{\"content\":\"{MARKER}\n\\ud800\"}}");
        let events = events_of(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(field(&events[0], "repair_kinds"), "control_chars");
        assert_eq!(field(&events[0], "repaired_still_invalid"), "true");
        assert_eq!(field(&events[0], "ends_in_open_string"), "false");

        // No event, at any level, carries any of the document text.
        for event in events_of(&raw).iter().chain(&events_of(&format!("{{\"c\":\"{MARKER}\n"))) {
            for (name, value) in &event.1 {
                assert!(!value.contains(MARKER) && !name.contains(MARKER));
            }
        }
    }

    // ---- property-style checks ----------------------------------------------

    /// xorshift64*: deterministic, so a failing case index reproduces.
    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, bound: usize) -> usize {
            usize::try_from(self.next_u64() >> 33).unwrap() % bound
        }
        fn chance(&mut self, percent: usize) -> bool {
            self.below(100) < percent
        }
        fn whitespace(&mut self) -> &'static str {
            ["", "", " ", "\n", "\t", "\r\n  "][self.below(6)]
        }
    }

    #[derive(Clone, Copy)]
    struct Damage {
        controls: bool,
        escapes: bool,
        commas: bool,
        wrap: bool,
    }

    /// A generated document: its (possibly damaged) text, the value the model
    /// meant, and which damage classes were actually emitted into the text.
    struct Generated {
        text: String,
        expected: Value,
        applied: RepairKinds,
    }

    const POOL: [char; 32] = [
        'a', 'b', 'n', 'u', 't', 'Z', '0', ' ', 'æ', 'ø', 'å', '🙂', '"', '\\', '/', '\'', '*',
        '_', ',', '}', ']', '{', ':', '\n', '\r', '\t', '\u{1}', '\u{8}', '\u{c}', '\u{1f}',
        '\u{feff}', '\u{a0}',
    ];

    fn generate_string(rng: &mut Rng, damage: Damage, applied: &mut RepairKinds) -> (String, String) {
        let mut text = String::from("\"");
        let mut meant = String::new();
        for _ in 0..rng.below(14) {
            let c = POOL[rng.below(POOL.len())];
            meant.push(c);
            match c {
                '"' => text.push_str("\\\""),
                '\\' => text.push_str("\\\\"),
                '/' => text.push_str(if rng.chance(50) { "\\/" } else { "/" }),
                c if u32::from(c) < 0x20 => {
                    if damage.controls && rng.chance(60) {
                        applied.control_chars = true;
                        if damage.escapes && rng.chance(25) {
                            // A backslash right before the raw control
                            // character (a Markdown hard line break): the
                            // backslash is meant literally.
                            applied.invalid_escapes = true;
                            text.push('\\');
                            meant.pop();
                            meant.push('\\');
                            meant.push(c);
                        }
                        text.push(c);
                    } else {
                        text.push_str(&match c {
                            '\n' => "\\n".to_owned(),
                            '\r' => "\\r".to_owned(),
                            '\t' => "\\t".to_owned(),
                            '\u{8}' => "\\b".to_owned(),
                            '\u{c}' => "\\f".to_owned(),
                            other => format!("\\u{:04x}", u32::from(other)),
                        });
                    }
                }
                c if damage.escapes
                    && !matches!(c, 'b' | 'f' | 'n' | 'r' | 't' | 'u')
                    && rng.chance(30) =>
                {
                    applied.invalid_escapes = true;
                    text.push('\\');
                    text.push(c);
                    // `\'` means just the apostrophe; any other invalid escape
                    // was meant literally, backslash included.
                    if c != '\'' {
                        meant.pop();
                        meant.push('\\');
                        meant.push(c);
                    }
                }
                c if !c.is_ascii() && rng.chance(30) => {
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        text.push_str(&format!("\\u{:04x}", unit));
                    }
                }
                c => text.push(c),
            }
        }
        text.push('"');
        (text, meant)
    }

    fn generate_value(rng: &mut Rng, damage: Damage, applied: &mut RepairKinds, depth: usize) -> (String, Value) {
        match rng.below(if depth >= 3 { 4 } else { 6 }) {
            0 => ("null".to_owned(), Value::Null),
            1 => {
                let flag = rng.chance(50);
                (flag.to_string(), Value::Bool(flag))
            }
            2 => {
                let number = i64::try_from(rng.below(2000)).unwrap() - 1000;
                (number.to_string(), json!(number))
            }
            3 => {
                let (text, meant) = generate_string(rng, damage, applied);
                (text, Value::String(meant))
            }
            4 => generate_container(rng, damage, applied, depth, false),
            _ => generate_container(rng, damage, applied, depth, true),
        }
    }

    fn generate_container(rng: &mut Rng, damage: Damage, applied: &mut RepairKinds, depth: usize, object: bool) -> (String, Value) {
        let mut text = String::from(if object { "{" } else { "[" });
        let (mut items, mut members) = (Vec::new(), serde_json::Map::new());
        let count = rng.below(4);
        for index in 0..count {
            if index > 0 {
                text.push(',');
            }
            text.push_str(rng.whitespace());
            if object {
                let (key_text, key) = generate_string(rng, damage, applied);
                text.push_str(&key_text);
                text.push_str(rng.whitespace());
                text.push(':');
                text.push_str(rng.whitespace());
                let (value_text, value) = generate_value(rng, damage, applied, depth + 1);
                text.push_str(&value_text);
                members.insert(key, value);
            } else {
                let (value_text, value) = generate_value(rng, damage, applied, depth + 1);
                text.push_str(&value_text);
                items.push(value);
            }
            text.push_str(rng.whitespace());
        }
        if count > 0 && damage.commas && rng.chance(50) {
            applied.trailing_commas = true;
            text.push(',');
            text.push_str(rng.whitespace());
        }
        text.push(if object { '}' } else { ']' });
        (text, if object { Value::Object(members) } else { Value::Array(items) })
    }

    fn generate(rng: &mut Rng, damage: Damage) -> Generated {
        let mut applied = NONE;
        let (mut text, expected) = generate_container(rng, damage, &mut applied, 0, true);
        if damage.wrap {
            let prefix = ["", "\n", "\u{feff}", "\u{feff}\n ", "\u{a0}"][rng.below(5)];
            let suffix = ["", "\n", "\u{a0}", "\u{3000}", " "][rng.below(5)];
            if prefix.chars().chain(suffix.chars()).any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r')) {
                applied.bom_or_whitespace = true;
            }
            text = format!("{prefix}{text}{suffix}");
        }
        Generated { text, expected, applied }
    }

    /// The brackets outside string literals, by an independent scan.
    fn structure(text: &str) -> String {
        let (mut out, mut in_string) = (String::new(), false);
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            match (in_string, c) {
                (true, '\\') => {
                    chars.next();
                }
                (true, '"') => in_string = false,
                (false, '"') => in_string = true,
                (false, '{' | '}' | '[' | ']') => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn generated_damage_is_repaired_losslessly_and_valid_text_is_untouched() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut damaged, mut clean) = (0, 0);
        for case in 0..6000_usize {
            let damage = Damage {
                controls: case & 1 != 0,
                escapes: case & 2 != 0,
                commas: case & 4 != 0,
                wrap: case & 8 != 0,
            };
            let doc = generate(&mut rng, damage);
            let strict = serde_json::from_str::<Value>(&doc.text);
            if doc.applied.any() {
                damaged += 1;
                assert!(strict.is_err(), "case {case}: damaged text parsed strictly");
                let repaired = repair_tool_arguments(&doc.text).unwrap_or_else(|| panic!("case {case}: not repaired"));
                assert_eq!(repaired.kinds, doc.applied, "case {case}");
                assert_eq!(
                    serde_json::from_str::<Value>(&repaired.text).unwrap(),
                    doc.expected,
                    "case {case}"
                );
                assert_eq!(parse_arguments(&doc.text).unwrap(), doc.expected, "case {case}");
                assert!(repair_tool_arguments(&repaired.text).is_none(), "case {case}: not idempotent");
                assert_eq!(structure(&doc.text), structure(&repaired.text), "case {case}");
            } else {
                clean += 1;
                assert_eq!(strict.unwrap(), doc.expected, "case {case}");
                assert!(repair_tool_arguments(&doc.text).is_none(), "case {case}: valid text changed");
                let got = parse_arguments_with(&doc.text, |_| panic!("repair ran on valid text"));
                assert_eq!(got.unwrap(), doc.expected, "case {case}");
            }
            // Cutting the document anywhere before its final `}` never yields
            // a document, whatever the damage.
            if case % 20 == 0 {
                let last_closer = doc.text.rfind('}').unwrap();
                for cut in doc.text.char_indices().map(|(at, _)| at).filter(|&at| at <= last_closer) {
                    assert!(parse_arguments(&doc.text[..cut]).is_err(), "case {case}: cut at {cut} accepted");
                }
            }
        }
        assert!(damaged > 1500 && clean > 1500, "generator balance: {damaged} damaged, {clean} clean");
    }

    /// What the reference decoder made of a document.
    #[derive(Debug)]
    enum Reference {
        /// The intended text of `k`, and which damage classes it had to undo.
        Decoded { text: String, invalid_escapes: bool, control_chars: bool },
        /// A rule says the document must stay invalid: the string never closes
        /// or a `\u` is malformed.
        Refused,
        /// Not `{"k":"<one string>"}` after all (a quote closed the string
        /// early), or a lone surrogate, which is outside this model.
        OutOfModel,
    }

    /// Decodes `{"k":"<body>"}` straight to the text the model meant, from the
    /// stated rule alone: one pass over indexed characters, no rewritten JSON
    /// and no shared code with `repair_tool_arguments` or `serde_json`.
    fn reference_decode(document: &str) -> Reference {
        let chars: Vec<char> = document.chars().collect();
        let opening: Vec<char> = "{\"k\":\"".chars().collect();
        if !chars.starts_with(&opening) {
            return Reference::OutOfModel;
        }
        let (mut text, mut invalid_escapes, mut control_chars) = (String::new(), false, false);
        let mut at = opening.len();
        loop {
            let Some(&c) = chars.get(at) else { return Reference::Refused };
            at += 1;
            if c == '"' {
                break;
            }
            if c != '\\' {
                control_chars |= u32::from(c) < 0x20;
                text.push(c);
                continue;
            }
            let Some(&escaped) = chars.get(at) else { return Reference::Refused };
            at += 1;
            match escaped {
                '"' | '\\' | '/' => text.push(escaped),
                'b' => text.push('\u{8}'),
                'f' => text.push('\u{c}'),
                'n' => text.push('\n'),
                'r' => text.push('\r'),
                't' => text.push('\t'),
                'u' => {
                    let digits: String = chars[at..].iter().take(4).collect();
                    if digits.chars().count() < 4 || !digits.chars().all(|d| d.is_ascii_hexdigit()) {
                        return Reference::Refused;
                    }
                    at += 4;
                    match char::from_u32(u32::from_str_radix(&digits, 16).unwrap()) {
                        Some(decoded) => text.push(decoded),
                        None => return Reference::OutOfModel,
                    }
                }
                other => {
                    invalid_escapes = true;
                    control_chars |= u32::from(other) < 0x20;
                    if other != '\'' {
                        text.push('\\');
                    }
                    text.push(other);
                }
            }
        }
        if chars[at..] == ['}'] {
            Reference::Decoded { text, invalid_escapes, control_chars }
        } else {
            Reference::OutOfModel
        }
    }

    #[test]
    fn repaired_strings_match_an_independent_decoder() {
        // Pieces glued at random, so backslashes meet quotes, `u`, hex digits,
        // pairs and raw control characters in every combination.
        const PIECES: &[&str] = &[
            "a", "n", "u", "0", "9", "d", "F", " ", "æ", "å", "🙂", "'", "*", "_", "#", ",", "}",
            "/", "\\\\", "\\\"", "\\'", "\\*", "\\_", "\\ ", "\\å", "\\a", "\\n", "\\t", "\\/",
            "\\u00e5", "\\u12", "\\U0001F600", "\\", "\\\n", "\\\r\n", "\n", "\r", "\t", "\u{1}",
        ];
        let mut rng = Rng(0xA5A5_1234_DEAD_BEEF);
        let (mut decoded, mut invalid, mut refused, mut skipped) = (0, 0, 0, 0);
        for case in 0..40_000_usize {
            let body: String = (0..rng.below(9)).map(|_| PIECES[rng.below(PIECES.len())]).collect();
            let document = format!("{{\"k\":\"{body}\"}}");
            match reference_decode(&document) {
                Reference::OutOfModel => skipped += 1,
                Reference::Refused => {
                    refused += 1;
                    assert!(repair_tool_arguments(&document).is_none(), "case {case}: {document:?}");
                    assert!(parse_arguments(&document).is_err(), "case {case}: {document:?}");
                }
                Reference::Decoded { text, invalid_escapes, control_chars } => {
                    decoded += 1;
                    invalid += usize::from(invalid_escapes);
                    assert_eq!(
                        parse_arguments(&document).unwrap_or_else(|_| panic!("case {case}: {document:?}")),
                        json!({ "k": text }),
                        "case {case}: {document:?}"
                    );
                    let expected = RepairKinds { control_chars, invalid_escapes, ..NONE };
                    match repair_tool_arguments(&document) {
                        Some(repaired) => assert_eq!(repaired.kinds, expected, "case {case}: {document:?}"),
                        None => {
                            assert!(!expected.any(), "case {case}: {document:?}");
                            assert!(serde_json::from_str::<Value>(&document).is_ok(), "case {case}");
                        }
                    }
                }
            }
        }
        assert!(decoded > 15_000 && invalid > 5_000 && refused > 500, "{decoded} {invalid} {refused} {skipped}");
    }

    #[test]
    fn repair_on_arbitrary_text_never_invents_structure_or_rewrites_valid_text() {
        const ALPHABET: [char; 24] = [
            '{', '}', '[', ']', '"', '\\', ',', ':', ' ', '\n', '1', 'a', 'u', '0', 'n', '\u{feff}',
            '\u{a0}', '\'', '\u{1}', 'å', '/', 't', 'e', 'F',
        ];
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let mut repaired_count = 0;
        for case in 0..80_000_usize {
            let text: String = (0..rng.below(30)).map(|_| ALPHABET[rng.below(ALPHABET.len())]).collect();
            let strict_ok = serde_json::from_str::<Value>(&text).is_ok();
            let outcome = parse_arguments(&text);
            if let Some(repaired) = repair_tool_arguments(&text) {
                repaired_count += 1;
                assert!(!strict_ok, "case {case}: valid text was rewritten");
                assert!(repaired.kinds.any() && repaired.text != text, "case {case}");
                assert_eq!(structure(&text), structure(&repaired.text), "case {case}: structure changed");
                assert!(repair_tool_arguments(&repaired.text).is_none(), "case {case}: not idempotent");
                // What the parser accepts after repair is exactly what parse returns.
                if let Ok(value) = serde_json::from_str::<Value>(&repaired.text) {
                    assert_eq!(outcome.unwrap(), value, "case {case}");
                } else {
                    assert!(outcome.is_err(), "case {case}");
                }
            } else if strict_ok {
                assert_eq!(outcome.unwrap(), serde_json::from_str::<Value>(&text).unwrap());
            } else {
                assert!(outcome.is_err(), "case {case}: accepted without a repair");
            }
        }
        assert!(repaired_count > 200, "generator produced too few repairs: {repaired_count}");
    }
}
