//! Multi-file static web bundles (W5-087, F-t3-087 phase 0).
//!
//! `create_artifact` with `kind` `web_bundle` carries a manifest instead of a
//! page: `{"entry":"index.html","files":{"index.html":"...","app.js":"..."}}`.
//! [`inline_bundle`] checks the manifest and folds it into ONE HTML document,
//! which the gateway stores and emits as an ordinary `html` artifact. The
//! preview stays the existing sandboxed srcdoc iframe under its existing CSP,
//! and the manifest is kept nowhere: the store only ever holds the inlined page.
//!
//! What a bundle may hold, and the refusal behind each rule:
//!
//! - at most [`MAX_BUNDLE_FILES`] text files, within the artifact character
//!   limit, at relative lowercase paths with no `..` and only the extensions
//!   `.html .css .js .json .svg .txt`;
//! - one page, `index.html`, and no `type="module"`: the preview runs classic
//!   scripts only;
//! - no `import`, `export` or `import(` statement in a script, which a classic
//!   script cannot run and which would name a module to fetch;
//! - no reference that leaves the page: a `src`, `href` or similar that is
//!   absolute (`http:`, `https:`, `//`, `data:`, `blob:`) or names no file of the
//!   bundle. An `<a>` may link to an http(s) URL, since following a link is a
//!   navigation the preview does not fetch.
//!
//! Off unless `MODEL_GATEWAY_WEB_BUNDLE_ARTIFACTS` is the literal `true`. Off,
//! `web_bundle` is an unknown kind and the `create_artifact` definition is
//! unchanged.

use std::collections::{BTreeMap, BTreeSet};

use mp_contracts::model_plane::v1::ToolDefinition;
use serde_json::Value;

use crate::artifacts::MAX_TEXT_ARTIFACT_CHARS;

/// The switch that lets `create_artifact` accept a `web_bundle`. Only the literal `true` turns it on.
pub const WEB_BUNDLE_ENV: &str = "MODEL_GATEWAY_WEB_BUNDLE_ARTIFACTS";

/// The `kind` a bundle manifest is sent under.
pub const BUNDLE_KIND: &str = "web_bundle";

/// The most files one bundle may hold.
pub const MAX_BUNDLE_FILES: usize = 20;

/// The one page of a bundle, and the only entry it may name.
const ENTRY: &str = "index.html";

/// The file extensions a bundle may hold.
const ALLOWED_EXTENSIONS: [&str; 6] = [".html", ".css", ".js", ".json", ".svg", ".txt"];

/// Attributes whose value names a URL the browser fetches or follows.
const URL_ATTRIBUTES: [&str; 9] = [
    "src",
    "href",
    "xlink:href",
    "action",
    "formaction",
    "poster",
    "data",
    "srcset",
    "background",
];

/// The attributes of an `<img>` that an inlined SVG keeps, besides its alt text.
const CARRIED_ATTRIBUTES: [&str; 5] = ["id", "class", "style", "width", "height"];

/// The `kind` description once bundles are switched on: the three kinds the tool always had, then the bundle.
const KIND_DESCRIPTION: &str = "document = Markdown prose; code = source code; html = a complete HTML page previewed live; web_bundle = a static multi-file page sent as one JSON manifest, {\"entry\":\"index.html\",\"files\":{\"index.html\":\"...\",\"app.js\":\"...\"}}, holding at most 20 text files (.html .css .js .json .svg .txt) at relative lowercase paths. The gateway inlines it into one HTML page: link files with <link rel=\"stylesheet\"> and <script src>, and put SVG images in <img src>. A .json or .txt file is read in the page as window.__bundle[\"data.json\"]. The page may not use import or export, type=\"module\", fetch, or any URL that leaves the page. To revise a bundle, call create_artifact again with the same title and the complete manifest.";

/// The `content` description once bundles are switched on.
const CONTENT_DESCRIPTION: &str = "The full content. For document, Markdown. For html, a complete document. For web_bundle, the JSON manifest described under kind.";

/// Refusal: the manifest is not a JSON object with `files`.
const MANIFEST_SHAPE: &str = "a web_bundle's content must be a JSON object such as {\"entry\":\"index.html\",\"files\":{\"index.html\":\"<!doctype html>...\"}}";

/// Refusal: a tag in a bundle file has no closing `>`.
const UNCLOSED_TAG: &str = "a tag in a web_bundle file is never closed with '>'";

/// Refusal: the page carries a module script, which the preview cannot run as one.
const MODULE_SCRIPT: &str = "type=\"module\" is refused in a web_bundle: the preview runs classic scripts, so use <script src=\"app.js\"></script> with plain code";

/// Refusal: a deferred or async script would run before or after the page it expects.
const DEFER_REFUSAL: &str = "a web_bundle script cannot be defer or async: put its <script src> at the end of the body, after the elements it uses";

/// Refusal: an `@import` in a stylesheet would load another file the bundle does not inline.
const IMPORT_REFUSAL: &str = "@import is refused in a web_bundle: link each stylesheet with its own <link rel=\"stylesheet\">";

/// Refusal: an `.svg` file is not an SVG document.
const SVG_REFUSAL: &str = "an .svg file in a web_bundle must be an SVG document whose first element is <svg>";

/// Whether `raw` (the switch's value, if set) is the literal on-word.
#[must_use]
fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether bundle artifacts are switched on.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(WEB_BUNDLE_ENV).ok().as_deref())
}

/// Whether a `create_artifact` kind names a bundle this gateway checks and inlines. Only with the switch on:
/// off, `web_bundle` is an unknown kind, as it was before the kind existed.
#[must_use]
pub fn is_bundle_kind(kind: &str) -> bool {
    kind.trim().eq_ignore_ascii_case(BUNDLE_KIND) && enabled()
}

/// Names the bundle kind on the `create_artifact` definition, only while the switch is on: the kind joins the
/// enum, and the `kind` and `content` descriptions say what a bundle is. Off, the definitions are untouched.
pub(crate) fn offer(defs: &mut [ToolDefinition], enabled: bool) {
    if !enabled {
        return;
    }
    let Some(definition) = defs.iter_mut().find(|def| def.name == "create_artifact") else {
        return;
    };
    let Ok(mut parameters) = serde_json::from_str::<Value>(&definition.parameters_json) else {
        return;
    };
    if let Some(kinds) = parameters["properties"]["kind"]["enum"].as_array_mut() {
        kinds.push(Value::String(BUNDLE_KIND.to_owned()));
    }
    parameters["properties"]["kind"]["description"] = Value::String(KIND_DESCRIPTION.to_owned());
    parameters["properties"]["content"]["description"] =
        Value::String(CONTENT_DESCRIPTION.to_owned());
    if let Ok(text) = serde_json::to_string(&parameters) {
        definition.parameters_json = text;
    }
}

/// Checks a bundle manifest and inlines it into one HTML document.
///
/// The manifest is `{"entry":"index.html","files":{"path":"text", ...}}`. The result is a complete page with each
/// linked stylesheet and script written in place, each SVG image written inline, and the `.json` and `.txt` files
/// exposed as `window.__bundle["path"]` by one leading script. Nothing in it loads from outside the page.
///
/// # Errors
///
/// Returns a reason the model can act on when the bundle is not one this gateway serves: a manifest of the wrong
/// shape, more than [`MAX_BUNDLE_FILES`] files, a path or extension outside the rules, a second page, module
/// syntax, a reference that leaves the page or names no bundle file, a file the page never uses, invalid JSON,
/// or text over the artifact character limit.
pub fn inline_bundle(manifest_json: &str) -> Result<String, String> {
    let manifest: Value =
        serde_json::from_str(manifest_json).map_err(|_| MANIFEST_SHAPE.to_owned())?;
    let Some(object) = manifest.as_object() else {
        return Err(MANIFEST_SHAPE.to_owned());
    };
    if let Some(entry) = object.get("entry") {
        if !entry.is_null() && entry.as_str() != Some(ENTRY) {
            return Err(format!("a web_bundle's entry must be \"{ENTRY}\""));
        }
    }
    let Some(files) = object.get("files").and_then(Value::as_object) else {
        return Err(MANIFEST_SHAPE.to_owned());
    };
    if files.len() > MAX_BUNDLE_FILES {
        return Err(format!("a web_bundle holds at most {MAX_BUNDLE_FILES} files"));
    }
    if !files.contains_key(ENTRY) {
        return Err(format!("a web_bundle needs a \"{ENTRY}\" file"));
    }
    let mut bundle: BTreeMap<String, String> = BTreeMap::new();
    let mut characters = 0;
    for (path, value) in files {
        check_path(path)?;
        let Some(text) = value.as_str() else {
            return Err(format!("web_bundle file '{path}' must be a string of text"));
        };
        characters += text.chars().count();
        bundle.insert(path.clone(), text.to_owned());
    }
    if characters > MAX_TEXT_ARTIFACT_CHARS {
        return Err(format!(
            "the web_bundle's files exceed the {MAX_TEXT_ARTIFACT_CHARS} character limit"
        ));
    }
    for (path, text) in &bundle {
        if path.as_str() != ENTRY && path.ends_with(".html") {
            return Err(format!(
                "'{path}' is a second page; a web_bundle is one page, so put its content in \"{ENTRY}\""
            ));
        }
        if path.ends_with(".json") && serde_json::from_str::<Value>(text).is_err() {
            return Err(format!("'{path}' is not valid JSON"));
        }
        if path.ends_with(".js") && has_module_syntax(text) {
            return Err(module_refusal(&format!("'{path}'")));
        }
    }
    let Some(page) = bundle.get(ENTRY) else {
        return Err(format!("a web_bundle needs a \"{ENTRY}\" file"));
    };
    if has_module_script(page) {
        return Err(MODULE_SCRIPT.to_owned());
    }
    let mut used = BTreeSet::new();
    let mut html = rewrite_markup(page, &bundle, &mut used, true)?;
    for path in bundle.keys() {
        let needs_a_reference =
            path.ends_with(".css") || path.ends_with(".js") || path.ends_with(".svg");
        if needs_a_reference && !used.contains(path) {
            return Err(format!(
                "'{path}' is not used by \"{ENTRY}\"; link it with <link rel=\"stylesheet\">, <script src> or <img src>, or remove it"
            ));
        }
    }
    if let Some(script) = data_script(&bundle) {
        let at = after_doctype(&html);
        html.insert_str(at, &script);
    }
    if html.chars().count() > MAX_TEXT_ARTIFACT_CHARS {
        return Err(format!(
            "the inlined page exceeds the {MAX_TEXT_ARTIFACT_CHARS} character limit; make the files smaller"
        ));
    }
    Ok(html)
}

/// Checks one bundle path: relative, lowercase, no `..`, no empty or dot-prefixed segment, and an allowed
/// extension.
fn check_path(path: &str) -> Result<(), String> {
    let plain = path
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '/' | '-'));
    let relative = !path.starts_with('/')
        && !path.contains("..")
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && !segment.starts_with('.'));
    if !plain || !relative {
        return Err(format!(
            "web_bundle file '{}' must be a relative path of lowercase letters, digits, _ . / and -, with no '..' and no empty or dot-prefixed segment",
            brief(path)
        ));
    }
    if !ALLOWED_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(*extension))
    {
        return Err(format!(
            "web_bundle file '{}' must end in .html, .css, .js, .json, .svg or .txt",
            brief(path)
        ));
    }
    Ok(())
}

/// The `window.__bundle` script for the `.json` and `.txt` files, or `None` when the bundle has none. The JSON is
/// escaped so no `<` can open a closing tag inside the script.
fn data_script(bundle: &BTreeMap<String, String>) -> Option<String> {
    let mut data = serde_json::Map::new();
    for (path, text) in bundle {
        if path.ends_with(".json") {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                data.insert(path.clone(), value);
            }
        } else if path.ends_with(".txt") {
            data.insert(path.clone(), Value::String(text.clone()));
        }
    }
    if data.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&Value::Object(data)).ok()?;
    let safe = json
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Some(format!("<script>window.__bundle = {safe};</script>"))
}

/// Where the leading script goes: just after a doctype that opens the page, otherwise at the start, so the
/// doctype keeps the page out of quirks mode.
fn after_doctype(page: &str) -> usize {
    let start = page.len() - page.trim_start().len();
    let is_doctype = page
        .get(start..start + 9)
        .is_some_and(|head| head.eq_ignore_ascii_case("<!doctype"));
    if !is_doctype {
        return 0;
    }
    page[start..].find('>').map_or(0, |end| start + end + 1)
}

/// Whether the page carries a `type="module"` script, whatever the spacing and case.
fn has_module_script(html: &str) -> bool {
    let squeezed: String = html
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    ["type=\"module\"", "type='module'", "type=module"]
        .iter()
        .any(|needle| squeezed.contains(*needle))
}

/// Whether `code` has module syntax a classic script cannot run: an `import` or `export` statement, or a dynamic
/// `import(`. Comments and strings are not parsed, so a line that starts with `export` inside a string is refused
/// too; that is the safe direction.
fn has_module_syntax(code: &str) -> bool {
    let bytes = code.as_bytes();
    (0..bytes.len()).any(|at| module_keyword_at(bytes, at))
}

/// Whether `bytes[at..]` starts a module keyword that stands on its own.
fn module_keyword_at(bytes: &[u8], at: usize) -> bool {
    let rest = &bytes[at..];
    let is_import = rest.starts_with(b"import");
    if !is_import && !rest.starts_with(b"export") {
        return false;
    }
    // A keyword inside a longer word (`reimport`, `obj.export`) is not a statement.
    if at > 0 && is_word_byte(bytes[at - 1]) {
        return false;
    }
    // Both keywords are six bytes long.
    let Some(&after) = bytes.get(at + 6) else {
        return false;
    };
    if is_import && matches!(after, b'(' | b'.') {
        return true;
    }
    (after.is_ascii_whitespace() || matches!(after, b'{' | b'*' | b'"' | b'\''))
        && statement_start(bytes, at)
}

/// Whether an identifier character can sit next to a keyword and make it part of a longer name.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'.')
}

/// Whether `bytes[at]` opens a statement: it is the first thing on its line, or follows `;` or `}`.
fn statement_start(bytes: &[u8], at: usize) -> bool {
    let mut before = at;
    while before > 0 && matches!(bytes[before - 1], b' ' | b'\t') {
        before -= 1;
    }
    before == 0 || matches!(bytes[before - 1], b'\n' | b'\r' | b';' | b'}')
}

/// Refusal for module syntax found in `place`.
fn module_refusal(place: &str) -> String {
    format!(
        "{place} uses import or export syntax, which a web_bundle cannot run: write a plain classic script with no import, export or import() statement"
    )
}

/// Refusal for a URL that would load, or follow, something outside the page.
fn outside_url(name: &str, value: &str) -> String {
    format!(
        "{name} '{}' would load from outside this page; only #anchors and files in the web_bundle may be used",
        brief(value)
    )
}

/// A reference as an error message shows it: at most 80 characters, so a `data:` URL cannot flood the reply.
fn brief(value: &str) -> String {
    const SHOWN: usize = 80;
    if value.chars().count() > SHOWN {
        format!("{}...", value.chars().take(SHOWN).collect::<String>())
    } else {
        value.to_owned()
    }
}

/// Whether `url` is absolute: protocol-relative, or opening with a scheme such as `http:` or `data:`.
fn is_absolute(url: &str) -> bool {
    let url = url.trim();
    if url.starts_with("//") {
        return true;
    }
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// Whether `url` names a place on this page: a `#fragment`.
fn is_fragment(url: &str) -> bool {
    url.trim_start().starts_with('#')
}

/// Whether `url` is an http(s) or protocol-relative link, which an `<a>` may follow.
fn is_external_link(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//")
}

/// Checks a stylesheet, or a `style` attribute: no `@import`, and `url()` names only a `#fragment`.
fn check_css(css: &str) -> Result<(), String> {
    let lower = css.to_ascii_lowercase();
    if lower.contains("@import") {
        return Err(IMPORT_REFUSAL.to_owned());
    }
    let mut from = 0;
    while let Some(at) = lower[from..].find("url(") {
        let start = from + at + 4;
        let Some(close) = lower[start..].find(')') else {
            return Err("a url( in a web_bundle stylesheet is never closed".to_owned());
        };
        let target = css[start..start + close]
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\'')
            .trim();
        if !is_fragment(target) {
            return Err(format!(
                "url({}) in CSS would load from outside this page; use only #fragment references",
                brief(target)
            ));
        }
        from = start + close + 1;
    }
    Ok(())
}

/// Resolves a `src` or `href` to a bundle file with `extension`, returning its path and text. A URL, a missing
/// file or a file of another kind is refused.
fn bundle_file<'a>(
    bundle: &'a BTreeMap<String, String>,
    reference: &str,
    extension: &str,
    role: &str,
) -> Result<(&'a str, &'a str), String> {
    if is_absolute(reference) {
        return Err(format!(
            "{role} '{}' is an absolute URL; a web_bundle loads only its own files, so name one by a relative path",
            brief(reference)
        ));
    }
    let target = reference.trim();
    let path = target.strip_prefix("./").unwrap_or(target);
    let Some((key, text)) = bundle.get_key_value(path) else {
        return Err(format!(
            "{role} '{}' does not name a file in this web_bundle",
            brief(reference)
        ));
    };
    if !key.ends_with(extension) {
        return Err(format!(
            "{role} '{}' must name a {extension} file in this web_bundle",
            brief(reference)
        ));
    }
    Ok((key.as_str(), text.as_str()))
}

/// Escapes a closing `</name` (in any case) inside raw text, so the text cannot end the element early.
fn escape_closing(code: &str, name: &str) -> String {
    let closing = format!("</{name}");
    let mut out = String::with_capacity(code.len());
    let mut copied = 0;
    while let Some(at) = find_ascii_ci(code, &closing, copied) {
        out.push_str(&code[copied..at]);
        out.push_str("<\\/");
        copied = at + 2;
    }
    out.push_str(&code[copied..]);
    out
}

/// The index of the first `needle` at or after `from`, comparing ASCII letters without case. Both strings are
/// searched as bytes; the match starts on an ASCII byte, so it is a character boundary.
fn find_ascii_ci(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let hay = haystack.as_bytes();
    let pat = needle.as_bytes();
    if pat.is_empty() || from > hay.len() || hay.len() - from < pat.len() {
        return None;
    }
    (from..=hay.len() - pat.len())
        .find(|&at| hay[at..at + pat.len()].eq_ignore_ascii_case(pat))
}

/// The raw-text span of an element whose content starts at `from`: the index of its closing `</name`, and the
/// index just past that tag's `>`.
fn raw_text_span(source: &str, from: usize, name: &str) -> Result<(usize, usize), String> {
    let close = find_ascii_ci(source, &format!("</{name}"), from).ok_or_else(|| {
        format!("a <{name}> in a web_bundle is never closed with </{name}>")
    })?;
    let end = source[close..]
        .find('>')
        .map(|offset| close + offset + 1)
        .ok_or_else(|| UNCLOSED_TAG.to_owned())?;
    Ok((close, end))
}

/// Escapes a value for a double-quoted attribute.
fn attr_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// One opening tag: its lowercase name, its attributes in source order (a boolean attribute has no value), and
/// the byte index just past its closing `>`.
struct Tag {
    name: String,
    attrs: Vec<(String, Option<String>)>,
    end: usize,
}

/// The value of the first attribute named `name`. A boolean attribute reads as "".
fn attr<'a>(tag: &'a Tag, name: &str) -> Option<&'a str> {
    tag.attrs
        .iter()
        .find(|(attr_name, _)| attr_name.as_str() == name)
        .map(|(_, value)| value.as_deref().unwrap_or(""))
}

/// Whether the tag is a `<link>` whose `rel` names a stylesheet.
fn is_stylesheet(tag: &Tag) -> bool {
    attr(tag, "rel").is_some_and(|rel| {
        rel.split_ascii_whitespace()
            .any(|token| token.eq_ignore_ascii_case("stylesheet"))
    })
}

/// Whether a `<script>` is classic: no `type`, or a JavaScript type.
fn is_script_type(tag: &Tag) -> bool {
    attr(tag, "type").map_or(true, |kind| {
        let kind = kind.trim().to_ascii_lowercase();
        kind.is_empty() || kind.contains("javascript") || kind.contains("ecmascript")
    })
}

/// The attributes an inlined SVG takes from the `<img>` it replaces, as text for the SVG's opening tag.
fn carried_attributes(tag: &Tag) -> String {
    let mut carried = String::new();
    for name in CARRIED_ATTRIBUTES {
        if let Some(value) = attr(tag, name) {
            carried.push_str(&format!(" {name}=\"{}\"", attr_escape(value)));
        }
    }
    if let Some(alt) = attr(tag, "alt").filter(|alt| !alt.trim().is_empty()) {
        carried.push_str(&format!(" role=\"img\" aria-label=\"{}\"", attr_escape(alt)));
    }
    carried
}

/// Checks the attributes of a tag that are not handled by name: an event handler may not use module syntax, a
/// `style` may not reach outside the page, and a URL attribute other than `skip` must be a `#fragment`.
fn check_attributes(tag: &Tag, skip: &str) -> Result<(), String> {
    for (name, value) in &tag.attrs {
        let Some(value) = value.as_deref() else {
            continue;
        };
        if name.starts_with("on") && has_module_syntax(value) {
            return Err(module_refusal(&format!("the {name} handler")));
        }
        if name.as_str() == "style" {
            check_css(value)?;
        }
        if name.as_str() != skip
            && URL_ATTRIBUTES.contains(&name.as_str())
            && !is_fragment(value)
        {
            return Err(outside_url(name, value));
        }
    }
    Ok(())
}

/// Reads one opening tag at `start`, which holds `<` and a letter. Quoted values may hold `>`.
fn parse_tag(source: &str, start: usize) -> Result<Tag, String> {
    let bytes = source.as_bytes();
    let mut pos = start + 1;
    let name_start = pos;
    while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || matches!(bytes[pos], b'-' | b':')) {
        pos += 1;
    }
    let name = source[name_start..pos].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while pos < bytes.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/') {
            pos += 1;
        }
        let Some(&byte) = bytes.get(pos) else {
            return Err(UNCLOSED_TAG.to_owned());
        };
        if byte == b'>' {
            return Ok(Tag {
                name,
                attrs,
                end: pos + 1,
            });
        }
        let attr_start = pos;
        while pos < bytes.len()
            && !matches!(bytes[pos], b'=' | b'>')
            && !bytes[pos].is_ascii_whitespace()
        {
            pos += 1;
        }
        if pos == attr_start {
            // A stray `=` with no name before it: skip it.
            pos += 1;
            continue;
        }
        let attr_name = source[attr_start..pos].to_ascii_lowercase();
        let mut look = pos;
        while look < bytes.len() && bytes[look].is_ascii_whitespace() {
            look += 1;
        }
        if bytes.get(look) != Some(&b'=') {
            attrs.push((attr_name, None));
            continue;
        }
        look += 1;
        while look < bytes.len() && bytes[look].is_ascii_whitespace() {
            look += 1;
        }
        let Some(&quote) = bytes.get(look) else {
            return Err(UNCLOSED_TAG.to_owned());
        };
        let value;
        if quote == b'"' || quote == b'\'' {
            let value_start = look + 1;
            let Some(close) = bytes[value_start..].iter().position(|&b| b == quote) else {
                return Err(UNCLOSED_TAG.to_owned());
            };
            value = source[value_start..value_start + close].to_owned();
            pos = value_start + close + 1;
        } else {
            let value_start = look;
            let mut value_end = look;
            while value_end < bytes.len()
                && !bytes[value_end].is_ascii_whitespace()
                && bytes[value_end] != b'>'
            {
                value_end += 1;
            }
            value = source[value_start..value_end].to_owned();
            pos = value_end;
        }
        attrs.push((attr_name, Some(value)));
    }
}

/// What the scanner does with one tag. `Keep` copies the source through `end`. `Replace` puts `text` in place of
/// the source from the tag's `<` through `end`.
enum Step {
    Keep { end: usize },
    Replace { text: String, end: usize },
}

/// Decides what one opening tag becomes. Stylesheets, scripts and SVG images are written in place; a reference
/// that leaves the page or names no bundle file is refused.
fn handle_tag(
    source: &str,
    tag: &Tag,
    bundle: &BTreeMap<String, String>,
    used: &mut BTreeSet<String>,
    allow_images: bool,
) -> Result<Step, String> {
    match tag.name.as_str() {
        "link" if is_stylesheet(tag) => {
            check_attributes(tag, "href")?;
            let Some(href) = attr(tag, "href") else {
                return Err("a <link rel=\"stylesheet\"> in a web_bundle needs an href naming a .css file".to_owned());
            };
            let (path, css) = bundle_file(bundle, href, ".css", "<link href>")?;
            check_css(css)?;
            used.insert(path.to_owned());
            let media = attr(tag, "media")
                .map_or_else(String::new, |media| format!(" media=\"{}\"", attr_escape(media)));
            Ok(Step::Replace {
                text: format!("<style{media}>\n{}\n</style>", escape_closing(css, "style")),
                end: tag.end,
            })
        }
        "script" => {
            check_attributes(tag, "src")?;
            let (content_end, end) = raw_text_span(source, tag.end, "script")?;
            match attr(tag, "src") {
                Some(reference) => {
                    if attr(tag, "defer").is_some() || attr(tag, "async").is_some() {
                        return Err(DEFER_REFUSAL.to_owned());
                    }
                    let (path, code) = bundle_file(bundle, reference, ".js", "<script src>")?;
                    used.insert(path.to_owned());
                    Ok(Step::Replace {
                        text: format!("<script>\n{}\n</script>", escape_closing(code, "script")),
                        end,
                    })
                }
                None => {
                    if is_script_type(tag) && has_module_syntax(&source[tag.end..content_end]) {
                        return Err(module_refusal("an inline script"));
                    }
                    Ok(Step::Keep { end })
                }
            }
        }
        "style" => {
            let (content_end, end) = raw_text_span(source, tag.end, "style")?;
            check_css(&source[tag.end..content_end])?;
            Ok(Step::Keep { end })
        }
        "img" => {
            check_attributes(tag, "src")?;
            let Some(reference) = attr(tag, "src") else {
                return Ok(Step::Keep { end: tag.end });
            };
            if !allow_images {
                return Err(format!(
                    "an <img> inside an inlined SVG cannot load '{}'; draw the shapes in the SVG instead",
                    brief(reference)
                ));
            }
            let (path, svg) = bundle_file(bundle, reference, ".svg", "<img src>")?;
            used.insert(path.to_owned());
            let text = svg_markup(svg, bundle, used, &carried_attributes(tag))?;
            Ok(Step::Replace {
                text,
                end: tag.end,
            })
        }
        "a" => {
            check_attributes(tag, "href")?;
            match attr(tag, "href") {
                Some(href) if !(is_fragment(href) || is_external_link(href)) => Err(format!(
                    "link '{}' must be a #anchor or an http(s) URL; a web_bundle is one page",
                    brief(href)
                )),
                _ => Ok(Step::Keep { end: tag.end }),
            }
        }
        _ => {
            check_attributes(tag, "")?;
            Ok(Step::Keep { end: tag.end })
        }
    }
}

/// Scans markup and returns it with its bundle references written in place. The page and an inlined SVG both come
/// through here; an SVG passes `allow_images` false, so an image inside it cannot pull in another SVG.
fn rewrite_markup(
    source: &str,
    bundle: &BTreeMap<String, String>,
    used: &mut BTreeSet<String>,
    allow_images: bool,
) -> Result<String, String> {
    let mut out = String::with_capacity(source.len());
    let mut copied = 0;
    let mut cursor = 0;
    while let Some(offset) = source[cursor..].find('<') {
        let start = cursor + offset;
        let rest = &source[start..];
        cursor = if rest.starts_with("<!--") {
            rest.find("-->").map_or(source.len(), |end| start + end + 3)
        } else if rest.starts_with("<![CDATA[") {
            rest.find("]]>").map_or(source.len(), |end| start + end + 3)
        } else if rest.starts_with("<!") || rest.starts_with("<?") || rest.starts_with("</") {
            rest.find('>').map_or(source.len(), |end| start + end + 1)
        } else if rest.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic) {
            let tag = parse_tag(source, start)?;
            match handle_tag(source, &tag, bundle, used, allow_images)? {
                Step::Keep { end } => end,
                Step::Replace { text, end } => {
                    out.push_str(&source[copied..start]);
                    out.push_str(&text);
                    copied = end;
                    end
                }
            }
        } else {
            start + 1
        };
    }
    out.push_str(&source[copied..]);
    Ok(out)
}

/// The markup of an `.svg` file, ready to sit inline: its XML prolog and doctype are dropped, its references are
/// checked and written in place, and the attributes of the `<img>` it replaces are carried onto its root.
fn svg_markup(
    text: &str,
    bundle: &BTreeMap<String, String>,
    used: &mut BTreeSet<String>,
    carried: &str,
) -> Result<String, String> {
    let mut body = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        let after = if body.starts_with("<!--") {
            body.find("-->").map(|end| end + 3)
        } else if body.starts_with("<?") || body.starts_with("<!") {
            body.find('>').map(|end| end + 1)
        } else {
            None
        };
        let Some(after) = after else {
            break;
        };
        body = body[after..].trim_start();
    }
    if !body
        .get(..4)
        .is_some_and(|root| root.eq_ignore_ascii_case("<svg"))
    {
        return Err(SVG_REFUSAL.to_owned());
    }
    let inlined = rewrite_markup(body, bundle, used, false)?;
    let Some(rest) = inlined.get(4..) else {
        return Err(SVG_REFUSAL.to_owned());
    };
    Ok(format!("<svg{carried}{rest}"))
}
